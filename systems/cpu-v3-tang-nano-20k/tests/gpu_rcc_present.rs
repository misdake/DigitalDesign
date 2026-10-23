//! Real CPU/GPU/Display with production shared-memory arbitration.
use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache};
use cpu_v3_tang_nano_20k::{
    hardware::{CpuV3MemoryArbiter, SystemControlDevice},
    CpuV3Gpu, FramebufferHdmi,
};
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
#[allow(dead_code)]
#[path = "support/gpu_rcc_present_fixture.rs"]
mod present_fixture;
use present_fixture::draw_fixture;
use present_fixture::{program as present_program, surface as present_surface};
#[test]
#[ignore = "real full-size scan + CPU/GPU arbitration; requires Python, Icarus and Gowin; accelerated pixel clock"]
fn real_cpu_gpu_display_swaps_after_old_scan_reads_drain() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("../../target/gpu-rcc-present");
    std::fs::create_dir_all(&out).unwrap();
    assert_eq!(draw_fixture::expected_commands(16368).len(), 384);
    let source = present_program();
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
            present_surface(frame)
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
        VerilogProject::generate::<FramebufferHdmi>().unwrap(),
        VerilogProject::generate::<CpuV3MemoryArbiter>().unwrap(),
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
    let mut tb = include_str!("gpu_rcc_present_tb.v")
        .replace(
            "__FST_ADDRESS__",
            &present_fixture::fst_address(&program).to_string(),
        )
        .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
        .replace(
            "__CACHE__",
            &CpuV3DataCache::verilog_identity().module_name(),
        )
        .replace(
            "__SYSCTL__",
            &SystemControlDevice::<4>::verilog_identity().module_name(),
        );
    tb = tb
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
        result.status.success() && stdout.contains("RCC_PRESENT_PASS"),
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
        if start.elapsed() > Duration::from_secs(1800) {
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
