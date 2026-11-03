use cpu_v3::{device_receive, device_send, halt, load_immediate16};
use cpu_v3_tang_nano_20k::{boot::pack_single_application, system_cycle_emu::System};
#[test]
fn cold_prefix_retains_real_initialization_and_releases_cseg7_cpu() {
    let mut words = load_immediate16(0, 0xc000).to_vec();
    words.push(device_send(0, 7, 0));
    words.push(device_receive(4, 0, 11));
    words.extend(load_immediate16(0, 0x1234));
    words.push(halt());
    let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let package = pack_single_application(&bytes, 2).unwrap();
    let mut system = System::from_preloaded_cold(&package, 0xa55a).unwrap();
    let mut black_pixels = 0;
    let mut halted = false;
    while system.clock().logic_edges < 1000000 {
        let step = system.step().unwrap();
        if step.instant.pixel_rise && step.display.video.de {
            black_pixels += 1;
            assert_eq!(step.display.video.rgb888, [0; 3]);
        }
        if let Some(cpu) = step.cpu {
            assert!(!cpu.fault, "fault {} at {}", cpu.fault_code, cpu.fault_pc);
            if cpu.halted {
                assert_eq!(cpu.halt_signal, 0x1234);
                halted = true;
                break;
            }
        }
    }
    assert!(
        halted,
        "CPU not halted at {} logic cycles",
        system.clock().logic_edges
    );
    let release = system.release.unwrap();
    assert!(release.native_cycles >= 21600);
    assert!(release.system_microseconds > 0);
    assert!(release.logic_edges >= 10800);
    assert_eq!(system.first_fetch(), Some(0x70000));
    assert_eq!(system.observation(0), Some(0xc000));
    assert_eq!(&system.backing()[..2], &[0, 0]);
    assert_eq!(system.backing()[2], 0xa55a);
    assert_eq!(
        system.statistics.accepts[0], 0,
        "virtual black groups must not read SDRAM"
    );
    assert!(
        black_pixels > 0,
        "must release through real pixel-origin ack"
    );
    eprintln!(
        "cold release={release:?} halt_logic={} black_pixels={black_pixels}",
        system.clock().logic_edges
    );
}

fn rcc_image(source: &str, resources: &[cpu_v3_tang_nano_20k::boot::BootResource]) -> Vec<u8> {
    use cpu_v3::rcc_backend::{self, CompilerOptions};
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = cpu_v3_tang_nano_20k::boot::SINGLE_BOOT_STACK;
    let parsed =
        rcc::frontend::compile_program_named("cycle-app.rs", source, &options, &mut |name| {
            Err(format!("unexpected module {name}"))
        })
        .unwrap();
    let statics = parsed.debug.static_data_end;
    let compiled = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let bytes: Vec<_> = compiled
        .words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    cpu_v3_tang_nano_20k::boot::pack_application_resources(&bytes, statics, resources).unwrap()
}

#[test]
fn rcc_cpu_two_submissions_and_presents_share_physical_memory_and_scanout() {
    use cpu_v3_tang_nano_20k::{
        boot::BootResource,
        input_device::{ControllerSnapshot, InputMode},
        rgb565_to_rgb888, FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD, FRAMEBUFFER_SLOT_WORDS,
        GPU_DEPTH_SECTOR, GPU_FRAMEBUFFER_PHYSICAL_SECTORS,
    };
    use gpu_v2::{
        framebuffer::ports::MaterializedSurface,
        system::vliw_command::{self as abi, Command, CommandBuffer, DeviceSubmission},
    };
    let colors = [0x13a5, 0x5678];
    let depths = [0xb628, 0x9876];
    let bases = [FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD];
    let depth = u32::from(GPU_DEPTH_SECTOR) * 64;
    let mut resources = Vec::new();
    let mut submissions = Vec::new();
    for job in 0..2 {
        let object = 0x110000 + job as u32 * 32;
        let cb = 0x100000 + job as u32 * 128;
        let target = abi::target_params(MaterializedSurface {
            color_base_bytes: bases[job] * 2,
            depth_base_bytes: depth * 2,
            width: 400,
            height: 240,
        })
        .unwrap();
        resources.push(BootResource::copy(object as usize, target.to_vec()));
        let mut commands = CommandBuffer::new();
        for c in [
            Command::dma_local(object, 264, 32).unwrap(),
            Command::wait(true, true),
            Command::FLUSH,
            Command::set_target(264).unwrap(),
            Command::clear(false, colors[job]),
            Command::clear(true, depths[job]),
            Command::wait(true, true),
            Command::FLUSH,
            Command::END,
        ] {
            commands.push(c);
        }
        let image = commands.image().unwrap();
        submissions.push(DeviceSubmission::new(cb, image.valid_bytes).unwrap());
        resources.push(BootResource::copy(cb as usize, image.storage));
    }
    let source = format!(
        r#"
use crate::dsl_rt::*;
static SCRATCH: Buf<u16,4096> = Buf::new([0;4096]);
static MIXED: Buf<u16,2> = Buf::new([0x3456,0]);
fn watch(device:u16,channel:u16){{dev_send(0,6,(channel<<3)|device);dev_send(0,7,1);}}
fn submit(count:u16,base:u16){{dev_send(4,0,count);dev_send(4,1,base);}}
fn work(salt:u16){{let mut data=SCRATCH.as_array();let mut i=0u16;while i<4096{{data[i]=i^salt;i+=1;}}dev_send(0,4,0);}}
fn present(high:u16){{dev_send(3,1,0);dev_send(3,2,high);dev_send(3,4,0);dev_send(3,3,1);watch(3,3);}}
fn main(){{
 if MIXED.as_array()[0u16]!=0x3456||MIXED.as_array()[1u16]!=0{{halt(1);}}
 if dev_recv(5,8)!=3{{halt(2);}}dev_send(5,8,0);if dev_recv(5,8)!=1{{halt(3);}}
 submit({count0},{base0});work(0x1357);watch(4,2);present({high0});dev_send(7,1,0xa001);
 submit({count1},{base1});work(0x2468);watch(4,2);present({high1});dev_send(7,1,0xb002);
 if dev_recv(4,2)!=0||dev_recv(3,3)!=0{{halt(4);}}halt(0x5a35);
}}
"#,
        count0 = submissions[0].count_minus_one,
        base0 = submissions[0].base_sector,
        count1 = submissions[1].count_minus_one,
        base1 = submissions[1].base_sector,
        high0 = bases[0] >> 16,
        high1 = bases[1] >> 16
    );
    let package = rcc_image(&source, &resources);
    let artifacts =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/c4/rcc-app");
    std::fs::create_dir_all(&artifacts).unwrap();
    std::fs::write(artifacts.join("application.cpu-v3-boot"), &package).unwrap();
    std::fs::write(artifacts.join("application.rs"), &source).unwrap();
    let mut system = System::from_preloaded_cold(&package, 0xa55a).unwrap();
    system.publish_input(Some(ControllerSnapshot {
        mode: InputMode::Analog,
        buttons_active_low: 0xffff,
        left: [128; 2],
        right: [128; 2],
    }));
    let expected_rgb = colors.map(|c| {
        let (r, g, b) = rgb565_to_rgb888(c, true);
        [r, g, b]
    });
    let mut pixels = [0u64; 2];
    let mut halted = false;
    let mut first_checked = false;
    let started = std::time::Instant::now();
    let mut report = 1_000_000;
    let mut scanout = cpu_v3_tang_nano_20k::system_cycle_emu::scanout::Scanout::default();
    while system.clock().logic_edges < 3_800_000 {
        let mut bus = None;
        let step = system
            .step_observing(|o| {
                bus = o.bus;
            })
            .unwrap();
        if let Some(cpu) = step.cpu {
            assert!(!cpu.fault, "fault {} at {}", cpu.fault_code, cpu.fault_pc);
            if cpu.halted {
                assert_eq!(cpu.halt_signal, 0x5a35);
                halted = true;
            }
        }
        if !first_checked && system.observation(1) == Some(0xa001) {
            assert_eq!(system.statistics.gpu_submits, 1);
            assert_eq!(
                system.statistics.presents_applied,
                1,
                "regs={:x?} stats={:?}",
                (0..5)
                    .map(|c| system.display_register(c))
                    .collect::<Vec<_>>(),
                system.statistics
            );
            let words = usize::from(GPU_FRAMEBUFFER_PHYSICAL_SECTORS) * 64;
            assert!(
                system.backing()[bases[0] as usize..bases[0] as usize + words]
                    .iter()
                    .all(|&v| v == colors[0])
            );
            assert!(system.backing()[depth as usize..depth as usize + words]
                .iter()
                .all(|&v| v == depths[0]));
            first_checked = true;
            eprintln!(
                "short CPU CB/wait/present passed at {} logic cycles",
                system.clock().logic_edges
            );
        }
        if step.instant.pixel_rise {
            if let Some(frame) = scanout.sample(step.display.video).unwrap() {
                if frame.effective_pixels == 800 * 480 {
                    let job = if frame.rgb[..3] == expected_rgb[0] {
                        0
                    } else {
                        1
                    };
                    assert!(
                        frame
                            .rgb
                            .as_chunks::<3>()
                            .0
                            .iter()
                            .all(|p| *p == expected_rgb[job]),
                        "scanout must match the full independently expected raster"
                    );
                    pixels[job] += frame.effective_pixels as u64;
                    let mut ppm = b"P6\n800 480\n255\n".to_vec();
                    ppm.extend_from_slice(&frame.rgb);
                    std::fs::write(artifacts.join(format!("present-{job}.ppm")), ppm).unwrap();
                }
            }
        }
        if let Some(b) = bus {
            if b.index == 7 && b.write && b.channel == 1 {
                assert!(b.data == 0xa001 || b.data == 0xb002);
            }
        }
        if system.clock().logic_edges >= report {
            eprintln!(
                "APP logic={} presents={} pixels={pixels:?} host={:?}",
                system.clock().logic_edges,
                system.statistics.presents_applied,
                started.elapsed()
            );
            report += 1_000_000;
        }
        if halted && pixels.iter().all(|&n| n >= 800 * 480) {
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(600),
            "host watchdog"
        );
    }
    assert!(
        first_checked && halted && pixels.iter().all(|&n| n >= 800 * 480),
        "bounded APP incomplete: halted={halted} pixels={pixels:?} stats={:?}",
        system.statistics
    );
    assert_eq!(system.core().architectural_gprs()[13], 0xc000);
    assert_eq!(system.core().data_segments(), [0, 1, 2, 3]);
    assert_eq!(system.observation(1), Some(0xb002));
    assert_eq!(system.statistics.gpu_submits, 2);
    assert_eq!(system.statistics.gpu_completed, 2);
    assert_eq!(system.statistics.present_requests, 2);
    assert_eq!(system.statistics.presents_applied, 2);
    assert_eq!(
        system.statistics.accepts, system.statistics.terminal_responses,
        "every physical descriptor must have exactly one owner terminal"
    );
    assert_eq!(system.statistics.accepts[4], 4);
    assert_eq!(system.statistics.response_beats[4], 40);
    assert_eq!(system.statistics.accepts[6], 6400);
    assert_eq!(system.statistics.write_beats[6], 102400);
    assert_eq!(system.statistics.accepts[0], 12000);
    assert_eq!(system.statistics.response_beats[0], 48000);
    assert!(
        system.statistics.three_way_pending > 0,
        "real CPU+GPU+Display must overlap"
    );
    let words = usize::from(GPU_FRAMEBUFFER_PHYSICAL_SECTORS) * 64;
    for (job, base) in bases.into_iter().enumerate() {
        let base = base as usize;
        assert!(system.backing()[base..base + words]
            .iter()
            .all(|&v| v == colors[job]));
        assert_eq!(system.backing()[base - 1], 0xa55a);
        assert!(
            system.backing()[base + words..base + FRAMEBUFFER_SLOT_WORDS as usize]
                .iter()
                .all(|&v| v == 0xa55a)
        );
    }
    assert!(system.backing()[depth as usize..depth as usize + words]
        .iter()
        .all(|&v| v == depths[1]));
    assert_eq!(system.backing()[depth as usize + words], 0xa55a);
    for (i, &word) in system.backing()[..4096].iter().enumerate() {
        assert_eq!(word, (i as u16) ^ 0x2468);
    }
    eprintln!(
        "two-present APP final logic={} native={} pixels={pixels:?} stats={:?} host={:?}",
        system.clock().logic_edges,
        system.native_cycles(),
        system.statistics,
        started.elapsed()
    );
}
