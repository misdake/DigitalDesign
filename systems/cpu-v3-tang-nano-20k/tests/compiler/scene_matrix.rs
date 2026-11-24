//! Exact old-source oracle and equal-input six-call clocked CPU experiment.
use super::*;
use std::path::Path;

const BEFORE: &str = include_str!("../fixtures/scene_matrix_before.rs");
const AFTER: &str = concat!(
    include_str!("../../rcc/gpu_scene_math.rs"),
    include_str!("../support/rcc/gpu_scene_legacy.rs")
);
const SOURCE: &str = r#"
mod scene;
static CAMERA:Buf<u16,32>=Buf::new([0;32]);
static MODEL:Buf<u16,42>=Buf::new([0;42]);
static OUTPUT:Buf<u16,46>=Buf::new([0;46]);
fn main(){
    let out=unsafe{OUTPUT.as_ptr().add(1).as_u16_array()};
    let mut i:u16=0;
    while i<6 {gpu_scene_matrix_prepare(CAMERA.as_array(),MODEL.as_array(),out);i+=1;}
    halt(0x600d);
}
"#;
fn compile(math: &str, enabled: bool) -> (CpuV3Program, String) {
    let mut opts = CompilerOptions {
        stack_init: 0xfff0,
        ..CompilerOptions::default()
    };
    opts.inline.enabled = enabled;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let parsed = compile_program_named("matrix-workload.rs", SOURCE, &opts, &mut |name| {
        if name == "scene" {
            let mut selected =
                String::from("use crate::dsl_rt::*;mod gpu_schema;use gpu_schema::*;\n");
            for name in [
                "gpu_scene_fix_read",
                "gpu_scene_fix_write",
                "gpu_scene_vec_read",
                "gpu_scene_matrix_prepare",
            ] {
                let at = math.find(&format!("pub fn {name}(")).unwrap();
                if math[..at].trim_end().ends_with("#[inline]") {
                    selected.push_str("#[inline]\n");
                }
                let tail = &math[at..];
                let mut depth = 0;
                let mut opened = false;
                for (i, c) in tail.char_indices() {
                    if c == '{' {
                        depth += 1;
                        opened = true;
                    } else if c == '}' {
                        depth -= 1;
                    }
                    if opened && depth == 0 {
                        selected.push_str(&tail[..=i]);
                        selected.push('\n');
                        break;
                    }
                }
            }
            Ok(selected)
        } else if name == "gpu_schema" {
            let schema = std::fs::read_to_string(root.join("gpu_schema.rs")).unwrap();
            Ok(schema[..schema.find("#[derive").unwrap()].to_string())
        } else {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        }
    })
    .unwrap();
    let (program, report) = try_compile_with_inline_report(parsed, &opts, "main").unwrap();
    (program, report.to_string())
}
fn address(p: &CpuV3Program, name: &str) -> usize {
    match p.debug.globals.iter().find(|g| g.name == name).unwrap().loc {
        rcc::VarLoc::Global(a) => usize::from(a),
        _ => panic!("not static"),
    }
}
fn run(p: &CpuV3Program, camera: &[u16; 32], model: &[u16; 42]) -> (Vec<u16>, usize) {
    let mut cpu = CpuV3Sim::default();
    cpu.load_program(p.code_base, &p.words).unwrap();
    let ca = address(p, "CAMERA");
    let mo = address(p, "MODEL");
    let out = address(p, "OUTPUT");
    let guards: Vec<_> = (0..46).map(|i| 0xa55au16.wrapping_add(i * 37)).collect();
    let mem = cpu.physical_memory_mut();
    mem[ca..ca + 32].copy_from_slice(camera);
    mem[mo..mo + 42].copy_from_slice(model);
    mem[out..out + 46].copy_from_slice(&guards);
    for steps in 1..=250_000 {
        if let cpu_v3::StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d);
            let mem = cpu.physical_memory_mut();
            assert_eq!(&mem[ca..ca + 32], camera);
            assert_eq!(&mem[mo..mo + 42], model);
            assert_eq!(mem[out], guards[0]);
            assert_eq!(&mem[out + 43..out + 46], &guards[43..]);
            return (mem[out..out + 46].to_vec(), steps);
        }
    }
    panic!("matrix architectural run exceeded 250000 steps")
}
#[test]
fn matrix_is_bit_exact_for_signed_fractional_overflow_inputs_and_guards() {
    let (before, _) = compile(BEFORE, false);
    let (after, report) = compile(AFTER, true);
    let (after_no_inline, _) = compile(AFTER, false);
    assert!(report.contains("Applied"), "{report}");
    let edge = [0, 1, 0xffff, 0x7fff, 0x8000, 0x5555, 0xaaaa, 0xfffe];
    let mut seed = 0x3124abcdu32;
    for case in 0..96 {
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u16
        };
        let camera = std::array::from_fn(|i| {
            if case < 16 {
                edge[(case + i) % edge.len()]
            } else {
                next()
            }
        });
        let model = std::array::from_fn(|i| {
            if case < 16 {
                edge[(case * 3 + i) % edge.len()]
            } else {
                next()
            }
        });
        let expected = run(&before, &camera, &model).0;
        assert_eq!(
            expected,
            run(&after, &camera, &model).0,
            "inline case {case}"
        );
        assert_eq!(
            expected,
            run(&after_no_inline, &camera, &model).0,
            "no-inline case {case}"
        );
    }
    println!("96 fixed-seed cases x 6 preparations, inline on/off, 42 coefficient words exact; unaligned output guards and complete inputs unchanged\n{report}");
}
#[test]
fn matrix_six_calls_reduce_clocked_core_cycles_and_architectural_steps() {
    let (before, _) = compile(BEFORE, false);
    let hints = BEFORE
        .replace("pub fn gpu_scene_fix_", "#[inline]\npub fn gpu_scene_fix_")
        .replace(
            "pub fn gpu_scene_vec_read",
            "#[inline]\npub fn gpu_scene_vec_read",
        );
    let (hints, report_hint) = compile(&hints, true);
    let (after, report) = compile(AFTER, true);
    let mut measured = Vec::new();
    for (label, p) in [
        ("before", &before),
        ("inline_only", &hints),
        ("after", &after),
    ] {
        let arch = architectural_steps(p);
        assert_eq!(arch.0, 0x600d);
        let clocked = core_cycles(p, 500_000);
        assert_eq!(clocked.0, 0x600d);
        println!(
            "matrix six calls; {label}: cycles={} retired={} words={} steps={} lowest_SP={:#06x}",
            clocked.1,
            clocked.2,
            p.words.len(),
            arch.1,
            arch.2
        );
        measured.push((clocked.1, arch.1));
        if let Some(out) = std::env::var_os("MATRIX_EVIDENCE_DIR") {
            let out = Path::new(&out);
            std::fs::create_dir_all(out).unwrap();
            std::fs::write(out.join(format!("{label}-listing.txt")), &p.listing).unwrap();
            std::fs::write(out.join(format!("{label}-debug.txt")), p.debug.render()).unwrap();
        }
    }
    assert!(measured[2].0 < measured[0].0, "{measured:?}");
    assert!(measured[2].1 < measured[0].1, "{measured:?}");
    println!("inline-only report:\n{report_hint}\nfinal report:\n{report}");
}

#[path = "../../build_support/rcc_modules.rs"]
#[allow(dead_code, unused_imports)]
mod resolver;

#[test]
#[ignore = "bounded actual Bloxorz CPU emu + functional GPU comparison; run intentionally"]
fn bloxorz_one_play_frame_has_identical_color_depth_and_lower_matrix_cost() {
    use cpu_v3_tang_nano_20k::{boot, system_emu::CpuV3SystemEmu, system_sim::VBlankMode};
    use std::{collections::BTreeMap, fs, time::Instant};
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::var_os("MATRIX_EVIDENCE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../target/matrix-source-opt"));
    fs::create_dir_all(&out).unwrap();
    let assets = out.join("bloxorz-assets");
    assert!(std::process::Command::new("python")
        .arg("-B")
        .arg(root.join("examples/game_bloxorz/tools/assets.py"))
        .arg("--output")
        .arg(&assets)
        .status()
        .unwrap()
        .success());
    let catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(assets.join("assets.json")).unwrap()).unwrap();
    let mut config = String::from("format 2\ngpu-api\napplication main.rs\n");
    let mut resources = Vec::new();
    for r in catalog["resources"].as_array().unwrap() {
        let name = r["file"].as_str().unwrap();
        let base = r["address"].as_u64().unwrap() as usize;
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {base} {name}\n",
            name.trim_end_matches(".bin").to_uppercase()
        ));
        resources.push(boot::BootResource::copy(
            base,
            fs::read(assets.join(name)).unwrap(),
        ));
    }
    let constants = boot::parse_boot_project(&config).unwrap().constants()
        + &fs::read_to_string(assets.join("renderer_assets.rs")).unwrap();
    let app = root.join("examples/game_bloxorz/rcc");
    let original = fs::read_to_string(app.join("main.rs")).unwrap();
    assert_eq!(original.matches("run(0, false, 128);").count(), 1);
    let source = original
        .replace("run(0, false, 128);", "run(1, false, 128);halt(0x600d);")
        .replace(
            "session_init();",
            "session_init();level_reset(0);SESSION[0u16]=PLAY;BACKGROUND[0u16]=0;",
        );
    fs::write(out.join("bounded-bloxorz-main.rs"), &source).unwrap();
    let mut old = AFTER[..AFTER.find("// Writes exactly 42").unwrap()]
        .replace("#[inline]\r\n", "")
        .replace("#[inline]\n", "");
    old.push_str(&BEFORE[BEFORE.find("pub fn gpu_scene_matrix_prepare(").unwrap()..]);
    old.push_str(&AFTER[AFTER.find("pub fn gpu_scene_draw_finish(").unwrap()..]);
    let mut reference = None;
    let mut results = Vec::new();
    for (label, math) in [("before", old.as_str()), ("after", AFTER)] {
        let work = out.join(label);
        let library = work.join("library");
        fs::create_dir_all(&library).unwrap();
        for entry in fs::read_dir(root.join("rcc")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|x| x == "rs") {
                fs::copy(&path, library.join(path.file_name().unwrap())).unwrap();
            }
        }
        fs::write(library.join("gpu_scene_math.rs"), math).unwrap();
        let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
        options.stack_init = 0xc000;

        let parsed = resolver::compile_explicit_gpu_program_checked_with_sources(
            "matrix-bloxorz",
            &source,
            &options,
            &app,
            &library,
            &constants,
            &[root.join("applications/bloxorz/rcc")],
            &rcc::frontend::rustc_check::RustcCheck::new(work.join("checked")),
            &mut |_| {},
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let static_words = parsed.debug.static_data_end;
        let (p, inline) = try_compile_with_inline_report(parsed, &options, "main").unwrap();
        fs::write(work.join("inline.txt"), inline.to_string()).unwrap();
        fs::write(work.join("listing.txt"), &p.listing).unwrap();
        fs::write(work.join("debug.txt"), p.debug.render()).unwrap();
        let matrix = u16::try_from(
            p.debug
                .functions
                .iter()
                .find(|f| f.name == "gpu_scene_matrix_prepare")
                .unwrap()
                .addr
                .0,
        )
        .unwrap();
        let mut opcodes = BTreeMap::new();
        let mut disasm = BTreeMap::new();
        for line in cpu_v3::disassemble_words(&p.words, p.code_base) {
            let mut opcode = line.text.split_whitespace().next().unwrap().to_string();
            if ["mov", "add", "mul", "neg"].contains(&opcode.as_str())
                && line
                    .text
                    .split_whitespace()
                    .nth(1)
                    .is_some_and(|arg| arg.starts_with('f'))
            {
                opcode = format!("fpu_{opcode}");
            }
            disasm.insert(line.address, opcode.clone());
            if line.wide {
                disasm.insert(line.address + 1, opcode);
            }
        }
        let bytes: Vec<_> = p.words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let package = boot::pack_application_resources(&bytes, static_words, &resources).unwrap();
        fs::write(work.join("application.bin"), &package).unwrap();
        let mut native =
            CpuV3SystemEmu::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
                .unwrap()
                .with_functional_gpu();
        let wall = Instant::now();
        let mut halted = false;
        let mut starts = Vec::new();
        let mut presents = 0;
        let mut active = None;
        let mut last_pc = matrix;
        let mut matrix_calls = Vec::new();
        for _ in 0..4_000_000 {
            assert!(
                wall.elapsed().as_secs_f64() < 90.,
                "bounded native host timeout"
            );
            if native.waiting_for_vblank() && native.advance_vblank() {
                presents += 1;
            }
            let cycle = native.cycles();
            let (exec, gprs) = {
                let core = native.core();
                (
                    core.executing_instruction_address(),
                    core.architectural_gprs(),
                )
            };
            if let Some((_, pc)) = exec {
                if let Some((ret, begin)) = active {
                    if pc == ret {
                        matrix_calls.push(cycle - begin);
                        active = None;
                    }
                }
                if pc == matrix {
                    assert!(active.is_none());
                    active = Some((gprs[14], cycle));
                }
                last_pc = pc;
            }
            if active.is_some() {
                *opcodes
                    .entry(
                        disasm
                            .get(&last_pc)
                            .cloned()
                            .unwrap_or_else(|| "unknown".into()),
                    )
                    .or_insert(0u64) += 1;
            }
            let output = native
                .step_observing(|write, _| {
                    if write.device == 4 && write.channel == 1 {
                        starts.push(cycle);
                    }
                    true
                })
                .unwrap()
                .unwrap();
            assert!(
                !output.fault,
                "native fault {label} {} at {}",
                output.fault_code, output.fault_pc
            );
            if output.halted {
                assert_eq!(output.halt_signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted);
        assert!(native.gpu().last_error().is_none());
        assert_eq!(presents, 1);
        assert_eq!(starts.len(), 2);
        assert!(active.is_none());
        assert!(!matrix_calls.is_empty());
        let base = native.display_state().active_base as usize;
        let mem = native.physical_memory();
        let content = (
            mem[base..base + 102400].to_vec(),
            mem[0x9000 * 64..0x9000 * 64 + 102400].to_vec(),
            native.render_active_framebuffer(),
        );
        let result = serde_json::json!({"variant":label,"logic_cycles":native.cycles(),"native_memory_cycles":native.native_cycles(),
            "host_seconds":wall.elapsed().as_secs_f64(),"matrix_call_cycles":matrix_calls,"matrix_cycles":matrix_calls.iter().sum::<u64>(),
            "matrix_instruction_residency":opcodes,"gpu_starts":starts,"presents":presents,"code_words":p.words.len(),"static_words":static_words,
            "matrix_frame_words":p.debug.functions.iter().find(|f|f.name=="gpu_scene_matrix_prepare").unwrap().frame_size,
            "scope":"actual Bloxorz main, bounded first PLAY frame stage1, solid background, neutral input; CPU emu/cache/native memory, functional GPU; immediate host vblank; no GPU/Display timing or contention"});
        fs::write(
            work.join("measurement.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        println!("{result}");
        results.push(result);
        if let Some(expected) = &reference {
            assert_eq!(&content, expected, "full unmasked color, depth and display");
        } else {
            reference = Some(content);
        }
    }
    assert_eq!(
        results[0]["matrix_call_cycles"].as_array().unwrap().len(),
        results[1]["matrix_call_cycles"].as_array().unwrap().len()
    );
    assert!(results[1]["matrix_cycles"].as_u64() < results[0]["matrix_cycles"].as_u64());
    assert!(results[1]["logic_cycles"].as_u64() < results[0]["logic_cycles"].as_u64());
    fs::write(
        out.join("bloxorz-comparison.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"full_color_depth_display_exact":true,"results":results}),
        )
        .unwrap(),
    )
    .unwrap();
}
