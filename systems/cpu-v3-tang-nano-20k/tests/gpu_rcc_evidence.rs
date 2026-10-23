//! Fresh identity checks bind native evidence to the current RCC and RTL.
#[path = "support/gpu_rcc_d2.rs"]
mod fixture;
use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache};
use cpu_v3_tang_nano_20k::{hardware::SystemControlDevice, CpuV3Gpu};
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{collections::BTreeMap, path::Path};
#[test]
#[ignore = "requires completed D2 and retained Clear/fault native evidence"]
fn native_evidence_matches_final_d2_sources() {
    check_evidence(&[
        ("gpu-rcc-draw", "program", true),
        ("gpu-rcc-submit", "repeat-clear", false),
        ("gpu-rcc-submit", "cache-error", false),
    ]);
}
#[test]
#[ignore = "requires current complete two-page draw native evidence"]
fn current_two_page_draw_evidence_matches_final_sources() {
    check_evidence(&[("gpu-rcc-draw-current", "program", true)]);
}
fn check_evidence(cases: &[(&str, &str, bool)]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let mut dut = BTreeMap::new();
    for p in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
        VerilogProject::generate::<CpuV3Gpu>().unwrap(),
    ] {
        for (path, source) in p.files {
            if !source.contains("module tb") {
                if let Some(old) = dut.insert(path, source.clone()) {
                    assert_eq!(old, source);
                }
            }
        }
    }
    for &(folder, name, draw) in cases {
        let out = root.join("../../target").join(folder);
        let source = std::fs::read_to_string(out.join(format!("{name}.rs"))).unwrap();
        if draw {
            assert_eq!(source, fixture::program());
        }
        let parsed = rcc::frontend::compile_program_named(
            "final identity",
            &source,
            &options,
            &mut |name| {
                std::fs::read_to_string(root.join(format!("rcc/{name}.rs")))
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let hex = program
            .words
            .iter()
            .map(|w| format!("{w:04x}\n"))
            .collect::<String>();
        assert_eq!(
            hex,
            std::fs::read_to_string(out.join(format!("{name}.hex"))).unwrap(),
            "RCC image: {name}"
        );
        let mut tb = if draw {
            include_str!("gpu_rcc_draw_tb.v")
        } else {
            include_str!("gpu_rcc_submit_tb.v")
        }
        .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
        .replace(
            "__CACHE__",
            &CpuV3DataCache::verilog_identity().module_name(),
        )
        .replace(
            "__SYSCTL__",
            &SystemControlDevice::<4>::verilog_identity().module_name(),
        )
        .replace("__FAULT__", if name == "cache-error" { "1" } else { "0" });
        let path = |n: &str| out.join(n).to_string_lossy().replace('\\', "/");
        tb = tb.replace("__PROGRAM__", &path(&format!("{name}.hex")));
        if draw {
            for (key, n) in [
                ("__ASSETS__", "assets.hex"),
                ("__GOLD1__", "gold1.hex"),
                ("__GOLD2__", "gold2.hex"),
            ] {
                tb = tb.replace(key, &path(n));
            }
            let mut assets: BTreeMap<u32, u64> = BTreeMap::new();
            for (a, b) in fixture::assets() {
                *assets.entry(a / 8).or_default() |= u64::from(b) << ((a % 8) * 8);
            }
            assert_eq!(
                std::fs::read_to_string(out.join("assets.hex")).unwrap(),
                assets
                    .iter()
                    .map(|(a, v)| format!("@{a:x}\n{v:016x}\n"))
                    .collect::<String>()
            );
            for frame in [1, 2] {
                assert_eq!(
                    std::fs::read_to_string(out.join(format!("gold{frame}.hex"))).unwrap(),
                    fixture::surface(frame)
                        .iter()
                        .map(|v| format!("{v:04x}\n"))
                        .collect::<String>()
                );
            }
            assert_eq!(fixture::expected_commands(16368).len(), 384);
            assert!(std::fs::read_to_string(out.join("asset-audit.json"))
                .unwrap()
                .contains("\"PASS\""));
        }
        assert_eq!(
            tb,
            std::fs::read_to_string(out.join(if draw {
                "tb.v".into()
            } else {
                format!("{name}-tb.v")
            }))
            .unwrap(),
            "testbench: {name}"
        );
        let log = std::fs::read_to_string(out.join(if draw {
            "run.log".into()
        } else {
            format!("{name}.log")
        }))
        .unwrap();
        assert!(log.contains(if draw {
            "RCC_DRAW_PASS"
        } else {
            "RCC_GPU_PASS"
        }));
        for (i, current) in dut.values().enumerate() {
            assert_eq!(
                *current,
                std::fs::read_to_string(out.join(format!("source_{i}.v"))).unwrap(),
                "{folder} RTL source{i}"
            );
        }
    }
}
