//! Bounded functional validation of the exact build-selected board application.
use cpu_v3::{PhysicalWordAddress, RunOutcome};
use cpu_v3_tang_nano_20k::{
    boot::{SystemControlDevice, SYSTEM_CONTROL_DEVICE},
    framebuffer_word_at,
    system_sim::{CpuV3SystemSim, VBlankMode},
    FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD,
};

#[test]
fn application_instruction_boundaries_are_safe() {
    use cpu_v3::{decode, rcc_backend, Instruction};
    use std::path::PathBuf;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("rcc/system-display-test.rs");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = rcc::frontend::compile_program_named(
        &source.display().to_string(),
        &std::fs::read_to_string(&source).unwrap(),
        &options,
        &mut |name| {
            std::fs::read_to_string(root.join(format!("rcc/{name}.rs"))).map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let bytes: Vec<_> = program.words.iter().flat_map(|x| x.to_le_bytes()).collect();
    assert_eq!(
        bytes,
        include_bytes!(concat!(env!("OUT_DIR"), "/application.v3bin"))
    );
    let switches: Vec<_> = program
        .words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| {
            matches!(
                decode(*w),
                Instruction::WriteDataSegment { index: None, .. }
            )
            .then_some(i)
        })
        .collect();
    assert_eq!(
        switches.len(),
        2,
        "only the diagnostic read leaf may change mapping"
    );
    let mut loads = 0;
    for word in &program.words[switches[0] + 1..switches[1]] {
        match decode(*word) {
            Instruction::Load { base, .. } => {
                assert_ne!(base, 13);
                loads += 1;
            }
            Instruction::Immediate { .. }
            | Instruction::Prefix { .. }
            | Instruction::Move { .. } => {}
            other => panic!("unsafe instruction under temporary mapping: {other:?}"),
        }
    }
    assert_eq!(loads, 1);
    let maintenance: Vec<_> = program
        .words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| {
            matches!(
                decode(*w),
                Instruction::DeviceSend {
                    device: 0,
                    channel: 1,
                    ..
                }
            )
            .then_some(i)
        })
        .collect();
    assert_eq!(maintenance.len(), 1);
    let mut waited = false;
    let mut drained = false;
    for word in program.words[..maintenance[0]].iter().rev().take(8) {
        match decode(*word) {
            Instruction::DataCacheWait => waited = true,
            Instruction::DataCacheCleanLine { .. } => {
                drained = true;
                break;
            }
            Instruction::Immediate { .. }
            | Instruction::Prefix { .. }
            | Instruction::Move { .. } => {}
            other => panic!("unsafe instruction before full maintenance: {other:?}"),
        }
    }
    assert!(
        drained && waited,
        "the diagnostic retains its conservative completed DCLEANL sequence"
    );
    let evidence = root.join("../../target/system-board-ready/audit");
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(evidence.join("application.lst"), program.listing).unwrap();
    println!(
        "physical read switch interval: {}..{}; exactly one non-stack load, no call/store",
        switches[0], switches[1]
    );
}

#[test]
fn cold_board_application_initializes_surfaces_and_presents_continuously() {
    let package = include_bytes!(concat!(env!("OUT_DIR"), "/cpu-v3-boot.bin"));
    let map = include_str!(concat!(env!("OUT_DIR"), "/cpu-v3-boot.map"));
    assert!(map.contains("rcc/system-display-test.rs"));
    let mut system =
        CpuV3SystemSim::from_single_flash(package, 0xd00d, VBlankMode::Manual).unwrap();
    let mut reports = 0;
    let mut last_base = FRAMEBUFFER_A_BASE_WORD;
    for chunk in 0..500 {
        let outcome = system.run(20_000).unwrap();
        assert!(
            matches!(outcome, RunOutcome::StepLimit { .. }),
            "{outcome:?}"
        );
        let uart = &system
            .cpu()
            .device::<SystemControlDevice>(SYSTEM_CONTROL_DEVICE)
            .unwrap()
            .uart;
        for packet in uart.as_chunks::<8>().0.iter().skip(reports) {
            assert_eq!(&packet[..7], &[0x44, 0x44, 0x48, 0x54, 1, 0x0b, 0]);
            assert_eq!(packet.iter().fold(0, |a, b| a ^ b), 0);
        }
        let count = uart.len() / 8;
        if count > reports {
            println!(
                "functional presentation {count}: at most {} instructions",
                (chunk + 1) * 20_000
            );
            assert_eq!(count, reports + 1);
            let display = system.display_state();
            let expected = if reports % 2 == 0 {
                FRAMEBUFFER_B_BASE_WORD
            } else {
                FRAMEBUFFER_A_BASE_WORD
            };
            assert_eq!(display.active_base, expected);
            assert_ne!(display.active_base, last_base);
            assert!(!display.swap_pending);
            if reports == 0 {
                for (target, base) in [(0, FRAMEBUFFER_A_BASE_WORD), (1, FRAMEBUFFER_B_BASE_WORD)] {
                    for y in 0..240 {
                        for x in 0..400 {
                            let column = x / 16;
                            let expected = if target == 1 && x < 16 && (112..128).contains(&y) {
                                0xffff
                            } else if target == 1 && column == 0 {
                                0x001f
                            } else if column < 5 {
                                0xf800
                            } else if column < 9 {
                                0x07e0
                            } else if column < 13 {
                                0x001f
                            } else if column < 17 {
                                0xffe0
                            } else if column < 21 {
                                0x07ff
                            } else {
                                0xffff
                            };
                            assert_eq!(
                                system.cpu().physical_memory(PhysicalWordAddress::new(
                                    framebuffer_word_at(base, x, y)
                                )),
                                expected,
                                "target {target} pixel {x},{y}"
                            );
                        }
                    }
                }
            }
            reports = count;
            last_base = display.active_base;
            if reports == 4 {
                return;
            }
        }
        system.advance_vblank();
    }
    panic!("board application did not present four frames in 10,000,000 instructions");
}
