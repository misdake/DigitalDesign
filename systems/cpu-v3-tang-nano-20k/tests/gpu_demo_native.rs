//! Real CPU/GPU/Display with production shared-memory arbitration.
use cpu_v3::{CpuV3Core, CpuV3DataCache};
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
#[path = "../examples/gpu_demo/support.rs"]
mod support;
#[test]
#[ignore = "requires completed one-frame demo and cache-fault native runs"]
fn final_native_evidence_matches_current_executable_inputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let reference = root.join("../../target/gpu-demo-native");
    let constants = std::fs::read_to_string(reference.join("assets/demo_assets.rs")).unwrap();
    let boot_dir = root.join("../../target/gpu-demo-boot");
    let boot_constants = std::fs::read_to_string(boot_dir.join("boot_resources.rs")).unwrap();
    let current_main = support::compile(
        include_str!("../examples/gpu_demo/rcc/main.rs"),
        &boot_constants,
        "main",
    );
    assert_eq!(
        current_main
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
        std::fs::read(boot_dir.join("application.v3bin")).unwrap()
    );
    let mut sources = BTreeMap::new();
    for project in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
        VerilogProject::generate::<CpuV3Gpu>().unwrap(),
        VerilogProject::generate::<FramebufferHdmi>().unwrap(),
        VerilogProject::generate::<CpuV3MemoryArbiter>().unwrap(),
    ] {
        for (path, source) in project.files {
            if !source.contains("module tb") {
                if let Some(old) = sources.insert(path, source.clone()) {
                    assert_eq!(old, source);
                }
            }
        }
    }
    for (folder, entry, mode, marker) in [
        ("gpu-demo-native", "demo_one_frame_main", 0, "GPU_DEMO_PASS"),
        (
            "gpu-demo-fault",
            "demo_native_main",
            1,
            "GPU_DEMO_FAILURE_PASS",
        ),
    ] {
        let out = root.join("../../target").join(folder);
        let program = support::compile(
            include_str!("../examples/gpu_demo/rcc/main.rs"),
            &constants,
            entry,
        );
        assert_eq!(
            program
                .words
                .iter()
                .map(|w| format!("{w:04x}\n"))
                .collect::<String>(),
            std::fs::read_to_string(out.join("program.hex")).unwrap()
        );
        assert_eq!(
            testbench(&out, &reference, mode),
            std::fs::read_to_string(out.join("tb.v")).unwrap()
        );
        for (i, source) in sources.values().enumerate() {
            assert_eq!(
                *source,
                std::fs::read_to_string(out.join(format!("source_{i}.v"))).unwrap()
            );
        }
        assert!(std::fs::read_to_string(out.join("run.log"))
            .unwrap()
            .contains(marker));
    }
}
#[test]
#[ignore = "real full-size scan + CPU/GPU arbitration; requires Python, Icarus and Gowin; accelerated pixel clock"]
fn real_rotating_demo_presents_one_complete_frame() {
    run_demo("demo_one_frame_main", "gpu-demo-native", 0);
}
#[test]
#[ignore = "real cache terminal fault; Python/Icarus/Gowin required"]
fn real_demo_cache_fault_never_starts_gpu_or_presents() {
    run_demo("demo_native_main", "gpu-demo-fault", 1);
}

fn run_demo(entry: &'static str, folder: &str, mode: u8) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("../../target").join(folder);
    std::fs::create_dir_all(&out).unwrap();
    let reference = root.join("../../target/gpu-demo-native");
    std::fs::create_dir_all(&reference).unwrap();
    if mode == 0 {
        support::write_matrices(&reference.join("matrices.json"));
        let mut gold = Command::new("python");
        gold.env("PYTHONDONTWRITEBYTECODE", "1")
            .arg(root.join("examples/gpu_demo/tools/gold.py"))
            .arg("--output")
            .arg(&reference)
            .arg("--matrices")
            .arg(reference.join("matrices.json"));
        let result = bounded(&mut gold, &reference.join("reference.log"));
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let source = include_str!("../examples/gpu_demo/rcc/main.rs");
    let constants = std::fs::read_to_string(reference.join("assets/demo_assets.rs")).unwrap();
    let program = support::compile(source, &constants, entry);
    std::fs::write(out.join("program.rs"), source).unwrap();
    std::fs::write(out.join("entry.txt"), entry).unwrap();
    std::fs::write(
        out.join("program.hex"),
        program
            .words
            .iter()
            .map(|w| format!("{w:04x}\n"))
            .collect::<String>(),
    )
    .unwrap();
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
    let tb = testbench(&out, &reference, mode);
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
        result.status.success()
            && stdout.contains(if mode == 0 {
                "GPU_DEMO_PASS"
            } else {
                "GPU_DEMO_FAILURE_PASS"
            }),
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
        if start.elapsed() > Duration::from_secs(5400) {
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

fn testbench(out: &Path, reference: &Path, mode: u8) -> String {
    let mut tb = include_str!("gpu_demo_tb.v")
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
    tb = tb.replace(
        "__PROGRAM__",
        &out.join("program.hex").to_string_lossy().replace('\\', "/"),
    );
    for (key, name) in [
        ("__ASSETS__", "assets.hex"),
        ("__GOLD1__", "gold1.hex"),
        ("__GOLD2__", "gold2.hex"),
        ("__DEPTH1__", "depth1.hex"),
        ("__DEPTH2__", "depth2.hex"),
        ("__MATRIX1__", "matrix1.hex"),
        ("__MATRIX2__", "matrix2.hex"),
    ] {
        tb = tb.replace(
            key,
            &reference.join(name).to_string_lossy().replace('\\', "/"),
        );
    }
    for (key, name) in [
        ("__ACTUAL1__", "actual1.hex"),
        ("__ACTUAL2__", "actual2.hex"),
    ] {
        tb = tb.replace(key, &out.join(name).to_string_lossy().replace('\\', "/"));
    }
    if mode != 0 {
        tb = tb.replace("if(cycle>6000000)", "if(cycle>600000)");
        let begin = tb.find(" if(fault||signal").unwrap();
        let end = tb[begin..].find(" $finish;").unwrap() + begin;
        let check = r#"
 if(fault||signal!==16'hd007||!injected||starts!=0||writes!=0||presents!=0||swaps!=0||completions!=0||boundaries!=1||retained_state!=1||error_reports!=1||normal_reports!=0)$fatal(1,"Demo cache-fault retention failed signal=%h",signal);
 $display("GPU_DEMO_FAILURE_PASS mode=cache starts=%0d writes=%0d swaps=%0d",starts,writes,swaps);
"#;

        tb.replace_range(begin..end, check);
        if mode == 1 {
            tb=tb.replace("localparam FAULT=0;","localparam FAULT=1;reg injected=0;")
                .replace("assign ae=1'b0;","assign ae=ms==5&&write_owner==1&&mb>=('h4a0180/8)&&mb<('h4a0500/8)&&!injected;")
                .replace("5:if(arr)begin","5:if(arr)begin if(ae)injected<=1;")
                .replace("for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];","if(!ae)for(k=0;k<16;k=k+1)if(k<mn)mem[mb+k]<=write_buffer[k];");
        }
    }
    tb
}
