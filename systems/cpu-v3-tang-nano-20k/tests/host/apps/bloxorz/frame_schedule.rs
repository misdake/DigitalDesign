//! Same-input application preparation checks against immutable accepted sources.
use super::scenarios;
use super::{application_case, write_display_ppm};
use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::{
    boot,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
const BASE: &str = "4e14c791115dc5d9bfa3c5422eb332cd6d0fe5de";
const EXACT_TRIANGLE_CANDIDATE: &str = "7bd58281f21bde46d22a7ed521858ec6e609a111";
fn baseline_sources(root: &Path) -> PathBuf {
    sources_at(root, BASE)
}
fn sources_at(root: &Path, revision: &str) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("accepted-sources");
    if target.join("identity.json").exists() {
        return target.join("systems/cpu-v3-tang-nano-20k");
    }
    let output = std::process::Command::new("git")
        .current_dir(&repo)
        .args([
            "ls-tree",
            "-r",
            "--name-only",
            revision,
            "--",
            "systems/cpu-v3-tang-nano-20k/rcc",
            "systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/rcc",
            "systems/cpu-v3-tang-nano-20k/examples/bloxorz/rcc",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let mut paths = vec![];
    for path in String::from_utf8(output.stdout).unwrap().lines() {
        let content = std::process::Command::new("git")
            .current_dir(&repo)
            .args(["show", &format!("{revision}:{path}")])
            .output()
            .unwrap();
        assert!(content.status.success());
        let out = target.join(path);
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, &content.stdout).unwrap();
        paths.push(path.to_owned());
    }
    std::fs::write(target.join("identity.json"),serde_json::to_vec_pretty(&serde_json::json!({"accepted_source":revision,"paths":paths,"purpose":"immutable baseline input closure; no old worktree or branch restored"})).unwrap()).unwrap();
    target.join("systems/cpu-v3-tang-nano-20k")
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
    let heap = super::global(program, "GROUND_BASE");
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
#[test]
#[ignore = "historical pre-batch accepted-vs-candidate exact entry/goal/fallback comparison"]
fn historical_pad_roles_match_accepted_images_through_stable_and_changing_openings() {
    let root =
        PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit task output"));
    std::fs::create_dir_all(&root).unwrap();
    let base = baseline_sources(&root);
    let candidate = sources_at(
        &root.join("historical-exact-candidate"),
        EXACT_TRIANGLE_CANDIDATE,
    );
    let mut cases = vec![];
    for case in ["entry", "goal", "fallback15", "fallback17"] {
        let (before, old_resources, old_out) = application_case(
            &format!("frame-schedule-{case}-before"),
            None,
            Some(&base),
            scenarios::frame_schedule(case, false),
        );
        let a = instruction_frames(&before, &old_resources, &old_out);
        let (after, new_resources, new_out) = application_case(
            &format!("frame-schedule-{case}-after"),
            None,
            Some(&candidate),
            scenarios::frame_schedule(case, false),
        );
        assert_eq!(old_resources, new_resources, "runtime resources differ");
        let b = instruction_frames(&after, &new_resources, &new_out);
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            assert_eq!(x.rgb, y.rgb, "{case} frame{i} Display");
            assert_eq!(x.color, y.color, "{case} frame{i} physical color");
            assert_eq!(x.depth, y.depth, "{case} frame{i} physical depth");
        }
        assert_eq!(a[0].rgb, a[1].rgb, "identical pose must stay stable");
        cases.push(
            serde_json::json!({"case":case,"frames":3,"full_color_depth_Display_equal":true}),
        );
    }
    std::fs::write(
        root.join("comparison.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"accepted_source":BASE,"cases":cases,"performance_claim":false}),
        )
        .unwrap(),
    )
    .unwrap();
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
#[ignore = "historical pre-batch native CPU shadow cost; no GPU cycles/scanout contention"]
fn historical_exact_triangle_shadow_cpu_cost_matches_instruction() {
    use cpu_v3_tang_nano_20k::system_emu::CpuV3SystemEmu;
    let root = PathBuf::from(std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT").expect("explicit output"));
    std::fs::create_dir_all(&root).unwrap();
    let baseline = baseline_sources(&root);
    let candidate = sources_at(
        &root.join("historical-exact-candidate"),
        EXACT_TRIANGLE_CANDIDATE,
    );
    let mut all = vec![];
    for case in ["stationary", "moving"] {
        let mut reference: Option<Vec<Frame>> = None;
        for (arm, sources) in [
            ("before", Some(baseline.as_path())),
            ("after", Some(candidate.as_path())),
        ] {
            let (program, resources, out) = application_case(
                &format!("frame-schedule-{case}-{arm}-cpu-shadow-cost"),
                None,
                sources,
                scenarios::frame_schedule(case, true),
            );
            let expected = instruction_frames(&program, &resources, &out);
            if let Some(old) = &reference {
                for (i, (a, b)) in old.iter().zip(&expected).enumerate() {
                    assert_eq!(a.rgb, b.rgb, "{case} frame{i} accepted Display");
                    assert_eq!(a.color, b.color, "{case} frame{i} accepted physical color");
                    assert_eq!(a.depth, b.depth, "{case} frame{i} accepted physical depth");
                }
            }
            if arm == "before" && std::env::var_os("BLOXORZ_SHADOW_COMBINED_ONLY").is_some() {
                reference = Some(expected);
                continue;
            }
            let package = std::fs::read(out.join("cpu-v3-boot.bin")).unwrap();
            let mut native =
                CpuV3SystemEmu::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
                    .unwrap()
                    .with_functional_gpu();
            let wall = std::time::Instant::now();
            let heap = super::global(&program, "GROUND_BASE");
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
        serde_json::to_vec_pretty(&serde_json::json!({"accepted_source":BASE,"arms":all,"native_before_skipped":std::env::var_os("BLOXORZ_SHADOW_COMBINED_ONLY").is_some(),"candidate_source":EXACT_TRIANGLE_CANDIDATE,"historical_control_not_current_batch":true})).unwrap(),
    )
    .unwrap();
}

#[test]
fn ordinary_batch_slots_publish_after_retirement_and_free_on_finite_exit() {
    use super::global;
    let (program, resources, out) = application_case(
        "frame-schedule-split-lifetime",
        None,
        None,
        scenarios::frame_schedule("split-lifetime", false),
    );
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    super::preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let batch = global(&program, "SHADOW_WORKSPACE.batch_base");
    let records = global(&program, "GPU_CB_RECORDS");
    let facts = global(&program, "GPU_CB_RESOURCE_FACTS");
    let heap_begin = global(&program, "HEAP_BEGIN");
    let heap_end = global(&program, "HEAP_END");
    let heads = global(&program, "FREE_HEADS");
    let mut starts = 0;
    let mut dynamic = 0;
    let mut frames = 0;
    let mut min_sp = 0xc000u16;
    let mut halted = false;
    let mut pixels = None;
    let mut samples = vec![];
    for _ in 0..10_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        let sp = sim.cpu().registers()[13];
        if sp != 0 {
            min_sp = min_sp.min(sp);
        }
        let outcome=sim.step_observing(|w,m|{
            if w.device==4 && w.channel==1 {
                starts+=1;
                let base=usize::from(m[batch]);
                assert_ne!(base,0,"ordinary caller must allocate the batch owner");
                assert_eq!(base&63,0);
                if w.value==0x9880 {
                    dynamic+=1;
                    assert_eq!(m[base],2,"two visible split shadows must use distinct slots");
                    let count=usize::from(m[records+15]);
                    let allocations=&m[facts..facts+count*4];
                    let mut keys=vec![];
                    for slot in 0..2 {
                        let at=base+64+slot*4224;
                        assert_eq!(m[at+4],1);
                        keys.push(m[at+5..at+29].to_vec());
                        let sector=(at/64) as u16;
                        assert!(allocations.as_chunks::<4>().0.iter().any(|f|*f==[sector+1,65,1,1]),"public Mesh facts must name only directory/records");
                        assert!(!allocations.as_chunks::<4>().0.iter().any(|f|f[0]<=sector && u32::from(f[0])+u32::from(f[1])>u32::from(sector)),"CPU-private slot metadata must not enter GPU read facts");
                    }
                    assert_ne!(keys[0],keys[1]);
                    samples.push(serde_json::json!({"batch_base_word":base,"slot_keys":keys,"allocation_facts":allocations}));
                }
            }
            true
        }).unwrap().unwrap();
        match outcome {
            StepOutcome::Signaled(e) if e.signal_type == 4 => {
                let m = sim.cpu().physical_memory_slice();
                let base = usize::from(m[batch]);
                assert_ne!(base, 0);
                assert_eq!(m[base], 0, "reset private cursor before frame marker");
            }
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let rgb = sim.render_active_framebuffer();
                if let Some(old) = &pixels {
                    assert_eq!(old, &rgb, "identical split pose must remain stable");
                } else {
                    pixels = Some(rgb.clone());
                }
                write_display_ppm(
                    &out.join(format!("ordinary-split-frame-{frames}.ppm")),
                    &rgb,
                );
                frames += 1;
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded10M ordinary batch lifetime timeout");
    assert_eq!((starts, dynamic, frames), (7, 3, 3));
    assert!(sim.gpu().last_error().is_none());
    let m = sim.cpu().physical_memory_slice();
    assert_eq!(m[batch], 0, "release clears original owner registration");
    let begin = usize::from(m[heap_begin]);
    let end = usize::from(m[heap_end]);
    assert!(usize::from(min_sp) >= end, "stack must stay above heap");
    let whole = 0x8000 | (end - begin) as u16;
    assert_eq!(
        m[begin], whole,
        "all application allocations coalesce after finite exit"
    );
    assert_eq!(m[end - 1], whole);
    assert_eq!(&m[heads..heads + 3], &[0, 0, 0]);
    assert_eq!(usize::from(m[heads + 3]), begin + 1);
    std::fs::write(out.join("ordinary-batch-lifetime.json"),serde_json::to_vec_pretty(&serde_json::json!({"frames":frames,"gpu_starts":starts,"dynamic_starts":dynamic,"samples":samples,"heap_begin_word":begin,"heap_end_exclusive_word":end,"minimum_stack_word":min_sp,"peak_observed_stack_bytes":(0xc000-u32::from(min_sp))*2,"whole_heap_released":true,"batch_owner_zero_on_exit":true,"max_steps":10000000,"source_path":"actual ordinary main lifecycle; no normalized/injected batch hooks","mode":"CPU instruction + functional GPU, not CPU timing"})).unwrap()).unwrap();
}
