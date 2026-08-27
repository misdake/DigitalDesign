//! End-to-end CpuV3 two-stage boot: the real rcc Stage0/Stage1/demo programs
//! are compiled with the CpuV3 backend, packed into a boot image with the
//! `cpu-v3-pack` builder, and executed on the `cpu_v3::sim::Machine` oracle from
//! reset (CSEG=0, PC=0). Device models attached to the machine's device bus
//! stand in for the boot DMA engine (device 2) and the system-control block
//! (device 0), with the flash image backing the DMA model.

use cpu_v3::rcc_backend::{self, CompilerOptions, CpuV3Program};
use cpu_v3::{Machine, PhysicalWordAddress};
use cpu_v3_tang_nano_20k::boot::{
    build_boot_image, BootDmaDevice, BootEntry, BootErrorReport, BootImageSpec, BootSelectDevice,
    BootTarget, InputSection, SectionKind, SystemControlDevice, SECTION_EXECUTE, SECTION_READ,
    SECTION_WRITE,
};

fn compile_cpu_v3(file: &str, opts: &CompilerOptions) -> CpuV3Program {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("rcc")
        .join(file);
    let src = std::fs::read_to_string(&path).expect("read rcc source");
    let program = rcc::frontend::compile_program_named(
        &path.display().to_string(),
        &src,
        opts,
        &mut |name| Err(format!("unknown module `{name}`")),
    )
    .expect("rcc compile failed");
    rcc_backend::compile(program, opts, "main")
}

fn words_bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn section(
    name: &str,
    destination: u32,
    data: Vec<u8>,
    memory_size_bytes: u32,
    flags: u16,
) -> InputSection {
    InputSection {
        name: name.into(),
        kind: SectionKind::Load,
        flags,
        destination: PhysicalWordAddress::new(destination),
        data,
        memory_size_bytes,
        alignment_bytes: 32,
    }
}

/// Compiles both boot stages and both applications into the flash image.
/// Returns the flash bytes and the Stage0 image (linked at code base 0).
fn boot_setup() -> (Vec<u8>, CpuV3Program) {
    let stage0 = compile_cpu_v3("stage0.rs", &CompilerOptions::default());
    let stage1 = compile_cpu_v3(
        "stage1.rs",
        &CompilerOptions {
            code_base: 0x0100,
            stack_init: 0xf000,
            ..CompilerOptions::default()
        },
    );
    let application = compile_cpu_v3(
        "boot-demo.rs",
        &CompilerOptions {
            code_base: 0x0200,
            stack_init: 0xe000,
            ..CompilerOptions::default()
        },
    );
    let alternate_application = compile_cpu_v3(
        "boot-alt.rs",
        &CompilerOptions {
            code_base: 0x0200,
            stack_init: 0xe000,
            ..CompilerOptions::default()
        },
    );
    // Stage0 must fit the BSRAM boot window (physical instruction words
    // 0x0000..0x03ff).
    assert!(
        stage0.words.len() < 0x400,
        "stage0 uses {} words; the boot window holds 0x400",
        stage0.words.len()
    );

    let stage1_bytes = words_bytes(&stage1.words);
    let application_bytes = words_bytes(&application.words);
    let alternate_application_bytes = words_bytes(&alternate_application.words);
    let image = build_boot_image(BootImageSpec {
        target: BootTarget::TangNano20K,
        stage1_section: "stage1".into(),
        stage1_entry: BootEntry {
            code_segment: 1,
            offset: 0x0100,
            data_segment: 2,
            stack_offset: 0xf000,
        },
        application_entry: BootEntry {
            code_segment: 3,
            offset: 0x0200,
            data_segment: 4,
            stack_offset: 0xe000,
        },
        sections: vec![
            InputSection {
                name: "stage1".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_EXECUTE,
                destination: PhysicalWordAddress::new(0x0001_0100),
                memory_size_bytes: stage1_bytes.len() as u32,
                data: stage1_bytes,
                alignment_bytes: 32,
            },
            InputSection {
                name: "application".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_EXECUTE,
                destination: PhysicalWordAddress::new(0x0003_0200),
                memory_size_bytes: application_bytes.len() as u32,
                data: application_bytes,
                alignment_bytes: 32,
            },
            InputSection {
                name: "application-alt".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_EXECUTE,
                destination: PhysicalWordAddress::new(0x0005_0200),
                memory_size_bytes: alternate_application_bytes.len() as u32,
                data: alternate_application_bytes,
                alignment_bytes: 32,
            },
            section(
                "data",
                0x0004_0000,
                vec![0xef, 0xbe, 0x55],
                8,
                SECTION_READ | SECTION_WRITE,
            ),
            InputSection {
                name: "bss".into(),
                kind: SectionKind::Zero,
                flags: SECTION_READ | SECTION_WRITE,
                destination: PhysicalWordAddress::new(0x0004_0100),
                data: vec![],
                memory_size_bytes: 64,
                alignment_bytes: 32,
            },
        ],
    })
    .expect("boot image builds");

    let target = BootTarget::TangNano20K;
    let mut flash = vec![0xff; target.flash_bytes() as usize];
    let base = target.payload_flash_offset() as usize;
    flash[base..base + image.bytes.len()].copy_from_slice(&image.bytes);
    (flash, stage0)
}

/// Runs the packed image from reset with the device models attached, bounded
/// by `max_steps`. The BSS range is dirtied first so the test proves the
/// Zero section DMA actually clears it.
fn run_boot(
    flash: Vec<u8>,
    stage0: &CpuV3Program,
    boot_selection: u16,
    max_steps: usize,
) -> Machine {
    let mut machine = Machine::default();
    machine
        .load_physical(PhysicalWordAddress::new(0x0004_0100), &[0xffff; 32])
        .unwrap();
    machine
        .load_physical(PhysicalWordAddress::new(0x0003_0200), &[0xdead])
        .unwrap();
    machine
        .load_physical(PhysicalWordAddress::new(0x0005_0200), &[0xdead])
        .unwrap();
    machine.attach_device(0, Box::<SystemControlDevice>::default());
    machine.attach_device(1, Box::new(BootSelectDevice::new(boot_selection)));
    machine.attach_device(
        2,
        Box::new(BootDmaDevice::new(flash, machine.physical_memory_words())),
    );
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
    assert_eq!(machine.code_segment(), 3);
    assert_eq!(machine.data_segment(), 4);
    assert_ne!(
        machine.physical_memory(PhysicalWordAddress::new(0x0003_0200)),
        0xdead
    );
    assert_eq!(
        machine.physical_memory(PhysicalWordAddress::new(0x0005_0200)),
        0xdead,
        "the unselected alternate application must not be DMA-loaded"
    );
    // The application prologue set the stack to its --stack-init (0xe000)
    // minus its small frame.
    let sp = machine.register(13).unwrap();
    assert!((0xdfc0..=0xe000).contains(&sp), "sp = {sp:#06x}");

    // The data section landed (with zero-filled tail) and BSS was cleared.
    assert_eq!(
        machine.physical_memory(PhysicalWordAddress::new(0x0004_0000)),
        0xbeef
    );
    assert_eq!(
        machine.physical_memory(PhysicalWordAddress::new(0x0004_0001)),
        0x0055
    );
    assert_eq!(
        machine.physical_memory(PhysicalWordAddress::new(0x0004_0003)),
        0
    );
    assert_eq!(
        machine.physical_memory(PhysicalWordAddress::new(0x0004_0100)),
        0
    );
}

#[test]
fn button_10_boots_the_alternate_application_from_flash() {
    let (flash, stage0) = boot_setup();
    let machine = run_boot(flash, &stage0, 0b10, 500_000);

    let sysctl = machine.device::<SystemControlDevice>(0).unwrap();
    let frame = ddht_frame();
    assert!(sysctl.uart.len() >= frame.len() * 2);
    assert_eq!(sysctl.uart[..8], frame);
    assert_eq!(sysctl.uart[8..16], frame);
    assert_eq!(sysctl.led, Some(0b01_0101));
    assert_eq!(machine.code_segment(), 5);
    assert_eq!(machine.data_segment(), 6);
    assert_eq!(
        machine.physical_memory(PhysicalWordAddress::new(0x0003_0200)),
        0xdead,
        "the unselected primary application must not be DMA-loaded"
    );
    assert_ne!(
        machine.physical_memory(PhysicalWordAddress::new(0x0005_0200)),
        0xdead
    );
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
