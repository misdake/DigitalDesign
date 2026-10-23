//! Recompile current software and re-export every endpoint before accepting evidence.
#[allow(dead_code)]
#[path = "support/gpu_rcc_ui_fixture.rs"]
mod fixture;
use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache};
use cpu_v3_tang_nano_20k::{
    hardware::{CpuV3MemoryArbiter, SystemControlDevice},
    CpuV3Gpu, FramebufferHdmi,
};
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{collections::BTreeMap, path::Path};
#[test]
#[ignore = "requires completed current D3 present and maintenance-fault native runs"]
fn ui_native_evidence_matches_final_sources() {
    check(true);
}
#[test]
#[ignore = "requires generated current native inputs, but not completed scanout"]
fn ui_native_inputs_match_final_sources() {
    check(false);
}
fn check(require_completed: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let mut dut = BTreeMap::new();
    for p in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
        VerilogProject::generate::<CpuV3Gpu>().unwrap(),
        VerilogProject::generate::<FramebufferHdmi>().unwrap(),
        VerilogProject::generate::<CpuV3MemoryArbiter>().unwrap(),
    ] {
        for (path, source) in p.files {
            if !source.contains("module tb") {
                if let Some(old) = dut.insert(path, source.clone()) {
                    assert_eq!(old, source);
                }
            }
        }
    }
    for fault in [false, true] {
        let folder = if fault {
            "gpu-rcc-ui-fault"
        } else {
            "gpu-rcc-ui-present"
        };
        let out = root.join("../../target").join(folder);
        let source = if fault {
            fixture::fault_program()
        } else {
            fixture::program()
        };
        assert_eq!(
            source,
            std::fs::read_to_string(out.join("program.rs")).unwrap()
        );
        let parsed = rcc::frontend::compile_program_named(
            "D3 final identity",
            &source,
            &options,
            &mut |name| {
                std::fs::read_to_string(root.join(format!("rcc/{name}.rs")))
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        assert_eq!(
            program
                .words
                .iter()
                .map(|w| format!("{w:04x}\n"))
                .collect::<String>(),
            std::fs::read_to_string(out.join("program.hex")).unwrap()
        );
        let mut tb = include_str!("gpu_rcc_ui_present_tb.v")
            .replace(
                "__FST_ADDRESS__",
                &fixture::present::fst_address(&program).to_string(),
            )
            .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
            .replace(
                "__CACHE__",
                &CpuV3DataCache::verilog_identity().module_name(),
            )
            .replace(
                "__SYSCTL__",
                &SystemControlDevice::<4>::verilog_identity().module_name(),
            )
            .replace(
                "__DISPLAY__",
                &FramebufferHdmi::verilog_identity().module_name(),
            )
            .replace(
                "__ARBITER__",
                &CpuV3MemoryArbiter::verilog_identity().module_name(),
            );
        for (key, name) in [
            ("__PROGRAM__", "program.hex"),
            ("__DEPTH_GOLD__", "depth.hex"),
            ("__ASSETS__", "assets.hex"),
            ("__GOLD1__", "gold1.hex"),
            ("__GOLD2__", "gold2.hex"),
        ] {
            tb = tb.replace(key, &out.join(name).to_string_lossy().replace('\\', "/"));
        }
        if fault {
            tb = fixture::fault_testbench(tb);
        }
        assert_eq!(tb, std::fs::read_to_string(out.join("tb.v")).unwrap());
        for (i, current) in dut.values().enumerate() {
            assert_eq!(
                *current,
                std::fs::read_to_string(out.join(format!("source_{i}.v"))).unwrap()
            );
        }
        let mut seed: BTreeMap<u32, u64> = BTreeMap::new();
        for (a, b) in fixture::assets() {
            *seed.entry(a / 8).or_default() |= u64::from(b) << ((a % 8) * 8);
        }
        assert_eq!(
            seed.iter()
                .map(|(a, v)| format!("@{a:x}\n{v:016x}\n"))
                .collect::<String>(),
            std::fs::read_to_string(out.join("assets.hex")).unwrap()
        );
        for frame in [1, 2] {
            assert_eq!(
                fixture::surface(frame)
                    .iter()
                    .map(|v| format!("{v:04x}\n"))
                    .collect::<String>(),
                std::fs::read_to_string(out.join(format!("gold{frame}.hex"))).unwrap()
            );
        }
        assert_eq!(
            fixture::depth()
                .iter()
                .map(|v| format!("{v:04x}\n"))
                .collect::<String>(),
            std::fs::read_to_string(out.join("depth.hex")).unwrap()
        );
        assert!(std::fs::read_to_string(out.join("asset-audit.json"))
            .unwrap()
            .contains("\"PASS\""));
        let log = std::fs::read_to_string(out.join("run.log")).unwrap();
        if require_completed {
            assert!(log.contains(if fault {
                "RCC_UI_FAULT_PASS"
            } else {
                "RCC_UI_PRESENT_PASS"
            }));
        }
    }
}
