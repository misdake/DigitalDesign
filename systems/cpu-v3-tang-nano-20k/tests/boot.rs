//! End-to-end CpuV3 two-stage boot: the real rcc Stage0/Stage1/demo programs
//! are compiled with the CpuV3 backend, packed into a boot image with the
//! `cpu-v3-pack` builder, and executed on the `cpu_v3::sim::CpuV3Sim` oracle from
//! reset (CSEG=0, PC=0). Device models attached to the machine's device bus
//! stand in for the boot DMA engine (device 2) and the system-control block
//! (device 0), with the flash image backing the DMA model.

use cpu_v3::rcc_backend::{self, CompilerOptions, CpuV3Program};
use cpu_v3::{decode, CpuV3Sim, FpuOp, FpuUnaryOp, Instruction};
use cpu_v3_tang_nano_20k::boot::{
    BootDmaDevice, BootErrorReport, BootSelectDevice, BootTarget, SystemControlDevice,
    CACHE_MAINTENANCE_STATUS, D_CLEAN_ALL, S1_APPLICATION_LAYOUT, S2_APPLICATION_LAYOUT,
    STAGE1_LAYOUT, SYSTEM_CONTROL_DEVICE,
};
use cpu_v3_tang_nano_20k::{
    DisplayDevice, DISPLAY_CONTROL, DISPLAY_DEVICE, DISPLAY_FRAMEBUFFER_HIGH,
    DISPLAY_FRAMEBUFFER_LOW,
};

fn compile_cpu_v3(file: &str, opts: &CompilerOptions) -> CpuV3Program {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("rcc")
        .join(file);
    let src = std::fs::read_to_string(&path).expect("read rcc source");
    let source_dir = path.parent().expect("rcc source directory");
    let program = rcc::frontend::compile_program_named(
        &path.display().to_string(),
        &src,
        opts,
        &mut |name| {
            let path = if name == "boot_selection" {
                std::path::PathBuf::from(env!("OUT_DIR")).join("boot-selection.generated.rs")
            } else {
                source_dir.join(format!("{name}.rs"))
            };
            std::fs::read_to_string(path).map_err(|error| format!("read module `{name}`: {error}"))
        },
    )
    .expect("rcc compile failed");
    rcc_backend::compile(program, opts, "main")
}

fn assert_canonical_cache_handoff(stage: &str, words: &[u16]) {
    let handoffs = words
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] & 0xfff0 == 0x7800 && pair[1] & 0xff00 == 0x6f00)
        .collect::<Vec<_>>();
    assert_eq!(handoffs.len(), 1, "{stage} must contain one cache handoff");
    let (icache_index, tail) = handoffs[0];
    assert_eq!(
        tail[0] & 0xfff0,
        0x7800,
        "{stage} handoff must issue ICACHE_INVALIDATE_ALL_DELAYED"
    );
    assert_eq!(tail[1] & 0xff00, 0x6f00, "{stage} handoff must issue JSEG");
    assert_eq!(
        tail[0] & 0x000f,
        (tail[1] >> 4) & 0x000f,
        "{stage} invalidate payload must reuse the final CSEG register"
    );
    let dcache_index = words[..icache_index]
        .iter()
        .position(|word| word & 0xfff0 == 0x7810)
        .unwrap_or_else(|| panic!("{stage} must invalidate D-cache before its final handoff"));
    assert!(
        words[dcache_index + 1..icache_index]
            .iter()
            .any(|word| word & 0xff00 == 0x6e00),
        "{stage} must prepare DSEG after D-cache invalidation"
    );
}

#[test]
fn display_demo_exercises_fpu_rounding_and_cpu_framebuffer_stores() {
    let program = compile_cpu_v3("display-demo.rs", &CompilerOptions::default());
    let instructions = program
        .words
        .iter()
        .copied()
        .map(decode)
        .collect::<Vec<_>>();

    for (description, present) in [
        (
            "FSINCOS",
            instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::FpuUnary {
                        op: FpuUnaryOp::SinCos,
                        ..
                    }
                )
            }),
        ),
        (
            "FROUND",
            instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::FpuUnary {
                        op: FpuUnaryOp::Round,
                        ..
                    }
                )
            }),
        ),
        (
            "FMUL",
            instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::Fpu { op: FpuOp::Mul, .. })),
        ),
        (
            "FSTORE integer bridge",
            instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Fpu {
                        op: FpuOp::Store,
                        ..
                    }
                )
            }),
        ),
        (
            "CPU framebuffer store",
            instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::Store { .. })),
        ),
    ] {
        assert!(present, "display demo must contain {description}");
    }
}

#[test]
fn display_swap_waits_for_dcache_clean_before_publishing_the_back_buffer() {
    let program = compile_cpu_v3("display-demo.rs", &CompilerOptions::default());
    let publish = program
        .debug
        .functions
        .iter()
        .find(|function| function.name == "select_next_framebuffer")
        .expect("display demo must retain its framebuffer publish function");
    let start = publish.addr.0 - usize::from(program.code_base);
    let end = publish.addr.1 - usize::from(program.code_base);
    let instructions = program.words[start..end]
        .iter()
        .copied()
        .map(decode)
        .collect::<Vec<_>>();

    let clean = instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::DeviceSend {
                    device: SYSTEM_CONTROL_DEVICE,
                    channel: D_CLEAN_ALL,
                    ..
                }
            )
        })
        .expect("framebuffer publish must start D_CLEAN_ALL");
    assert!(
        matches!(
            instructions.get(clean + 1),
            Some(Instruction::DeviceReceive {
                device: SYSTEM_CONTROL_DEVICE,
                channel: CACHE_MAINTENANCE_STATUS,
                ..
            })
        ),
        "D_CLEAN_ALL must immediately wait for its final maintenance status"
    );

    let display_channels = instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| match instruction {
            Instruction::DeviceSend {
                device: DISPLAY_DEVICE,
                channel,
                ..
            } => Some((index, *channel)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        display_channels
            .iter()
            .map(|(_, channel)| *channel)
            .collect::<Vec<_>>(),
        [
            DISPLAY_FRAMEBUFFER_LOW,
            DISPLAY_FRAMEBUFFER_HIGH,
            DISPLAY_CONTROL
        ],
        "publish must stage low/high addresses and then request NEXT_SWAP"
    );
    assert!(
        display_channels[0].0 > clean + 1,
        "no framebuffer register may be published before clean completes"
    );
}

/// Loads the package generated from the declarative application project and
/// recompiles both boot stages for focused handoff assertions.
fn boot_setup() -> (Vec<u8>, CpuV3Program) {
    let stage0 = compile_cpu_v3("stage0.rs", &CompilerOptions::default());
    let stage1 = compile_cpu_v3(
        "stage1.rs",
        &CompilerOptions {
            code_base: STAGE1_LAYOUT.entry.offset,
            stack_init: STAGE1_LAYOUT.entry.stack_offset,
            ..CompilerOptions::default()
        },
    );
    assert_canonical_cache_handoff("Stage0", &stage0.words);
    assert_canonical_cache_handoff("Stage1", &stage1.words);
    // Stage0 must fit the BSRAM boot window (physical instruction words
    // 0x0000..0x03ff).
    assert!(
        stage0.words.len() < 0x400,
        "stage0 uses {} words; the boot window holds 0x400",
        stage0.words.len()
    );

    let target = BootTarget::TangNano20K;
    let mut flash = vec![0xff; target.flash_bytes() as usize];
    let base = target.payload_flash_offset() as usize;
    let package = include_bytes!(concat!(env!("OUT_DIR"), "/cpu-v3-boot.bin"));
    flash[base..base + package.len()].copy_from_slice(package);
    (flash, stage0)
}

/// Runs the packed image from reset with the device models attached, bounded
/// by `max_steps`.
fn run_boot(
    flash: Vec<u8>,
    stage0: &CpuV3Program,
    boot_selection: u16,
    max_steps: usize,
) -> CpuV3Sim {
    let mut machine = CpuV3Sim::default();
    machine
        .load_physical(S1_APPLICATION_LAYOUT.destination(), &[0xdead])
        .unwrap();
    machine
        .load_physical(S2_APPLICATION_LAYOUT.destination(), &[0xdead])
        .unwrap();
    machine.attach_device(0, Box::<SystemControlDevice>::default());
    machine.attach_device(1, Box::new(BootSelectDevice::new(boot_selection)));
    machine.attach_device(
        2,
        Box::new(BootDmaDevice::new(flash, machine.physical_memory_words())),
    );
    machine.attach_device(DISPLAY_DEVICE, Box::<DisplayDevice>::default());
    // Stage0 executes from the BSRAM boot window: on hardware, instruction
    // fetches from physical words 0x0000..0x03ff read BSRAM while data
    // accesses (descriptor scratch at word 0x40) go to SDRAM.
    machine.set_boot_window(&stage0.words);
    // The success path loops forever in the demo and the failure path
    // retransmits the error frame forever; both end at the step limit.
    machine.run(max_steps).expect("boot chain must not fault");
    machine
}

fn ddht_frame() -> [u8; 8] {
    let mut frame = [0x44, 0x44, 0x48, 0x54, 1, 0x07, 0, 0];
    frame[7] = frame[..7].iter().fold(0, |checksum, byte| checksum ^ byte);
    frame
}

#[test]
fn button_01_boots_the_primary_application_from_flash() {
    let (flash, stage0) = boot_setup();
    let machine = run_boot(flash, &stage0, 0b01, 500_000);

    // The demo application repeats the DDHT 0x07 success frame forever.
    let sysctl = machine.device::<SystemControlDevice>(0).unwrap();
    let frame = ddht_frame();
    assert!(
        sysctl.uart.len() >= frame.len() * 2,
        "expected at least two DDHT frames, got {:02x?}",
        sysctl.uart
    );
    assert_eq!(sysctl.uart[..8], frame);
    assert_eq!(sysctl.uart[8..16], frame);
    // Both stages invalidate both caches before their segment switch.
    assert_eq!(sysctl.icache_invalidations, 2);
    assert_eq!(sysctl.dcache_invalidations, 2);
    // The demo starts its six-LED bounce at the rightmost logical LED. The
    // bounded model run observes this first position before the visual delay.
    assert_eq!(sysctl.led, Some(0b00_0001));

    // The machine reached the application segments.
    assert_eq!(
        machine.code_segment(),
        S1_APPLICATION_LAYOUT.entry.code_segment
    );
    assert_eq!(
        machine.data_segment(),
        S1_APPLICATION_LAYOUT.entry.data_segment
    );
    assert_ne!(
        machine.physical_memory(S1_APPLICATION_LAYOUT.destination()),
        0xdead
    );
    assert_eq!(
        machine.physical_memory(S2_APPLICATION_LAYOUT.destination()),
        0xdead,
        "the unselected display application must not be DMA-loaded"
    );
    // The application prologue set the stack to its --stack-init (0xe000)
    // minus its small frame.
    let sp = machine.register(13).unwrap();
    assert!((0xdfc0..=0xe000).contains(&sp), "sp = {sp:#06x}");
}

#[test]
fn button_10_boots_the_fpu_display_application_from_flash() {
    let (flash, stage0) = boot_setup();
    let machine = run_boot(flash, &stage0, 0b10, 500_000);

    assert_eq!(
        machine.code_segment(),
        S2_APPLICATION_LAYOUT.entry.code_segment
    );
    assert_eq!(
        machine.data_segment(),
        S2_APPLICATION_LAYOUT.entry.data_segment
    );
    assert_eq!(
        machine.physical_memory(S1_APPLICATION_LAYOUT.destination()),
        0xdead,
        "the unselected primary application must not be DMA-loaded"
    );
    assert_ne!(
        machine.physical_memory(S2_APPLICATION_LAYOUT.destination()),
        0xdead
    );
    let sysctl = machine.device::<SystemControlDevice>(0).unwrap();
    assert_eq!(sysctl.icache_invalidations, 2);
    assert_eq!(sysctl.dcache_invalidations, 2);
}

#[test]
fn a_corrupt_descriptor_magic_reports_stage0_category1() {
    let (mut flash, stage0) = boot_setup();
    let base = BootTarget::TangNano20K.payload_flash_offset() as usize;
    flash[base] ^= 1; // break the "CPU3BOOT" magic
    let machine = run_boot(flash, &stage0, 0, 100_000);

    let report = BootErrorReport {
        stage: 1,
        category: 1,
        code: 1,
        detail: 0,
    };
    let sysctl = machine.device::<SystemControlDevice>(0).unwrap();
    assert_eq!(sysctl.led, Some(report.led()));
    let frame = report.uart_frame();
    assert!(
        sysctl.uart.len() >= frame.len() * 2,
        "expected repeating CV3B frames, got {:02x?}",
        sysctl.uart
    );
    assert_eq!(sysctl.uart[..10], frame);
    assert_eq!(sysctl.uart[10..20], frame);

    // Stage0 never left the boot segment.
    assert_eq!(machine.code_segment(), 0);
}

#[test]
fn a_corrupt_manifest_magic_reports_stage1_category2() {
    let (mut flash, stage0) = boot_setup();
    let base = BootTarget::TangNano20K.payload_flash_offset() as usize;
    flash[base + 64] ^= 1; // break the "CPU3SECT" magic
    let machine = run_boot(flash, &stage0, 0, 200_000);

    let report = BootErrorReport {
        stage: 2,
        category: 2,
        code: 6,
        detail: 0,
    };
    let sysctl = machine.device::<SystemControlDevice>(0).unwrap();
    assert_eq!(sysctl.led, Some(report.led()));
    let frame = report.uart_frame();
    assert!(
        sysctl.uart.len() >= frame.len() * 2,
        "expected repeating boot-error frames, got {:02x?}",
        sysctl.uart
    );
    assert_eq!(sysctl.uart[..10], frame);
    assert_eq!(sysctl.uart[10..20], frame);

    // Stage0 handed off successfully, but Stage1 rejected its manifest before
    // entering the application segment.
    assert_eq!(machine.code_segment(), 1);
}
