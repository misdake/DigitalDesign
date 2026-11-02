//! Bounded real-stage rules, native Instances, RTT and actual Display tests.
use super::changes::{global, preload};
use super::*;
use cpu_v3::StepOutcome;
use std::path::PathBuf;
fn application(
    name: &str,
    probe: Option<&str>,
) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = root.join("examples/gpu_renderer_bloxorz");
    let out = root.join("../../target/bloxorz-rtt").join(name);
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
    assert_eq!(catalog["profile"], "production");
    let mut resources = vec![];
    let mut config = String::from("format 2\ngpu-api\napplication main.rs\n");
    for r in catalog["resources"].as_array().unwrap() {
        let file = r["file"].as_str().unwrap();
        let base = r["address"].as_u64().unwrap() as usize;
        config.push_str(&format!(
            "copy GPU_RENDERER_{} {base} {file}\n",
            file.trim_end_matches(".bin").to_uppercase()
        ));
        resources.push((base, std::fs::read(out.join(file)).unwrap()));
    }
    let constants = cpu_v3_tang_nano_20k::boot::parse_boot_project(&config)
        .unwrap()
        .constants()
        + &std::fs::read_to_string(out.join("renderer_assets.rs")).unwrap();
    let mut source = std::fs::read_to_string(example.join("rcc/main.rs")).unwrap();
    if name.starts_with("web-stage") {
        let level = u16::from(name.contains("stage2"));
        let background = u16::from(name.ends_with("gradient"));
        source = source.replace("session_init();", &format!("session_init();level_reset({level});SESSION[0u16]=PLAY;BACKGROUND[0u16]={background};"));
        source = source.replace(
            "run(3, false, 128);",
            if name.contains("warm") {
                "run(3, false, 128);"
            } else {
                "run(1, false, 128);"
            },
        );
    }
    if name == "web-goal-mid" {
        let initialize = r#"fn initialize_goal_mid(){session_init();BACKGROUND[0u16]=0;
        let mut goal_tick:u16=0;while goal_tick<15 {session_tick(0,0,40);goal_tick+=1;}
        let goal_route:Buf<u16,7>=Buf::new([0,0,2,0,0,0,2]);let mut goal_move:u16=0;
        while goal_move<7 {session_move(goal_route[goal_move]);goal_tick=0;while goal_tick<5 {session_tick(0,0,50);goal_tick+=1;}goal_move+=1;}
        goal_tick=0;while goal_tick<4 {session_tick(0,0,50);goal_tick+=1;}
        goal_tick=0;while goal_tick<4 {session_tick(0,0,50);goal_tick+=1;}session_tick(0,0,10);
}
"#;
        source = source
            .replace("session_init();", "initialize_goal_mid();")
            .replace("run(3, false, 128);", "run(1, false, 128);")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;");
        source.push_str(initialize);
    }
    if name.starts_with("web-cache-") {
        source = source
            .replace("run(3, false, 128);", "run(9, false, 128);")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;");
        source=source.replace("    loop {",r#"    loop {
        if number==0 {level_reset(1);GAME[0u16]=4;GAME[1u16]=4;GAME[2u16]=0;SESSION[0u16]=PLAY;BACKGROUND[0u16]=0;}
        if number==2 {GAME[0u16]=5;}
        if number==3 {GAME[3u16]=1;}
        if number==4 {level_reset(0);GAME[0u16]=7;GAME[1u16]=4;SESSION[0u16]=PLAY;}
        if number==5 {SESSION[0u16]=GOAL_DROP;SESSION[1u16]=0;}
        if number==6 {level_reset(0);GAME[0u16]=7;GAME[1u16]=4;SESSION[0u16]=GOAL_DROP;SESSION[1u16]=0;}
        if number==7 {SESSION[0u16]=ENTRY_OPEN;SESSION[1u16]=0;}
        if number==8 {SESSION[0u16]=PLAY;}
"#);
        source=source.replace("gpu.submit(commands);","signal(14,commands.push_words());signal(15,commands.command_words());gpu.submit(commands);");
        if name.ends_with("forced") {
            source = source.replace("draw_shadow(", "shadow_invalidate();draw_shadow(");
        }
    }
    if let Some(case_name) = name.strip_prefix("web-shadow-") {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../examples/gpu_renderer_bloxorz/assets/web-shadow-reference.json"
        ))
        .unwrap();
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == case_name)
            .unwrap();
        source = source.replace(
            "session_init();",
            if case_name == "entry" {
                "session_init();SESSION[0u16]=ENTRY_RISE;SESSION[1u16]=190;BACKGROUND[0u16]=0;"
            } else {
                "session_init();SESSION[0u16]=PLAY;BACKGROUND[0u16]=0;"
            },
        );
        source = source
            .replace("run(3, false, 128);", "run(1, false, 128);")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;");
        let begin = source.find("fn prepare_block(").unwrap();
        let end = begin + source[begin..].find("\nfn draw_block(").unwrap();
        source.replace_range(begin..end, &shadow_reference_prepare(case));
        source=source.replace("gpu.submit(commands);","signal(15,commands.command_words());signal(14,commands.push_words());gpu.submit(commands);");
    }
    let entry = if let Some(body) = probe {
        source += &format!(
            "\nfn check(ok:bool){{if !ok{{halt(0xbad);}}}}\npub fn probe(){{{body}halt(0x600d);}}"
        );
        "probe"
    } else if name.starts_with("web-stage")
        || name.starts_with("web-shadow-")
        || name.starts_with("web-cache-")
        || name == "web-goal-mid"
    {
        "renderer_game_three_frames"
    } else if name == "automatic-capture-full-application" {
        "renderer_game_capture_full"
    } else if name == "automatic-capture-application" {
        "renderer_game_capture"
    } else {
        "renderer_game_frames"
    };
    // Test-only readonly instrumentation uses the public getter, never arena slot layout.
    // Original marker12 is preserved for the independent input-transition trials.
    source = source.replace(
        "block_pose(model);",
        "block_pose(model);world_probe(model);",
    );
    source += "\nfn world_probe(model:&Model){let world=model.world();let words:Buf<u16,24>=Buf::new([world.row0.x.lo_bits(),world.row0.x.hi_bits(),world.row0.y.lo_bits(),world.row0.y.hi_bits(),world.row0.z.lo_bits(),world.row0.z.hi_bits(),world.row0.w.lo_bits(),world.row0.w.hi_bits(),world.row1.x.lo_bits(),world.row1.x.hi_bits(),world.row1.y.lo_bits(),world.row1.y.hi_bits(),world.row1.z.lo_bits(),world.row1.z.hi_bits(),world.row1.w.lo_bits(),world.row1.w.hi_bits(),world.row2.x.lo_bits(),world.row2.x.hi_bits(),world.row2.y.lo_bits(),world.row2.y.hi_bits(),world.row2.z.lo_bits(),world.row2.z.hi_bits(),world.row2.w.lo_bits(),world.row2.w.hi_bits()]);signal(13,words.as_ptr().addr());}\n";
    let source = crate::shared_application_entry::select(&source, entry).unwrap();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let mut dependencies = vec![];
    let ir = resolver::compile_explicit_gpu_program_checked_with_sources(
        "Bloxorz RTT",
        &source,
        &options,
        &example.join("rcc"),
        &root.join("rcc"),
        &constants,
        &[root.join("examples/bloxorz/rcc")],
        &resolver::RendererCaptureConfiguration::without_capture(
            resolver::RendererLayoutProfile::Column16,
        ),
        &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
        &mut |p| dependencies.push(p.to_owned()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(dependencies.contains(&root.join("examples/bloxorz/rcc/bloxorz_core.rs")));
    std::fs::write(
        out.join("budget.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"static_words":ir.debug.static_data_end}))
            .unwrap(),
    )
    .unwrap();
    (
        rcc_backend::try_compile(ir, &options, "main").unwrap(),
        resources,
        out,
    )
}

#[test]
fn first_two_stage_solutions_undo_and_overlapping_fade_follow_real_rules() {
    let body = r#"
session_init();let mut n:u16=0;while n<15 {session_tick(0,0,40);n+=1;}
check(SESSION[0u16]==PLAY && GAME[5u16]==0);signal(6,1);
let first:Buf<u16,7>=Buf::new([0,0,2,0,0,0,2]);n=0;
while n<7 {
    let old_mask=GAME[3u16];check(session_move(first[n]));check(visual_mask()==old_mask);
    let mut k:u16=0;while k<5 {session_tick(0,0,50);k+=1;}n+=1;
}
check(GAME[4u16]==1 && GAME[5u16]==7 && SESSION[0u16]==GOAL_OPEN);signal(6,2);
session_tick(32768,0,0);check(GAME[4u16]==0 && GAME[5u16]==6 && SESSION[0u16]==PLAY);
check(session_move(2));n=0;while n<5 {session_tick(0,0,50);n+=1;}
n=0;while !fading() && n<32 {session_tick(0,0,40);n+=1;}
check(fading() && SESSION[0u16]==GOAL_CLOSE && MOTION[6u16]==0);signal(6,3);
session_presented();check(MOTION[6u16]==0);
n=0;while n<5 {session_tick(0,0,50);n+=1;}
check(SESSION[1u16]>=200 && !block_visible() && TRANSITION[0u16]==FADE_OUT);
n=0;while TRANSITION[1u16]<320 && n<10 {session_tick(0,0,50);n+=1;}
check(MOTION[6u16]==0);session_presented();check(MOTION[6u16]==1 && TRANSITION[0u16]==HOLD && GAME[5u16]==0);signal(6,4);
n=0;while n<16 {session_tick(8|4096|32,32,50);n+=1;}
check(TRANSITION[0u16]==FADE_IN && TRANSITION[1u16]==380 && GAME[5u16]==0);
session_presented();check(!fading() && SESSION[0u16]==ENTRY_OPEN);
n=0;while n<15 {session_tick(0,0,40);n+=1;}
check(SESSION[0u16]==PLAY);signal(6,5);
let second:Buf<u16,17>=Buf::new([3,0,2,0,0,0,0,3,3,2,0,2,0,0,3,0,3]);n=0;
while n<17 {
    let old_mask=GAME[3u16];check(session_move(second[n]));check(visual_mask()==old_mask);
    let mut k:u16=0;while k<5 {session_tick(0,0,50);k+=1;}n+=1;
}
check(GAME[4u16]==1 && GAME[5u16]==17);signal(6,6);
// Full unsupported release leaves before220ms, then continues during the veil.
level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;TRANSITION[0u16]=0;
check(session_move(0));n=0;while n<5 {session_tick(0,0,50);n+=1;}
check(session_move(0));n=0;while n<5 {session_tick(0,0,50);n+=1;}
check(session_move(3));session_tick(0,0,40);session_tick(0,0,40);
check(SESSION[0u16]==FALL && MOTION[4u16]==80 && SESSION[1u16]==0);
n=0;while n<12 {session_tick(0,0,50);n+=1;}
check(fading() && SESSION[1u16]==600);session_tick(0,0,50);check(SESSION[1u16]==650);signal(6,7);
// Held movement repeats after settle; newest held direction wins.
level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;TRANSITION[0u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;
session_tick(32,32,0);check(GAME[5u16]==1);n=0;while n<5 {session_tick(0,32,50);n+=1;}
check(GAME[5u16]==2 && SESSION[0u16]==ROLL);
session_tick(64,32|64,0);n=0;while n<5 {session_tick(0,32|64,50);n+=1;}
check(MOTION[3u16]==2 && GAME[5u16]==3);signal(6,8);
// Retain256 complete snapshots across a ring wrap, including a16-bit move count.
level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;TRANSITION[0u16]=0;
n=0;while n<260 {check(session_move(if n&1==0 {0u16}else{1u16}));let mut k:u16=0;while k<5 {session_tick(0,0,50);k+=1;}n+=1;}
check(GAME[5u16]==260);n=0;while n<256 {session_tick(32768,0,0);n+=1;}
check(GAME[5u16]==4 && GAME[0u16]==LEVEL[2u16] && GAME[1u16]==LEVEL[3u16]);
session_tick(32768,0,0);check(GAME[5u16]==4);signal(6,9);
// Re-pressing one held direction cannot evict another still-held direction.
SESSION[0u16]=ENTRY_OPEN;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;
session_tick(128,128,0);session_tick(32,128|32,0);n=0;
while n<6 {session_tick(32,128|32,0);n+=1;}
session_tick(0,128,0);check((SESSION[8u16]&15)==2);signal(6,10);
// Web switch press duration is260ms, independently of the220ms roll.
level_reset(1);SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;GAME[3u16]=1;
session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,30);
let half=switch_fraction(0);check(half>fix32::from_words(32760,0) && half<fix32::from_words(32776,0));
session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,30);
check(switch_fraction(0)==fix32::from_int(1));signal(6,11);
// Accepted moves stop at65535; undo reconstructs the previous full16-bit count.
level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;GAME[5u16]=65534;
check(session_move(0));check(GAME[5u16]==65535);n=0;while n<5 {session_tick(0,0,50);n+=1;}check(SESSION[0u16]==PLAY);check(!session_move(0));
session_tick(32768,0,0);check(GAME[5u16]==65534 && GAME[0u16]==LEVEL[2u16] && GAME[1u16]==LEVEL[3u16]);signal(6,12);


"#;
    let (program, resources, _) = application("web-rules-lifecycle", Some(body));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    let mut stage = 0;
    let mut halted = false;
    for _ in 0..2_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 6 => stage = e.value,
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d, "lifecycle stage{stage}");
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded real-stage rules timeout");
    assert_eq!(stage, 12);
}

#[test]
fn actual_session_poses_match_independent_web_entry_tip_and_release_matrices() {
    let body = r#"
let mut gpu=Gpu::init(demo_config());let mut model=game_plain_model();
session_init();let mut n:u16=0;while n<15 {session_tick(0,0,40);n+=1;}
block_pose(&mut model);world_probe(&model);
session_init();n=0;while n<4 {session_tick(0,0,50);n+=1;}n=0;while n<3 {session_tick(0,0,50);n+=1;}session_tick(0,0,40);
block_pose(&mut model);world_probe(&model);
level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;check(session_move(1));
n=0;while n<4 {session_tick(0,0,50);n+=1;}session_tick(0,0,20);check(SESSION[0u16]==TIP);
n=0;while n<3 {session_tick(0,0,50);n+=1;}session_tick(0,0,30);
block_pose(&mut model);world_probe(&model);
level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;
check(session_move(0));n=0;while n<4 {session_tick(0,0,50);n+=1;}session_tick(0,0,20);
check(session_move(0));n=0;while n<4 {session_tick(0,0,50);n+=1;}session_tick(0,0,20);
check(session_move(3));session_tick(0,0,40);session_tick(0,0,40);n=0;while n<3 {session_tick(0,0,50);n+=1;}
check(SESSION[0u16]==FALL && SESSION[1u16]==150);block_pose(&mut model);world_probe(&model);
"#;
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let (program, resources, out) = application("web-actual-session-poses", Some(body));
    let mut sim = CpuV3SystemSim::new(VBlankMode::Manual).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut matrices = vec![];
    let mut halted = false;
    for _ in 0..2_000_000 {
        match sim.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 13 => {
                let at = usize::from(e.value);
                matrices.push(sim.cpu().physical_memory_slice()[at..at + 24].to_vec());
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted);
    assert_eq!(matrices.len(), 4);
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../examples/gpu_renderer_bloxorz/assets/web-shadow-reference.json"
    ))
    .unwrap();
    let mut errors = vec![];
    for (words, case) in matrices.iter().zip(reference["cases"].as_array().unwrap()) {
        let mut error = 0f64;
        for r in 0..3 {
            for c in 0..4 {
                let actual = scene::fixed(words, (r * 4 + c) * 2);
                let expected = case["world"][c * 4 + r].as_f64().unwrap();
                error = error.max((actual - expected).abs());
            }
        }
        assert!(
            error < 0.003,
            "{} actual session world error {error}",
            case["name"]
        );
        errors.push(serde_json::json!({"case":case["name"],"max_world_coefficient_error":error,"actual_u16":words}));
    }
    std::fs::write(
        out.join("actual-session-world.json"),
        serde_json::to_vec_pretty(&errors).unwrap(),
    )
    .unwrap();
}

fn goal_region(catalog: &serde_json::Value) -> (usize, usize, usize, usize) {
    let c = &catalog["cameras"][0];
    let eye: [f64; 3] = std::array::from_fn(|i| c["eye"][i].as_f64().unwrap());
    let target = [0., 0.6, 0.];
    let mut back: [f64; 3] = std::array::from_fn(|i| eye[i] - target[i]);
    let n = back.iter().map(|x| x * x).sum::<f64>().sqrt();
    for x in &mut back {
        *x /= n;
    }
    let mut right = [back[2], 0., -back[0]];
    let n = right.iter().map(|x| x * x).sum::<f64>().sqrt();
    for x in &mut right {
        *x /= n;
    }
    let up = [
        back[1] * right[2] - back[2] * right[1],
        back[2] * right[0] - back[0] * right[2],
        back[0] * right[1] - back[1] * right[0],
    ];
    let f = c["focal"].as_f64().unwrap();
    let mut bounds = [400f64, 240., 0., 0.];
    // Real stage1 goal(7,4) and its adjacent tiles, including well depth.
    for x in [1., 4.] {
        for y in [-2.2, 0.05] {
            for z in [0., 3.] {
                let p: [f64; 3] = std::array::from_fn(|i| [x, y, z][i] - eye[i]);
                let depth = -(0..3).map(|i| back[i] * p[i]).sum::<f64>();
                let sx = 200. + f * (0..3).map(|i| right[i] * p[i]).sum::<f64>() / depth;
                let sy = 120. - f * (0..3).map(|i| up[i] * p[i]).sum::<f64>() / depth;
                bounds[0] = bounds[0].min(sx);
                bounds[1] = bounds[1].min(sy);
                bounds[2] = bounds[2].max(sx);
                bounds[3] = bounds[3].max(sy);
            }
        }
    }
    (
        bounds[0].floor().max(0.) as usize,
        bounds[1].floor().max(0.) as usize,
        bounds[2].ceil().min(400.) as usize,
        bounds[3].ceil().min(240.) as usize,
    )
}
fn write_display_ppm(path: &Path, pixels: &[u32]) {
    let mut ppm = b"P6\n400 240\n255\n".to_vec();
    for &p in pixels {
        ppm.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(path, ppm).unwrap();
}
fn expanded565(c: u16) -> [u32; 3] {
    let r = u32::from(c >> 11);
    let g = u32::from((c >> 5) & 63);
    let b = u32::from(c & 31);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}
#[test]
fn live_veil_uses_current_offscreen_pixels_and_discards_locked_boundary_input() {
    use cpu_v3_tang_nano_20k::{
        input_device::{ControllerSnapshot, InputMode},
        system_sim::{CpuV3SystemSim, VBlankMode},
    };
    let (program, resources, out) = application("web-live-veil", None);
    let session = global(&program, "SESSION");
    let transition = global(&program, "TRANSITION");
    let motion = global(&program, "MOTION");
    let game = global(&program, "GAME");
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    let region = goal_region(&catalog);
    let mut goal_pixels: Option<Vec<u32>> = None;
    let mut goal_checks = 0;
    let initial_background = sim.cpu().physical_memory_slice()[0x580000 / 2..0x5b2000 / 2].to_vec();
    let mut connected = false;
    let mut moved = false;
    let mut restarted = false;
    let mut boundary = false;
    let mut held_play = 0;
    let mut released = false;
    let mut new_press = false;
    let mut accepted = false;
    let mut count = 0u16;
    let mut sources = 0;
    let mut consumers = 0;
    let mut ordinary = [false; 2];
    let mut source_images = vec![];
    let mut half = false;
    let mut opaque = false;
    let mut saw_live_roll = false;
    let mut initial_closed = false;
    for _ in 0..40_000_000 {
        if sim.waiting_for_vblank() {
            let m = sim.cpu().physical_memory_slice();
            if !boundary && m[transition] == 11 && m[transition + 1] == 380 {
                sim.publish_input(Some(ControllerSnapshot {
                    mode: InputMode::Analog,
                    buttons_active_low: !(32u16 | 8 | 4096),
                    right: [128; 2],
                    left: [128; 2],
                }));
                boundary = true;
            }
            sim.advance_vblank();
        }
        let step = sim
            .step_observing(|write, m| {
                if write.device == 4 && write.channel == 0 {
                    count = write.value;
                }
                if write.device == 4 && write.channel == 1 {
                    let at = usize::from(write.value) * 64;
                    let code = m[at..at + (usize::from(count) + 1) * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16))
                        .collect::<Vec<_>>();
                    assert_eq!(code.last(), Some(&0xf0000000));
                    let mut local = vec![0u16; 2048];
                    let mut render = 0usize;
                    let mut target = 0u32;
                    let mut depth = 0u32;
                    let mut fixed = false;
                    let mut copy = false;
                    let mut invalidate = false;
                    let mut clear = None;
                    for c in code {
                        match c >> 28 {
                            1 => {
                                let src = ((c >> 10) & 0x3ffff) as usize * 16;
                                let dst = ((c >> 3) & 127) as usize * 16;
                                let n = 16 << ((c >> 1) & 3);
                                local[dst..dst + n].copy_from_slice(&m[src..src + n]);
                            }
                            6 => render = (c & 127) as usize * 16,
                            10 => {
                                let q = (c & 127) as usize * 16;
                                let w = u64::from(local[q])
                                    | (u64::from(local[q + 1]) << 16)
                                    | (u64::from(local[q + 2]) << 32)
                                    | (u64::from(local[q + 3]) << 48);
                                target = ((w & 0x3fff) << 9) as u32;
                                depth = (((w >> 14) & 0x3fff) << 9) as u32;
                            }
                            3 if local[render + 1] & 128 != 0 => {
                                fixed = true;
                                assert_eq!(local[render], 0xc000);
                                assert_ne!(local[render + 4] & 16, 0);
                            }
                            8 if c & 65536 == 0 => clear = Some(c as u16),
                            9 => invalidate = true,
                            12 => copy = true,
                            _ => {}
                        }
                    }
                    if fixed {
                        consumers += 1;
                        assert!(invalidate && !copy);
                        assert_eq!(clear, Some(0x1107));
                        assert_eq!(depth, 0x480000);
                        assert!(target == 0x400000 || target == 0x440000);
                    } else {
                        assert!(copy && clear.is_none());
                        if target == 0x600000 {
                            sources += 1;
                            assert_eq!(depth, 0x640000);
                        } else {
                            assert!(target == 0x400000 || target == 0x440000);
                            ordinary[usize::from(target == 0x440000)] = true;
                        }
                    }
                }
                true
            })
            .unwrap()
            .unwrap();
        match step {
            StepOutcome::Signaled(e) if e.signal_type == 4 => {
                if e.value != 0 {
                    sim.advance_host_time(std::time::Duration::from_millis(40));
                }
                let m = sim.cpu().physical_memory_slice();
                let mut buttons = 0u16;
                if m[session]==3 && m[session+1]>=300 {connected=true;}
                if boundary {
                    if !released {
                        buttons = 32 | 8 | 4096;
                        if m[session] == 0 && m[transition] == 0 {
                            assert_eq!(m[game + 5], 0);
                            assert_eq!(m[motion + 6], 0);
                            held_play += 1;
                            if held_play >= 2 {
                                buttons = 0;
                                released = true;
                            }
                        }
                    } else if !new_press {
                        buttons = 8;
                        new_press = true;
                    }
                } else if m[session] == 0 && !moved {
                    buttons = 32;
                    moved = true;
                } else if m[session] == 1 && m[session + 1] >= 40 && !restarted {
                    buttons = 8;
                    restarted = true;
                }
                if connected {
                    sim.publish_input(Some(ControllerSnapshot {
                        mode: InputMode::Analog,
                        buttons_active_low: !buttons,
                        right: [128; 2],
                        left: [128; 2],
                    }));
                } else {
                    sim.publish_input(None);
                }
            }
            StepOutcome::Signaled(e) if e.signal_type == 11 => {
                let m = sim.cpu().physical_memory_slice();
                source_images.push(m[0x600000 / 2..0x632000 / 2].to_vec());
                if m[session] == 1 {
                    saw_live_roll = true;
                }
            }
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let m = sim.cpu().physical_memory_slice();
                let phase = m[transition];
                let time = m[transition + 1];
                if phase==0 {
                    let pixels=sim.render_active_framebuffer();let mut current=vec![];
                    for y in region.1..region.3 {current.extend_from_slice(&pixels[y*400+region.0..y*400+region.2]);}
                    if let Some(expected)=&goal_pixels {assert!(expected==&current,"goal/neighbor pixels changed during entry or the first move");goal_checks+=1;}else{goal_pixels=Some(current);}
                }
                if m[session] == 2 && m[session + 1] == 0 && !initial_closed {
                    write_display_ppm(
                        &out.join("entry-closed-no-controller-display.ppm"),
                        &sim.render_active_framebuffer(),
                    );
                    initial_closed = true;
                }
                if phase == 9 && time == 160 && !half {
                    let active = sim.display_state().active_base as usize;
                    let veil = expanded565(0x1107);
                    for y in 0..240 {
                        for x in 0..400 {
                            let offset =
                                (((x >> 4) * 16 + (y >> 4)) * 256) + (y & 15) * 16 + (x & 15);
                            let source = expanded565(m[0x600000 / 2 + offset]);
                            let mix: [u32; 3] =
                                std::array::from_fn(|i| (source[i] + veil[i]).div_ceil(2));
                            let expected = (((mix[0] * 31 + 127) / 255) << 11)
                                | (((mix[1] * 63 + 127) / 255) << 5)
                                | ((mix[2] * 31 + 127) / 255);
                            assert_eq!(
                                u32::from(m[active + offset]),
                                expected,
                                "half veil pixel {x},{y}"
                            );
                        }
                    }
                    write_display_ppm(
                        &out.join("half-veil-display.ppm"),
                        &sim.render_active_framebuffer(),
                    );
                    half = true;
                }
                if phase == 9 && time == 320 && !opaque {
                    assert!(sim
                        .render_active_framebuffer()
                        .iter()
                        .all(|p| *p == 0x102039));
                    write_display_ppm(
                        &out.join("opaque-veil-display.ppm"),
                        &sim.render_active_framebuffer(),
                    );
                    opaque = true;
                }
                if new_press && phase == 9 {
                    accepted = true;
                    break;
                }
            }
            StepOutcome::Halted { signal } => panic!("unexpected live-veil halt {signal:#x}, moved={moved} restarted={restarted} boundary={boundary} held_play={held_play}"),
            _ => {}
        }
    }
    assert!(
        accepted
            && moved
            && restarted
            && boundary
            && half
            && opaque
            && saw_live_roll
            && initial_closed
    );
    assert!(ordinary.iter().all(|v| *v));
    assert!(goal_checks > 10);
    assert_eq!(sources, consumers);
    assert!(sources > 10);
    assert!(
        source_images.windows(2).any(|w| w[0] != w[1]),
        "outgoing rolling scene froze during fade"
    );
    assert_eq!(
        &sim.cpu().physical_memory_slice()[0x580000 / 2..0x5b2000 / 2],
        initial_background
    );
    assert!(sim.gpu().last_error().is_none());
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"functional_gpu_actual_display":true,"live_sources":sources,"consumers":consumers,"goal_neighbor_invariant_normal_frames":goal_checks,"initial_closed_entry_without_input":initial_closed,"full_half_veil_formula_matches":half,"opaque_color_matches":opaque,"boundary_held_dpad_start_triangle_suppressed":true,"fresh_start_accepted":accepted,"both_normal_back_targets":ordinary,"source_immutable":true,"scope":"instruction CPU + functional GPU/Display; explicit40ms host ticks; no board, scanout timing or native CPU performance claim"})).unwrap()).unwrap();
}

#[test]
fn web_first_two_stage_play_frames_use_functional_gpu_and_actual_display() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    for name in [
        "web-stage1-play",
        "web-stage2-play",
        "web-stage1-gradient",
        "web-goal-mid",
    ] {
        let (program, resources, out) = application(name, None);
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
    let (program, resources, out) = application("web-stage1-native-warm", None);
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

fn shadow_reference_prepare(case: &serde_json::Value) -> String {
    let m = case["world"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect::<Vec<_>>();
    let a = [m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10]];
    let c = [
        a[4] * a[8] - a[5] * a[7],
        a[5] * a[6] - a[3] * a[8],
        a[3] * a[7] - a[4] * a[6],
        a[2] * a[7] - a[1] * a[8],
        a[0] * a[8] - a[2] * a[6],
        a[1] * a[6] - a[0] * a[7],
        a[1] * a[5] - a[2] * a[4],
        a[2] * a[3] - a[0] * a[5],
        a[0] * a[4] - a[1] * a[3],
    ];
    let determinant = a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
    assert!(determinant.abs() > 0.1);
    let fixed = |v: f64| {
        let n = (v * 65536.).round() as i32 as u32;
        format!("fix32::from_words({},{})", n & 65535, n >> 16)
    };
    let mut world = vec![];
    let mut normal = vec![];
    for row in 0..3 {
        world.push(format!(
            "row{row}:Vec4{{x:{},y:{},z:{},w:{}}}",
            fixed(m[row]),
            fixed(m[4 + row]),
            fixed(m[8 + row]),
            fixed(m[12 + row])
        ));
        normal.push(format!(
            "row{row}:demo_point({},{},{})",
            fixed(c[row * 3] / determinant),
            fixed(c[row * 3 + 1] / determinant),
            fixed(c[row * 3 + 2] / determinant)
        ));
    }
    format!(
        "fn prepare_block(model:&mut Model){{model.set_matrices(Mat3x4{{{}}},Mat3x3{{{}}});}}",
        world.join(","),
        normal.join(",")
    )
}
fn hull_distance(point: [f64; 2], polygon: &[[f64; 2]]) -> f64 {
    polygon
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = polygon[(i + 1) % polygon.len()];
            let dx = b[0] - a[0];
            let dz = b[1] - a[1];
            let length = dx * dx + dz * dz;
            let t = if length > 0. {
                ((point[0] - a[0]) * dx + (point[1] - a[1]) * dz) / length
            } else {
                0.
            }
            .clamp(0., 1.);
            (point[0] - a[0] - t * dx).hypot(point[1] - a[1] - t * dz)
        })
        .fold(f64::INFINITY, f64::min)
}
fn polygon_area(p: &[[f64; 2]]) -> f64 {
    p.iter()
        .enumerate()
        .map(|(i, a)| {
            let b = p[(i + 1) % p.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        * 0.5
}
// Decode the actual native command/Instance path, without modifying or flattening shadow.rs.
fn submitted_shadow_triangles(
    memory: &[u16],
    catalog: &serde_json::Value,
    command_words: u16,
) -> Vec<[[f64; 2]; 3]> {
    let sector = catalog["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "shadow-triangle")
        .unwrap()["record_sector"]
        .as_u64()
        .unwrap() as usize;
    let camera = &catalog["cameras"][0];
    let eye: [f64; 3] = std::array::from_fn(|i| camera["eye"][i].as_f64().unwrap());
    let target: [f64; 3] = std::array::from_fn(|i| camera["target"][i].as_f64().unwrap());
    let mut back: [f64; 3] = std::array::from_fn(|i| eye[i] - target[i]);
    let length = back.iter().map(|v| v * v).sum::<f64>().sqrt();
    for v in &mut back {
        *v /= length;
    }
    let mut right = [back[2], 0., -back[0]];
    let length = right.iter().map(|v| v * v).sum::<f64>().sqrt();
    for v in &mut right {
        *v /= length;
    }
    let up = [
        back[1] * right[2] - back[2] * right[1],
        back[2] * right[0] - back[0] * right[2],
        back[0] * right[1] - back[1] * right[0],
    ];
    let k = camera["focal"].as_f64().unwrap() / 256.;
    let mut local = vec![0u16; 2048];
    let mut records = [0usize; 2];
    let mut result = vec![];
    let commands = &memory[0x9800 * 64..0x9800 * 64 + usize::from(command_words)];
    for words in commands.as_chunks::<2>().0 {
        let c = u32::from(words[0]) | (u32::from(words[1]) << 16);
        match c >> 28 {
            1 => {
                let src = ((c >> 10) & 0x3ffff) as usize * 16;
                let dst = ((c >> 3) & 127) as usize * 16;
                let n = 16 << ((c >> 1) & 3);
                local[dst..dst + n].copy_from_slice(&memory[src..src + n]);
            }
            2 => records[(c & 1) as usize] = ((c >> 12) & 65535) as usize,
            3 if records[(c & 1) as usize] == sector => {
                let uniform = usize::from(memory[sector * 64 + 1] & 63);
                let at = uniform * 64;
                assert_eq!(&local[at + 44..at + 46], &[0x0d0a, 0x5712]);
                let p: [[f64; 3]; 3] = std::array::from_fn(|vertex| {
                    std::array::from_fn(|r| {
                        let translation = scene::fixed(&local, at + r * 8 + 6);
                        if vertex == 0 {
                            translation
                        } else {
                            translation
                                + scene::fixed(&local, at + r * 8 + if vertex == 1 { 0 } else { 4 })
                        }
                    })
                });
                result.push(std::array::from_fn(|v| {
                    let w: [f64; 3] = std::array::from_fn(|r| {
                        eye[r] + right[r] * p[v][0] / k - up[r] * p[v][1] / k - back[r] * p[v][2]
                    });
                    assert!(
                        (w[1] - 393. / 65536.).abs() < 0.004,
                        "shadow plane y={}",
                        w[1]
                    );
                    [w[0], w[2]]
                }));
            }
            _ => {}
        }
    }
    result
}

#[test]
fn two_word_previous_index_recurrence_matches_derived_index() {
    let mut results = vec![];
    for derived in [false, true] {
        let body = if derived {
            "let mut i:u16=0;while i<4 {let previous=if i==0 {3u16}else{i-1};signal(15,(previous<<8)|i);i+=1;}"
        } else {
            "let mut i:u16=0;let mut previous:u16=3;while i<4 {signal(15,(previous<<8)|i);previous=i;i+=1;}"
        };
        let (program, _, out) = application(
            if derived {
                "index-derived"
            } else {
                "index-carried"
            },
            Some(body),
        );
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let mut values = vec![];
        let mut halted = false;
        for _ in 0..100_000 {
            match cpu.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 15 => values.push(e.value),
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d);
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(halted, "bounded index recurrence timeout");
        std::fs::write(
            out.join("observed.json"),
            serde_json::to_vec_pretty(&values).unwrap(),
        )
        .unwrap();
        results.push(values);
    }
    assert_eq!(
        results[0],
        [0x300, 0x001, 0x102, 0x203],
        "carried previous index"
    );
    assert_eq!(
        results[1],
        [0x300, 0x001, 0x102, 0x203],
        "derived previous index"
    );
}

#[test]
fn native_triangle_instances_match_independent_web_shadow_clipping_samples() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../examples/gpu_renderer_bloxorz/assets/web-shadow-reference.json"
    ))
    .unwrap();
    let mut results = vec![];
    for case in fixture["cases"].as_array().unwrap() {
        let name = format!("web-shadow-{}", case["name"].as_str().unwrap());
        let (program, resources, out) = application(&name, None);
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut halted = false;
        let mut steps = 0;
        let mut push = 0;
        let mut command_words = 0;
        for _ in 0..2_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            steps += 1;
            match sim.step().unwrap() {
                StepOutcome::Halted { signal } => {
                    assert_eq!(signal, 0x600d);
                    halted = true;
                    break;
                }
                StepOutcome::Signaled(event) if event.signal_type == 14 => push = event.value,
                StepOutcome::Signaled(event) if event.signal_type == 15 => {
                    command_words = event.value
                }
                _ => {}
            }
        }
        assert!(halted, "bounded shadow sample timeout");
        assert!(sim.gpu().last_error().is_none());
        assert!(push < 32768);
        let memory = sim.cpu().physical_memory_slice();
        let info = global(&program, "SHADOW_INFO");
        let stats = memory[info..info + 3].to_vec();
        assert_eq!(
            usize::from(stats[1]),
            case["cells"].as_array().unwrap().len(),
            "{name} receivers"
        );

        let at = global(&program, "SHADOW_HULL");
        let actual = (0..usize::from(stats[0]))
            .map(|i| {
                [
                    scene::fixed(memory, at + i * 4),
                    scene::fixed(memory, at + i * 4 + 2),
                ]
            })
            .collect::<Vec<_>>();
        let expected = case["hull"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
            .collect::<Vec<_>>();
        let error = actual
            .iter()
            .map(|p| hull_distance(*p, &expected))
            .chain(expected.iter().map(|p| hull_distance(*p, &actual)))
            .fold(0., f64::max);
        assert!(error <= 0.002, "{name}: Q16 hull boundary error {error}");
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
        std::fs::write(
            out.join("submission-commands.bin"),
            memory[0x9800 * 64..0x9800 * 64 + usize::from(command_words)]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(
            out.join("submission-push.bin"),
            memory[0x9a00 * 64..0x9a00 * 64 + usize::from(push)]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let triangles = submitted_shadow_triangles(memory, &catalog, command_words);
        assert_eq!(
            triangles.len(),
            usize::from(stats[2]),
            "published shadow Instances"
        );
        let mut checked_cells = vec![];
        for cell in case["cells"].as_array().unwrap() {
            let x = cell["x"].as_f64().unwrap();
            let z = cell["z"].as_f64().unwrap();
            let expected = cell["polygon"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
                .collect::<Vec<_>>();
            let local = triangles
                .iter()
                .filter(|t| {
                    let cx = t.iter().map(|p| p[0]).sum::<f64>() / 3.;
                    let cz = t.iter().map(|p| p[1]).sum::<f64>() / 3.;
                    cx.floor() == x && cz.floor() == z
                })
                .collect::<Vec<_>>();
            let area = local
                .iter()
                .map(|t| polygon_area(t.as_slice()).abs())
                .sum::<f64>();
            let expected_area = polygon_area(&expected).abs();
            assert!(
                (area - expected_area).abs() < 0.002,
                "{name} cell ({x},{z}): submitted area {area} != {expected_area}"
            );
            for t in &local {
                for p in t.iter() {
                    assert!(
                        hull_distance(*p, &expected) < 0.003,
                        "{name}: emitted fan vertex outside reference boundary"
                    );
                }
            }
            for p in &expected {
                let distance = local
                    .iter()
                    .map(|t| hull_distance(*p, t.as_slice()))
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    distance < 0.003,
                    "{name}: missing reference cell boundary {p:?}, distance {distance}"
                );
            }
            checked_cells.push(serde_json::json!({"x":x,"z":z,"reference_area":expected_area,"published_area":area,"triangles":local}));
        }
        assert!(
            triangles.iter().all(|t| {
                let x = (t.iter().map(|p| p[0]).sum::<f64>() / 3.).floor();
                let z = (t.iter().map(|p| p[1]).sum::<f64>() / 3.).floor();
                case["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|cell| cell["x"].as_f64() == Some(x) && cell["z"].as_f64() == Some(z))
            }),
            "{name}: triangle outside all receiving cells"
        );
        std::fs::write(
            out.join("coverage.json"),
            serde_json::to_vec_pretty(&checked_cells).unwrap(),
        )
        .unwrap();
        let effective = case["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                let p = cell["polygon"].as_array().unwrap();
                (2..p.len())
                    .filter(|&i| {
                        let ax = p[i - 1][0].as_f64().unwrap() - p[0][0].as_f64().unwrap();
                        let az = p[i - 1][1].as_f64().unwrap() - p[0][1].as_f64().unwrap();
                        let bx = p[i][0].as_f64().unwrap() - p[0][0].as_f64().unwrap();
                        let bz = p[i][1].as_f64().unwrap() - p[0][1].as_f64().unwrap();
                        (ax * bz - az * bx).abs() >= 2. / 65536.
                    })
                    .count()
            })
            .sum::<usize>();
        // Coverage and published area above are the contract; Q16 may choose a different fan.
        assert!(
            triangles.len() <= 352,
            "bounded per-frame triangle capacity"
        );
        let pixels = sim.render_active_framebuffer();
        let mut ppm = b"P6\n400 240\n255\n".to_vec();
        for pixel in pixels {
            ppm.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
        std::fs::write(out.join("frame-display.ppm"), ppm).unwrap();
        results.push(serde_json::json!({"case":case["name"],"hull_points":stats[0],"receiver_cells":stats[1],"triangles":stats[2],"command_words_u16":command_words,"push_words_u16":push,"reference_effective_triangles_diagnostic":effective,"instruction_steps_including_whole_scene":steps,"max_hull_boundary_error_world_units":error,"pose":"independent Web matrix imported; shadow expression test, not game-animation qualification"}));
    }
    let out =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bloxorz-web/shadow-sample-v1");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("qualification.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
    eprintln!("{results:?}");
}

#[test]
fn shadow_geometry_cache_matches_fresh_instances_across_real_inputs() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    struct Frame {
        commands: Vec<u16>,
        push: Vec<u16>,
        colors: Vec<u16>,
        depth: Vec<u16>,
        pixels: Vec<u32>,
        info: Vec<u16>,
    }
    let mut runs = vec![];
    for name in ["web-cache-cached", "web-cache-forced"] {
        let (program, resources, _) = application(name, None);
        let info = global(&program, "SHADOW_INFO");
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut frames = vec![];
        let mut commands = vec![];
        let mut push = vec![];
        let mut push_words = 0usize;
        let mut halted = false;
        for _ in 0..6_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            match sim.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 14 => {
                    push_words = usize::from(e.value)
                }
                StepOutcome::Signaled(e) if e.signal_type == 15 => {
                    let m = sim.cpu().physical_memory_slice();
                    commands = m[0x9800 * 64..0x9800 * 64 + usize::from(e.value)].to_vec();
                    push = m[0x9a00 * 64..0x9a00 * 64 + push_words].to_vec();
                }
                StepOutcome::Signaled(e) if e.signal_type == 12 => {
                    let m = sim.cpu().physical_memory_slice();
                    frames.push(Frame {
                        commands: commands.clone(),
                        push: push.clone(),
                        colors: m[0x400000 / 2..0x480000 / 2].to_vec(),
                        depth: m[0x480000 / 2..0x4c0000 / 2].to_vec(),
                        pixels: sim.render_active_framebuffer(),
                        info: m[info..info + 3].to_vec(),
                    });
                }
                StepOutcome::Halted { signal } => {
                    let f = global(&program, "GPU_API_FAULT");
                    assert_eq!(
                        signal,
                        0x600d,
                        "{name}: frame{} fault{:?}",
                        frames.len(),
                        &sim.cpu().physical_memory_slice()[f..f + 6]
                    );
                    halted = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(halted, "bounded cache invalidation scene timeout");
        assert!(sim.gpu().last_error().is_none());
        assert_eq!(frames.len(), 9);
        runs.push(frames);
    }
    for (i, (a, b)) in runs[0].iter().zip(&runs[1]).enumerate() {
        assert!(
            a.commands == b.commands,
            "frame{i}: command stream differs from fresh geometry"
        );
        assert!(
            a.push == b.push,
            "frame{i}: native Instances differ from fresh geometry"
        );
        assert!(
            a.colors == b.colors && a.depth == b.depth && a.pixels == b.pixels,
            "frame{i}: full physical surfaces/Display differ"
        );
        assert_eq!(a.info, b.info, "frame{i}: shadow footprint differs");
    }
    assert_ne!(
        runs[0][2].info[2], runs[0][3].info[2],
        "bridge-mask case must change actual receivers"
    );
    assert_ne!(
        runs[0][4].info[2], runs[0][5].info[2],
        "goal hatch changes receivers at an unchanged world pose"
    );
    assert_eq!(
        runs[0][7].info[2], 0,
        "hidden player cannot replay a shadow"
    );
    assert!(
        runs[0][8].info[2] > 0,
        "visible player after hidden state must rebuild/replay correctly"
    );
    let summary=runs[0].iter().enumerate().map(|(i,f)|serde_json::json!({"frame":i,"shadow":f.info,"command_u16":f.commands.len(),"push_u16":f.push.len(),"equal_to_forced_fresh":true})).collect::<Vec<_>>();
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/bloxorz-web/shadow-cache-inputs.json");
    std::fs::write(out, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
}
