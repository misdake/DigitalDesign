//! Exploratory fixed-point packing and full-pixel comparisons; no CPU timing claim.
use cpu_v3::{rcc_backend, StepOutcome};
use cpu_v3_tang_nano_20k::{
    boot,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[path = "../build_support/application_entry.rs"]
mod application_entry;
#[path = "../build_support/rcc_modules.rs"]
#[allow(dead_code, unused_imports)]
mod resolver;
const BASE: &str = "44b03dd25a0257a1c45b63ddbd74aa1d29d241a0";
fn shadow_blob_identity() -> String {
    let output = std::process::Command::new("git")
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args([
            "hash-object",
            "systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/rcc/shadow.rs",
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
fn replace_once(source: &mut String, before: &str, after: &str) {
    assert_eq!(
        source.matches(before).count(),
        1,
        "prototype patch anchor {before}"
    );
    *source = source.replace(before, after);
}
fn application_from_sources(
    name: &str,
    baseline: Option<&Path>,
) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/gpu_renderer_bloxorz");
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
    if name.contains("-split-") {
        source=source.replace("BACKGROUND[0u16]=0;","BACKGROUND[0u16]=0;GAME[0u16]=1;GAME[1u16]=1;GAME[2u16]=0;GAME[6u16]=2;GAME[7u16]=1;GAME[8u16]=0;GAME[9u16]=1;");
    }

    if name.contains("-sweep-") {
        source = source.replace("run(3, false, 128);", "run(24, false, 128);");
        source = source.replace(
            "session_tick(buttons,held,0);",
            "if number==0 {session_move(0);}session_tick(buttons,held,10);",
        );
    }
    let extra = if name.contains("-bridge-off-") || name.contains("-bridge-on-") {
        if name.contains("-bridge-on-") {
            "level_reset(1);SESSION[0u16]=PLAY;GAME[0u16]=5;GAME[1u16]=4;GAME[2u16]=0;GAME[3u16]=1;"
        } else {
            "level_reset(1);SESSION[0u16]=PLAY;GAME[0u16]=5;GAME[1u16]=4;GAME[2u16]=0;GAME[3u16]=0;"
        }
    } else if name.contains("-goal-") {
        "GAME[0u16]=7;GAME[1u16]=4;SESSION[0u16]=GOAL_DROP;SESSION[1u16]=0;"
    } else if name.contains("-fragile-") {
        "level_reset(3);SESSION[0u16]=PLAY;GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;session_move(0);batch_ticks(220);batch_ticks(140);batch_ticks(150);"
    } else if name.contains("-boundary-") {
        "GAME[0u16]=0;GAME[1u16]=1;"
    } else {
        ""
    };
    if !extra.is_empty() {
        source = source.replace(
            "BACKGROUND[0u16]=0;",
            &format!("BACKGROUND[0u16]=0;{extra}"),
        );
    }
    source+="\nfn batch_ticks(total:u16){let mut remain=total;while remain!=0 {let dt=if remain>50 {50u16}else{remain};session_tick(0,0,dt);remain-=dt;}}\n";
    if name.contains("-cache-fresh-") {
        source = source.replace("draw_shadow(", "shadow_invalidate();draw_shadow(");
    }
    // Match David's readonly observation cost in both arms.
    source = source.replace(
        "block_pose(model);",
        "block_pose(model);world_probe(model);",
    );
    source+="\nfn world_probe(model:&Model){let world=model.world();let words:Buf<u16,24>=Buf::new([world.row0.x.lo_bits(),world.row0.x.hi_bits(),world.row0.y.lo_bits(),world.row0.y.hi_bits(),world.row0.z.lo_bits(),world.row0.z.hi_bits(),world.row0.w.lo_bits(),world.row0.w.hi_bits(),world.row1.x.lo_bits(),world.row1.x.hi_bits(),world.row1.y.lo_bits(),world.row1.y.hi_bits(),world.row1.z.lo_bits(),world.row1.z.hi_bits(),world.row1.w.lo_bits(),world.row1.w.hi_bits(),world.row2.x.lo_bits(),world.row2.x.hi_bits(),world.row2.y.lo_bits(),world.row2.y.hi_bits(),world.row2.z.lo_bits(),world.row2.z.hi_bits(),world.row2.w.lo_bits(),world.row2.w.hi_bits()]);signal(13,words.as_ptr().addr());}\n";
    if baseline.is_none() {
        source = source.replace(
            "let ground_owner = ground_begin();",
            "let ground_owner = ground_begin();let shadow_owner=shadow_batch_begin();",
        );
        source = source.replace(
            "signal(4, number);",
            "shadow_batch_frame();signal(4, number);",
        );
        source = source.replace(
            "ground_end(ground_owner);",
            "ground_end(ground_owner);shadow_batch_end(shadow_owner);",
        );
    }
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
        let output=std::process::Command::new("git").current_dir(root).args(["show",&format!("{BASE}:systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/rcc/shadow.rs")]).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    } else {
        std::fs::read_to_string(&path).unwrap()
    };
    if baseline.is_none() && !name.contains("-cost") {
        replace_once(
            &mut shadow,
            "const SHADOW_BATCH_DIAGNOSTICS: u16 = 0;",
            "const SHADOW_BATCH_DIAGNOSTICS: u16 = 1;",
        );
    }
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
pub fn draw_shadow(p:&mut Pass,m:&Mesh,o:&mut Model,a:&Material,b:&Model){signal(5,0);measured_body_draw_shadow(p,m,o,a,b);signal(6,0);}
fn shadow_project(w:Mat3x4)->u16{signal(7,1);let n=measured_body_shadow_project(w);signal(8,1);n}
fn shadow_hull(c:u16)->u16{signal(7,2);let n=measured_body_shadow_hull(c);signal(8,2);n}
fn shadow_clip(a:Array<ShadowScalar>,b:Array<ShadowScalar>,c:u16,d:fix32,e:u16)->u16{signal(7,3);let n=measured_body_shadow_clip(a,b,c,d,e);signal(8,3);n}
fn shadow_draw(p:&mut Pass,m:&Mesh,o:&mut Model,a:&Material,t:ShadowTriangle){let v:Buf<u16,12>=Buf::new([t.x.lo_bits(),t.x.hi_bits(),t.z.lo_bits(),t.z.hi_bits(),t.ax.lo_bits(),t.ax.hi_bits(),t.az.lo_bits(),t.az.hi_bits(),t.bx.lo_bits(),t.bx.hi_bits(),t.bz.lo_bits(),t.bz.hi_bits()]);signal(9,v.as_ptr().addr());signal(7,4);measured_body_shadow_draw(p,m,o,a,t);signal(8,4);}
"#;

    if name.ends_with("-snapped") {
        shadow=shadow.replace("measured_body_shadow_draw(p,m,o,a,t);", "let x=batch_snap(t.x);let z=batch_snap(t.z);let triangle:ShadowTriangle=ShadowTriangle{x:x,z:z,ax:batch_snap(t.x+t.ax)-x,az:batch_snap(t.z+t.az)-z,bx:batch_snap(t.x+t.bx)-x,bz:batch_snap(t.z+t.bz)-z};measured_body_shadow_draw(p,m,o,a,triangle);");
        shadow+="\nfn batch_snap(v:fix32)->fix32{fix32::from_int((v*fix32::from_int(512)+fix32::from_words(32768,0)).to_int())*fix32::from_words(128,0)}\n";
    }

    if name.contains("-cost") {
        // Rebuild instrumentation from the original chosen module; the two
        // total-shadow signals have identical frequency on both arms.
        let mut plain = if baseline.is_some() {
            let result=std::process::Command::new("git").current_dir(root).args(["show",&format!("{BASE}:systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/rcc/shadow.rs")]).output().unwrap();
            assert!(result.status.success());
            String::from_utf8(result.stdout).unwrap()
        } else {
            std::fs::read_to_string(example.join("rcc/shadow.rs")).unwrap()
        };
        replace_once(
            &mut plain,
            "fn draw_shadow(",
            "fn measured_body_draw_shadow(",
        );
        plain+="\npub fn draw_shadow(p:&mut Pass,m:&Mesh,o:&mut Model,a:&Material,b:&Model){signal(5,0);measured_body_draw_shadow(p,m,o,a,b);signal(6,0);}\n";
        shadow = plain;
    }
    std::fs::write(path, shadow).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let profile = resolver::RendererLayoutProfile::Column16;
    let ir = resolver::compile_explicit_gpu_program_checked_with_sources(
        "Shadow CPU comparison",
        &source,
        &options,
        &local,
        &root.join("rcc"),
        &constants,
        &[root.join("applications/bloxorz/rcc")],
        &resolver::RendererCaptureConfiguration::with_default_hook(profile),
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
    geometry: Vec<Vec<i32>>,
    shadow: Vec<u16>,
    batch: Vec<Vec<u16>>,
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
    let mut geometry = vec![];
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
                geometry.clear();
            }
            StepOutcome::Signaled(e) if e.signal_type == 9 => {
                let m = sim.cpu().physical_memory_slice();
                let at = usize::from(e.value);
                geometry.push(
                    (0..6)
                        .map(|i| {
                            (u32::from(m[at + i * 2]) | (u32::from(m[at + i * 2 + 1]) << 16)) as i32
                        })
                        .collect::<Vec<_>>(),
                );
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
                let info = global(program, "SHADOW_INFO");
                let mut batch = vec![];
                if let Some(g) = program
                    .debug
                    .globals
                    .iter()
                    .find(|g| g.name == "SHADOW_BATCH_BASE")
                {
                    if let rcc::VarLoc::Global(a) = g.loc {
                        let base = usize::from(m[usize::from(a)]);
                        if base != 0 {
                            std::fs::write(
                                out.join(format!("frame-{}-batch-storage.bin", frames.len() + 1)),
                                m[base..base + 8512]
                                    .iter()
                                    .flat_map(|w| w.to_le_bytes())
                                    .collect::<Vec<_>>(),
                            )
                            .unwrap();
                            for slot in 0..2 {
                                let at = base + 64 + slot * 4224;
                                batch.push(m[at..at + 40].to_vec());
                            }
                        }
                    }
                }
                frames.push(Frame {
                    geometry: geometry.clone(),
                    shadow: m[info..info + 3].to_vec(),
                    batch,
                    rgb,
                    color: m[active..active + 102400].to_vec(),
                    depth: m[0x480000 / 2..0x480000 / 2 + 102400].to_vec(),
                });
                in_frame = false;
            }
            StepOutcome::Halted { signal } => {
                let f = global(program, "GPU_API_FAULT");
                let m = sim.cpu().physical_memory_slice();
                if signal != 0x600d {
                    std::fs::write(
                        out.join("fault-memory.bin"),
                        m.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>(),
                    )
                    .unwrap();
                }
                assert_eq!(
                    signal,
                    0x600d,
                    "GPU fault {:?}, renderer {:?}",
                    &m[f..f + 6],
                    sim.gpu().last_error()
                );
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded12M instruction timeout");
    let expected_frames = if out
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains("-sweep-")
    {
        24
    } else {
        3
    };
    assert_eq!(frames.len(), expected_frames);
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
        if code >> 28 == 2 {
            let sector = ((code >> 12) & 65535) as usize;
            let sectors = (((code >> 1) & 7) + 2) as usize;
            result.extend_from_slice(&m[sector * 64..(sector + sectors) * 64]);
        }
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
#[ignore = "bounded exploratory cell-meshlet pixel and quantization report; no performance claim"]
fn per_receiver_cell_batch_pixel_and_quantization_diagnostic() {
    let root =
        PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit task output"));
    let mut results = vec![];
    for case in [
        "stationary",
        "moving",
        "split",
        "sweep",
        "bridge-off",
        "bridge-on",
        "goal",
        "fragile",
        "boundary",
        "cache-fresh",
    ] {
        if let Ok(selected) = std::env::var("BLOXORZ_BATCH_CASES") {
            if !selected.split(',').any(|v| v == case) {
                continue;
            }
        }
        let (p, r, out) = application_from_sources(&format!("batch-{case}-before"), Some(&root));
        let before = instruction_frames(&p, &r, &out);
        let (p, r, out) = application_from_sources(&format!("batch-{case}-snapped"), Some(&root));
        let snapped = instruction_frames(&p, &r, &out);
        let (p, r, out) = application_from_sources(&format!("batch-{case}-after"), None);
        let after = instruction_frames(&p, &r, &out);
        if case == "split" {
            for frame in &after {
                assert_eq!(frame.batch.len(), 2);
                assert!(frame.batch.iter().all(|b| b[0] > 0 && b[4] == 1));
                assert_ne!(
                    &frame.batch[0][5..29],
                    &frame.batch[1][5..29],
                    "both live shadow slots must retain distinct world keys"
                );
            }
        }
        for (i, (a, b)) in before.iter().zip(&after).enumerate() {
            let pixels = a
                .rgb
                .iter()
                .zip(&b.rgb)
                .enumerate()
                .filter_map(|(p, (x, y))| {
                    (x != y)
                        .then_some(serde_json::json!({"x":p%400,"y":p/400,"before":x,"after":y}))
                })
                .collect::<Vec<_>>();
            let color = a.color.iter().zip(&b.color).filter(|(x, y)| x != y).count();
            let depth = a.depth.iter().zip(&b.depth).filter(|(x, y)| x != y).count();
            let quantized_differences = a
                .rgb
                .iter()
                .zip(&snapped[i].rgb)
                .enumerate()
                .filter_map(|(p, (x, y))| {
                    (x != y)
                        .then_some(serde_json::json!({"x":p%400,"y":p/400,"before":x,"after":y}))
                })
                .collect::<Vec<_>>();
            let shared_model_differences = snapped[i]
                .rgb
                .iter()
                .zip(&b.rgb)
                .enumerate()
                .filter_map(|(p, (x, y))| {
                    (x != y).then_some(
                        serde_json::json!({"x":p%400,"y":p/400,"snapped_triangle":x,"batch":y}),
                    )
                })
                .collect::<Vec<_>>();
            let equal = pixels.is_empty() && color == 0 && depth == 0;
            results.push(serde_json::json!({"case":case,"frame":i+1,"Display_differences":pixels,"quantization_only_Display_differences":quantized_differences,"shared_model_after_quantization_Display_differences":shared_model_differences,"color_differences":color,"depth_differences":depth,"equivalent":equal,"shadow_before":a.shadow,"world_triangle_coefficients_q16":a.geometry,"shadow_after":b.shadow,"batch_metadata":b.batch}));
        }
    }
    let equal = results.iter().all(|v| v["equivalent"] == true);
    std::fs::write(root.join("cell-batch-comparison.json"),serde_json::to_vec_pretty(&serde_json::json!({"baseline":BASE,"frames":results,"all_frames_equivalent":equal,"scope":"instruction CPU + functional GPU; exploratory report; no speedup claim"})).unwrap()).unwrap();
    eprintln!("per-cell batching bit-exact equivalence: {equal}");
}

#[test]
#[ignore = "bounded native CPU + functional GPU shadow cost; no GPU cycles/scanout contention"]
fn per_cell_batch_stationary_and_rolling_native_cpu_cost() {
    use cpu_v3_tang_nano_20k::system_emu::CpuV3SystemEmu;
    let root = PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit output"));
    std::fs::create_dir_all(&root).unwrap();
    let baseline = root.clone();
    let mut all = vec![];
    for case in ["stationary", "moving"] {
        let mut reference: Option<Vec<Frame>> = None;
        for (arm, sources) in [("before", Some(baseline.as_path())), ("after", None)] {
            let (program, resources, out) =
                application_from_sources(&format!("batch-{case}-{arm}-cost"), sources);
            let expected = instruction_frames(&program, &resources, &out);
            if let Some(old) = &reference {
                for (i, (a, b)) in old.iter().zip(&expected).enumerate() {
                    assert_eq!(a.depth, b.depth, "{case} frame{i} depth");
                    let display_diff = a.rgb.iter().zip(&b.rgb).filter(|(x, y)| x != y).count();
                    assert!(
                        display_diff <= 4,
                        "prequalified tiny differences expanded: {case} frame{i}={display_diff}"
                    );
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
                                    expected[i].shadow[2] > 0,
                                    "same-arm instruction oracle must contain a visible shadow"
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
            let metrics = serde_json::json!({"case":case,"arm":arm,"frames":frames,"native_cycles":native.cycles(),"native_memory_cycles":native.native_cycles(),"host_seconds":wall.elapsed().as_secs_f64(),"minimum_stack_word":min_sp,"max_cycles":24000000,"max_host_seconds":240,"part_legend":{"1":"project","2":"hull","3":"clip","4":"triangle command preparation"},"scope":"total-shadow marker pair only; identical probe count in both arms; native CPU/MC + synchronous functional GPU, immediate vblank; same-arm instruction full-publication/color/depth/Display oracle; cross-arm tiny coverage differences accepted by user, depth exact; no GPU cycles/scanout/FPS"});
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
