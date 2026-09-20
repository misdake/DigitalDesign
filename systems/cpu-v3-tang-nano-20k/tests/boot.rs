//! End-to-end CpuV3 single-stage boot: the real rcc Stage0 and demo programs
//! are compiled with the CpuV3 backend, packed into a boot image with the
//! `cpu-v3-pack` builder, and executed on the `cpu_v3::sim::CpuV3Sim` oracle from
//! reset (CSEG=0, PC=0). Device models attached to the machine's device bus
//! stand in for the boot DMA engine (device 2) and the system-control block
//! (device 0), with the flash image backing the DMA model.

use cpu_v3::rcc_backend::{self, CompilerOptions, CpuV3Program};
use cpu_v3::{decode_fpu_pair, CpuV3Sim, FpuScalarSubop, FpuSinCosMode, Instruction};
use cpu_v3_tang_nano_20k::boot::{
    BootDmaDevice, BootErrorReport, BootSelectDevice, BootTarget, SystemControlDevice,
    S1_APPLICATION_LAYOUT, S2_APPLICATION_LAYOUT,
};
use cpu_v3_tang_nano_20k::{DisplayDevice, DISPLAY_DEVICE};

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

/// C3 restores the Q16.16 FPU display demo in the S2 slot: it must compile
/// (no software trig table) and emit the special-function subops it relies on.
#[test]
fn c3_display_demo_lowers_to_fpu_special_subops() {
    let program = compile_cpu_v3("display-demo.rs", &CompilerOptions::default());

    let mut rcp = 0usize;
    let mut rsqrt = 0usize;
    let mut sincos_single = 0usize;
    let mut sincos_dual = 0usize;
    let mut index = 0;
    while index + 1 < program.words.len() {
        if let Instruction::FpuScalar { subop, mode, .. } =
            decode_fpu_pair(program.words[index], program.words[index + 1])
        {
            match subop {
                FpuScalarSubop::Rcp => rcp += 1,
                FpuScalarSubop::Rsqrt => rsqrt += 1,
                FpuScalarSubop::SinCos => match FpuSinCosMode::from_mode(mode) {
                    Some(FpuSinCosMode::SinCos) => sincos_dual += 1,
                    Some(_) => sincos_single += 1,
                    None => panic!("display-demo emitted a reserved SINCOS mode"),
                },
                _ => {}
            }
        }
        index += 1;
    }
    assert!(
        sincos_dual > 0,
        "the display demo must emit the dual-output SINCOS"
    );
    assert_eq!(
        (rcp, rsqrt, sincos_single),
        (0, 0, 0),
        "the demo uses only dual-output SINCOS"
    );
}

/// Loads the package generated from the declarative application project and
/// recompiles the single boot stage for focused handoff assertions.
fn boot_setup() -> (Vec<u8>, CpuV3Program) {
    let stage0 = compile_cpu_v3("stage0.rs", &CompilerOptions::default());
    assert_canonical_cache_handoff("Stage0", &stage0.words);
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

fn ddht_frame_with_test_id(test_id: u8) -> [u8; 8] {
    let mut frame = [0x44, 0x44, 0x48, 0x54, 1, test_id, 0, 0];
    frame[7] = frame[..7].iter().fold(0, |checksum, byte| checksum ^ byte);
    frame
}

fn ddht_frame() -> [u8; 8] {
    ddht_frame_with_test_id(0x07)
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
    // The single first stage invalidates both caches once before its segment
    // switch.
    assert_eq!(sysctl.icache_invalidations, 1);
    assert_eq!(sysctl.dcache_invalidations, 1);
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

/// The default S2 boot runs the restored Q16.16 FPU display demo
/// (`rcc/display-demo.rs`). Within the bounded run the demo reports its DDHT
/// `0x0b` frame before the first frame is filled; the fill then keeps it busy
/// in framebuffer segments, so only the early observable effects are asserted.
#[test]
fn button_10_boots_the_restored_display_demo_from_flash() {
    let (flash, stage0) = boot_setup();
    let machine = run_boot(flash, &stage0, 0b10, 500_000);

    assert_eq!(
        machine.code_segment(),
        S2_APPLICATION_LAYOUT.entry.code_segment
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
    // Stage0 invalidates I-cache once during the handoff; the demo reports its
    // DDHT 0x0b display frame before it starts filling the framebuffers.
    assert_eq!(sysctl.icache_invalidations, 1);
    let frame = ddht_frame_with_test_id(0x0b);
    assert!(
        sysctl.uart.len() >= frame.len(),
        "expected a display-demo DDHT frame, got {:02x?}",
        sysctl.uart
    );
    assert_eq!(sysctl.uart[..frame.len()], frame);
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
fn a_corrupt_manifest_magic_reports_boot_category2() {
    let (mut flash, stage0) = boot_setup();
    let base = BootTarget::TangNano20K.payload_flash_offset() as usize;
    flash[base + 64] ^= 1; // break the "CPU3SECT" magic
    let machine = run_boot(flash, &stage0, 0, 200_000);

    let report = BootErrorReport {
        stage: 1,
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

    // The single first stage rejected its manifest before entering the
    // application segment.
    assert_eq!(machine.code_segment(), 0);
}

#[test]
fn an_oversized_manifest_section_count_is_rejected_before_the_section_loop() {
    let (mut flash, stage0) = boot_setup();
    let base = BootTarget::TangNano20K.payload_flash_offset() as usize;
    // Shrink the descriptor's manifest size to 48 bytes and set the manifest
    // count to 2048, whose `count << 5` wraps to zero in 16-bit arithmetic.
    // Without the explicit count bound this satisfies the size equation and the
    // section loop reads past the 192-word manifest buffer.
    flash[base + 48] = 48;
    flash[base + 78] = 0x00;
    flash[base + 79] = 0x08;
    let machine = run_boot(flash, &stage0, 0, 100_000);

    let report = BootErrorReport {
        stage: 1,
        category: 2,
        code: 6,
        detail: 0x0800,
    };
    let sysctl = machine.device::<SystemControlDevice>(0).unwrap();
    assert_eq!(sysctl.led, Some(report.led()));
    assert_eq!(sysctl.uart[..10], report.uart_frame());
    assert_eq!(machine.code_segment(), 0);
}
