use super::*;

#[test]
fn first_two_stage_solutions_undo_and_overlapping_fade_follow_real_rules() {
    let body = r#"
GAME_SESSION.begin();let mut n:u16=0;while n<15 {GAME_SESSION.tick(0,0,40);n+=1;}
check(GAME_SESSION.animation.phase==AnimationPhase::Play && GAME_SESSION.rules.moves==0);signal(6,1);
let first:Buf<u16,7>=Buf::new([0,0,2,0,0,0,2]);n=0;
while n<7 {
    let old_mask=GAME_SESSION.rules.bridge_mask;check(GAME_SESSION.try_move(first[n]));check(GAME_SESSION.visual_mask()==old_mask);
    let mut k:u16=0;while k<5 {GAME_SESSION.tick(0,0,50);k+=1;}n+=1;
}
check(GAME_SESSION.rules.status==1 && GAME_SESSION.rules.moves==7 && GAME_SESSION.animation.phase==AnimationPhase::GoalOpen);signal(6,2);
GAME_SESSION.tick(32768,0,0);check(GAME_SESSION.rules.status==0 && GAME_SESSION.rules.moves==6 && GAME_SESSION.animation.phase==AnimationPhase::Play);
check(GAME_SESSION.try_move(2));n=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}
n=0;while !GAME_SESSION.fading() && n<32 {GAME_SESSION.tick(0,0,40);n+=1;}
check(GAME_SESSION.fading() && GAME_SESSION.animation.phase==AnimationPhase::GoalClose && GAME_SESSION.motion.level==0);signal(6,3);
GAME_SESSION.presented();check(GAME_SESSION.motion.level==0);
n=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.animation.elapsed>=200 && !GAME_SESSION.block_visible() && GAME_SESSION.transition.phase==TransitionPhase::FadeOut);
n=0;while GAME_SESSION.transition.elapsed<320 && n<10 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.motion.level==0);GAME_SESSION.presented();check(GAME_SESSION.motion.level==1 && GAME_SESSION.transition.phase==TransitionPhase::Hold && GAME_SESSION.rules.moves==0);signal(6,4);
n=0;while n<16 {GAME_SESSION.tick(8|4096|32,32,50);n+=1;}
check(GAME_SESSION.transition.phase==TransitionPhase::FadeIn && GAME_SESSION.transition.elapsed==380 && GAME_SESSION.rules.moves==0);
GAME_SESSION.presented();check(!GAME_SESSION.fading() && GAME_SESSION.animation.phase==AnimationPhase::EntryOpen);
n=0;while n<15 {GAME_SESSION.tick(0,0,40);n+=1;}
check(GAME_SESSION.animation.phase==AnimationPhase::Play);signal(6,5);
let second:Buf<u16,17>=Buf::new([3,0,2,0,0,0,0,3,3,2,0,2,0,0,3,0,3]);n=0;
while n<17 {
    let old_mask=GAME_SESSION.rules.bridge_mask;check(GAME_SESSION.try_move(second[n]));check(GAME_SESSION.visual_mask()==old_mask);
    let mut k:u16=0;while k<5 {GAME_SESSION.tick(0,0,50);k+=1;}n+=1;
}
check(GAME_SESSION.rules.status==1 && GAME_SESSION.rules.moves==17);signal(6,6);
// Full unsupported release leaves before220ms, then continues during the veil.
GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;
check(GAME_SESSION.try_move(0));n=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.try_move(0));n=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.try_move(3));GAME_SESSION.tick(0,0,40);GAME_SESSION.tick(0,0,40);
check(GAME_SESSION.animation.phase==AnimationPhase::Fall && GAME_SESSION.motion.elapsed==80 && GAME_SESSION.animation.elapsed==0);
n=0;while n<12 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.fading() && GAME_SESSION.animation.elapsed==600);GAME_SESSION.tick(0,0,50);check(GAME_SESSION.animation.elapsed==650);signal(6,7);
// Held movement repeats after settle; newest held direction wins.
GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;
GAME_SESSION.tick(32,32,0);check(GAME_SESSION.rules.moves==1);n=0;while n<5 {GAME_SESSION.tick(0,32,50);n+=1;}
check(GAME_SESSION.rules.moves==2 && GAME_SESSION.animation.phase==AnimationPhase::Roll);
GAME_SESSION.tick(64,32|64,0);n=0;while n<5 {GAME_SESSION.tick(0,32|64,50);n+=1;}
check(GAME_SESSION.motion.direction==2 && GAME_SESSION.rules.moves==3);signal(6,8);
// Retain256 complete snapshots across a ring wrap, including a16-bit move count.
GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;
n=0;while n<260 {check(GAME_SESSION.try_move(if n&1==0 {0u16}else{1u16}));let mut k:u16=0;while k<5 {GAME_SESSION.tick(0,0,50);k+=1;}n+=1;}
check(GAME_SESSION.rules.moves==260);n=0;while n<256 {GAME_SESSION.tick(32768,0,0);n+=1;}
check(GAME_SESSION.rules.moves==4 && GAME_SESSION.rules.x==GAME_SESSION.level.start_x && GAME_SESSION.rules.z==GAME_SESSION.level.start_z);
GAME_SESSION.tick(32768,0,0);check(GAME_SESSION.rules.moves==4);signal(6,9);
// Re-pressing one held direction cannot evict another still-held direction.
GAME_SESSION.animation.phase=AnimationPhase::EntryOpen;GAME_SESSION.animation.elapsed=0;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;
GAME_SESSION.tick(128,128,0);GAME_SESSION.tick(32,128|32,0);n=0;
while n<6 {GAME_SESSION.tick(32,128|32,0);n+=1;}
GAME_SESSION.tick(0,128,0);check((GAME_SESSION.animation.direction_stack&15)==2);signal(6,10);
// Web switch press duration is260ms, independently of the220ms roll.
GAME_SESSION.reset_level(1);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.rules.bridge_mask=1;
GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,30);
let half=GAME_SESSION.switch_fraction(0);check(half>fix32::from_words(32760,0) && half<fix32::from_words(32776,0));
GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,30);
check(GAME_SESSION.switch_fraction(0)==fix32::from_int(1));signal(6,11);
// Accepted moves stop at65535; undo reconstructs the previous full16-bit count.
GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.rules.moves=65534;
check(GAME_SESSION.try_move(0));check(GAME_SESSION.rules.moves==65535);n=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}check(GAME_SESSION.animation.phase==AnimationPhase::Play);check(!GAME_SESSION.try_move(0));
GAME_SESSION.tick(32768,0,0);check(GAME_SESSION.rules.moves==65534 && GAME_SESSION.rules.x==GAME_SESSION.level.start_x && GAME_SESSION.rules.z==GAME_SESSION.level.start_z);signal(6,12);


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
fn split_swap_bridge_failure_level15_falls_undoes_and_resets() {
    check_swap_bridge_failure(
        14,
        &[
            0, 0, 0, 0, 3, 3, 1, 6, 3, 3, 3, 3, 3, 0, 0, 0, 3, 3, 0, 0, 0, 0, 2, 2, 6, 1, 6,
        ],
    );
}

#[test]
fn split_swap_bridge_failure_level20_falls_undoes_and_resets() {
    check_swap_bridge_failure(
        19,
        &[
            2, 1, 2, 0, 2, 1, 3, 3, 0, 3, 3, 1, 1, 1, 1, 2, 2, 0, 3, 1, 1, 2, 2, 2, 1, 2, 3, 0, 3,
            3, 0, 3, 3, 0, 0, 0, 2, 1, 2, 1, 1, 1, 1, 2, 2, 2, 2, 2, 6, 1, 3, 6, 0, 6,
        ],
    );
}

#[test]
fn actual_session_poses_match_independent_web_entry_tip_and_release_matrices() {
    let body = r#"
let mut gpu=Gpu::init(board_gpu_config());let mut model=game_plain_model();
GAME_SESSION.begin();let mut n:u16=0;while n<15 {GAME_SESSION.tick(0,0,40);n+=1;}
GAME_SESSION.block_pose(&mut model);world_probe(&model);
GAME_SESSION.begin();n=0;while n<4 {GAME_SESSION.tick(0,0,50);n+=1;}n=0;while n<3 {GAME_SESSION.tick(0,0,50);n+=1;}GAME_SESSION.tick(0,0,40);
GAME_SESSION.block_pose(&mut model);world_probe(&model);
GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;check(GAME_SESSION.try_move(1));
n=0;while n<4 {GAME_SESSION.tick(0,0,50);n+=1;}GAME_SESSION.tick(0,0,20);check(GAME_SESSION.animation.phase==AnimationPhase::Tip);
n=0;while n<3 {GAME_SESSION.tick(0,0,50);n+=1;}GAME_SESSION.tick(0,0,30);
GAME_SESSION.block_pose(&mut model);world_probe(&model);
GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;
check(GAME_SESSION.try_move(0));n=0;while n<4 {GAME_SESSION.tick(0,0,50);n+=1;}GAME_SESSION.tick(0,0,20);
check(GAME_SESSION.try_move(0));n=0;while n<4 {GAME_SESSION.tick(0,0,50);n+=1;}GAME_SESSION.tick(0,0,20);
check(GAME_SESSION.try_move(3));GAME_SESSION.tick(0,0,40);GAME_SESSION.tick(0,0,40);n=0;while n<3 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.animation.phase==AnimationPhase::Fall && GAME_SESSION.animation.elapsed==150);GAME_SESSION.block_pose(&mut model);world_probe(&model);
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
        "../../../../examples/game_bloxorz/assets/web-shadow-reference.json"
    ))
    .unwrap();
    let mut errors = vec![];
    for (index, (words, case)) in matrices
        .iter()
        .zip(reference["cases"].as_array().unwrap())
        .enumerate()
    {
        let mut error = 0f64;
        for r in 0..3 {
            for c in 0..4 {
                let actual = scene::fixed(words, (r * 4 + c) * 2);
                // The Web oracle uses canonical occupancy axes. Preserve its physical
                // transform while independently restoring the traveled local axes.
                let (column, sign) = if c == 3 {
                    (3, 1.)
                } else {
                    match index {
                        2 => (
                            if c == 0 {
                                1
                            } else {
                                if c == 1 {
                                    0
                                } else {
                                    2
                                }
                            },
                            if c == 1 { -1. } else { 1. },
                        ),
                        3 => (c, if c < 2 { -1. } else { 1. }),
                        _ => (c, 1.),
                    }
                };
                let expected = sign * case["world"][column * 4 + r].as_f64().unwrap();
                error = error.max((actual - expected).abs());
            }
        }
        assert!(
            error < 0.003,
            "{} actual session world error {error}",
            case["name"]
        );
        let determinant = world_determinant(words);
        assert!(
            determinant > 0.,
            "{} reverses the closed-mesh winding",
            case["name"]
        );
        errors.push(serde_json::json!({"case":case["name"],"max_world_coefficient_error":error,"actual_u16":words,"world_determinant":determinant}));
    }
    std::fs::write(
        out.join("actual-session-world.json"),
        serde_json::to_vec_pretty(&errors).unwrap(),
    )
    .unwrap();
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
fn compact_undo_restores_split_active_cube_and_ninth_bridge_bit() {
    let body = r#"GAME_SESSION.begin();GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.rules.x=3;GAME_SESSION.rules.z=3;GAME_SESSION.rules.orientation=0;GAME_SESSION.rules.bridge_mask=256;GAME_SESSION.rules.other_x=5;GAME_SESSION.rules.other_z=3;GAME_SESSION.rules.active_cube=1;GAME_SESSION.rules.split=1;
check(GAME_SESSION.try_move(0));GAME_SESSION.tick(32768,0,0);
check(GAME_SESSION.rules.x==3&&GAME_SESSION.rules.z==3&&GAME_SESSION.rules.orientation==0&&GAME_SESSION.rules.bridge_mask==256&&GAME_SESSION.rules.status==0&&GAME_SESSION.rules.moves==0&&GAME_SESSION.rules.other_x==5&&GAME_SESSION.rules.other_z==3&&GAME_SESSION.rules.active_cube==1&&GAME_SESSION.rules.split==1);"#;
    let (program, resources, _) = application("compact-undo-split-bit8", Some(body));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    assert!(matches!(
        cpu.run(1_000_000).unwrap(),
        cpu_v3::RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn all_stage_loading_split_swap_merge_fragile_and_navigation_follow_reference_rules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let reference =
        std::fs::read_to_string(root.join("applications/bloxorz/assets/reference.txt")).unwrap();
    let mut body = String::from("GAME_SESSION.begin();");
    let mut levels = 0;
    for line in reference.lines().filter(|l| l.starts_with("level b")) {
        let words = line.split_whitespace().collect::<Vec<_>>();
        let number = words[1]
            .strip_prefix('b')
            .unwrap()
            .parse::<usize>()
            .unwrap();
        if number > 33 {
            continue;
        }
        let header = words[3..11]
            .iter()
            .map(|v| v.parse::<u16>().unwrap())
            .collect::<Vec<_>>();
        body.push_str(&format!("GAME_SESSION.reset_level({});check(GAME_SESSION.level.width=={}&&GAME_SESSION.level.height=={}&&GAME_SESSION.level.start_x=={}&&GAME_SESSION.level.start_z=={}&&GAME_SESSION.rules.bridge_mask=={});",number-1,header[0],header[1],header[2],header[3],header[5]));
        levels += 1;
    }
    assert_eq!(levels, 33);
    body.push_str(r#"
signal(6,1);
// Actual stage8 splitter at(4,4): legal lying-X prepose(2,4), then+X.
GAME_SESSION.reset_level(7);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.animation.held_lock=0;
GAME_SESSION.rules.x=2;GAME_SESSION.rules.z=4;GAME_SESSION.rules.orientation=1;check(GAME_SESSION.try_move(0));
check(GAME_SESSION.rules.x==10&&GAME_SESSION.rules.z==1&&GAME_SESSION.rules.other_x==10&&GAME_SESSION.rules.other_z==7&&GAME_SESSION.rules.split==1&&GAME_SESSION.rules.active_cube==0&&GAME_SESSION.rules.moves==1);
check(!GAME_SESSION.split_visual());let mut n:u16=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.animation.phase==AnimationPhase::SplitPop&&GAME_SESSION.animation.elapsed==0&&GAME_SESSION.motion.landing_x==4&&GAME_SESSION.motion.landing_z==4);
GAME_SESSION.tick(0,0,35);let squash=GAME_SESSION.block_transform();check(squash.position.x==fix32::from_int(-3)&&squash.position.z==fix32::zero());check(squash.position.y>fix32::from_words(32760,0)&&squash.position.y<fix32::from_words(32776,0));
GAME_SESSION.tick(0,0,35);check(GAME_SESSION.split_visual());n=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.animation.phase==AnimationPhase::Play);signal(6,2);
check(GAME_SESSION.try_move(6));check(GAME_SESSION.rules.active_cube==1&&GAME_SESSION.rules.moves==2&&GAME_SESSION.animation.phase==AnimationPhase::Play);
GAME_SESSION.tick(32768,0,0);check(GAME_SESSION.rules.active_cube==0&&GAME_SESSION.rules.split==1&&GAME_SESSION.rules.moves==1);
GAME_SESSION.animation.held_lock=0;check(GAME_SESSION.try_move(2));GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,10);
let rolling=GAME_SESSION.block_transform();let idle=GAME_SESSION.idle_cube_transform();
check(rolling.position.x==fix32::from_int(3)&&rolling.position.z>fix32::from_words(32760,65533)&&rolling.position.z<fix32::from_words(32776,65533));
check(rolling.position.y>fix32::from_words(46330,0)&&rolling.position.y<fix32::from_words(46352,0));
check(rolling.scale.y==fix32::from_int(1)&&idle.position.z==fix32::from_int(3)&&idle.position.y==fix32::from_words(32768,0));
GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,10);check(GAME_SESSION.animation.phase==AnimationPhase::Play&&GAME_SESSION.rules.z==2);signal(6,3);
GAME_SESSION.tick(16384,0,0);check(GAME_SESSION.rules.active_cube==1&&GAME_SESSION.rules.moves==3);
n=0;while n<4 {check(GAME_SESSION.try_move(3));let mut k:u16=0;while k<5 {GAME_SESSION.tick(0,0,50);k+=1;}n+=1;}
check(GAME_SESSION.rules.split==0&&GAME_SESSION.rules.orientation==2&&GAME_SESSION.rules.x==10&&GAME_SESSION.rules.z==2&&GAME_SESSION.rules.moves==7&&!GAME_SESSION.split_visual());
GAME_SESSION.tick(32768,0,0);check(GAME_SESSION.rules.split==1&&GAME_SESSION.rules.active_cube==1&&GAME_SESSION.rules.z==2&&GAME_SESSION.rules.other_z==4&&GAME_SESSION.rules.moves==6&&GAME_SESSION.split_visual());signal(6,4);
// Stage4: two lying cells(3,0)/(4,0) stand on fragile(5,0).
GAME_SESSION.reset_level(3);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.animation.held_lock=0;
GAME_SESSION.rules.x=3;GAME_SESSION.rules.z=0;GAME_SESSION.rules.orientation=1;check(GAME_SESSION.try_move(0));check(GAME_SESSION.rules.status==2&&GAME_SESSION.rules.x==5&&GAME_SESSION.rules.moves==1);
n=0;while n<5 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.animation.phase==AnimationPhase::Crack&&GAME_SESSION.fracture_cell==5&&GAME_SESSION.fragile_dropped(5));
GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,40);check(GAME_SESSION.animation.phase==AnimationPhase::Fall&&GAME_SESSION.animation.elapsed==0&&GAME_SESSION.fragile_dropped(5));
GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);let falling=GAME_SESSION.block_transform();
check(falling.position.y>-fix32::from_words(16,0)&&falling.position.y<fix32::from_words(16,0));
check(falling.angles.x==fix32::zero()&&falling.angles.z==fix32::zero());
check(GAME_SESSION.fragile_height(5)>-fix32::from_words(16,1)&&GAME_SESSION.fragile_height(5)<-fix32::from_words(65520,0));
GAME_SESSION.tick(32768,0,0);check(GAME_SESSION.fracture_cell==65535&&GAME_SESSION.rules.status==0&&GAME_SESSION.rules.moves==0);signal(6,5);
// Actual previous-level button wraps1->33 only after opaque retirement.
GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;
GAME_SESSION.tick(1024,0,0);check(GAME_SESSION.animation.destination==32&&GAME_SESSION.motion.level==0);n=0;while n<7 {GAME_SESSION.tick(0,0,50);n+=1;}
check(GAME_SESSION.motion.level==0);GAME_SESSION.presented();check(GAME_SESSION.motion.level==32);
GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.animation.phase=AnimationPhase::GoalLand;GAME_SESSION.animation.elapsed=79;
GAME_SESSION.tick(0,0,1);check(GAME_SESSION.animation.phase==AnimationPhase::GoalClose&&!GAME_SESSION.fading());n=0;while n<4 {GAME_SESSION.tick(0,0,50);n+=1;}check(GAME_SESSION.animation.destination==0&&GAME_SESSION.fading());signal(6,6);
"#);
    let (program, resources, _) = application("all-stage-session", Some(&body));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    let mut marker = 0;
    let mut halted = false;
    for _ in 0..8_000_000 {
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 6 => marker = e.value,
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d, "stage marker{marker}");
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded all-stage session timeout");
    assert_eq!(marker, 6);
}
