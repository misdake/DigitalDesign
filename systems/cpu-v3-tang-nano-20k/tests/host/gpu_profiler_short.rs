//! Actual diagnostic applications and gate wrapper; peers only retire control work.
use super::*;

#[test]
fn repeated_rtt_reaches_only_consumer_runtime_with_pinned_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let board = root.join("examples/gpu_profiler_board");
    let out = root.join("../../target/profiler-short-rtt-control");
    assert!(std::process::Command::new("python")
        .env_remove("GPU_PROFILER_MANIFEST")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(board.join("tools/assets.py"))
        .args(["--short-case", "rtt", "--output"])
        .arg(&out)
        .status()
        .unwrap()
        .success());
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let manifest = out.join("build.json");
    std::fs::write(&manifest, serde_json::to_vec(&serde_json::json!({
        "schema":"digitaldesign.gpu-capture-build", "version":2,"command_abi":4,
        "manifest_id":"5254".repeat(16), "layout":resolver::RendererLayoutProfile::Column16.description(),
        "resources":catalog["resources"],"options":{}
    })).unwrap()).unwrap();
    let mut project =
        String::from("format 2\nrenderer\napplication examples/gpu_profiler_board/rcc/rtt.rs\n");
    for resource in catalog["resources"].as_array().unwrap() {
        let name = resource["file"].as_str().unwrap();
        project.push_str(&format!(
            "copy GPU_RENDERER_{} {} {name}\n",
            name.trim_end_matches(".bin").to_uppercase(),
            resource["address"]
        ));
    }
    let constants = cpu_v3_tang_nano_20k::boot::parse_boot_project(&project)
        .unwrap()
        .constants()
        + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    let configuration = resolver::RendererCaptureConfiguration::from_manifest(
        board.join("rcc/profiler_gate.rs"),
        &manifest,
    )
    .unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let ir = resolver::compile_explicit_gpu_program_checked_with_sources(
        "actual repeated RTT profiler",
        &std::fs::read_to_string(board.join("rcc/rtt.rs")).unwrap(),
        &options,
        &board.join("rcc"),
        &root.join("rcc"),
        &constants,
        &[root.join("examples/gpu_renderer_rtt/rcc")],
        &configuration,
        &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    let runtime = program
        .debug
        .functions
        .iter()
        .find(|f| f.name == "profiler_gate_run")
        .unwrap()
        .addr
        .0 as u16;
    let global = |name: &str| {
        program
            .debug
            .globals
            .iter()
            .find_map(|g| match g.loc {
                rcc::VarLoc::Global(a) if g.name == name => Some(usize::from(a)),
                _ => None,
            })
            .unwrap()
    };
    let header = global("GPU_CAPTURE_HEADER");
    let ranges = global("GPU_CAPTURE_RANGES");
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for resource in catalog["resources"].as_array().unwrap() {
        let data = std::fs::read(out.join(resource["file"].as_str().unwrap())).unwrap();
        let base = resource["address"].as_u64().unwrap() as usize;
        for (i, b) in data.into_iter().enumerate() {
            let address = base + i;
            let word = &mut cpu.physical_memory_mut()[address / 2];
            *word = (*word & !(255 << ((address & 1) * 8))) | (u16::from(b) << ((address & 1) * 8));
        }
    }
    cpu.physical_memory_mut()[0x600000 / 2..0x680000 / 2].fill(0xa55a);
    let state = attach(&mut cpu, Some(vec![vec![]; 6]));
    {
        let mut s = state.borrow_mut();
        s.automatic = true;
        s.capture_dynamic = true;
        s.targets = Some(vec![
            (0x600000, 0x640000),
            (0x440000, 0x480000),
            (0x600000, 0x640000),
            (0x400000, 0x480000),
            (0x600000, 0x640000),
            (0x440000, 0x480000),
        ]);
    }
    let mut calls = 0;
    for _ in 0..12_000_000 {
        if cpu.pc() == runtime {
            calls += 1;
            assert_eq!(
                state.borrow().starts.len(),
                calls * 2 - 1,
                "producer gate entered runtime"
            );
            let memory = cpu.physical_memory_mut();
            let h = &memory[header..header + 64];
            assert_eq!((h[39], h[40]), (2, 4));
            assert_ne!(h[27], 0x60);
            assert!(
                memory[ranges..ranges + usize::from(h[36]) * 8]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .any(|r| r[2] == 0 && r[3] == 0x60 && r[4] == 8192 && r[5] == 3),
                "missing physical 204800-byte RTT texture input"
            );
            if calls == 3 {
                break;
            }
        }
        if let cpu_v3::StepOutcome::Halted { signal } = cpu.step().unwrap() {
            panic!("halted {signal:04x}");
        }
    }
    assert_eq!(calls, 3);
    assert_eq!((state.borrow().starts.len(), state.borrow().swaps), (5, 2));
    assert!(
        cpu.physical_memory_mut()[0x600000 / 2..0x680000 / 2]
            .iter()
            .all(|w| *w == 0xa55a),
        "control peer must not invent raster output"
    );
}
