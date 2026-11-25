//! Exact unit-triangle control path; the batch target qualifies receiver-cell timing.
use cpu_v3::{rcc_backend, StepOutcome};
use cpu_v3_tang_nano_20k::{
    boot,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[path = "../../build_support/application_entry.rs"]
mod application_entry;
#[path = "../../build_support/rcc_modules.rs"]
#[allow(dead_code, unused_imports)]
mod resolver;
const BASE: &str = "3e4b94f80619a5df51111c75fdc37c0b9cd35d41";
fn shadow_blob_identity() -> String {
    let output = std::process::Command::new("git")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args([
            "hash-object",
            "systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/rcc/shadow.rs",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let blob = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    assert_eq!(blob.len(), 40);
    blob
}
#[test]
fn shadow_source_identity_is_resolved_from_the_repository_root() {
    shadow_blob_identity();
}
fn global(p: &rcc_backend::CpuV3Program, name: &str) -> usize {
    match p.debug.globals.iter().find(|g| g.name == name).unwrap().loc {
        rcc::VarLoc::Global(a) => usize::from(a),
        _ => panic!("not a global"),
    }
}
fn write_display_ppm(path: &Path, rgb: &[u32]) {
    assert_eq!(rgb.len(), 400 * 240);
    let mut data = b"P6\n400 240\n255\n".to_vec();
    for &v in rgb {
        data.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    }
    std::fs::write(path, data).unwrap();
}
fn application_from_sources(
    name: &str,
    baseline: Option<&Path>,
) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/game_bloxorz");
    let out =
        PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit task output"))
            .join(name);
    std::fs::create_dir_all(&out).unwrap();
    assert!(std::process::Command::new("python")
        .arg("-B")
        .arg(example.join("tools/assets.py"))
        .arg("--output")
        .arg(&out)
        .status()
        .unwrap()
        .success());
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let mut resources = vec![];
    let mut config = String::from("format 2\ngpu-api\napplication main.rs\n");
    for resource in catalog["resources"].as_array().unwrap() {
        let file = resource["file"].as_str().unwrap();
        let base = resource["address"].as_u64().unwrap() as usize;
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {base} {file}\n",
            file.trim_end_matches(".bin").to_uppercase()
        ));
        resources.push((base, std::fs::read(out.join(file)).unwrap()));
    }
    let constants = boot::parse_boot_project(&config).unwrap().constants()
        + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    let mut source = std::fs::read_to_string(example.join("rcc/main.rs")).unwrap();
    // Normalize caller lifecycle so the frozen exact-triangle control remains
    // valid after the ordinary entry enables batching. Batch arms add it once.
    source = source
        .replace("    let shadow_owner = shadow_batch_begin();\n", "")
        .replace("        shadow_batch_frame();\n", "")
        .replace("    shadow_batch_end(shadow_owner);\n", "");
    source = source.replace(
        "session_init();",
        "session_init();level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;BACKGROUND[0u16]=0;",
    );
    source = source.replace(
        "session_tick(buttons, held, elapsed);",
        if name.contains("moving") {
            "if number==0 {session_move(0);}session_tick(buttons,held,40);"
        } else {
            "session_tick(buttons,held,0);"
        },
    );
    // Match David's readonly observation cost in both arms.
    source = source.replace(
        "block_pose(model);",
        "block_pose(model);world_probe(model);",
    );
    source+="\nfn world_probe(model:&Transform){let world=model.world();let words:Buf<u16,24>=Buf::new([world.row0.x.lo_bits(),world.row0.x.hi_bits(),world.row0.y.lo_bits(),world.row0.y.hi_bits(),world.row0.z.lo_bits(),world.row0.z.hi_bits(),world.row0.w.lo_bits(),world.row0.w.hi_bits(),world.row1.x.lo_bits(),world.row1.x.hi_bits(),world.row1.y.lo_bits(),world.row1.y.hi_bits(),world.row1.z.lo_bits(),world.row1.z.hi_bits(),world.row1.w.lo_bits(),world.row1.w.hi_bits(),world.row2.x.lo_bits(),world.row2.x.hi_bits(),world.row2.y.lo_bits(),world.row2.y.hi_bits(),world.row2.z.lo_bits(),world.row2.z.hi_bits(),world.row2.w.lo_bits(),world.row2.w.hi_bits()]);signal(13,words.as_ptr().addr());}\n";
    let source = application_entry::select(&source, "renderer_game_three_frames").unwrap();
    std::fs::write(out.join("compiled-main.rs"), &source).unwrap();
    let local = out.join("instrumented-rcc");
    std::fs::create_dir_all(&local).unwrap();
    for entry in std::fs::read_dir(example.join("rcc")).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|v| v == "rs") {
            std::fs::copy(&p, local.join(p.file_name().unwrap())).unwrap();
        }
    }
    let path = local.join("shadow.rs");
    let mut shadow = if baseline.is_some() {
        let output=std::process::Command::new("git").current_dir(root).args(["show",&format!("{BASE}:systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/rcc/shadow.rs")]).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    } else {
        std::fs::read_to_string(&path).unwrap()
    };
    for name in [
        "draw_shadow",
        "shadow_project",
        "shadow_hull",
        "shadow_clip",
        "shadow_draw",
    ] {
        let needle = format!("fn {name}(");
        assert_eq!(shadow.matches(&needle).count(), 1);
        shadow = shadow.replace(&needle, &format!("fn measured_body_{name}("));
    }
    shadow += r#"
pub fn draw_shadow(p:&mut Pass,o:&mut Transform,m:&Mesh,a:&Material,b:&Transform){signal(5,0);measured_body_draw_shadow(p,o,m,a,b);signal(6,0);}
fn shadow_project(w:Mat3x4)->u16{signal(7,1);let n=measured_body_shadow_project(w);signal(8,1);n}
fn shadow_hull(c:u16)->u16{signal(7,2);let n=measured_body_shadow_hull(c);signal(8,2);n}
fn shadow_clip(a:Array<ShadowScalar>,b:Array<ShadowScalar>,c:u16,d:fix32,e:u16)->u16{signal(7,3);let n=measured_body_shadow_clip(a,b,c,d,e);signal(8,3);n}
fn shadow_draw(p:&mut Pass,o:&mut Transform,m:&Mesh,a:&Material,t:ShadowTriangle){signal(7,4);measured_body_shadow_draw(p,o,m,a,t);signal(8,4);}
"#;
    std::fs::write(path, shadow).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let ir = resolver::compile_explicit_gpu_program_checked_with_sources(
        "Shadow CPU comparison",
        &source,
        &options,
        &local,
        &root.join("rcc"),
        &constants,
        &[root.join("applications/bloxorz/rcc")],
        &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
        &mut |_| {},
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let static_words = ir.debug.static_data_end;
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    std::fs::write(
        out.join("budget.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"static_words":static_words})).unwrap(),
    )
    .unwrap();
    (program, resources, out)
}

struct Frame {
    rgb: Vec<u32>,
    color: Vec<u16>,
    depth: Vec<u16>,
}
fn instruction_frames(
    program: &cpu_v3::rcc_backend::CpuV3Program,
    resources: &[(usize, Vec<u8>)],
    out: &Path,
) -> Vec<Frame> {
    let code = program
        .words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect::<Vec<_>>();
    let budget: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("budget.json")).unwrap()).unwrap();
    let copies = resources
        .iter()
        .map(|(p, b)| boot::BootResource::copy(*p, b.clone()))
        .collect::<Vec<_>>();
    let package = boot::pack_application_resources(
        &code,
        budget["static_words"].as_u64().unwrap() as usize,
        &copies,
    )
    .unwrap();
    std::fs::write(out.join("cpu-v3-boot.bin"), &package).unwrap();
    let mut sim = CpuV3SystemSim::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
        .unwrap()
        .with_functional_gpu();
    let watched = [
        "gpu_scene_model_init",
        "gpu_scene_matrix_prepare",
        "shadow_project",
        "shadow_clip",
        "shadow_draw",
    ];
    let starts = program
        .debug
        .functions
        .iter()
        .filter(|f| watched.contains(&f.name.as_str()))
        .map(|f| (f.addr.0 as u16, f.name.clone()))
        .collect::<BTreeMap<_, _>>();
    assert!(starts.values().any(|s| s == "gpu_scene_model_init"));
    let mut calls = BTreeMap::<String, u64>::new();
    let mut frames = vec![];
    let mut metrics = vec![];
    let mut halted = false;
    let mut steps = 0;
    let mut in_frame = false;
    let mut count = 0;
    let mut submissions = 0;
    let heap = global(program, "GROUND_BASE");
    for _ in 0..12_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        if in_frame {
            if let Some(name) = starts.get(&sim.cpu().pc()) {
                *calls.entry(name.clone()).or_default() += 1;
            }
        }
        steps += 1;
        match sim
            .step_observing(|w, m| {
                if w.device == 4 && w.channel == 0 {
                    count = w.value;
                }
                if w.device == 4 && w.channel == 1 {
                    let words = published_inputs(m, w.value, count, heap);
                    std::fs::write(
                        out.join(format!("published-{submissions}.bin")),
                        words
                            .iter()
                            .flat_map(|w| w.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                    submissions += 1;
                }
                true
            })
            .unwrap()
            .unwrap()
        {
            StepOutcome::Signaled(e) if e.signal_type == 4 => {
                in_frame = true;
                calls.clear();
            }
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let m = sim.cpu().physical_memory_slice();
                let active = sim.display_state().active_base as usize;
                let rgb = sim.render_active_framebuffer();
                write_display_ppm(&out.join(format!("frame-{}.ppm", frames.len() + 1)), &rgb);
                metrics
                    .push(serde_json::json!({"frame":frames.len()+1,"calls":calls,"steps":steps}));
                for (kind, at) in [("color", active), ("depth", 0x480000 / 2)] {
                    std::fs::write(
                        out.join(format!("frame-{}-{kind}.bin", frames.len() + 1)),
                        m[at..at + 102400]
                            .iter()
                            .flat_map(|w| w.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                }
                frames.push(Frame {
                    rgb,
                    color: m[active..active + 102400].to_vec(),
                    depth: m[0x480000 / 2..0x480000 / 2 + 102400].to_vec(),
                });
                in_frame = false;
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded12M instruction timeout");
    assert_eq!(frames.len(), 3);
    assert!(sim.gpu().last_error().is_none());
    std::fs::write(out.join("frame-counts.json"),serde_json::to_vec_pretty(&serde_json::json!({"frames":metrics,"steps":steps,"max_steps":12000000,"mode":"CPU instruction + functional GPU; function entry counts, not cycles","gpu_error":null})).unwrap()).unwrap();
    frames
}

// Actual Device4 START-visible native commands, every DMA_LOCAL payload and
// generated ground storage; the same arm's instruction run is the oracle.
fn published_inputs(m: &[u16], base: u16, count: u16, heap_global: usize) -> Vec<u16> {
    let at = usize::from(base) * 64;
    let mut result = m[at..at + (usize::from(count) + 1) * 2].to_vec();
    for pair in m[at..at + (usize::from(count) + 1) * 2].as_chunks::<2>().0 {
        let code = u32::from(pair[0]) | (u32::from(pair[1]) << 16);
        if code >> 28 == 1 {
            let src = ((code >> 10) & 0x3ffff) as usize * 16;
            let n = 16usize << ((code >> 1) & 3);
            result.extend_from_slice(&m[src..src + n]);
        }
    }
    let heap = usize::from(m[heap_global]);
    if heap != 0 {
        result.extend_from_slice(&m[heap..heap + usize::from(m[heap + 6]) * 64]);
    }
    result
}
#[test]
#[ignore = "bounded native CPU + functional GPU shadow cost; no GPU cycles/scanout contention"]
fn visible_stationary_and_rolling_shadow_cpu_cost_matches_instruction() {
    use cpu_v3_tang_nano_20k::system_emu::CpuV3SystemEmu;
    let root = PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit output"));
    std::fs::create_dir_all(&root).unwrap();
    let baseline = root.clone();
    let mut all = vec![];
    for case in ["stationary", "moving"] {
        let mut reference: Option<Vec<Frame>> = None;
        for (arm, sources) in [("before", Some(baseline.as_path())), ("after", None)] {
            let (program, resources, out) = application_from_sources(
                &format!("frame-schedule-{case}-{arm}-cpu-shadow-cost"),
                sources,
            );
            let expected = instruction_frames(&program, &resources, &out);
            if let Some(old) = &reference {
                for (i, (a, b)) in old.iter().zip(&expected).enumerate() {
                    assert_eq!(a.rgb, b.rgb, "{case} frame{i} accepted Display");
                    assert_eq!(a.color, b.color, "{case} frame{i} accepted physical color");
                    assert_eq!(a.depth, b.depth, "{case} frame{i} accepted physical depth");
                }
            }
            let package = std::fs::read(out.join("cpu-v3-boot.bin")).unwrap();
            let mut native =
                CpuV3SystemEmu::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
                    .unwrap()
                    .with_functional_gpu();
            let wall = std::time::Instant::now();
            let heap = global(&program, "GROUND_BASE");
            let mut count = 0;
            let mut submissions = 0;
            let mut last = None;
            let mut frame_start = 0;
            let mut shadow_start = None;
            let mut part_start = None;
            let mut shadow_cycles = 0;
            let mut parts = BTreeMap::<u16, (u64, u64)>::new();
            let mut frames = vec![];
            let mut halted = false;
            let mut min_sp = 0xc000u16;
            for _ in 0..24_000_000 {
                assert!(
                    wall.elapsed().as_secs() < 240,
                    "bounded240s native timeout {case}/{arm}"
                );
                if native.waiting_for_vblank() {
                    native.advance_vblank();
                }
                let outcome = native
                    .step_observing(|w, m| {
                        if w.device == 4 && w.channel == 0 {
                            count = w.value;
                        }
                        if w.device == 4 && w.channel == 1 {
                            let actual = published_inputs(m, w.value, count, heap);
                            let expected =
                                std::fs::read(out.join(format!("published-{submissions}.bin")))
                                    .unwrap();
                            assert_eq!(
                                actual
                                    .iter()
                                    .flat_map(|w| w.to_le_bytes())
                                    .collect::<Vec<_>>(),
                                expected,
                                "native publication {case}/{arm}/{submissions}"
                            );
                            submissions += 1;
                        }
                        true
                    })
                    .unwrap()
                    .unwrap();
                assert!(!outcome.fault);
                let core = native.core();
                let sp = core.architectural_gprs()[13];
                if sp != 0 {
                    min_sp = min_sp.min(sp);
                }
                let event = core.signal_event();
                drop(core);
                if event != last {
                    if let Some(e) = event {
                        let now = native.cycles();
                        match e.signal_type {
                            4 => {
                                frame_start = now;
                                shadow_cycles = 0;
                                parts.clear();
                            }
                            5 => {
                                assert!(shadow_start.replace(now).is_none());
                            }
                            6 => shadow_cycles += now - shadow_start.take().unwrap(),
                            7 => {
                                assert!(part_start.replace((e.value, now)).is_none());
                            }
                            8 => {
                                let (part, begin) = part_start.take().unwrap();
                                assert_eq!(part, e.value);
                                let metric = parts.entry(part).or_default();
                                metric.0 += now - begin;
                                metric.1 += 1;
                            }
                            12 => {
                                let i = frames.len();
                                let active = native.display_state().active_base as usize;
                                assert_eq!(
                                    native.render_active_framebuffer(),
                                    expected[i].rgb,
                                    "native Display {case}/{arm}/{i}"
                                );
                                let m = native.physical_memory();
                                assert_eq!(
                                    &m[active..active + 102400],
                                    expected[i].color,
                                    "native color"
                                );
                                assert_eq!(
                                    &m[0x480000 / 2..0x480000 / 2 + 102400],
                                    expected[i].depth,
                                    "native depth"
                                );
                                assert!(
                                    parts.get(&4).is_some_and(|v| v.1 > 0),
                                    "must render visible shadow"
                                );
                                frames.push(serde_json::json!({"frame":i+1,"start":frame_start,"end":now,"cpu_cycles":now-frame_start,"shadow_total_cycles":shadow_cycles,"parts":parts,"publication_and_pixels_equal":true}));
                            }
                            _ => {}
                        }
                    }
                    last = event;
                }
                if outcome.halted {
                    assert_eq!(outcome.halt_signal, 0x600d);
                    halted = true;
                    break;
                }
            }
            assert!(halted, "bounded24M native timeout {case}/{arm}");
            assert_eq!(frames.len(), 3);
            assert_eq!(submissions, 6);
            assert!(native.gpu().last_error().is_none());
            assert!(min_sp > 0x9fff, "stack/heap collision");
            let metrics = serde_json::json!({"case":case,"arm":arm,"frames":frames,"native_cycles":native.cycles(),"native_memory_cycles":native.native_cycles(),"host_seconds":wall.elapsed().as_secs_f64(),"minimum_stack_word":min_sp,"max_cycles":24000000,"max_host_seconds":240,"part_legend":{"1":"project","2":"hull","3":"clip","4":"triangle command preparation"},"scope":"instrumented exact CPU/native MC + synchronous functional GPU, immediate vblank, same-arm instruction publication/color/depth/Display oracle, then accepted-before pixel oracle; nested parts are not additive with total; markers include wrapper overhead; no GPU cycles/scanout/FPS"});
            std::fs::write(
                out.join("cpu-shadow-cost.json"),
                serde_json::to_vec_pretty(&metrics).unwrap(),
            )
            .unwrap();
            all.push(metrics);
            if arm == "before" {
                reference = Some(expected);
            }
        }
    }
    std::fs::write(
        root.join("cpu-shadow-comparison.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"accepted_source":BASE,"arms":all,"native_before_skipped":false,"comparison":"only shadow.rs comes from the baseline commit; identical surrounding current code and resources in both arms","surrounding_source_commit":std::process::Command::new("git").args(["rev-parse","HEAD"]).output().map(|o|String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap(),"candidate_shadow_blob":shadow_blob_identity()})).unwrap(),
    )
    .unwrap();
}
