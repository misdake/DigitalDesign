//! Real CPU/cache/SystemControl ordering; no diagnostic software pre-drain.
use cpu_v3::{CpuV3Core, CpuV3DataCache, FpuAuxKind};
use cpu_v3_tang_nano_20k::hardware::SystemControlDevice;
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{collections::BTreeMap, path::PathBuf, process::Command};

#[test]
#[ignore = "bounded CPU + cache + SystemControl maintenance protocol RTL"]
fn maintenance_drains_old_work_and_owns_completion() {
    let mut files = BTreeMap::<PathBuf, String>::new();
    for project in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
    ] {
        for (path, source) in project.files {
            if !source.contains("module tb") {
                if let Some(previous) = files.insert(path, source.clone()) {
                    assert_eq!(previous, source, "conflicting exported module");
                }
            }
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/cpu-maintenance-fix-20261022/protocol");
    std::fs::create_dir_all(&root).unwrap();
    let mut paths = Vec::new();
    for (index, source) in files.values().enumerate() {
        let path = root.join(format!("source_{index}.v"));
        std::fs::write(&path, source).unwrap();
        paths.push(path);
    }
    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    for case in 0..14 {
        let fst = matches!(case, 2 | 7);
        let mut p = cpu_v3::load_immediate16(1, 0x4000).to_vec();
        if case == 8 {
            p.extend(cpu_v3::load_immediate16(3, 2));
        }
        if fst {
            for lane in 0..4 {
                for (subop, value) in [(2, 0x1000 + lane), (3, 0x2000 + lane)] {
                    p.extend(cpu_v3::load_immediate16(0, value));
                    p.extend(cpu_v3::fpu_aux_raw(
                        FpuAuxKind::IntegerRegister,
                        0,
                        0,
                        lane as u8,
                        subop,
                        0,
                    ));
                }
            }
            p.extend(cpu_v3::fpu_aux_raw(
                FpuAuxKind::IntegerRegister,
                1,
                0,
                0,
                1,
                3,
            ));
        } else {
            p.extend(cpu_v3::load_immediate16(0, 0x1234));
            p.push(cpu_v3::store(0, 1, 0));
        }
        if case == 8 {
            p.push(cpu_v3::line_copy(1, 3));
        }
        if case >= 9 {
            // Arithmetic must finish its real register-file write before the
            // immediately following maintenance command can retire. FST is
            // tested separately because it deliberately retires early.
            for lane in 0..4 {
                p.extend(cpu_v3::load_immediate16(0, lane + 1));
                p.extend(cpu_v3::fpu_aux_raw(
                    FpuAuxKind::IntegerRegister,
                    0,
                    0,
                    lane as u8,
                    6,
                    0,
                ));
            }
            p.extend(match case {
                9 => cpu_v3::fpu_scalar(0, 1, 8, cpu_v3::FpuScalarSubop::Add, 0),
                10 => cpu_v3::fpu_vector(
                    0,
                    0,
                    8,
                    cpu_v3::FpuVectorLength::Vec4,
                    cpu_v3::FpuVectorSubop::VMul,
                    0,
                ),
                11 => cpu_v3::fpu_vector(
                    0,
                    0,
                    8,
                    cpu_v3::FpuVectorLength::Vec4,
                    cpu_v3::FpuVectorSubop::DotStore,
                    0,
                ),
                12 => cpu_v3::fpu_scalar(0, 0, 8, cpu_v3::FpuScalarSubop::Rcp, 0),
                13 => cpu_v3::fpu_scalar(0, 0, 8, cpu_v3::FpuScalarSubop::SinCos, 0),
                _ => unreachable!(),
            });
        }
        p.push(cpu_v3::device_send(0, 0, 4));
        if case == 3 {
            p.push(cpu_v3::device_send(0, 0, 4)); // clean with no dirty data
            p.push(cpu_v3::device_send(0, 0, 1)); // then invalidate
        }
        p.push(cpu_v3::device_receive(2, 0, 5));
        if case >= 9 {
            p.extend(cpu_v3::fpu_aux_raw(
                FpuAuxKind::IntegerRegister,
                2,
                match case {
                    10 => 11, // Last vector lane.
                    13 => 9,  // Last write of the dual-output operation.
                    _ => 8,
                },
                0,
                if case == 13 { 4 } else { 5 },
                0,
            ));
        }
        p.extend(cpu_v3::load_immediate16(0, 0x55));
        p.push(cpu_v3::signal(2, 0));
        let init = p
            .iter()
            .enumerate()
            .map(|(i, w)| format!("rom[{i}]=16'h{w:04x};\n"))
            .collect::<String>();
        let tb = include_str!("cache_maintenance_protocol_tb.v")
            .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
            .replace(
                "__CACHE__",
                &CpuV3DataCache::verilog_identity().module_name(),
            )
            .replace(
                "__SYSCTL__",
                &SystemControlDevice::<4>::verilog_identity().module_name(),
            )
            .replace("__CASE__", &case.to_string())
            .replace(
                "__RESULT__",
                &match case {
                    4 => 0x8000u16,
                    9 => 3,
                    10 => 16,
                    11 => 30,
                    12 => 1,
                    13 => cpu_v3::sincos_q16(1 << 16).1 as u16,
                    _ => 0,
                }
                .to_string(),
            )
            .replace("__PROGRAM__", &init);
        let tb_path = root.join(format!("case_{case}.v"));
        std::fs::write(&tb_path, tb).unwrap();
        let executable = root.join(format!("case_{case}.vvp"));
        let compile = Command::new(&iverilog)
            .args(["-g2012", "-s", "tb", "-o"])
            .arg(&executable)
            .args(&paths)
            .arg(&tb_path)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let result = Command::new(&vvp).arg(&executable).output().unwrap();
        let stdout = String::from_utf8_lossy(&result.stdout);
        std::fs::write(root.join(format!("case_{case}.log")), result.stdout.clone()).unwrap();
        assert!(
            result.status.success() && stdout.contains("PROTOCOL_PASS"),
            "case {case}: {stdout}\n{}",
            String::from_utf8_lossy(&result.stderr)
        );
        println!("{stdout}");
    }
}
