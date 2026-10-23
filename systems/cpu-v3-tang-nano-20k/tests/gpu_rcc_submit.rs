//! Real RCC -> CPU -> D-cache/SystemControl -> Device4/GPU -> burst memory.
//! This fixture has no Display scanner; only Color B and Depth may be written.
use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache};
use cpu_v3_tang_nano_20k::{hardware::SystemControlDevice, CpuV3Gpu};
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
#[path = "support/gpu_rcc_d2.rs"]
mod draw_fixture;

#[test]
#[ignore = "bounded D2 real CPU/cache/Device4/GPU draw; requires Python, Icarus and Gowin models"]
fn rcc_records_and_renders_two_ordered_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("../../target/gpu-rcc-draw-current");
    std::fs::create_dir_all(&out).unwrap();
    assert_eq!(draw_fixture::expected_commands(16368).len(), 384);
    let source = draw_fixture::program();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("D2 real draws", &source, &options, &mut |name| {
            std::fs::read_to_string(root.join(format!("rcc/{name}.rs"))).map_err(|e| e.to_string())
        })
        .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let hex = out.join("program.hex");
    std::fs::write(out.join("program.rs"), source).unwrap();
    std::fs::write(
        &hex,
        program
            .words
            .iter()
            .map(|w| format!("{w:04x}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let mut assets: BTreeMap<u32, u64> = BTreeMap::new();
    for (address, b) in draw_fixture::assets() {
        *assets.entry(address / 8).or_default() |= u64::from(b) << ((address % 8) * 8);
    }
    std::fs::write(
        out.join("assets.hex"),
        assets
            .iter()
            .map(|(a, v)| format!("@{a:x}\n{v:016x}\n"))
            .collect::<String>(),
    )
    .unwrap();
    for frame in [1, 2] {
        std::fs::write(
            out.join(format!("gold{frame}.hex")),
            draw_fixture::surface(frame)
                .iter()
                .map(|v| format!("{v:04x}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }
    let mut asset_encoder = Command::new("python");
    asset_encoder
        .arg(root.join("tests/support/gpu_rcc_asset_audit.py"))
        .arg(&out);
    let audit = bounded(&mut asset_encoder, &out.join("asset-encoder.log"));
    assert!(
        audit.status.success(),
        "{}",
        String::from_utf8_lossy(&audit.stderr)
    );
    let mut sources = BTreeMap::new();
    for p in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
        VerilogProject::generate::<CpuV3Gpu>().unwrap(),
    ] {
        for (path, source) in p.files {
            if !source.contains("module tb") {
                if let Some(old) = sources.insert(path, source.clone()) {
                    assert_eq!(old, source);
                }
            }
        }
    }
    let mut paths = Vec::new();
    for (i, source) in sources.values().enumerate() {
        let path = out.join(format!("source_{i}.v"));
        std::fs::write(&path, source).unwrap();
        paths.push(path);
    }
    let mut tb = include_str!("gpu_rcc_draw_tb.v")
        .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
        .replace(
            "__CACHE__",
            &CpuV3DataCache::verilog_identity().module_name(),
        )
        .replace(
            "__SYSCTL__",
            &SystemControlDevice::<4>::verilog_identity().module_name(),
        );
    for (key, name) in [
        ("__PROGRAM__", "program.hex"),
        ("__ASSETS__", "assets.hex"),
        ("__GOLD1__", "gold1.hex"),
        ("__GOLD2__", "gold2.hex"),
    ] {
        tb = tb.replace(key, &out.join(name).to_string_lossy().replace('\\', "/"));
    }
    let tb_path = out.join("tb.v");
    std::fs::write(&tb_path, tb).unwrap();
    let models = PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
        .join("IDE/simlib/gw2a/prim_sim.v");
    let exe = out.join("draw.vvp");
    let mut compiler =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compiler
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(&exe)
        .arg(models)
        .args(paths)
        .arg(tb_path);
    let result = bounded(&mut compiler, &out.join("compile.log"));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut sim = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    sim.arg(exe);
    let result = bounded(&mut sim, &out.join("run.log"));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success() && stdout.contains("RCC_DRAW_PASS"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{stdout}");
}

fn bounded(command: &mut Command, log: &Path) -> Output {
    let mut child = command
        .stdout(std::fs::File::create(log).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(600) {
            child.kill().unwrap();
            let result = child.wait_with_output().unwrap();
            panic!(
                "bounded RCC GPU tool timed out: {}\n{}",
                std::fs::read_to_string(log).unwrap_or_default(),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut result = child.wait_with_output().unwrap();
    result.stdout = std::fs::read(log).unwrap();
    result
}
#[test]
#[ignore = "bounded real CPU/cache/SystemControl/Device4/GPU; requires Icarus and GOWIN_HOME"]
fn rcc_builds_high_command_buffers_and_submits_twice() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-rcc-submit");
    std::fs::create_dir_all(&out).unwrap();
    let mut sources = BTreeMap::new();
    for p in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
        VerilogProject::generate::<CpuV3Gpu>().unwrap(),
    ] {
        for (path, source) in p.files {
            if !source.contains("module tb") {
                if let Some(old) = sources.insert(path, source.clone()) {
                    assert_eq!(old, source);
                }
            }
        }
    }
    let mut paths = Vec::new();
    for (i, source) in sources.values().enumerate() {
        let path = out.join(format!("source_{i}.v"));
        std::fs::write(&path, source).unwrap();
        paths.push(path);
    }
    for fault in [false, true] {
        let ending = if fault {
            r#"
 check(!gpu_pass_clear(cb,0x1357,0x2468));
 check(read_dseg_page(3)==19);check(read_dseg_page(0)==0 && read_dseg_page(1)==1 && read_dseg_page(2)==2);
 check(gpu_poll(cb)==GPU_FAULT);check(gpu_cb_begin()==GPU_CB_NONE);
 check(gpu_submit_idle(cb)==GPU_FAULT);
"#
        } else {
            r#"
 check(gpu_pass_clear(cb,0x1357,0x2468));
 check(read_dseg_page(3)==19);
 dev_send(7,1,FST.as_ptr().addr());
 let v=vec4::new(fix32::from_int(1),fix32::from_int(2),fix32::from_int(3),fix32::from_int(4));
 vec4::export(v,FST.as_ptr());
 check(gpu_submit_idle(cb)==GPU_PENDING);
 check(gpu_submit_idle(cb)==GPU_REJECTED);check(gpu_cb_begin()==GPU_CB_NONE);
 check(gpu_wait(cb,1)==GPU_TIMEOUT);check(!gpu_pass_clear(cb,0,0));
 check(gpu_init()==GPU_REJECTED);
 check(gpu_wait(cb,65535)==GPU_COMPLETED);dev_send(7,0,1);
 check(gpu_poll(cb)==GPU_IDLE);check(gpu_discard_ready(cb));let next=gpu_cb_begin();check(next==GPU_CB_HANDLE);
 check(gpu_cb_begin()==GPU_CB_NONE);check(gpu_pass_clear(next,0xabcd,0x5678));
 check(gpu_submit_idle(next)==GPU_PENDING);
 check(gpu_wait(next,65535)==GPU_COMPLETED);dev_send(7,0,2);
 check(read_dseg_page(3)==19);check(read_dseg_page(0)==0 && read_dseg_page(1)==1 && read_dseg_page(2)==2);
"#
        };
        let source = format!(
            r#"
mod gpu;
static FST:Buf<u16,8>=Buf::new([0;8]);
fn check(ok:bool){{if !ok{{halt(0xbad);}}}}
fn main(){{
 unsafe {{write_dseg_page(3,19);}}
 check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();check(cb==GPU_CB_HANDLE);
 {ending}
 halt(0x600d);
}}
"#
        );
        let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rcc");
        let parsed = rcc::frontend::compile_program_named(
            "real GPU driver",
            &source,
            &options,
            &mut |name| {
                std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let name = if fault { "cache-error" } else { "repeat-clear" };
        std::fs::write(out.join(format!("{name}.rs")), source).unwrap();
        let hex = out.join(format!("{name}.hex"));
        std::fs::write(
            &hex,
            program
                .words
                .iter()
                .map(|w| format!("{w:04x}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let tb = include_str!("gpu_rcc_submit_tb.v")
            .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
            .replace(
                "__CACHE__",
                &CpuV3DataCache::verilog_identity().module_name(),
            )
            .replace(
                "__SYSCTL__",
                &SystemControlDevice::<4>::verilog_identity().module_name(),
            )
            .replace("__FAULT__", if fault { "1" } else { "0" })
            .replace("__PROGRAM__", &hex.to_string_lossy().replace('\\', "/"));
        let tb_path = out.join(format!("{name}-tb.v"));
        std::fs::write(&tb_path, tb).unwrap();
        let exe = out.join(format!("{name}.vvp"));
        let models = PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
            .join("IDE/simlib/gw2a/prim_sim.v");
        let mut compiler =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        compiler
            .args(["-g2012", "-s", "tb", "-o"])
            .arg(&exe)
            .arg(models)
            .args(&paths)
            .arg(tb_path);
        let result = bounded(&mut compiler, &out.join(format!("{name}-compile.log")));
        std::fs::write(
            out.join(format!("{name}-compile.log")),
            [&result.stdout[..], &result.stderr[..]].concat(),
        )
        .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let mut sim = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        sim.arg(exe);
        let result = bounded(&mut sim, &out.join(format!("{name}.log")));
        std::fs::write(
            out.join(format!("{name}.log")),
            [&result.stdout[..], &result.stderr[..]].concat(),
        )
        .unwrap();
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(
            result.status.success() && stdout.contains("RCC_GPU_PASS"),
            "{name}: {stdout}\n{}",
            String::from_utf8_lossy(&result.stderr)
        );
        println!("{stdout}");
    }
}

#[test]
#[ignore = "post-run identity audit requires the completed native gpu-rcc-submit fixture"]
fn native_evidence_matches_current_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("../../target/gpu-rcc-submit");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    for name in ["repeat-clear", "cache-error"] {
        let source = std::fs::read_to_string(out.join(format!("{name}.rs"))).unwrap();
        let parsed = rcc::frontend::compile_program_named(
            "real GPU driver",
            &source,
            &options,
            &mut |name| {
                std::fs::read_to_string(root.join(format!("rcc/{name}.rs")))
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let current = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let measured: Vec<u16> = std::fs::read_to_string(out.join(format!("{name}.hex")))
            .unwrap()
            .lines()
            .map(|w| u16::from_str_radix(w, 16).unwrap())
            .collect();
        assert_eq!(
            current.words, measured,
            "{name}: current RCC source differs from native executable"
        );
        let expected = include_str!("gpu_rcc_submit_tb.v")
            .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
            .replace(
                "__CACHE__",
                &CpuV3DataCache::verilog_identity().module_name(),
            )
            .replace(
                "__SYSCTL__",
                &SystemControlDevice::<4>::verilog_identity().module_name(),
            )
            .replace("__FAULT__", if name == "cache-error" { "1" } else { "0" })
            .replace(
                "__PROGRAM__",
                &out.join(format!("{name}.hex"))
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        assert_eq!(
            expected,
            std::fs::read_to_string(out.join(format!("{name}-tb.v"))).unwrap()
        );
        let log = std::fs::read_to_string(out.join(format!("{name}.log"))).unwrap();
        assert!(log.contains("RCC_GPU_PASS"));
    }
    let mut sources = BTreeMap::new();
    for p in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
        VerilogProject::generate::<CpuV3Gpu>().unwrap(),
    ] {
        for (path, source) in p.files {
            if !source.contains("module tb") {
                if let Some(old) = sources.insert(path, source.clone()) {
                    assert_eq!(old, source);
                }
            }
        }
    }
    for (i, current) in sources.values().enumerate() {
        assert_eq!(
            *current,
            std::fs::read_to_string(out.join(format!("source_{i}.v"))).unwrap(),
            "DUT source_{i}"
        );
    }
}
