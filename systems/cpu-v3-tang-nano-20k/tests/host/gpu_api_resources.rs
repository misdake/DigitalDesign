//! Actual approved recording facade, without a substitute Gpu/CB/Pass fixture.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

struct IdleGpu;
impl Device for IdleGpu {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        assert_eq!(channel, 2);
        0
    }
    fn write(&mut self, _: &mut [u16], _: u8, _: u16) {
        panic!("Recording must not SEND");
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
struct FaultAfterInit(usize);
impl Device for FaultAfterInit {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        assert_eq!(channel, 2);
        self.0 += 1;
        if self.0 == 1 {
            0
        } else {
            2
        }
    }
    fn write(&mut self, _: &mut [u16], _: u8, _: u16) {
        panic!("fault path must not SEND");
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
pub(super) fn parse(
    source: &str,
    label: &str,
    enabled: bool,
) -> Result<rcc::frontend::Program, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = enabled;
    let directory = output.join(format!("resources-{label}-{enabled}"));
    let local = directory.join("application");
    std::fs::create_dir_all(&local).map_err(|error| error.to_string())?;
    std::fs::write(local.join("app.rs"), source).map_err(|error| error.to_string())?;
    use super::shared_build_support_rcc_modules as resolver;
    let configuration = resolver::RendererCaptureConfiguration::with_default_hook(
        resolver::RendererLayoutProfile::Column16,
    );
    resolver::compile_explicit_gpu_program_checked(
        "public-recording",
        source,
        &options,
        &local,
        &root,
        "",
        &configuration,
        &RustcCheck::new(directory),
        &mut |_| {},
    )
}
#[test]
fn approved_root_value_and_resource_factories_compile() {
    for (label, body) in [
        ("rgb", "let value:Rgb565=Rgb565::from_raw(0x1234);halt(value.raw);"),
        ("memory", "let value:GpuMemory=unsafe{GpuMemory::from_sectors(0x9800,512)};halt(value.first_sector());"),
        ("command-memory", "let value:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};halt(value.commands.first_sector());"),
        ("optional-command-memory", "let absent:Option<CommandMemory>=None;halt(0);"),
        ("config", "let value:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};let absent:Option<CommandMemory>=None;let config:GpuConfig=GpuConfig{object_arena_bytes:8192,command_a:value,command_b:absent};halt(config.object_arena_bytes);"),
        ("gpu", "let value:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};let absent:Option<CommandMemory>=None;let config:GpuConfig=GpuConfig{object_arena_bytes:8192,command_a:value,command_b:absent};let gpu:Gpu=Gpu::init(config);drop(gpu);halt(0);"),
    ] {
        let source = format!("mod gpu_api;use gpu_api::*;fn main(){{{body}}}");
        parse(&source, label, false).unwrap_or_else(|error| panic!("{label}: {error}"));
    }
}
#[test]
fn public_resources_snapshot_targets_across_drop_reuse_and_reset() {
    let source = r#"mod gpu_api;use gpu_api::*;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let b:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0xfc00,512)},push:unsafe{GpuMemory::from_sectors(0xfe00,512)}};
 let config:GpuConfig=GpuConfig{object_arena_bytes:8192,command_a:a,command_b:Some(b)};
 let mut gpu:Gpu=Gpu::init(config);let mut first:CommandBuffer=gpu.command_buffer();let second:CommandBuffer=gpu.command_buffer();
 let color:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth:DepthBuffer=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 {let pass:Pass=first.pass(&color,&depth,ColorLoad::clear(Rgb565::from_raw(0x1234)),DepthLoad::load());
 drop(color);drop(depth);
 let replacement:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,2048)});drop(replacement);}
 check(first.command_words()==16&&first.push_words()==16);first.invalidate_textures();check(first.command_words()==20);
 first.reset();check(first.command_words()==0&&first.push_words()==0);
 drop(first);drop(second);drop(gpu);halt(0x600d);}"#;
    for enabled in [false, true] {
        let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        let parsed = parse(source, "snapshot", enabled).unwrap_or_else(|error| panic!("{error}"));
        assert!(parsed.debug.static_data_end <= 8192);
        let mut options = options;
        options.gpu_debug = enabled;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(4, Box::new(IdleGpu));
        let mut halted = false;
        for _ in 0..500_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded public recording timeout");
        let memory = cpu.physical_memory_mut();
        let push = 0x9a00 * 64;
        assert_eq!(&memory[push..push + 2], &[0x2000, 0x0900]);
        assert_eq!(
            &memory[0x9800 * 64..0x9800 * 64 + 20],
            &[
                3, 0x4000, 0, 0x7000, 0x228, 0x19a0, 1, 0x4000, 69, 0xa000, 0x1234, 0x8000, 3,
                0x4000, 0, 0x7000, 3, 0x4000, 0, 0x9000
            ]
        );
    }
}
#[test]
fn application_cannot_bypass_the_approved_resource_root() {
    let forged = r#"mod gpu_api;fn main(){let gpu:Gpu=Gpu{identity:1};halt(0);}"#;
    let private = r#"mod gpu_api;fn main(){let slot=gpu_config_take_buffer();halt(slot);}"#;
    let missing_unsafe = r#"mod gpu_api;fn main(){let memory:GpuMemory=GpuMemory::from_sectors(0x9800,512);halt(0);}"#;
    for (label, source, diagnostic) in [
        ("forged", forged, "private"),
        ("private", private, "private"),
        ("missing-unsafe", missing_unsafe, "unsafe"),
    ] {
        let error = parse(source, label, false)
            .err()
            .unwrap_or_else(|| panic!("accepted {label}"));
        assert!(error.to_string().contains(diagnostic), "{label}: {error}");
    }
}

#[test]
fn required_memory_registration_rejects_private_field_construction() {
    let source = r#"mod gpu_api;fn main(){let memory:GpuMemory=GpuMemory{first_sector:0x9800,sector_count:512};halt(0);}"#;
    let error = parse(source, "forged-memory", false)
        .err()
        .unwrap_or_else(|| panic!("application forged GpuMemory without unsafe registration"));
    assert!(error.to_string().contains("private"), "{error}");
}

#[test]
fn public_constructors_fail_stop_before_gpu_send_in_both_profiles() {
    use super::gpu_api_fault::{Peer, Uart};
    for enabled in [false, true] {
        for (label, body, expected) in [
            ("extent-overflow", "let memory:GpuMemory=unsafe{GpuMemory::from_sectors(65535,2)};", [1, 1, 65535, 2]),
            ("extent-empty", "let memory:GpuMemory=unsafe{GpuMemory::from_sectors(0,0)};", [1, 1, 0, 0]),
            ("config-overlap", "let memory:GpuMemory=unsafe{GpuMemory::from_sectors(0x9800,512)};let a:CommandMemory=CommandMemory{commands:memory,push:memory};let absent:Option<CommandMemory>=None;let gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});", [2, 1, 0, 0]),
        ] {
            let source = format!("mod gpu_api;use gpu_api::*;fn main(){{{body}halt(0xbad);}}");
            let parsed = parse(&source, label, enabled).unwrap_or_else(|error| panic!("{error}"));
            let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
            options.gpu_debug = enabled;
            let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
            let shared = Rc::new(RefCell::new(Uart::default()));
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&program.words);
            cpu.attach_device(0, Box::new(Peer(shared.clone())));
            cpu.attach_device(4, Box::new(IdleGpu));
            let mut halted = false;
            for _ in 0..500_000 {
                if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                    assert_eq!(signal, 0xe001, "{label}/{enabled}");
                    halted = true;
                    break;
                }
            }
            assert!(halted, "bounded public fault timeout: {label}/{enabled}");
            let uart = shared.borrow();
            assert_eq!(uart.busy, 0);
            assert_eq!(uart.bytes.len(), 32);
            let words = uart.bytes.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect::<Vec<_>>();
            assert_eq!(words.len(),16);
            assert_eq!(words[0],0xfa17);
            assert_eq!(&words[1..5], &expected);
            assert_eq!(words[5],3);
            assert!(words[6..].iter().all(|&word| word==0));
        }
    }
}

#[test]
fn public_display_snapshots_color_and_waits_for_actual_retirement() {
    use super::gpu_api_present::{peer_state, Peer};
    let source = r#"mod gpu_api;use gpu_api::*;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;
 let gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});
 let color:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,2048)});
 let mut display:Display=Display::open();let before=display.frame_index();check(before==65535);
 display.set_next_swap(&color);drop(color);
 let replacement:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});drop(replacement);
 display.wait_vblank();check(display.frame_index()!=before);display.wait_vblank();drop(display);drop(gpu);halt(0x600d);
}"#;
    for enabled in [false, true] {
        let parsed =
            parse(source, "public-display", enabled).unwrap_or_else(|error| panic!("{error}"));
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let shared = peer_state(false);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(
            3,
            Box::new(Peer {
                kind: 3,
                state: shared.clone(),
            }),
        );
        cpu.attach_device(4, Box::new(IdleGpu));
        let mut halted = false;
        for _ in 0..500_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d, "public Display/{enabled}");
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded public Display timeout/{enabled}");
        assert_eq!(
            shared.borrow().events,
            vec![(1, 0), (2, 0x22), (4, 0), (3, 1)]
        );
    }
}

#[test]
fn public_reinitialization_rebuilds_only_after_old_owners_exit() {
    let source = r#"mod gpu_api;use gpu_api::*;
fn record(mut gpu:Gpu,code:u16){
 let mut cb:CommandBuffer=gpu.command_buffer();
 let color:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth:DepthBuffer=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 {let pass:Pass=cb.pass(&color,&depth,ColorLoad::clear(Rgb565::from_raw(code)),DepthLoad::load());}
 drop(color);drop(depth);drop(cb);drop(gpu);
}
fn main(){
 let first:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;
 let old_gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:first,command_b:absent});record(old_gpu,0x1234);
 let next:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0xa000,512)},push:unsafe{GpuMemory::from_sectors(0xa200,512)}};
 let absent:Option<CommandMemory>=None;
 let new_gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:4096,command_a:next,command_b:absent});record(new_gpu,0xabcd);halt(0x600d);
}"#;
    for enabled in [false, true] {
        let parsed =
            parse(source, "public-reinit", enabled).unwrap_or_else(|error| panic!("{error}"));
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(4, Box::new(IdleGpu));
        let mut halted = false;
        for _ in 0..500_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded public reinit/{enabled}");
        let memory = cpu.physical_memory_mut();
        assert_eq!(memory[0x9800 * 64 + 10], 0x1234);
        assert_eq!(memory[0xa000 * 64 + 10], 0xabcd);
        assert_eq!(&memory[0xa200 * 64..0xa200 * 64 + 2], &[0x2000, 0x0900]);
    }
}

#[test]
fn public_capacity_failure_precedes_the_failing_pass_writes() {
    use super::gpu_api_fault::{Peer, Uart};
    let source = r#"mod gpu_api;use gpu_api::*;
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,1)},push:unsafe{GpuMemory::from_sectors(0x9a00,1)}};
 let absent:Option<CommandMemory>=None;
 let mut gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});
 let mut cb:CommandBuffer=gpu.command_buffer();
 let color:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 let depth:DepthBuffer=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,2048)});
 let mut i:u16=0;while i<4{let pass:Pass=cb.pass(&color,&depth,ColorLoad::clear(Rgb565::from_raw(0x1234+i)),DepthLoad::load());i+=1;}
 halt(0xbad);
}"#;
    for enabled in [false, true] {
        let parsed =
            parse(source, "public-capacity", enabled).unwrap_or_else(|error| panic!("{error}"));
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let uart = Rc::new(RefCell::new(Uart::default()));
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(4, Box::new(IdleGpu));
        cpu.attach_device(0, Box::new(Peer(uart.clone())));
        cpu.physical_memory_mut()[0x9800 * 64..0x9800 * 64 + 64].fill(0xa55a);
        cpu.physical_memory_mut()[0x9a00 * 64..0x9a00 * 64 + 64].fill(0x5aa5);
        let mut halted = false;
        for _ in 0..500_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0xe001);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded public capacity/{enabled}");
        let memory = cpu.physical_memory_mut();
        for i in 0..3 {
            assert_eq!(memory[0x9800 * 64 + 16 * i + 10], 0x1234 + i as u16);
        }
        assert!(memory[0x9800 * 64 + 48..0x9800 * 64 + 64]
            .iter()
            .all(|&word| word == 0xa55a));
        assert!(memory[0x9a00 * 64 + 48..0x9a00 * 64 + 64]
            .iter()
            .all(|&word| word == 0x5aa5));
        let uart = uart.borrow();
        assert_eq!(uart.busy, 0);
        assert_eq!(&uart.bytes[..6], &[0x17, 0xfa, 4, 0, 2, 0]);
    }
}

#[test]
fn public_mesh_subsets_are_independent_after_parent_drop_and_reuse() {
    let source = r#"mod gpu_api;use gpu_api::*;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;
 let gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});
 let memory:GpuMemory=unsafe{GpuMemory::from_sectors(0xc000,256)};
 let desc:MeshDesc=MeshDesc{directory_sector:0xc003,directory_word:63,meshlet_count:65,storage:memory};
 let base:Mesh=Mesh::from_native(desc);let first:Mesh=base.subset(1,64);let tail:Mesh=first.subset(63,1);
 let empty:Mesh=base.subset(65,0);let one:Mesh=base.subset(0,1);let two:Mesh=base.subset(1,2);
 check(base.meshlet_count()==65&&first.meshlet_count()==64&&tail.meshlet_count()==1);
 check(empty.meshlet_count()==0&&one.meshlet_count()==1&&two.meshlet_count()==2);
 drop(base);let replacement:Mesh=Mesh::from_native(MeshDesc{directory_sector:0xc000,directory_word:0,meshlet_count:2,storage:memory});
 check(replacement.meshlet_count()==2&&first.meshlet_count()==64&&tail.meshlet_count()==1&&empty.meshlet_count()==0);
 drop(replacement);drop(first);check(tail.meshlet_count()==1);drop(tail);drop(empty);drop(one);drop(two);drop(gpu);halt(0x600d);
}"#;
    for enabled in [false, true] {
        let parsed =
            parse(source, "public-mesh", enabled).unwrap_or_else(|error| panic!("{error}"));
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(4, Box::new(IdleGpu));
        cpu.physical_memory_mut()[0xc000 * 64..0xc000 * 64 + 256 * 64].fill(0x5eed);
        let mut halted = false;
        for _ in 0..500_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded public Mesh/{enabled}");
        assert!(
            cpu.physical_memory_mut()[0xc000 * 64..0xc000 * 64 + 256 * 64]
                .iter()
                .all(|&word| word == 0x5eed)
        );
    }
}

#[test]
fn public_passes_snapshot_all_target_allocations_independently_of_descriptors() {
    let source = r#"mod gpu_api;use gpu_api::*;
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;
 let mut gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});let mut cb:CommandBuffer=gpu.command_buffer();
 let depth:DepthBuffer=DepthBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x9000,1600)});
 let color_a:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 {let pass:Pass=cb.pass(&color_a,&depth,ColorLoad::clear(Rgb565::from_raw(0x1234)),DepthLoad::load());}drop(color_a);
 let color_b:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8800,1600)});
 {let pass:Pass=cb.pass(&color_b,&depth,ColorLoad::load(),DepthLoad::clear(0xffff));}drop(color_b);
 let again:ColorBuffer=ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)});
 {let pass:Pass=cb.pass(&again,&depth,ColorLoad::load(),DepthLoad::load());}drop(again);drop(depth);drop(cb);drop(gpu);halt(0x600d);
}"#;
    for enabled in [false, true] {
        let parsed =
            parse(source, "target-facts", enabled).unwrap_or_else(|error| panic!("{error}"));
        let address = |name: &str| match parsed
            .debug
            .globals
            .iter()
            .find(|g| g.name == name)
            .unwrap()
            .loc
        {
            rcc::VarLoc::Global(address) => usize::from(address),
            _ => panic!("not a static {name}"),
        };
        let facts = address("GPU_CB_CAPTURE_FACTS");
        let records = address("GPU_CB_RECORDS");
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        cpu.attach_device(4, Box::new(IdleGpu));
        let mut halted = false;
        for _ in 0..500_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded public target facts/{enabled}");
        let memory = cpu.physical_memory_mut();
        assert_eq!(memory[records + 15], 3);
        assert_eq!(memory[records + 208 + 15], 0);
        assert_eq!(
            &memory[facts..facts + 12],
            &[0x8000, 2048, 5, 3, 0x9000, 1600, 6, 3, 0x8800, 1600, 5, 3]
        );
        assert_eq!(memory[records + 9], 0x8000);
        assert_eq!(memory[records + 10], 0x9000);
    }
}

#[test]
fn public_waits_distinguish_none_no_active_wait_and_device_fault() {
    use super::gpu_api_fault::{Peer, Uart};
    let source = r#"mod gpu_api;use gpu_api::*;
fn main(){
 let a:CommandMemory=CommandMemory{commands:unsafe{GpuMemory::from_sectors(0x9800,512)},push:unsafe{GpuMemory::from_sectors(0x9a00,512)}};
 let absent:Option<CommandMemory>=None;
 let mut gpu:Gpu=Gpu::init(GpuConfig{object_arena_bytes:8192,command_a:a,command_b:absent});
 let first:Option<CommandBuffer>=gpu.try_wait();if let Some(buffer)=first{drop(buffer);halt(0xbad);}
 let second:Option<CommandBuffer>=gpu.try_wait();if let Some(buffer)=second{drop(buffer);halt(0xbad);}
 let buffer:CommandBuffer=gpu.wait();drop(buffer);halt(0xbad);
}"#;
    for (enabled, fault) in [(false, false), (true, false), (false, true), (true, true)] {
        let parsed = parse(source, &format!("public-wait-empty-{fault}"), enabled)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let uart = Rc::new(RefCell::new(Uart::default()));
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let gpu: Box<dyn Device> = if fault {
            Box::new(FaultAfterInit(0))
        } else {
            Box::new(IdleGpu)
        };
        cpu.attach_device(4, gpu);
        cpu.attach_device(0, Box::new(Peer(uart.clone())));
        let mut halted = false;
        for _ in 0..500_000 {
            if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                assert_eq!(signal, 0xe001);
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded public empty wait/{enabled}");
        let uart = uart.borrow();
        assert_eq!(uart.busy, 0);
        let expected = if fault {
            [0x17, 0xfa, 6, 0, 3, 0, 3, 0, 0, 0]
        } else {
            [0x17, 0xfa, 6, 0, 4, 0, 1, 0, 0, 0]
        };
        assert_eq!(&uart.bytes[..10], &expected);
    }
}
