use super::scenarios;
use super::*;

#[test]
fn web_first_two_stage_play_frames_use_functional_gpu_and_actual_display() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    for (name, case) in [
        ("web-stage1-play", scenarios::stage(0, false, false, false)),
        ("web-stage2-play", scenarios::stage(1, false, false, false)),
        (
            "web-stage2-right-standing",
            scenarios::stage(1, false, true, false),
        ),
        (
            "web-stage1-gradient",
            scenarios::stage(0, true, false, false),
        ),
        ("web-goal-mid", scenarios::goal_mid()),
    ] {
        let (program, resources, out) = application_case(name, None, None, case);
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut halted = false;
        let mut steps = 0usize;
        let mut command_count = 0u16;
        let mut gpu_inputs = vec![];
        std::fs::write(
            out.join("program.bin"),
            program
                .words
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        for _ in 0..2_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            steps += 1;
            let outcome = sim.step_observing(|write,memory| {
                if write.device==4 && write.channel==0 {command_count=write.value;}
                if write.device==4 && write.channel==1 {
                    let file=format!("gpu-before-{}.bin",gpu_inputs.len());
                    std::fs::write(out.join(&file),memory.iter().flat_map(|w|w.to_le_bytes()).collect::<Vec<_>>()).unwrap();
                    gpu_inputs.push(serde_json::json!({"memory":file,"base_bytes":u32::from(write.value)*128,"count_minus_one":command_count,"maximum_work":10000000}));
                }
                true
            }).unwrap().unwrap();
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d, "GPU error: {:?}", sim.gpu().last_error());
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded Web stage frame timeout");
        assert!(sim.gpu().last_error().is_none());
        let active = sim.display_state().active_base as usize;
        let memory = sim.cpu_mut().physical_memory_mut();
        std::fs::write(
            out.join("color-physical.bin"),
            memory[active..active + 1600 * 64]
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(
            out.join("depth-physical.bin"),
            memory[0x9000 * 64..0x9000 * 64 + 1600 * 64]
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        if name == "web-stage2-right-standing" {
            // Frozen front-face depth values: the old unculled back face wrote
            // 7107/7247 here and incorrectly rejected these nearer +Z pixels.
            for (x, y, expected) in [(353, 127, 7118), (350, 141, 7260)] {
                let offset = ((x >> 4) * 16 + (y >> 4)) * 256 + (y & 15) * 16 + (x & 15);
                assert_eq!(
                    memory[0x480000 / 2 + offset],
                    expected,
                    "back face occluded front at ({x},{y})"
                );
            }
        }
        let game = global(&program, "GAME");
        let pose = memory[game..game + 10].to_vec();
        let session = global(&program, "SESSION");
        let animation = memory[session..session + 2].to_vec();
        if name == "web-goal-mid" {
            assert_eq!(pose[4], 1);
            assert_eq!(animation, [5, 210]);
        }
        let background = resources
            .iter()
            .find(|(base, _)| *base == 0x580000)
            .unwrap();
        assert_eq!(
            memory[0xb000 * 64..0xb000 * 64 + 1600 * 64]
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
            background.1,
            "immutable background source"
        );
        let pixels = sim.render_active_framebuffer();
        assert_eq!(pixels.len(), 400 * 240);
        let mut ppm = b"P6\n400 240\n255\n".to_vec();
        for pixel in pixels {
            ppm.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
        std::fs::write(out.join("frame-display.ppm"), ppm).unwrap();
        std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "cpu_mode":"instruction","steps":steps,"gpu_error":null,"gpu_inputs":gpu_inputs,
            "active_color_byte_base":active*2,"pose_words":pose,
            "display_format":"Rgb565 (open default; actual Display conversion used)","frame":"frame-display.ppm",
            "animation":animation,"pose":if name=="web-goal-mid" {"actual seven-move solution advanced to mid goal drop before first GPU frame"}else{"PLAY initialized without entry animation; same real level/rules/render path"},
        })).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "bounded native CPU cost experiment; run intentionally"]
fn web_native_cpu_baseline_matches_the_same_instruction_program_and_pose() {
    use cpu_v3_tang_nano_20k::{
        boot,
        system_emu::CpuV3SystemEmu,
        system_sim::{CpuV3SystemSim, VBlankMode},
    };
    let (program, resources, out) = application_case(
        "web-stage1-native-warm",
        None,
        None,
        scenarios::stage(0, false, false, true),
    );
    let budget: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("budget.json")).unwrap()).unwrap();
    let code = program
        .words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect::<Vec<_>>();
    let resources = resources
        .iter()
        .map(|(base, data)| boot::BootResource::copy(*base, data.clone()))
        .collect::<Vec<_>>();
    let package = boot::pack_application_resources(
        &code,
        budget["static_words"].as_u64().unwrap() as usize,
        &resources,
    )
    .unwrap();
    std::fs::write(out.join("cpu-v3-boot.bin"), &package).unwrap();
    let mut fast =
        CpuV3SystemSim::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
            .unwrap()
            .with_functional_gpu();
    let mut instruction_steps = 0;
    let mut fast_halted = false;
    for _ in 0..2_000_000 {
        if fast.waiting_for_vblank() {
            fast.advance_vblank();
        }
        instruction_steps += 1;
        if let StepOutcome::Halted { signal } = fast.step().unwrap() {
            assert_eq!(signal, 0x600d);
            fast_halted = true;
            break;
        }
    }
    assert!(fast_halted);
    let expected = fast.render_active_framebuffer();
    let expected_base = fast.display_state().active_base as usize;
    let expected_color =
        fast.cpu().physical_memory_slice()[expected_base..expected_base + 102400].to_vec();
    let expected_depth =
        fast.cpu().physical_memory_slice()[0x9000 * 64..0x9000 * 64 + 102400].to_vec();
    let mut native =
        CpuV3SystemEmu::from_single_flash(&package, 0, VBlankMode::PauseOnFrameIndexWait)
            .unwrap()
            .with_functional_gpu();
    let wall = std::time::Instant::now();
    let mut starts = vec![];
    let mut presents = vec![];
    let mut halted = false;
    for _ in 0..4_000_000 {
        if native.waiting_for_vblank() && native.advance_vblank() {
            presents.push(native.cycles());
        }
        let cycle = native.cycles();
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
            "native CPU fault at cycle {}",
            native.cycles()
        );
        if output.halted {
            assert_eq!(output.halt_signal, 0x600d);
            halted = true;
            break;
        }
        if native.cycles().is_multiple_of(1_000_000) {
            eprintln!(
                "native baseline {} logic cycles, {} GPU starts",
                native.cycles(),
                starts.len()
            );
        }
    }
    let host_seconds = wall.elapsed().as_secs_f64();
    assert!(halted, "bounded native CPU baseline timeout");
    assert!(native.gpu().last_error().is_none());
    assert_eq!(native.render_active_framebuffer(), expected);
    let base = native.display_state().active_base as usize;
    assert_eq!(
        &native.physical_memory()[base..base + 102400],
        expected_color
    );
    assert_eq!(
        &native.physical_memory()[0x9000 * 64..0x9000 * 64 + 102400],
        expected_depth
    );
    assert_eq!(starts.len(), 3);
    assert_eq!(presents.len(), 3);
    let result = serde_json::json!({
        "scope":"same CB06 program/assets, 3 stationary stage1 PLAY frames, solid background, no input; functional preload outside clock, cold CPU/cache/native MC to HALT",
        "gpu_display_contention":false,"gpu_execution":"functional synchronous host callback; no GPU cycles charged",
        "display_wait":"explicit immediate host vblank when requested; not scanout latency or FPS",
        "logic_cycles":native.cycles(),"native_memory_cycles":native.native_cycles(),"host_wall_seconds_including_functional_gpu":host_seconds,
        "instruction_steps_same_package":instruction_steps,"gpu_start_pre_edge_logic_cycles":starts,"published_swap_logic_cycles":presents,
        "start_to_start_logic_cycles":starts.windows(2).map(|w|w[1]-w[0]).collect::<Vec<_>>(),
        "full_color_depth_and_display_equal":true,
    });
    std::fs::write(
        out.join("native-baseline.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    eprintln!("{result}");
}

/// A diagnostic experiment runs real CPU session/render code and actual functional
/// GPU submissions; only the fixed input and explicit counterfactual are changed.
#[test]
#[ignore = "intentional bounded Bloxorz rotation colour/depth diagnostic"]
fn optimization_rotation_colour_depth_diagnostic() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    const MAX_STEPS: usize = 4_000_000;
    let mut executed = 0;
    for variant in [
        "baseline",
        "always",
        "no-shadow",
        "no-block",
        "far64",
        "far64-always",
        "goal-always",
        "bridge-off-always",
        "bridge-on-always",
        "production",
        "right-standing",
    ] {
        if let Ok(selected) = std::env::var("BLOXORZ_SHADOW_CASE") {
            if !selected.split(',').any(|v| v == variant) {
                continue;
            }
        }
        executed += 1;
        let name = format!("opt-shadow-{variant}");
        let (program, resources, out) =
            application_case(&name, None, None, scenarios::shadow_diagnostic(variant));
        std::fs::write(
            out.join("program.bin"),
            program
                .words
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut halted = false;
        let mut steps = 0;
        let mut count = 0;
        let mut inputs = vec![];
        for _ in 0..MAX_STEPS {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            steps += 1;
            let outcome=sim.step_observing(|write,memory|{
                if write.device==4 && write.channel==0 {count=write.value;}
                if write.device==4 && write.channel==1 {
                    let file=format!("gpu-before-{}.bin",inputs.len());
                    std::fs::write(out.join(&file),memory.iter().flat_map(|w|w.to_le_bytes()).collect::<Vec<_>>()).unwrap();
                    inputs.push(serde_json::json!({"memory":file,"base_bytes":u32::from(write.value)*128,"count_minus_one":count,"maximum_work":10000000}));
                }
                true
            }).unwrap().unwrap();
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d, "{:?}", sim.gpu().last_error());
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded rotation experiment timeout");
        assert!(sim.gpu().last_error().is_none());
        let active = sim.display_state().active_base as usize;
        let m = sim.cpu().physical_memory_slice();
        for (file, at) in [
            ("color-physical.bin", active),
            ("depth-physical.bin", 0x9000 * 64),
        ] {
            std::fs::write(
                out.join(file),
                m[at..at + 1600 * 64]
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        let game = global(&program, "GAME");
        let session = global(&program, "SESSION");
        let motion = global(&program, "MOTION");
        let expected = if variant == "goal-always" {
            [5, 210]
        } else if variant.starts_with("bridge-") || variant == "right-standing" {
            [0, 0]
        } else {
            [1, 110]
        };
        assert_eq!(&m[session..session + 2], &expected);
        let facts = serde_json::json!({"reference_base":"0aacaeb05a95ab7afcccf8d8ba7f26242967b026","compiled_source":"compiled-main.rs","variant":variant,"level":2,"input":if variant=="goal-always" {"level2 (13,2) lying Z, real -Z move then630ms to goal-drop210ms"} else if variant=="right-standing" {"level2 standing (13,2), PLAY, unchanged production depth state"} else if variant.starts_with("bridge-") {"level2 standing (12,5), bridge mask0 or2, PLAY"} else {"level2 standing (13,2), real +Z then110ms"},"camera":if variant.starts_with("far64") {"level2 camera, diagnostic far63"} else {"level2 fixed eye/focal, conservative far from stage_archive"},"steps":steps,"max_steps":MAX_STEPS,"game":m[game..game+10],"session":m[session..session+2],"motion":m[motion..motion+11],"gpu_inputs":inputs,"active_color_byte_base":active*2,"display":"actual render_active_framebuffer output; no additional transfer function","diagnostic_hooks":"existing test compiler without_capture; formal before boot retained separately with hooks enabled"});
        write_display_ppm(
            &out.join("frame-display.ppm"),
            &sim.render_active_framebuffer(),
        );
        std::fs::write(
            out.join("qualification.json"),
            serde_json::to_vec_pretty(&facts).unwrap(),
        )
        .unwrap();
    }
    assert!(executed > 0, "unknown/empty BLOXORZ_SHADOW_CASE");
}

#[test]
fn retained_static_scene_matches_fresh_recording_and_reuses_immutable_commands() {
    static_cache::retained_static_scene_regression();
}

#[test]
fn all_stages_and_new_presentations_render_with_real_cpu_gpu_and_display() {
    use cpu_v3_tang_nano_20k::{
        input_device::{ControllerSnapshot, InputMode},
        system_sim::{CpuV3SystemSim, VBlankMode},
    };
    let start = std::env::var("BLOXORZ_GALLERY_START").map_or(0, |v| v.parse::<usize>().unwrap());
    let expected_frames =
        std::env::var("BLOXORZ_GALLERY_COUNT").map_or(52 - start, |v| v.parse::<usize>().unwrap());
    let (program, resources, out) =
        application_case("all-stage-gallery", None, None, scenarios::gallery());
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    std::fs::write(
        out.join("program.bin"),
        program
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let game = global(&program, "GAME");
    let session = global(&program, "SESSION");
    let motion = global(&program, "MOTION");
    let hud = global(&program, "HUD");
    let mut frames: Vec<serde_json::Value> = vec![];
    let mut count = 0;
    let mut submissions = vec![];
    let mut halted = false;
    let mut winding_samples = Vec::new();
    for step in 1..=160_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        let outcome=sim.step_observing(|write,m|{
            if write.device==4 && write.channel==0 {count=write.value;}
            if write.device==4 && write.channel==1 {
                let base=usize::from(write.value)*64;
                let words=m[base..base+(usize::from(count)+1)*2].as_chunks::<2>().0.iter().map(|p|u32::from(p[0])|(u32::from(p[1])<<16)).collect::<Vec<_>>();
                submissions.push(serde_json::json!({"base_sector":write.value,"commands":words.len(),"native_draws":words.iter().filter(|c|**c>>28==3).count(),"ui_draws":words.iter().filter(|c|**c>>28==11).count()}));
            }
            true
        }).unwrap().unwrap();
        if let StepOutcome::Signaled(e) = outcome {
            if e.signal_type == 13 {
                let at = usize::from(e.value);
                let m = sim.cpu().physical_memory_slice();
                let determinant = world_determinant(&m[at..at + 24]);
                assert!(
                    determinant > 0.,
                    "application world reverses closed-mesh winding"
                );
                winding_samples.push(determinant);
            }
            if e.signal_type == 4 && usize::from(e.value) + start >= 40 {
                let buttons =
                    if usize::from(e.value) + start == 42 || usize::from(e.value) + start == 44 {
                        8192u16
                    } else {
                        0u16
                    };
                sim.publish_input(Some(ControllerSnapshot {
                    mode: InputMode::Analog,
                    buttons_active_low: !buttons,
                    right: [128; 2],
                    left: [128; 2],
                }));
                if usize::from(e.value) + start >= 42 {
                    sim.advance_host_time(std::time::Duration::from_millis(17));
                }
            }
            if e.signal_type == 12 {
                let frame = frames.len() + start;
                let pixels = sim.render_active_framebuffer();
                assert_eq!(pixels.len(), 400 * 240);
                assert!(
                    pixels.iter().filter(|p| **p != pixels[0]).count() > 500,
                    "frame{frame}: missing scene"
                );
                write_display_ppm(&out.join(format!("frame-{frame:02}-display.ppm")), &pixels);
                let m = sim.cpu().physical_memory_slice();
                let calls = submissions
                    .iter()
                    .map(|v| v["native_draws"].as_u64().unwrap() + v["ui_draws"].as_u64().unwrap())
                    .sum::<u64>();
                assert_eq!(
                    u64::from(m[hud + 1]),
                    calls,
                    "frame{frame}: recorded HUD count must match actual DRAW+UI instructions"
                );
                let row0 = actual_hud_cells(m, 0);
                assert_eq!(&row0[..6], b"LEVEL ");
                assert_eq!(actual_hud_number(&row0[6..8]), m[motion + 6] + 1);
                if frame >= 42 {
                    let expected = if frame < 44 { 1u16 } else { 0u16 };
                    assert_eq!(m[hud], expected, "Circle edges must toggle stats");
                    if expected != 0 {
                        let row1 = actual_hud_cells(m, 1);
                        let row2 = actual_hud_cells(m, 2);
                        assert_eq!(
                            u64::from(actual_hud_number(&row1[5..9])),
                            frames.last().unwrap()["executed_draw_calls"]
                                .as_u64()
                                .unwrap()
                        );
                        if let Some(previous) =
                            frames.last().filter(|frame| frame["render_ms"][1] == 1)
                        {
                            assert_eq!(
                                u64::from(actual_hud_number(&row2[3..8])),
                                previous["render_ms"][0].as_u64().unwrap()
                            );
                        } else {
                            assert_eq!(&row2[3..8], b"-----");
                        }
                    }
                }
                frames.push(serde_json::json!({"frame":frame,"executed_draw_calls":calls,"hud":m[hud..hud+3],"render_ms":m[global(&program,"FRAME_RENDER_MS")..global(&program,"FRAME_RENDER_MS")+2],"level":m[motion+6]+1,"steps":step,"game":m[game..game+10],"session":m[session..session+2],"motion":m[motion..motion+17],"submissions":std::mem::take(&mut submissions)}));
                assert!(sim.gpu().last_error().is_none());
                if frame >= 33 {
                    for (file, base) in [
                        ("color", sim.display_state().active_base as usize),
                        ("depth", 0x480000 / 2),
                    ] {
                        std::fs::write(
                            out.join(format!("frame-{frame:02}-{file}.bin")),
                            m[base..base + 102400]
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                    }
                }
            }
        }
        if let StepOutcome::Halted { signal } = outcome {
            assert_eq!(
                signal,
                0x600d,
                "gallery frame{}: {:?}",
                frames.len(),
                sim.gpu().last_error()
            );
            halted = true;
            break;
        }
    }
    assert!(
        halted,
        "bounded all-stage render timeout after{}frames",
        frames.len()
    );
    assert_eq!(frames.len(), expected_frames);
    std::fs::write(out.join("winding.json"),serde_json::to_vec_pretty(&serde_json::json!({"positive_world_determinants":winding_samples,"scope":"actual whole-block world_probe samples; no negative-scale transforms are generated"})).unwrap()).unwrap();
    for f in frames.iter().filter(|f| f["frame"].as_u64().unwrap() < 33) {
        assert_eq!(f["level"], f["frame"].as_u64().unwrap() + 1);
    }
    std::fs::write(out.join("frames.json"),serde_json::to_vec_pretty(&serde_json::json!({"max_steps":160000000,"input":"stages1..33 fixed PLAY then stage8 splitter/cubes/merge and stage4 crack/drop; real session actions at fixed ticks","backend":"instruction CPU sim + functional GPU sim + actual Display conversion","frame_ms_or_fps_claim":false,"frames":frames})).unwrap()).unwrap();
}

#[test]
fn load_time_ground_meshlets_preserve_all_legacy_grid_vertices_and_attributes() {
    let body = r#"
let owner=malloc(512);let words=unsafe{owner.as_u16_array()};let info:Buf<u16,5>=Buf::new([0;5]);
let mut level:u16=0;while level<33 {
 level_reset(level);let mut first:u16=0;
 loop {
  ground_meshlet(first,words,info.as_array());
  if info[1u16]==0 {break;}
  signal(1,level);signal(2,first);signal(3,info.as_ptr().addr());signal(4,words.as_ptr().addr());
  first=info[0u16];
 }
 level+=1;
}
free(owner);
"#;
    let (program, resources, out) = application("ground-batch-builder", Some(body));
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/gpu_renderer_bloxorz");
    assert!(std::process::Command::new("python")
        .arg("-B")
        .arg(example.join("tools/check_ground_batches.py"))
        .arg("--assets")
        .arg(out.join("assets.json"))
        .arg("--output")
        .arg(out.join("ground-reference.json"))
        .status()
        .unwrap()
        .success());
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("ground-reference.json")).unwrap()).unwrap();
    let mut cpu = cpu_v3::CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    let mut level = 0usize;
    let mut first = 0usize;
    let mut info = [0u16; 5];
    let mut jobs = [0usize; 33];
    let mut consumed = [0usize; 33];
    let mut halted = false;
    let mut steps = 0;
    let mut raw_records = vec![];
    for _ in 0..40_000_000 {
        steps += 1;
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) => match e.signal_type {
                1 => level = usize::from(e.value),
                2 => first = usize::from(e.value),
                3 => info.copy_from_slice(
                    &cpu.physical_memory_slice()[usize::from(e.value)..usize::from(e.value) + 5],
                ),
                4 => {
                    let sectors = usize::from(info[1]);
                    assert!((2..=8).contains(&sectors));
                    let address = usize::from(e.value);
                    let words = &cpu.physical_memory_slice()[address..address + sectors * 64];
                    let bytes = words
                        .iter()
                        .flat_map(|w| w.to_le_bytes())
                        .collect::<Vec<_>>();
                    raw_records.push(serde_json::json!({"level":level+1,"first":first,"info":info,"record":bytes}));
                    let v = usize::from(words[0] & 127);
                    let t = usize::from((words[0] >> 8) & 127);
                    let q = usize::from(info[4]);
                    assert_eq!((v, t), (q * 4, q * 2));
                    assert!(v <= 64 && t <= 96);
                    assert_eq!(usize::from(info[0]), first + q);
                    assert_eq!(words[1] & 255, 20);
                    assert_eq!(words[1] >> 15, 0);
                    let u32_at = |index: usize| {
                        u32::from(words[index]) | (u32::from(words[index + 1]) << 16)
                    };
                    let origin = [u32_at(8) as i32, u32_at(10) as i32, u32_at(12) as i32];
                    let shift = words[14] & 31;
                    let faces = expected[level]["faces"].as_array().unwrap();
                    for quad in 0..q {
                        let face = &faces[first + quad];
                        assert_eq!(face["material"].as_u64().unwrap(), u64::from(info[2]));
                        assert_eq!(face["bucket"].as_u64().unwrap(), u64::from(info[3]));
                        for corner in 0..4 {
                            let at = 64 + (quad * 4 + corner) * 6;
                            let pos = u32_at(at);
                            let mut vertex = (0..3)
                                .map(|axis| {
                                    i64::from(origin[axis])
                                        + i64::from((pos >> (axis * 10)) & 1023) * (1i64 << shift)
                                })
                                .collect::<Vec<_>>();
                            vertex.push(i64::from(u32_at(at + 2)));
                            vertex.push(i64::from(u32_at(at + 4)));
                            let reference = face["vertices"][corner]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| v.as_i64().unwrap())
                                .collect::<Vec<_>>();
                            assert_eq!(
                                vertex,
                                reference,
                                "level{} face{} corner{}",
                                level + 1,
                                first + quad,
                                corner
                            );
                        }
                    }
                    let offset = usize::from((words[1] >> 8) & 127);
                    assert_eq!(offset * 4, 24 * q);
                    for index in 0..t.div_ceil(3) {
                        let at = 64 + offset * 4 + index * 4;
                        let bits =
                            (0..4).fold(0u64, |a, i| a | (u64::from(words[at + i]) << (16 * i)));
                        assert_eq!(bits >> ((t - index * 3).min(3) * 18), 0);
                    }
                    for tri in 0..t {
                        let at = 64 + offset * 4 + (tri / 3) * 4;
                        let packed = (0..4)
                            .fold(0u64, |a, i| a | (u64::from(words[at + i]) << (16 * i)))
                            >> (18 * (tri % 3));
                        let got = [
                            (packed & 63) as usize,
                            ((packed >> 6) & 63) as usize,
                            ((packed >> 12) & 63) as usize,
                        ];
                        let base = tri / 2 * 4;
                        let want = if tri % 2 == 0 {
                            [base, base + 1, base + 2]
                        } else {
                            [base, base + 2, base + 3]
                        };
                        assert_eq!(got, want);
                    }
                    assert_eq!(consumed[level], first);
                    consumed[level] += q;
                    jobs[level] += 1;
                }
                _ => {}
            },
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "40M-step bounded builder timeout");
    for level in 0..33 {
        assert_eq!(
            consumed[level],
            expected[level]["faces"].as_array().unwrap().len()
        );
        assert!(jobs[level] <= 32);
    }
    std::fs::write(
        out.join("records.json"),
        serde_json::to_vec(&raw_records).unwrap(),
    )
    .unwrap();
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"levels":33,"steps":steps,"max_steps":40000000,"jobs":jobs.as_slice(),"faces":consumed.as_slice(),"all_source_positions_normals_uv_indices_exact":true,"gpu_executed":false,"scope":"RCC geometry construction; production GPU publication and lifecycle qualified separately"})).unwrap()).unwrap();
}

#[test]
fn same_heap_address_rebuilds_only_after_retirement_and_capture_reads_new_records() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let (program, resources, out) = application_case(
        "web-stage1-warm-heap-reload-capture",
        None,
        None,
        scenarios::heap_capture(),
    );
    let base_word = global(&program, "GROUND_BASE");
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut frame = 0usize;
    let mut captures = vec![];
    let mut current = vec![];
    let mut expected = vec![];
    let mut sampled_sector = 0u16;
    let mut images = vec![];
    let mut heap_bases = vec![];
    let mut halted = false;
    let mut submissions = 0;
    for _ in 0..20_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        let outcome = sim
            .step_observing(|w, m| {
                if w.device == 4 && w.channel == 1 {
                    let base = usize::from(m[base_word]);
                    assert_eq!(base & 63, 0);
                    assert!((0x2000..0xa000).contains(&base));
                    assert_eq!(m[base + 7], if frame == 1 { 2 } else { 1 });
                    assert!(m[base + 6] <= 128);
                    heap_bases.push(base);
                    submissions += 1;
                }
                true
            })
            .unwrap()
            .unwrap();
        if let StepOutcome::Signaled(e) = outcome {
            match e.signal_type {
                4 => frame = usize::from(e.value),
                5 => assert!(current.is_empty() || current.len() == 40),
                6 => {
                    sampled_sector = e.value;
                    current.clear();
                    let at = usize::from(e.value) * 64;
                    let m = sim.cpu().physical_memory_slice();
                    expected = m[at..at + 16]
                        .iter()
                        .chain(m[at + 64..at + 88].iter())
                        .copied()
                        .collect();
                }
                7 => {
                    assert_eq!(
                        e.value,
                        expected[current.len()],
                        "capture reused old heap data at frame{} sector{} sample{}",
                        frame,
                        sampled_sector,
                        current.len()
                    );
                    current.push(e.value);
                    if current.len() == 40 {
                        captures.push(serde_json::json!({"frame":frame,"sector":sampled_sector,"words":current}));
                    }
                }
                9 | 10 => {
                    assert_eq!(e.value, 0);
                    assert!(!sim.gpu().busy());
                }
                12 => {
                    let pixels = sim.render_active_framebuffer();
                    write_display_ppm(&out.join(format!("frame-{frame}.ppm")), &pixels);
                    images.push(pixels);
                }
                _ => {}
            }
        }
        if let StepOutcome::Halted { signal } = outcome {
            assert_eq!(
                signal,
                0x600d,
                "GPU API fault {:?}",
                &sim.cpu().physical_memory_slice()
                    [global(&program, "GPU_API_FAULT")..global(&program, "GPU_API_FAULT") + 6]
            );
            halted = true;
            break;
        }
    }
    assert!(halted);
    assert_eq!(images.len(), 3);
    assert_eq!(images[0], images[2]);
    assert_ne!(images[0], images[1]);
    assert_eq!(
        submissions, 9,
        "each of the three level generations builds a new cache"
    );
    assert!(heap_bases.iter().all(|b| *b == heap_bases[0]));
    assert!(!captures.is_empty());
    let first_sector = captures[0]["sector"].as_u64().unwrap();
    let values = (0..3)
        .map(|f| {
            captures
                .iter()
                .find(|v| v["frame"] == f && v["sector"] == first_sector)
                .unwrap()["words"]
                .clone()
        })
        .collect::<Vec<_>>();
    assert_ne!(values[0], values[1]);
    assert_eq!(values[0], values[2]);
    std::fs::write(out.join("capture-qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"max_steps":20000000,"levels":[1,2,1],"same_heap_base":heap_bases[0],"heap_capacity_bytes":16384,"all_capture_samples_match_current_physical_words":true,"new_and_returned_images_verified":true,"captures":captures,"gpu_submissions":submissions,"shared_driver_edits":false})).unwrap()).unwrap();
}
