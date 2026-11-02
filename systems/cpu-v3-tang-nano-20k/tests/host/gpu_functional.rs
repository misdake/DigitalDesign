//! The real public RCC facade runs against synchronous native GPU execution.
use cpu_v3::{rcc_backend, Device, StepOutcome};
use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
use gpu_v2::system::functional::PLANE_WORDS;

#[test]
fn public_color_copy_restores_both_targets_without_touching_source_depth_or_allocation_tails() {
    let application = r#"mod gpu_api;use gpu_api::*;
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;
 let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});
 let mut commands=gpu.command_buffer();
 let color_a=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let color_b=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let source=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0xb000,2048)});
 let mut display=Display::open();let mut frame:u16=0;
 while frame<3 {
  commands.reset();
  if frame&1==0 {
   {let pass=commands.pass(&color_a,&depth,ColorLoad::clear(Rgb565::from_raw(0)),DepthLoad::clear(8));}
   commands.copy_color(&source,&color_a);
   {let pass=commands.pass(&color_a,&depth,ColorLoad::load(),DepthLoad::clear(0x4567));}
  } else {
   {let pass=commands.pass(&color_b,&depth,ColorLoad::clear(Rgb565::from_raw(0)),DepthLoad::clear(8));}
   commands.copy_color(&source,&color_b);
   {let pass=commands.pass(&color_b,&depth,ColorLoad::load(),DepthLoad::clear(0x4567));}
  }
  gpu.submit(commands);commands=gpu.wait();
  if frame&1==0 {display.set_next_swap(&color_a);} else {display.set_next_swap(&color_b);}
  display.wait_vblank();frame+=1;
 }
 halt(0x600d);
}"#;
    let parsed =
        super::gpu_api_resources::parse(application, "functional-color-copy", false).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    let words = 2048 * 64;
    let visible = 1600 * 64;
    let pattern = (0..words)
        .map(|i| ((i * 37 + i / 256) & 65535) as u16)
        .collect::<Vec<_>>();
    let memory = sim.cpu_mut().physical_memory_mut();
    memory[0xb000 * 64..0xb000 * 64 + words].copy_from_slice(&pattern);
    for first in [0x8000, 0x8800, 0x9000] {
        memory[first * 64..first * 64 + words].fill(0x1d3b);
    }
    let mut halted = false;
    for _ in 0..2_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        if let StepOutcome::Halted { signal } = sim.step().unwrap() {
            assert_eq!(signal, 0x600d, "GPU error: {:?}", sim.gpu().last_error());
            halted = true;
            break;
        }
    }
    assert!(halted, "bounded color-copy timeout");
    assert!(sim.gpu().last_error().is_none());
    let memory = sim.cpu_mut().physical_memory_mut();
    assert_eq!(&memory[0xb000 * 64..0xb000 * 64 + words], &pattern);
    for first in [0x8000, 0x8800] {
        assert_eq!(
            &memory[first * 64..first * 64 + visible],
            &pattern[..visible]
        );
        assert!(memory[first * 64 + visible..first * 64 + words]
            .iter()
            .all(|w| *w == 0x1d3b));
    }
    assert!(memory[0x9000 * 64..0x9000 * 64 + visible]
        .iter()
        .all(|w| *w == 0x4567));
    assert!(memory[0x9000 * 64 + visible..0x9000 * 64 + words]
        .iter()
        .all(|w| *w == 0x1d3b));
}

#[test]
#[ignore = "requires GPU_SIM_COMPARE_REQUEST from a retained shared-frontend result"]
fn captured_submission_matches_independent_package_result_through_thin_device() {
    use cpu_v3_tang_nano_20k::{GpuDevice, GPU_COMMAND_COUNT, GPU_COMMAND_START, GPU_STATUS};
    let request = std::env::var("GPU_SIM_COMPARE_REQUEST").expect("retained comparison request");
    let request: serde_json::Value =
        serde_json::from_slice(&std::fs::read(request).unwrap()).unwrap();
    let before = std::fs::read(request["before"].as_str().unwrap()).unwrap();
    let expected = std::fs::read(request["after"].as_str().unwrap()).unwrap();
    assert_eq!(before.len(), 0x800000);
    assert_eq!(expected.len(), before.len());
    let mut memory: Vec<_> = before
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u16::from_le_bytes(*v))
        .collect();
    let mut device = GpuDevice::functional();
    device.write(
        &mut memory,
        GPU_COMMAND_COUNT,
        u16::try_from(request["count_minus_one"].as_u64().unwrap()).unwrap(),
    );
    device.write(
        &mut memory,
        GPU_COMMAND_START,
        u16::try_from(request["base_bytes"].as_u64().unwrap() / 128).unwrap(),
    );
    assert_eq!(
        device.read(&mut memory, GPU_STATUS),
        0,
        "{:?}",
        device.last_error()
    );
    assert!(device.last_effects().written_bytes.is_some());
    let span = request["effects"]["written_bytes"]
        .as_array()
        .expect("native completion write envelope");
    assert_eq!(
        device.last_effects().written_bytes,
        Some((
            u32::try_from(span[0].as_u64().unwrap()).unwrap(),
            u32::try_from(span[1].as_u64().unwrap()).unwrap(),
        ))
    );
    if let Some(target) = request["target"].as_array() {
        assert_eq!(
            device.last_effects().target,
            Some([
                u32::try_from(target[0].as_u64().unwrap()).unwrap(),
                u32::try_from(target[1].as_u64().unwrap()).unwrap(),
            ])
        );
    }
    for (index, (&actual, bytes)) in memory.iter().zip(expected.as_chunks::<2>().0).enumerate() {
        assert_eq!(
            actual,
            u16::from_le_bytes(*bytes),
            "physical byte {:#x}",
            index * 2
        );
    }
}

#[test]
fn public_ui_and_three_swaps_preserve_raw_bytes_and_display_interpretation() {
    const APPLICATION: &str = r#"mod gpu_api;use gpu_api::*;
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;
 let mut gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});
 let mut commands=gpu.command_buffer();
 let color_a=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let color_b=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,2048)});
 let depth=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let atlas=UiAtlas::from_native(unsafe{GpuMemory::from_sectors(0xa000,512)},255);
 let mut grid=UiGrid::from_memory(unsafe{GpuMemory::from_sectors(0xb100,8)},&atlas);
 grid.fill(255);grid.write(CellRect{x:0,y:0,width:1,height:1},b"\x00");
 let mut display=Display::open_with_format(DISPLAY_FORMAT);
 let mut frame:u16=0;
 while frame<3 {
  commands.reset();
  if frame&1==0 {
   let mut pass=commands.pass(&color_a,&depth,ColorLoad::clear(Rgb565::from_raw(0x1111+frame)),DepthLoad::clear(65535));
   pass.draw_ui(&grid,CellRect{x:0,y:0,width:1,height:1});
  } else {
   let mut pass=commands.pass(&color_b,&depth,ColorLoad::clear(Rgb565::from_raw(0x1111+frame)),DepthLoad::clear(65535));
   pass.draw_ui(&grid,CellRect{x:0,y:0,width:1,height:1});
  }
  gpu.submit(commands);commands=gpu.wait();
  if frame&1==0 {display.set_next_swap(&color_a);} else {display.set_next_swap(&color_b);}
  display.wait_vblank();frame+=1;
 }
 halt(0x600d);
}"#;
    for (format, expected) in [
        ("DisplayFormat::Rgb565", 0x848684),
        ("DisplayFormat::LinearRgb565", 0xbfbfbf),
    ] {
        let application = APPLICATION.replace("DISPLAY_FORMAT", format);
        let parsed = super::gpu_api_resources::parse(
            &application,
            &format!("functional-ui-{expected}"),
            false,
        )
        .unwrap();
        let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        let glyph = gpu_v2::system::vliw_command::ui_char(&[[0x8421; 8]; 16]);
        for (i, pair) in glyph.as_chunks::<2>().0.iter().enumerate() {
            sim.cpu_mut().physical_memory_mut()[0xa000 * 64 + i] = u16::from_le_bytes(*pair);
        }
        let mut halted = false;
        let mut vblanks = 0;
        for _ in 0..5_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
                vblanks += 1;
            }
            if let StepOutcome::Halted { signal } = sim.step().unwrap() {
                assert_eq!(signal, 0x600d, "GPU error: {:?}", sim.gpu().last_error());
                halted = true;
                break;
            }
        }
        if !halted {
            let pc = sim.cpu().pc();
            let function = program
                .debug
                .functions
                .iter()
                .filter(|f| f.addr.0 <= usize::from(pc))
                .max_by_key(|f| f.addr.0);
            panic!("bounded UI/display application timeout: pc={pc:#x} function={:?} display={:?} vblanks={vblanks} gpu_busy={} gpu_error={:?}",function.map(|f| f.name.as_str()),sim.display_state(),sim.gpu().busy(),sim.gpu().last_error());
        }
        assert_eq!(vblanks, 3);
        assert!(sim.gpu().last_error().is_none());
        assert_eq!(sim.display_state().active_base, 0x8000 * 64);
        assert_eq!(sim.cpu_mut().physical_memory_mut()[0x8000 * 64], 0x8430);
        assert_eq!(sim.cpu_mut().physical_memory_mut()[0x8000 * 64 + 8], 0x1113);
        assert_eq!(sim.render_active_framebuffer()[0], expected);
    }
}

#[test]
#[ignore = "same real submission through functional GPU and production RTL; requires Icarus/GOWIN_HOME"]
fn identical_public_clear_submission_matches_production_rtl() {
    use cpu_v3::{Device, Word};
    use cpu_v3_tang_nano_20k::{GpuDevice, GPU_COMMAND_COUNT, GPU_COMMAND_START, GPU_DEVICE};
    use std::{any::Any, path::PathBuf};
    struct CompareGpu {
        inner: GpuDevice,
        count: u16,
        directory: PathBuf,
        submissions: usize,
    }
    impl Device for CompareGpu {
        fn read(&mut self, memory: &mut [Word], channel: u8) -> Word {
            self.inner.read(memory, channel)
        }
        fn write(&mut self, memory: &mut [Word], channel: u8, value: Word) {
            if channel == GPU_COMMAND_COUNT {
                self.count = value;
            }
            if channel == GPU_COMMAND_START {
                let mut rtl_memory = memory.to_vec();
                self.inner.write(memory, channel, value);
                assert!(
                    self.inner.last_error().is_none(),
                    "{:?}",
                    self.inner.last_error()
                );
                super::gpu_api_pixels::execute(&self.directory, &mut rtl_memory, value, self.count);
                assert_eq!(
                    memory, rtl_memory,
                    "functional writes differ from actual RTL writes"
                );
                self.submissions += 1;
            } else {
                self.inner.write(memory, channel, value);
            }
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }
    let directory = PathBuf::from(std::env::var_os("RCC_RUSTC_TEST_DIR").unwrap())
        .join("functional-production-clear");
    super::gpu_api_pixels::compile_clear_fixture(&directory);
    let directory = directory.canonicalize().unwrap();
    let prefix = super::gpu_api_integration::APPLICATION
        .split(" gpu.submit(first);")
        .next()
        .unwrap();
    let application = format!("{prefix}gpu.submit(first);first=gpu.wait();drop(first);drop(second);drop(color);drop(depth);drop(gpu);halt(0x600d);}}");
    let parsed =
        super::gpu_api_resources::parse(&application, "functional-production-clear", false)
            .unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut sim = CpuV3SystemSim::new(VBlankMode::Manual);
    sim.cpu_mut().set_boot_window(&program.words);
    for sector in [0x8000usize, 0x9000] {
        sim.cpu_mut().physical_memory_mut()[sector * 64..(sector + 2048) * 64].fill(0xa55a);
    }
    sim.cpu_mut().attach_device(
        GPU_DEVICE,
        Box::new(CompareGpu {
            inner: GpuDevice::functional(),
            count: 0,
            directory,
            submissions: 0,
        }),
    );
    for _ in 0..3_000_000 {
        if let StepOutcome::Halted { signal } = sim.step().unwrap() {
            assert_eq!(signal, 0x600d);
            let gpu = sim.cpu().device::<CompareGpu>(GPU_DEVICE).unwrap();
            assert_eq!(gpu.submissions, 1);
            assert!(!gpu.inner.busy());
            return;
        }
    }
    panic!("bounded functional/RTL application timeout");
}

#[test]
fn public_submit_wait_replay_writes_actual_shared_memory() {
    for debug in [false, true] {
        let parsed = super::gpu_api_resources::parse(
            super::gpu_api_integration::APPLICATION,
            "functional-clear-replay",
            debug,
        )
        .unwrap();
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = debug;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut sim = CpuV3SystemSim::new(VBlankMode::Manual).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        for sector in [0x8000usize, 0x9000] {
            sim.cpu_mut().physical_memory_mut()[sector * 64..(sector + 2048) * 64].fill(0xa55a);
        }
        let mut halted = false;
        for _ in 0..3_000_000 {
            if let StepOutcome::Halted { signal } = sim.step().unwrap() {
                assert_eq!(signal, 0x600d, "GPU error: {:?}", sim.gpu().last_error());
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded functional application timeout");
        assert!(!sim.gpu().busy());
        assert!(sim.gpu().last_error().is_none());
        for (sector, value) in [(0x8000usize, 0x1357), (0x9000, 0x2468)] {
            let base = sector * 64;
            let memory = sim.cpu_mut().physical_memory_mut();
            assert!(memory[base..base + PLANE_WORDS]
                .iter()
                .all(|&word| word == value));
            assert!(memory[base + PLANE_WORDS..(sector + 2048) * 64]
                .iter()
                .all(|&word| word == 0xa55a));
        }
    }
}
