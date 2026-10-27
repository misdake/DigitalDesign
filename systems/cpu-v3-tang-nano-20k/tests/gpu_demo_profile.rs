//! One production classic-scene frame, timing only; no image oracle.
use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache};
use cpu_v3_tang_nano_20k::{
    hardware::{CpuV3MemoryArbiter, SystemControlDevice},
    CpuV3Gpu, FramebufferHdmi,
};
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{
    any::Any,
    cell::Cell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};
#[path = "../build_support/rcc_modules.rs"]
#[allow(dead_code)]
mod resolver;
#[test]
#[ignore = "one full classic frame; bounded Icarus timing profile, no image comparison"]
fn classic_frame_timing_profile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cpu_prep = std::env::var_os("GPU_DEMO_CPU_PREP_PROFILE").is_some();
    let native_profile = std::env::var_os("GPU_DEMO_NATIVE_PROFILE").is_some();
    let small = native_profile || std::env::var_os("GPU_DEMO_PROFILE_SMALL").is_some();
    let artifacts = root.join("../../target/maintainer/demo-toggle-20261023");
    let variant = std::env::var("GPU_DEMO_PROFILE_VARIANT").ok();
    let experiment = std::env::var("GPU_DEMO_PREPARE_MODE").ok();
    assert!(experiment
        .as_deref()
        .is_none_or(|v| v == "retained" || v == "pingpong" || v == "pingpong-present"));
    let pipeline = experiment
        .as_deref()
        .is_some_and(|v| v.starts_with("pingpong"));
    let present = experiment.as_deref() == Some("pingpong-present");
    assert!(experiment.is_none() || variant.is_some());
    if let Some(name) = &variant {
        assert!(
            cpu_prep
                && !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        );
    }
    let out = if native_profile {
        root.join("../../target/maintainer/stream-frame-20261024/input")
    } else {
        artifacts.join(match &variant {
            Some(name) => format!("cpu-prep-{name}"),
            None if cpu_prep => "cpu-prep".into(),
            None => "profile".into(),
        })
    };
    let boot = artifacts.join("boot");
    std::fs::create_dir_all(&out).unwrap();
    let constants = std::fs::read_to_string(boot.join("boot_resources.rs")).unwrap();
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = cpu_v3_tang_nano_20k::boot::SINGLE_BOOT_STACK;
    // One measured render with a previous-interval fixture, so numeric UI is
    // exercised without rendering a warm-up frame. Only setup/stop differ.
    let source = include_str!("../examples/gpu_demo/rcc/main.rs").replace("\r\n", "\n");
    let source = source.replace("let mut completed = false;", "let mut completed = true; STATS.as_array()[0u16] = time_milliseconds() - 17; STATS.as_array()[2u16] = 1;");
    let source = source.replace(
        "let mut rendered_us: u16 = 65535;",
        "let mut rendered_us: u16 = 17000;",
    );
    let begin = source.find("    // Bounded verification waits").unwrap();
    let end = source[begin..].find("\n}").unwrap() + begin;
    let mut source = source;
    source.replace_range(begin..end, "    halt(0x600d);\n");
    if pipeline || small {
        source = source
            .replace("count: DEMO_ROOM_COUNT,", "count: 1,")
            .replace("mesh.count = DEMO_BUNNY_COUNT;", "mesh.count = 2;");
    }
    let reference_source = if experiment.is_some() {
        source.replacen("use crate::dsl_rt::*;", "use crate::dsl_rt::*;\nmod overlay; use overlay::*;\nstatic CELLS:Buf<u16,375>=Buf::new([0;375]);", 1)
            .replace("demo_ui_reset(STATS.as_array())", "demo_overlay_init(CELLS.as_array(), STATS.as_array())")
            .replace("demo_ui_frame_begin(STATS.as_array(), elapsed, completed)", "demo_overlay_begin(CELLS.as_array(), STATS.as_array(), now, completed)")
    } else {
        source.clone()
    };
    if let Some(mode) = &experiment {
        source = source.replacen("use crate::dsl_rt::*;", "use crate::dsl_rt::*;\nmod overlay; use overlay::*;\nstatic CELLS:Buf<u16,375>=Buf::new([0;375]);", 1)
            .replace("demo_ui_reset(STATS.as_array())", "demo_overlay_init(CELLS.as_array(), STATS.as_array())")
            .replace("demo_ui_frame_begin(STATS.as_array(), elapsed, completed)", "demo_overlay_begin(CELLS.as_array(), STATS.as_array(), now, completed)");
        for (marker, index, size) in [
            (5, 0, 32),
            (7, 1, 64),
            (8, 2, 64),
            (10, 3, 64),
            (11, 4, 400),
        ] {
            source = source.replace(
                &format!("dev_send(7, 1, {marker});"),
                &format!(
                    "exp_note({index}, GPU_CB.object_words - {size}); dev_send(7, 1, {marker});"
                ),
            );
        }
        for stage in (1..=15).rev() {
            source = source.replace(
                &format!("dev_send(7, 1, {stage});"),
                &format!("dev_send(7, 1, {});", stage + 100),
            );
        }
        let begin = source.find("        dev_send(7, 1, 112);").unwrap();
        let end = source.find("\n}\nfn main()").unwrap();
        let entry = if mode == "retained" {
            "exp_retained_prefix"
        } else if present {
            "exp_pingpong_present"
        } else {
            "exp_pingpong"
        };
        source.replace_range(
            begin..end,
            &format!("        {entry}();\n    }}\n    halt(0x600d);\n"),
        );
        source = source.replacen(
            "use crate::dsl_rt::*;",
            "use crate::dsl_rt::*;\nmod template_experiment;",
            1,
        );
    }
    std::fs::write(out.join("program.rs"), &source).unwrap();
    let parsed =
        rcc::frontend::compile_program_named("Classic profile", &source, &options, &mut |name| {
            resolver::load_module(
                &root.join("examples/gpu_demo/rcc"),
                &root.join("rcc"),
                &constants,
                name,
                &mut |_| {},
            )
        })
        .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "demo_one_frame_main").unwrap();
    std::fs::write(out.join("program.listing"), &program.listing).unwrap();
    std::fs::write(out.join("program.debug"), program.debug.render()).unwrap();
    std::fs::write(
        out.join("program.hex"),
        program
            .words
            .iter()
            .map(|w| format!("{w:04x}\n"))
            .collect::<String>(),
    )
    .unwrap();
    if cpu_prep && variant.is_none() {
        assert_eq!(
            std::fs::read(out.join("program.hex")).unwrap(),
            std::fs::read(artifacts.join("profile/program.hex")).unwrap(),
            "CPU preparation profile must use the accepted frame's exact image"
        );
    }
    let mut sources = BTreeMap::new();
    for p in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
        VerilogProject::generate::<CpuV3Gpu>().unwrap(),
        VerilogProject::generate::<FramebufferHdmi>().unwrap(),
        VerilogProject::generate::<CpuV3MemoryArbiter>().unwrap(),
    ] {
        for (name, source) in p.files {
            if !source.contains("module tb") {
                if let Some(old) = sources.insert(name, source.clone()) {
                    assert_eq!(old, source);
                }
            }
        }
    }
    let mut paths = Vec::new();
    for (i, source) in sources.values().enumerate() {
        let p = out.join(format!("source_{i}.v"));
        std::fs::write(&p, source).unwrap();
        if variant.is_some() {
            assert_eq!(
                source.as_bytes(),
                std::fs::read(artifacts.join(format!("profile/source_{i}.v"))).unwrap(),
                "Software experiments must retain exact RTL"
            );
        }
        paths.push(p);
    }
    let mut tb = include_str!("gpu_demo_profile_tb.v")
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
    if small {
        tb = tb.replace("draws!=24", "draws!=4");
    }
    for (key, file) in [("__PROGRAM__", "program.hex"), ("__ASSETS__", "assets.hex")] {
        tb = tb.replace(key, &out.join(file).to_string_lossy().replace('\\', "/"));
    }
    if cpu_prep && !pipeline {
        tb = tb.replacen(
            "always @(posedge clk) if(!reset)begin",
            &format!(
                "{}\nalways @(posedge clk) if(!reset)begin",
                include_str!("gpu_demo_cpu_prep.vh")
            ),
            1,
        );
        tb = tb.replace(
            "__PREP_ARENA__",
            &out.join("arena.hex").to_string_lossy().replace('\\', "/"),
        );
    }
    if pipeline {
        tb = tb.replacen(
            "integer command_bytes=0",
            &format!(
                "{}\ninteger command_bytes=0",
                include_str!("gpu_demo_pipeline_trace.vh")
            ),
            1,
        );
        let at = tb.find(" wait(halted||fault);#1;").unwrap();
        tb.truncate(at);
        tb.push_str(&format!(" wait(halted||fault);#1;\n if(fault||signal!==16'h600d||starts!=2||draws!=8||uis!=2||swaps!={}||terms!=writes||pipe_completions!=2||pipe_overlap==0||pipe_prepared_busy!=2)$fatal(1,\"pipeline coverage\");\n $display(\"PIPELINE_PASS cycles=%0d overlap=%0d writes=%0d terminals=%0d\",cycle,pipe_overlap,writes,terms);\n $finish;end\n",usize::from(present)));
        tb.push_str("\nendmodule\n");
        tb = tb.replace("__PIPE_OUT__", &out.to_string_lossy().replace('\\', "/"));
    }
    let config = std::fs::read_to_string(boot.join("demo-repack.conf")).unwrap();
    let mut seed = String::new();
    for line in config.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.first() == Some(&"copy") {
            let address = u32::from_str_radix(parts[2].trim_start_matches("0x"), 16).unwrap();
            let bytes = std::fs::read(boot.join(parts[3])).unwrap();
            seed += &format!("@{:x}\n", address / 8);
            for q in bytes.as_chunks::<8>().0 {
                seed += &format!("{:016x}\n", u64::from_le_bytes(*q));
            }
        }
    }
    std::fs::write(out.join("assets.hex"), seed).unwrap();
    if native_profile {
        std::fs::write(out.join("tb.v"), tb).unwrap();
        for (name, early, groups) in [("legacy", false, false), ("stream", true, false)] {
            let dir = out.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            for (path, source) in digital_design_hardware_gowin::sdram_memory_controller::combination::rtl_sources_with_options(40, early, groups).unwrap() {
                let source = if name == "legacy" { source.replace(".EARLY_GRANT(1'b0)", ".EARLY_GRANT(1'b1)") } else { source };
                std::fs::write(dir.join(path.file_name().unwrap()), source).unwrap();
            }
        }
        println!("NATIVE_FRAME_INPUT_READY {}", out.display());
        return;
    }
    let mut reference = None;
    let expected = if experiment.is_some() {
        let compile = |phase: u16| {
            let src = reference_source.replace(
                "let mut phase: u16 = 0;",
                &format!("let mut phase: u16 = {phase};"),
            );
            let ir = rcc::frontend::compile_program_named(
                "Reference scene",
                &src,
                &options,
                &mut |name| {
                    resolver::load_module(
                        &root.join("examples/gpu_demo/rcc"),
                        &root.join("rcc"),
                        &constants,
                        name,
                        &mut |_| {},
                    )
                },
            )
            .unwrap();
            rcc_backend::try_compile(ir, &options, "demo_one_frame_main")
                .unwrap()
                .words
        };
        let first = record_prefix(&compile(0), &out.join("assets.hex"));
        let next = record_prefix(&compile(256), &out.join("assets.hex"));
        if experiment.as_deref() == Some("retained") {
            let actual = record_prefix(&program.words, &out.join("assets.hex"));
            assert_eq!(&actual[..16384], &next[..16384], "Retained commands");
            assert_eq!(
                &actual[32768..49152],
                &next[32768..49152],
                "Retained dynamic objects"
            );
            let clone = relocate_arena(&first);
            assert_eq!(
                &actual[16384..32768],
                &clone[16384..32768],
                "Inactive command slot"
            );
            assert_eq!(&actual[49152..], &clone[49152..], "Inactive object slot");
            Some(actual)
        } else {
            reference = Some((first, next));
            None
        }
    } else if variant.is_some() {
        let old = read_words(&artifacts.join("profile/program.hex"));
        let baseline = record_prefix(&old, &out.join("assets.hex"));
        assert_eq!(
            baseline,
            record_prefix(&program.words, &out.join("assets.hex")),
            "CB/object arenas must remain byte-identical"
        );
        Some(baseline)
    } else {
        None
    };
    std::fs::write(out.join("tb.v"), tb).unwrap();
    let exe = out.join("profile.vvp");
    let mut command =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    command
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(&exe)
        .arg(
            PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap())
                .join("IDE/simlib/gw2a/prim_sim.v"),
        )
        .args(paths)
        .arg(out.join("tb.v"));
    run(&mut command, &out.join("compile.log"));
    let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    command.arg(exe);
    run(&mut command, &out.join("run.log"));
    let text = std::fs::read_to_string(out.join("run.log")).unwrap();
    let marker = if pipeline {
        "PIPELINE_PASS"
    } else if cpu_prep {
        "CPU_PREP_PASS"
    } else {
        "CLASSIC_PROFILE_PASS"
    };
    assert!(text.contains(marker), "{text}");
    if let Some(expected) = expected {
        let actual = read_arena(&out.join("arena.hex"));
        assert_eq!(
            actual, expected,
            "RTL submission must publish the complete expected arenas"
        );
        println!("CPU_PREP_ARENA_PASS words={}", actual.len());
    }
    if let Some((first, next)) = reference {
        let a = read_arena(&out.join("submit-0.hex"));
        let b = read_arena(&out.join("submit-1.hex"));
        assert_eq!(&a[..16384], &first[..16384]);
        assert_eq!(&a[32768..49152], &first[32768..49152]);
        let mut next = relocate_arena(&next);
        if present {
            next[49152] = 0x2000;
        }
        assert_eq!(&b[16384..32768], &next[16384..32768]);
        assert_eq!(&b[49152..], &next[49152..]);
        println!(
            "PIPELINE_ARENA_PASS two distinct poses, alternating slots, exact reference inputs"
        );
    }
    if cpu_prep {
        println!(
            "{}",
            text.lines().find(|line| line.starts_with(marker)).unwrap()
        );
    } else {
        println!("{text}");
    }
}
fn read_arena(path: &Path) -> Vec<u16> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("//") && !l.trim().is_empty())
        .flat_map(|l| {
            let q = u64::from_str_radix(l.trim(), 16).unwrap();
            (0..4).map(move |i| (q >> (i * 16)) as u16)
        })
        .collect()
}
fn relocate_arena(input: &[u16]) -> Vec<u16> {
    let mut out = vec![0; 65536];
    out[16384..32768].copy_from_slice(&input[..16384]);
    out[49152..].copy_from_slice(&input[32768..49152]);
    for i in (16384..32768).step_by(2) {
        if out[i + 1] >> 12 == 1 {
            out[i + 1] += 16;
        }
    }
    out[49152 + 768] += 256;
    out
}
fn read_words(path: &Path) -> Vec<u16> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| u16::from_str_radix(line.trim(), 16).unwrap())
        .collect()
}
// Execute only CPU recording. Control peers never execute GPU commands.
fn record_prefix(words: &[u16], assets: &Path) -> Vec<u16> {
    let mut cpu = cpu_v3::CpuV3Sim::default();
    cpu.set_boot_window(words);
    let mut address = 0usize;
    for line in std::fs::read_to_string(assets).unwrap().lines() {
        if let Some(hex) = line.strip_prefix('@') {
            address = usize::from_str_radix(hex, 16).unwrap() * 4;
        } else if !line.is_empty() {
            let q = u64::from_str_radix(line, 16).unwrap();
            for i in 0..4 {
                cpu.physical_memory_mut()[address + i] = (q >> (i * 16)) as u16;
            }
            address += 4;
        }
    }
    let stopped = Rc::new(Cell::new(false));
    for kind in [0, 3, 4, 7] {
        cpu.attach_device(
            kind,
            Box::new(RecordPeer {
                kind,
                stopped: stopped.clone(),
            }),
        );
    }
    for _ in 0..2_000_000 {
        let result = cpu.step().unwrap();
        assert!(
            !matches!(result, cpu_v3::StepOutcome::Halted { .. }),
            "Recording halted before submit: {result:?}, pc={:04x}, link={:04x}",
            cpu.pc(),
            cpu.register(14).unwrap()
        );
        if stopped.get() {
            assert_eq!(cpu.data_segments(), [0, 1, 2, 3]);
            return (0x490000 / 2..0x4b0000 / 2)
                .map(|i| cpu.physical_memory(cpu_v3::PhysicalWordAddress::new(i)))
                .collect();
        }
    }
    panic!("CPU record step limit");
}
struct RecordPeer {
    kind: u8,
    stopped: Rc<Cell<bool>>,
}
impl cpu_v3::Device for RecordPeer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        if self.kind == 3 && channel == 2 {
            0x20
        } else {
            0
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        if self.kind == 7 && channel == 1 && value == 13 {
            self.stopped.set(true);
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(command: &mut Command, log: &Path) {
    let mut child = command
        .stdout(std::fs::File::create(log).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let begin = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if begin.elapsed() > Duration::from_secs(3600) {
            child.kill().unwrap();
            panic!("bounded profile timeout: {}", log.display());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}: {}",
        log.display(),
        String::from_utf8_lossy(&result.stderr)
    );
}
