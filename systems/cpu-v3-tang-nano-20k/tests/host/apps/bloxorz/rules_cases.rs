use super::*;

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
        "../../../../examples/gpu_renderer_bloxorz/assets/web-shadow-reference.json"
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
    let body = r#"session_init();SESSION[0u16]=PLAY;GAME[0u16]=3;GAME[1u16]=3;GAME[2u16]=0;GAME[3u16]=256;GAME[6u16]=5;GAME[7u16]=3;GAME[8u16]=1;GAME[9u16]=1;
check(session_move(0));session_tick(32768,0,0);
check(GAME[0u16]==3&&GAME[1u16]==3&&GAME[2u16]==0&&GAME[3u16]==256&&GAME[4u16]==0&&GAME[5u16]==0&&GAME[6u16]==5&&GAME[7u16]==3&&GAME[8u16]==1&&GAME[9u16]==1);"#;
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
    let mut body = String::from("session_init();");
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
        body.push_str(&format!("level_reset({});check(LEVEL[0u16]=={}&&LEVEL[1u16]=={}&&LEVEL[2u16]=={}&&LEVEL[3u16]=={}&&GAME[3u16]=={});",number-1,header[0],header[1],header[2],header[3],header[5]));
        levels += 1;
    }
    assert_eq!(levels, 33);
    body.push_str(r#"
signal(6,1);
// Actual stage8 splitter at(4,4): legal lying-X prepose(2,4), then+X.
level_reset(7);SESSION[0u16]=PLAY;SESSION[1u16]=0;TRANSITION[0u16]=0;SESSION[3u16]=0;
GAME[0u16]=2;GAME[1u16]=4;GAME[2u16]=1;check(session_move(0));
check(GAME[0u16]==10&&GAME[1u16]==1&&GAME[6u16]==10&&GAME[7u16]==7&&GAME[9u16]==1&&GAME[8u16]==0&&GAME[5u16]==1);
check(!split_visual());let mut n:u16=0;while n<5 {session_tick(0,0,50);n+=1;}
check(SESSION[0u16]==SPLIT_POP&&SESSION[1u16]==0&&MOTION[15u16]==4&&MOTION[16u16]==4);
session_tick(0,0,35);let squash=block_transform();check(squash.position.x==fix32::from_int(-3)&&squash.position.z==fix32::zero());check(squash.position.y>fix32::from_words(32760,0)&&squash.position.y<fix32::from_words(32776,0));
session_tick(0,0,35);check(split_visual());n=0;while n<5 {session_tick(0,0,50);n+=1;}
check(SESSION[0u16]==PLAY);signal(6,2);
check(session_move(6));check(GAME[8u16]==1&&GAME[5u16]==2&&SESSION[0u16]==PLAY);
session_tick(32768,0,0);check(GAME[8u16]==0&&GAME[9u16]==1&&GAME[5u16]==1);
SESSION[3u16]=0;check(session_move(2));session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,10);
let rolling=block_transform();let idle=idle_cube_transform();
check(rolling.position.x==fix32::from_int(3)&&rolling.position.z>fix32::from_words(32760,65533)&&rolling.position.z<fix32::from_words(32776,65533));
check(rolling.position.y>fix32::from_words(46330,0)&&rolling.position.y<fix32::from_words(46352,0));
check(rolling.scale.y==fix32::from_int(1)&&idle.position.z==fix32::from_int(3)&&idle.position.y==fix32::from_words(32768,0));
session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,10);check(SESSION[0u16]==PLAY&&GAME[1u16]==2);signal(6,3);
session_tick(16384,0,0);check(GAME[8u16]==1&&GAME[5u16]==3);
n=0;while n<4 {check(session_move(3));let mut k:u16=0;while k<5 {session_tick(0,0,50);k+=1;}n+=1;}
check(GAME[9u16]==0&&GAME[2u16]==2&&GAME[0u16]==10&&GAME[1u16]==2&&GAME[5u16]==7&&!split_visual());
session_tick(32768,0,0);check(GAME[9u16]==1&&GAME[8u16]==1&&GAME[1u16]==2&&GAME[7u16]==4&&GAME[5u16]==6&&split_visual());signal(6,4);
// Stage4: two lying cells(3,0)/(4,0) stand on fragile(5,0).
level_reset(3);SESSION[0u16]=PLAY;SESSION[1u16]=0;TRANSITION[0u16]=0;SESSION[3u16]=0;
GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;check(session_move(0));check(GAME[4u16]==2&&GAME[0u16]==5&&GAME[5u16]==1);
n=0;while n<5 {session_tick(0,0,50);n+=1;}
check(SESSION[0u16]==CRACK&&FRACTURE[0u16]==5&&fragile_dropped(5));
session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,40);check(SESSION[0u16]==FALL&&SESSION[1u16]==0&&fragile_dropped(5));
session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,50);let falling=block_transform();
check(falling.position.y>-fix32::from_words(16,0)&&falling.position.y<fix32::from_words(16,0));
check(falling.angles.x==fix32::zero()&&falling.angles.z==fix32::zero());
check(fragile_height(5)>-fix32::from_words(16,1)&&fragile_height(5)<-fix32::from_words(65520,0));
session_tick(32768,0,0);check(FRACTURE[0u16]==65535&&GAME[4u16]==0&&GAME[5u16]==0);signal(6,5);
// Actual previous-level button wraps1->33 only after opaque retirement.
level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;TRANSITION[0u16]=0;
session_tick(1024,0,0);check(SESSION[2u16]==32&&MOTION[6u16]==0);n=0;while n<7 {session_tick(0,0,50);n+=1;}
check(MOTION[6u16]==0);session_presented();check(MOTION[6u16]==32);
TRANSITION[0u16]=0;SESSION[0u16]=GOAL_LAND;SESSION[1u16]=79;
session_tick(0,0,1);check(SESSION[0u16]==GOAL_CLOSE&&!fading());n=0;while n<4 {session_tick(0,0,50);n+=1;}check(SESSION[2u16]==0&&fading());signal(6,6);
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
