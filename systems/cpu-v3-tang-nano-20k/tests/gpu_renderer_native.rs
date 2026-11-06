//! Real resource caller with production CPU/D-cache/GPU/Display and descriptor slots.
//! Instruction ROM + controlled external memory; no SDRAM pins/board qualification.
use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache};
use cpu_v3_tang_nano_20k::{
    hardware::{CpuV3MemoryArbiter, SystemControlDevice},
    CpuV3Gpu, FramebufferHdmi,
};
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
#[path = "support/gpu_renderer_native_cases.rs"]
mod cases;
#[allow(dead_code)]
#[path = "../build_support/rcc_modules.rs"]
mod resolver;
fn global(p: &rcc_backend::CpuV3Program, name: &str) -> u16 {
    match p.debug.globals.iter().find(|g| g.name == name).unwrap().loc {
        rcc::VarLoc::Global(a) => a,
        _ => panic!("not global {name}"),
    }
}
// Independent source-world projection, without driver matrices or GPU/cmodel code.
// The asset producer independently decodes (-1,-1), (1,-1), (0,1) at z=0.
// Native view reflects X; perspective uses focal=200 and eye/model depth=3.
fn pixel_oracle(out: &Path) {
    let project = |shift: f64| {
        [(-1., -1.), (1., -1.), (0., 1.)]
            .map(|(x, y)| [200. - 200. * (x + shift) / 3., 120. - 200. * y / 3.])
    };
    let inside = |triangle: [[f64; 2]; 3], point: [f64; 2]| {
        let edges: [f64; 3] = std::array::from_fn(|i| {
            let a = triangle[i];
            let b = triangle[(i + 1) % 3];
            (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])
        });
        edges.iter().all(|x| *x > 1.) || edges.iter().all(|x| *x < -1.)
    };
    let original = project(0.);
    let changed = project(0.25 - 0.125);
    for triangle in [original, changed] {
        assert!(inside(triangle, [200.5, 120.5]));
        assert!(!inside(triangle, [399.5, 239.5]));
    }
    assert!(inside(original, [258.5, 175.5]));
    assert!(!inside(changed, [258.5, 175.5]));
    std::fs::write(
        out.join("pixel-oracle.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "original_screen_vertices": original,
            "changed_screen_vertices": changed,
            "center": [200,120], "outside": [399,239],
            "old_inside_new_outside": [258,175],
            "colors_rgb565": {"original": 63488, "changed": 31},
            "boundary": "selected interior/exterior samples only; not full-frame numerical equality"
        }))
        .unwrap(),
    )
    .unwrap();
}
fn bounded(command: &mut Command, out: &Path, seconds: u64) {
    let mut child = command
        .stdout(std::fs::File::create(out.with_extension("log")).unwrap())
        .stderr(std::fs::File::create(out.with_extension("stderr")).unwrap())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "tool failed: {}\n{}\n{}",
                out.display(),
                std::fs::read_to_string(out.with_extension("log")).unwrap_or_default(),
                std::fs::read_to_string(out.with_extension("stderr")).unwrap_or_default()
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("tool wall timeout {}", out.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "bounded real CPU/cache/GPU/Display A/B/A; requires Python/Icarus/Gowin"]
fn real_resource_frames_overlap_and_retire_independently() {
    let case = cases::Case::from_env();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::var_os("GPU_RENDERER_NATIVE_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("../../target/gpu-renderer-native"));
    std::fs::create_dir_all(&out).unwrap();
    pixel_oracle(&out);
    let assets = out.join("assets");
    let mut producer = Command::new("python");
    producer
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("examples/gpu_renderer/tools/assets.py"))
        .arg("--output")
        .arg(&assets);
    bounded(&mut producer, &out.join("producer"), 60);
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap()).unwrap();
    let mut config = String::from("format 2\napplication fixture.rs\n");
    for resource in catalog["resources"].as_array().unwrap() {
        let file = resource["file"].as_str().unwrap();
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {} {}\n",
            file.trim_end_matches(".bin").to_uppercase(),
            resource["address"],
            file
        ));
    }
    // This test explicitly chooses the retired test-only compiler below;
    // the project supplies real copy constants, never a production facade mode.
    let project = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config).unwrap();
    let constants =
        project.constants() + &std::fs::read_to_string(assets.join("renderer_assets.rs")).unwrap();
    let opts = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let mut dependencies = Vec::new();
    let source = case.source(include_str!("fixtures/gpu_renderer_native.rcc"));
    std::fs::write(out.join("program.rs"), &source).unwrap();
    let ir = resolver::compile_renderer_program(
        "native resource caller",
        &source,
        &opts,
        &root.join("tests/fixtures"),
        &root.join("rcc"),
        &constants,
        &mut |p| dependencies.push(p.to_owned()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let program = rcc_backend::try_compile(ir, &opts, "main").unwrap();
    std::fs::write(
        out.join("program.hex"),
        program
            .words
            .iter()
            .map(|w| format!("{w:04x}\n"))
            .collect::<String>(),
    )
    .unwrap();
    std::fs::write(out.join("program.listing"), &program.listing).unwrap();
    std::fs::write(
        out.join("dependencies.txt"),
        dependencies
            .iter()
            .map(|p| format!("{}\n", p.display()))
            .collect::<String>(),
    )
    .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap()).unwrap();
    let mut image = BTreeMap::<usize, u64>::new();
    for resource in manifest["resources"].as_array().unwrap() {
        let address = resource["address"].as_u64().unwrap() as usize;
        for (i, b) in std::fs::read(assets.join(resource["file"].as_str().unwrap()))
            .unwrap()
            .into_iter()
            .enumerate()
        {
            *image.entry((address + i) / 8).or_default() |=
                u64::from(b) << (((address + i) % 8) * 8);
        }
    }
    std::fs::write(
        out.join("assets.hex"),
        image
            .iter()
            .map(|(a, v)| format!("@{a:x}\n{v:016x}\n"))
            .collect::<String>(),
    )
    .unwrap();
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
    let top = include_str!("../board/top.v");
    let (_, body) = top.split_once("module CpuV3GpuMemoryRequestSlot(").unwrap();
    sources.insert(
        "production-request-slot.v".into(),
        format!("module CpuV3GpuMemoryRequestSlot({body}"),
    );
    let mut paths = Vec::new();
    let mut source_names = Vec::new();
    let mut monitor_overrides = Vec::new();
    for (index, (name, source)) in sources.iter().enumerate() {
        let path = out.join(format!("source-{index}.v"));
        std::fs::write(&path, source).unwrap();
        if let Some(checked) = case.expected_error_monitor(source) {
            let monitored = out.join(format!("source-{index}-expected-error.v"));
            std::fs::write(&monitored, checked).unwrap();
            monitor_overrides.push((path, monitored.clone()));
            paths.push(monitored);
        } else {
            paths.push(path);
        }
        source_names.push(format!("source-{index}.v {}\n", name.display()));
    }
    std::fs::write(out.join("sources.txt"), source_names.concat()).unwrap();
    assert_eq!(
        monitor_overrides.len(),
        usize::from(case == cases::Case::GpuError)
    );
    std::fs::write(
        out.join("monitor-overrides.json"),
        serde_json::to_vec_pretty(&monitor_overrides).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("compiled-sources.txt"),
        paths
            .iter()
            .map(|p| format!("{}\n", p.display()))
            .collect::<String>(),
    )
    .unwrap();
    let mut tb = include_str!("gpu_renderer_native_tb.v").to_owned();
    for (key, value) in [
        ("__CORE__", CpuV3Core::verilog_identity().module_name()),
        (
            "__CACHE__",
            CpuV3DataCache::verilog_identity().module_name(),
        ),
        (
            "__SYSCTL__",
            SystemControlDevice::<4>::verilog_identity().module_name(),
        ),
        ("__GPU__", CpuV3Gpu::verilog_identity().module_name()),
        (
            "__DISPLAY__",
            FramebufferHdmi::verilog_identity().module_name(),
        ),
        (
            "__ARBITER__",
            CpuV3MemoryArbiter::verilog_identity().module_name(),
        ),
    ] {
        tb = tb.replace(key, &value);
    }
    for (key, name) in [
        ("__PAGES__", "GPU_PUI_PAGES"),
        ("__PIPE__", "GPU_PIPE"),
        ("__TARGETS__", "GPU_SLOT_TARGETS"),
        ("__RENDERER__", "GPU_RENDERER"),
        ("__OLD__", "OLD"),
        ("__DIRTY__", "DIRTY"),
        ("__SLOTA__", "GPU_PIPE_A"),
        ("__SLOTB__", "GPU_PIPE_B"),
    ] {
        tb = tb.replace(key, &global(&program, name).to_string());
    }
    for (key, name) in [("__PROGRAM__", "program.hex"), ("__ASSETS__", "assets.hex")] {
        tb = tb.replace(key, &out.join(name).to_string_lossy().replace('\\', "/"));
    }
    tb = tb.replace("__FAULT__", "0");
    std::fs::write(out.join("tb-base.v"), &tb).unwrap();
    let entry = program
        .debug
        .functions
        .iter()
        .find(|f| f.name == "renderer_frame")
        .unwrap()
        .addr
        .0;
    let tb = case.testbench(tb, entry);
    std::fs::write(out.join("tb.v"), tb).unwrap();
    let models = PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
        .join("IDE/simlib/gw2a/prim_sim.v");
    let exe = out.join("sim.vvp");
    let mut compiler =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compiler
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(&exe)
        .arg(models)
        .args(paths)
        .arg(out.join("tb.v"));
    bounded(&mut compiler, &out.join("compile"), 120);
    let mut sim = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    sim.arg(exe);
    bounded(&mut sim, &out.join("run"), 1800);
    let trace = std::fs::read_to_string(out.join("run.log")).unwrap();
    assert!(trace.contains(case.marker()));
    assert_eq!(
        trace.matches("NATIVE_EXPECTED_GPU_RO_ERROR").count(),
        usize::from(case == cases::Case::GpuError)
    );
    println!("{trace}");
}
