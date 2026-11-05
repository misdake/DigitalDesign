//! Bounded real-stage rules, native Instances, RTT and actual Display tests.
use super::changes::{global, preload};
use super::*;
use cpu_v3::StepOutcome;
use std::path::PathBuf;
fn application(
    name: &str,
    probe: Option<&str>,
) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>, PathBuf) {
    application_from_sources(name, probe, None)
}
fn application_from_sources(
    name: &str,
    probe: Option<&str>,
    frozen_sources: Option<&Path>,
) -> (rcc_backend::CpuV3Program, Vec<(usize, Vec<u8>)>, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = frozen_sources.unwrap_or(root);
    let source_example = sources.join("examples/gpu_renderer_bloxorz");
    let example = if name.starts_with("fragile-accepted-") {
        source_example.clone()
    } else {
        root.join("examples/gpu_renderer_bloxorz")
    };
    let out = std::env::var_os("BLOXORZ_SIM_OUTPUT_ROOT")
        .map_or_else(|| root.join("../../target/bloxorz-rtt"), PathBuf::from)
        .join(name);
    let mut producer = std::process::Command::new("python");
    producer
        .arg("-B")
        .arg(example.join("tools/assets.py"))
        .arg("--output")
        .arg(&out);
    if let Some(sizes) = name
        .strip_prefix("fragile-atlas-candidate-")
        .or_else(|| name.strip_prefix("fragile-pad-candidate-"))
    {
        let (main, marker) = sizes.split('-').next().unwrap().split_once('x').unwrap();
        producer.args(["--main-atlas-log2", main, "--marker-atlas-log2", marker]);
    }
    assert!(producer.status().unwrap().success());
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
    assert_eq!(catalog["profile"], "production");
    let ui = std::fs::read(out.join("atlas.bin")).unwrap();
    for glyph in b"LEVEL DRAW MS".iter().copied().filter(|g| *g != b' ') {
        assert!(
            ui[usize::from(glyph) * 256..(usize::from(glyph) + 1) * 256]
                .iter()
                .any(|v| *v != 0),
            "HUD glyph{} is invisible",
            char::from(glyph)
        );
    }

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
    let mut source = std::fs::read_to_string(source_example.join("rcc/main.rs"))
        .unwrap()
        .replace("\r\n", "\n");
    if name.starts_with("button-parts-") {
        source = source
            .replace(
                "session_init();",
                "session_init();level_reset(28);SESSION[0u16]=PLAY;SESSION[1u16]=0;HUD[0u16]=0;",
            )
            .replace(
                "session_tick(buttons, held, elapsed);",
                "button_test_pose(number);",
            )
            .replace(
                "pass.begin_prepared_part(part);",
                "if part>0 && part<14 {signal(14,part);}pass.begin_prepared_part(part);",
            );
        if name.ends_with("fresh") {
            source = source.replace(
                "sync_bridge_group(&mut commands);",
                "sync_bridge_group(&mut commands);commands.clear_prepared_group();",
            );
        }
        source += r#"
fn button_test_pose(n:u16){
 if n==9 || n==14 {level_reset(32);SESSION[0u16]=PLAY;SESSION[1u16]=0;GAME[3u16]=0;}
 if n==0 {GAME[3u16]=0;}
 let mut i:u16=0;let mut bit:u16=0;
 while i<LEVEL[6u16] && bit==0 {let k=level_field(i,0);if k==3 || k==4 {bit=level_field(i,1);}i+=1;}
 if n==2 || n==11 {GAME[3u16]=bit;}
 if n==16 {
  let mut cell:u16=0;while level_field(cell,0)!=3 {cell+=1;}
  LEVEL[9+cell*2]=(LEVEL[9+cell*2]&4095)|32768;
  LEVEL_GENERATION[0u16]+=1;GAME[3u16]=256;
 }
 if n==7 {GAME[3u16]=0;}
 session_tick(0,0,if n==3 || n==12 || n==17 {50u16}else{if n==5 {210u16}else{0u16}});
}
"#;
    }
    if name == "cpu-shadow-stage29" {
        source = source
            .replace(
                "session_init();",
                "session_init();level_reset(28);SESSION[0u16]=PLAY;SESSION[1u16]=0;HUD[0u16]=0;",
            )
            .replace(
                "session_tick(buttons, held, elapsed);",
                "session_tick(buttons, held, if number==2 {17u16}else{0u16});",
            );
        std::fs::write(out.join("compiled-main.rs"), &source).unwrap();
    }
    if name == "moving-cache-stage29" {
        source = source.replace(
            "session_init();",
            "session_init();level_reset(28);SESSION[0u16]=PLAY;SESSION[1u16]=0;HUD[0u16]=0;",
        );
        // Physical Device5 input and the real millisecond clock drive the roll.
        std::fs::write(out.join("compiled-main.rs"), &source).unwrap();
    }
    if name.starts_with("static-cache-") {
        let level = std::env::var("BLOXORZ_CACHE_LEVEL")
            .unwrap_or_else(|_| "29".into())
            .parse::<u16>()
            .unwrap();
        assert!((1..=33).contains(&level));
        source = source.replace(
            "session_init();",
            &format!(
                "session_init();level_reset({});SESSION[0u16]=PLAY;SESSION[1u16]=0;HUD[0u16]=0;",
                level - 1
            ),
        );
        source = source.replace(
            "session_tick(buttons, held, elapsed);",
            "session_tick(buttons, held, 0);",
        );
        std::fs::write(out.join("compiled-main.rs"), &source).unwrap();
    }
    if name.starts_with("fragile-accepted-pad-") || name.starts_with("fragile-pad-candidate-") {
        source = source
            .replace("session_init();", "session_init();HUD[0u16]=0;")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .replace(
                "session_tick(buttons, held, elapsed);",
                "pad_surface_pose(number);",
            );
        source += r#"
fn pad_surface_ticks(total:u16){let mut remain=total;while remain!=0 {let dt=if remain>50 {50u16}else{remain};session_tick(0,0,dt);remain-=dt;}}
fn pad_surface_pose(which:u16){
 let level:u16=if which<5 {0u16}else{if which<9 {1u16}else{if which==9 {14u16}else{if which==10 {16u16}else{if which==11 {3u16}else{7u16}}}}};
 level_reset(level);SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;HUD[0u16]=0;
 if which==0 {session_move(0);pad_surface_ticks(220);}
 else if which<5 {SESSION[0u16]=ENTRY_OPEN;SESSION[1u16]=0;if which==2 {pad_surface_ticks(100);}else if which==3 {pad_surface_ticks(390);}else if which==4 {pad_surface_ticks(580);}}
 else if which>=6 && which<=8 {GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=2;session_move(3);pad_surface_ticks(if which==6 {320u16}else{if which==7 {630u16}else{1020u16}});}
}
"#;
    }
    if name.contains("-atlas-") {
        source = source.replace("session_init();", "session_init();HUD[0u16]=0;")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .replace("session_tick(buttons, held, elapsed);", "let stage:u16=if number<2 || number==6 {3u16}else{if number==2 {29u16}else{if number==3 {14u16}else{if number==4 {16u16}else{7u16}}}};if MOTION[6u16]!=stage {level_reset(stage);}SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;if number==6 {GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;session_move(0);session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,10);}else{session_tick(0,0,0);}");
    }
    if name == "fragile-flow-restart-next" {
        source = source.replace("session_init();", "session_init();level_reset(3);SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;TRANSITION[0u16]=0;")
            .replace("let fragile_owner = fragile_begin();", "let fragile_owner = fragile_begin();signal(14,fragile_owner.addr());")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .replace("session_tick(buttons, held, elapsed);", "if number==1 {GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;session_move(0);let mut ticks:u16=0;while ticks<5 {session_tick(0,0,50);ticks+=1;}}else if number==2 {session_tick(8,0,0);TRANSITION[1u16]=320;}else if number==3 || number==6 {TRANSITION[1u16]=80;session_tick(0,0,0);}else if number==4 || number==7 {TRANSITION[1u16]=380;session_tick(0,0,0);}else if number==5 {session_tick(4096,0,0);TRANSITION[1u16]=320;}else {session_tick(buttons,held,elapsed);}");
    }
    if name.starts_with("fragile-") && name.contains("-representative-") {
        source = source.replace("session_init();", "session_init();HUD[0u16]=0;")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .replace("session_tick(buttons, held, elapsed);", "let stage:u16=if number==0 {7u16}else{if number==1 {14u16}else{if number==2 {16u16}else{if number==3 {29u16}else{0u16}}}};if MOTION[6u16]!=stage {level_reset(stage);}SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;session_tick(0,0,0);");
    }
    if name.starts_with("fragile-") && name.contains("-play-") {
        source = source
            .replace("session_init();", "session_init();level_reset(3);SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;HUD[0u16]=0;")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .replace("session_tick(buttons, held, elapsed);", "if number==3 {GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;session_move(0);let mut ticks:u16=0;while ticks<5 {session_tick(0,0,50);ticks+=1;}}else if number==4 {session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,40);}else if number==5 {session_tick(32768,0,0);}else {session_tick(buttons,held,elapsed);}");
    }
    // This independent Web oracle decodes the exact unit-triangle control.
    // Receiver-cell packing is qualified separately by bloxorz_shadow_batch.
    if name.starts_with("web-shadow-") {
        source = source
            .replace("    let shadow_owner = shadow_batch_begin();\n", "")
            .replace("        shadow_batch_frame();\n", "")
            .replace("    shadow_batch_end(shadow_owner);\n", "");
    }
    if name.starts_with("frame-schedule-") {
        let level = if name.contains("split-lifetime") {
            7
        } else if name.contains("fallback15") {
            14
        } else if name.contains("fallback17") {
            16
        } else if name.contains("goal") {
            1
        } else {
            0
        };
        let initial = if name.contains("entry") {
            "ENTRY_OPEN"
        } else {
            "PLAY"
        };
        source = source.replace("session_init();", &format!("session_init();level_reset({level});SESSION[0u16]={initial};SESSION[1u16]=0;BACKGROUND[0u16]=0;"));
        let tick = if name.contains("split-lifetime") {
            "if number==0 {GAME[0u16]=2;GAME[1u16]=4;GAME[2u16]=1;session_move(0);let mut ticks:u16=0;while ticks<10 {session_tick(0,0,50);ticks+=1;}}session_tick(0,0,0);"
        } else if name.contains("entry") {
            "session_tick(buttons,held,0);SESSION[0u16]=ENTRY_OPEN;SESSION[1u16]=if number<2 {50u16}else{100u16};"
        } else if name.contains("goal") || name.contains("fallback") {
            "session_tick(buttons,held,0);TRANSITION[0u16]=0;SESSION[0u16]=if number<2 {GOAL_OPEN}else{GOAL_CLOSE};SESSION[1u16]=100;"
        } else if name.contains("moving") {
            "if number==0 {session_move(0);}session_tick(buttons,held,40);"
        } else {
            "session_tick(buttons,held,0);"
        };
        source = source.replace("session_tick(buttons, held, elapsed);", tick);
    }
    if name.starts_with("web-stage")
        && name != "web-stage-orientation-continuity"
        && !name.starts_with("web-stage-hint-order-")
    {
        let level = u16::from(name.contains("stage2"));
        let background = u16::from(name.ends_with("gradient"));
        let pose = if name == "web-stage2-right-standing" {
            "GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=0;"
        } else {
            ""
        };
        source = source.replace("session_init();", &format!("session_init();level_reset({level});SESSION[0u16]=PLAY;BACKGROUND[0u16]={background};{pose}"));
        source = source.replace(
            "run(3, false, 128);",
            if name.contains("warm") {
                "run(3, false, 128);"
            } else {
                "run(1, false, 128);"
            },
        );
    }
    if name == "web-stage1-warm-cached-diagnostic" {
        source = source.replace("run(3, false, 128);", "run(5, true, 128);");
        source = source.replace(
            "session_init();",
            "session_init();TRANSITION[0u16]=FADE_OUT;TRANSITION[1u16]=0;CAPTURE_STIMULUS[0u16]=3;",
        );
    }
    if name.ends_with("warm-rerecord") {
        source = source.replace(
            "if scene_generation != LEVEL_GENERATION[0u16]",
            "if true || scene_generation != LEVEL_GENERATION[0u16]",
        );
    }
    if let Ok(alpha) = std::env::var("BLOXORZ_HINT_ALPHA") {
        assert!(["23", "60", "64", "96"].contains(&alpha.as_str()));
        assert_eq!(source.matches("color.a = 60;").count(), 1);
        source = source.replace("color.a = 60;", &format!("color.a = {alpha};"));
    }
    if name == "web-goal-close" {
        source = source
            .replace("run(3, false, 128);", "run(5, false, 128);")
            .replace(
                "session_tick(buttons, held, elapsed);",
                "goal_close_frame(number);",
            );
        source += r#"
fn goal_close_frame(n:u16){
 if n==0 {
 level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;TRANSITION[0u16]=0;
 let route:Buf<u16,7>=Buf::new([0,0,2,0,0,0,2]);let mut i:u16=0;
 while i<7 {if !session_move(route[i]) {halt(0xbad);}let mut k:u16=0;while k<5 {session_tick(0,0,50);k+=1;}i+=1;}
 let mut ticks:u16=0;while SESSION[0u16]!=GOAL_CLOSE && ticks<100 {session_tick(0,0,10);ticks+=1;}
 if SESSION[0u16]!=GOAL_CLOSE || fading() {halt(0xbad);}
 } else {session_tick(0,0,50);}
 HUD[0u16]=0;BACKGROUND[0u16]=0;
 if n<4 && fading() {halt(0xbad);}
 if n==4 && (!fading() || SESSION[1u16]!=200 || block_visible()) {halt(0xbad);}
}
"#;
    }
    if name == "web-stage-orientation-continuity" {
        source = source
            .replace("run(3, false, 128);", "run(4, false, 128);")
            .replace(
                "session_tick(buttons, held, elapsed);",
                "orientation_frame(number);",
            );
        source += r#"
fn orientation_frame(n:u16){
 BACKGROUND[0u16]=0;HUD[0u16]=0;
 if n==0 {level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;check(session_move(0));}
 if n==2 {check(session_move(1));}
 if n==0 || n==2 {SESSION[1u16]=220;MOTION[4u16]=220;}
 else {session_tick(0,0,0);}
}
fn check(ok:bool){if !ok {halt(0xbad);}}
"#;
    }
    if name.starts_with("web-stage-hint-order-") {
        source = source
            .replace("run(3, false, 128);", "run(4, false, 128);")
            .replace(
                "session_tick(buttons, held, elapsed);",
                "hint_order_frame(number);",
            );
        source += r#"
fn hint_order_frame(n:u16){
 if n==0 {level_reset(28);}
 BACKGROUND[0u16]=0;HUD[0u16]=0;TRANSITION[0u16]=0;
 let mut i:u16=0;while i<LEVEL[6u16] && !(level_field(i,0)==5 && (GAME[3u16]&level_field(i,1))==0) {i+=1;}
 if i==LEVEL[6u16] {halt(0xbad);}
 let half=fix32::from_words(32768,0);let one=fix32::from_int(1);
 SESSION[0u16]=FALL;SESSION[1u16]=0;MOTION[5u16]=0;
 FALL_STATE[0u16]=FallState{position:demo_point(fix32::from_int((i%LEVEL[0u16]) as i16)-fix32::from_int(LEVEL[0u16] as i16)*half+half,
 fix32::from_int(2)-fix32::from_int(n as i16),
 fix32::from_int((i/LEVEL[0u16]) as i16)-fix32::from_int(LEVEL[1u16] as i16)*half+half),velocity:demo_zero(),scale:demo_point(one,fix32::from_int(2),one),angle:fix32::zero(),spin:fix32::zero(),axis:0};
}
"#;
        if name.ends_with("early") {
            let start = source.rfind("                draw_bridge_group(").unwrap();
            let end = start + source[start..].find("                );").unwrap() + 19;
            let call = source[start..end].to_string();
            assert!(call.contains("true,"));
            source.replace_range(start..end, "");
            let at = source
                .find("                draw_block(&mut pass, &block, &mut block_model")
                .unwrap();
            source.insert_str(
                at,
                &format!(
                    "{call}\n                pass.set_camera(&camera);pass.set_state(&state);\n"
                ),
            );
        }
    }
    if name == "all-stage-gallery" {
        source = source
            .replace("run(3, false, 128);", "run(52, false, 128);")
            .replace(
                "session_tick(buttons, held, elapsed);",
                "session_tick(buttons,held,0);",
            );
        source = source.replace("    loop {", "    loop { gallery_pose(number);");
        let start =
            std::env::var("BLOXORZ_GALLERY_START").map_or(0, |v| v.parse::<usize>().unwrap());
        let count = std::env::var("BLOXORZ_GALLERY_COUNT")
            .map_or(52 - start, |v| v.parse::<usize>().unwrap());
        assert!(count > 0 && start + count <= 52);
        source = source
            .replace(
                "gallery_pose(number);",
                &format!("gallery_pose(number+{start});"),
            )
            .replace(
                "run(52, false, 128);",
                &format!("run({count}, false, 128);"),
            );
        source += r#"
fn gallery_ticks(total:u16){let mut remain=total;while remain!=0 {let dt=if remain>50 {50u16}else{remain};session_tick(0,0,dt);remain-=dt;}}
fn gallery_pose(which:u16){
    BACKGROUND[0u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;
    if which<33 {level_reset(which);SESSION[0u16]=PLAY;SESSION[1u16]=0;return;}
    if which>=46 {
        level_reset(1);SESSION[0u16]=PLAY;SESSION[1u16]=0;
        let sample=(which-46)%3;
        if which>=49 {BACKGROUND[0u16]=1;}
        if sample<2 {GAME[3u16]=3;gallery_ticks(if sample==0 {130u16}else{260u16});}
        return;
    }
    if which>=42 {level_reset(0);SESSION[0u16]=PLAY;SESSION[1u16]=0;return;}
    let scene=which-33;
    if scene<7 {
        level_reset(7);SESSION[0u16]=PLAY;SESSION[1u16]=0;GAME[0u16]=2;GAME[1u16]=4;GAME[2u16]=1;
        session_move(0);
        if scene==0 {gallery_ticks(110);return;}
        gallery_ticks(220);
        if scene==1 {gallery_ticks(35);return;}
        if scene==2 {gallery_ticks(140);return;}
        gallery_ticks(280);
        if scene==4 {session_move(2);gallery_ticks(110);}
        if scene==5 {session_move(6);}
        if scene==6 {let mut n:u16=0;while n<5 {session_move(2);gallery_ticks(220);n+=1;}}
    } else {
        level_reset(3);SESSION[0u16]=PLAY;SESSION[1u16]=0;GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;session_move(0);gallery_ticks(220);
        if scene==7 {gallery_ticks(70);}else{gallery_ticks(140);gallery_ticks(150);}
    }
}
"#;
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
    if let Some(variant) = name.strip_prefix("opt-shadow-") {
        assert!([
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
            "carrier-play",
            "carrier-open",
            "carrier-drop",
            "carrier-close",
            "carrier-soft-fallback",
            "carrier-heavy-fallback"
        ]
        .contains(&variant));
        assert_eq!(source.matches("session_init();").count(), 1);
        if variant == "far64" || variant == "far64-always" {
            assert_eq!(source.matches("let far = level_far(which);").count(), 1);
            source = source.replace(
                "let far = level_far(which);",
                "let far = fix32::from_int(63);",
            );
        }
        source = source.replace("session_init();", "diagnostic_pose();");
        let setup = if variant == "goal-always" {
            "level_reset(1);GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=2;SESSION[0u16]=PLAY;session_move(3);let mut tick:u16=0;while tick<63 {session_tick(0,0,10);tick+=1;}"
        } else if variant == "carrier-play" {
            "level_reset(1);SESSION[0u16]=PLAY;"
        } else if variant == "carrier-open" {
            "level_reset(1);GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=2;SESSION[0u16]=PLAY;session_move(3);let mut tick:u16=0;while tick<32 {session_tick(0,0,10);tick+=1;}"
        } else if variant == "carrier-drop" {
            "level_reset(1);GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=2;SESSION[0u16]=PLAY;session_move(3);let mut tick:u16=0;while tick<63 {session_tick(0,0,10);tick+=1;}"
        } else if variant == "carrier-close" {
            "level_reset(1);GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=2;SESSION[0u16]=PLAY;session_move(3);let mut tick:u16=0;while tick<102 {session_tick(0,0,10);tick+=1;}"
        } else if variant == "carrier-soft-fallback" {
            "level_reset(14);SESSION[0u16]=PLAY;"
        } else if variant == "carrier-heavy-fallback" {
            "level_reset(16);SESSION[0u16]=PLAY;"
        } else if variant == "right-standing" {
            "level_reset(1);GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=0;SESSION[0u16]=PLAY;"
        } else if variant.starts_with("bridge-") {
            if variant == "bridge-on-always" {
                "level_reset(1);GAME[0u16]=12;GAME[1u16]=5;GAME[2u16]=0;GAME[3u16]=2;SESSION[0u16]=PLAY;"
            } else {
                "level_reset(1);GAME[0u16]=12;GAME[1u16]=5;GAME[2u16]=0;GAME[3u16]=0;SESSION[0u16]=PLAY;"
            }
        } else {
            "level_reset(1);GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=0;SESSION[0u16]=PLAY;session_move(2);let mut tick:u16=0;while tick<11 {session_tick(0,0,10);tick+=1;}"
        };
        source.push_str(&format!(
            "\nfn diagnostic_pose(){{session_init();{setup}BACKGROUND[0u16]=0;}}\n"
        ));
        source = source
            .replace("run(3, false, 128);", "run(1, false, 128);")
            .replace("let elapsed = now - previous;", "let elapsed:u16=0;");
        let shadow_begin = source
            .find("let shadow_state = RenderState::new(RenderStateDesc {")
            .unwrap();
        let shadow_end = shadow_begin + source[shadow_begin..].find("});").unwrap();
        let old = source[shadow_begin..shadow_end].to_owned();
        if variant != "production"
            && variant != "right-standing"
            && !variant.starts_with("carrier-")
        {
            let desired = if variant.ends_with("always") {
                "depth: DepthFunc::Always"
            } else {
                "depth: DepthFunc::LessEqual"
            };
            let updated = old
                .replace("depth: DepthFunc::Always", desired)
                .replace("depth: DepthFunc::LessEqual", desired);
            source.replace_range(shadow_begin..shadow_end, &updated);
        } else {
            assert!(old.contains("depth: DepthFunc::Always"));
        }
        if variant == "no-shadow" {
            assert_eq!(source.matches("a: 87,").count(), 1);
            source = source.replace("a: 87,", "a: 0,");
        } else if variant == "no-block" {
            source = source.replace("if block_visible() {", "if false {");
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
    if name.contains("-render-time-") {
        source = source.replace(
            "session_init();",
            if name.ends_with("-fade") {
                "session_init();HUD[0u16]=1;TRANSITION[0u16]=FADE_OUT;TRANSITION[1u16]=160;"
            } else {
                "session_init();HUD[0u16]=1;"
            },
        );
        assert_eq!(
            source
                .matches("render_start = profiler_active_milliseconds();")
                .count(),
            1
        );
        source = source.replace(
            "render_start = profiler_active_milliseconds();",
            "signal(8,number);render_start = profiler_active_milliseconds();",
        );
    }
    if name.contains("-cpu-cost-") && !name.starts_with("fragile-") {
        assert_eq!(source.matches("level_reset(0);").count(), 1);
        source = source.replace("level_reset(0);", "level_reset(7);");
        let mode = if name.ends_with("-legacy") { 2 } else { 1 };
        source = source.replace(
            "pub fn renderer_game_three_frames() {",
            &format!("pub fn renderer_game_three_frames() {{GROUND_BATCH_MODE[0u16]={mode};"),
        );
        source = source.replace(
            "ground_rebuild();",
            "signal(9,0);ground_rebuild();signal(10,0);",
        );
    }
    if name == "static-cache-prepared-capture" {
        source = source.replace(
            "gpu.submit(commands);",
            "gpu.request_snapshot();gpu.submit_capturable(commands);",
        );
    }
    if name.ends_with("-heap-reload-capture") {
        source=source.replace("session_tick(buttons, held, elapsed);","if number==1 {level_reset(1);SESSION[0u16]=PLAY;}if number==2 {level_reset(0);SESSION[0u16]=PLAY;}session_tick(buttons,held,elapsed);");
        source = source.replace(
            "gpu.submit(commands);",
            "gpu.request_snapshot();gpu.submit_capturable(commands);",
        );
        source = source.replace(
            "gpu.submit(scene_commands);",
            "gpu.request_snapshot();gpu.submit_capturable(scene_commands);",
        );
        source=source.replace("ground_rebuild();","signal(9,scene_commands.command_words());signal(10,commands.command_words());ground_rebuild();");
    }
    let entry = if let Some(body) = probe {
        source += &format!(
            "\nfn check(ok:bool){{if !ok{{halt(0xbad);}}}}\npub fn probe(){{{body}halt(0x600d);}}"
        );
        "probe"
    } else if name == "all-stage-gallery"
        || name.starts_with("opt-shadow-")
        || name.starts_with("web-stage")
        || name.starts_with("frame-schedule-")
        || name.starts_with("web-shadow-")
        || name.starts_with("web-cache-")
        || name == "web-goal-mid"
        || name == "web-goal-close"
    {
        "renderer_game_three_frames"
    } else if name == "automatic-capture-full-application" {
        "renderer_game_capture_full"
    } else if name == "automatic-capture-application" {
        "renderer_game_capture"
    } else {
        "renderer_game_frames"
    };
    if name.starts_with("opt-shadow-")
        || name == "all-stage-gallery"
        || name.starts_with("fragile-")
    {
        std::fs::write(out.join("instrumented-main.rs"), &source).unwrap();
    }
    // Test-only readonly instrumentation uses the public getter, never arena slot layout.
    // Original marker12 is preserved for the independent input-transition trials.
    source = source.replace(
        "block_pose(model);",
        "block_pose(model);world_probe(model);",
    );
    source += "\nfn world_probe(model:&Model){let world=model.world();let words:Buf<u16,24>=Buf::new([world.row0.x.lo_bits(),world.row0.x.hi_bits(),world.row0.y.lo_bits(),world.row0.y.hi_bits(),world.row0.z.lo_bits(),world.row0.z.hi_bits(),world.row0.w.lo_bits(),world.row0.w.hi_bits(),world.row1.x.lo_bits(),world.row1.x.hi_bits(),world.row1.y.lo_bits(),world.row1.y.hi_bits(),world.row1.z.lo_bits(),world.row1.z.hi_bits(),world.row1.w.lo_bits(),world.row1.w.hi_bits(),world.row2.x.lo_bits(),world.row2.x.hi_bits(),world.row2.y.lo_bits(),world.row2.y.hi_bits(),world.row2.z.lo_bits(),world.row2.z.hi_bits(),world.row2.w.lo_bits(),world.row2.w.hi_bits()]);signal(13,words.as_ptr().addr());}\n";
    let source = crate::shared_application_entry::select(&source, entry).unwrap();
    if name.starts_with("opt-shadow-") || name == "all-stage-gallery" {
        std::fs::write(out.join("compiled-main.rs"), &source).unwrap();
    }
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let mut dependencies = vec![];
    let profile = resolver::RendererLayoutProfile::Column16;
    let capture = if name.ends_with("-heap-reload-capture")
        || name == "static-cache-prepared-capture"
    {
        let gate = out.join("capture-gate.rs");
        std::fs::write(
            &gate,
            r#"use crate::dsl_rt::*;mod gpu_api;use gpu_api::*;
pub fn profiler_gate(d:Array<u16>)->u16 {
 signal(5,d.as_ptr().addr());let mut r:u16=0;
 while r<d[36u16] {
  if gpu_capture_range_word(r,1)==4 && gpu_capture_range_word(r,3)<2 {
   let sector=(gpu_capture_range_word(r,2)>>7)|(gpu_capture_range_word(r,3)<<9);
   signal(6,sector);let mut i:u16=0;
   while i<40 {signal(7,gpu_capture_read_word(r,if i<16 {i}else{i+48},0));i+=1;}
  }
  r+=1;
 }
 if gpu_capture_error()!=0 {halt(0xca01);}0
}"#,
        )
        .unwrap();
        if name == "static-cache-prepared-capture" {
            let text = std::fs::read_to_string(&gate).unwrap().replace(
                "gpu_capture_range_word(r,1)==4 && gpu_capture_range_word(r,3)<2",
                "gpu_capture_range_word(r,1)==2",
            );
            std::fs::write(&gate, text).unwrap();
        }
        let manifest = out.join("capture-manifest.json");
        std::fs::write(&manifest,serde_json::to_vec_pretty(&serde_json::json!({"version":3,"schema":"digitaldesign.gpu-capture-build","command_abi":8,"manifest_id":"0000000000000000000000000000000000000000000000000000000000000000","layout":profile.recorded_description(),"resources":[],"options":{"capture_allocation_guards":false}})).unwrap()).unwrap();
        resolver::RendererCaptureConfiguration::from_manifest(gate, &manifest).unwrap()
    } else if name.contains("-cpu-cost-")
        || name.starts_with("frame-schedule-")
        || name.starts_with("fragile-")
    {
        resolver::RendererCaptureConfiguration::with_default_hook(profile)
    } else {
        resolver::RendererCaptureConfiguration::without_capture(profile)
    };
    let mut application_rcc = source_example.join("rcc");
    if name.ends_with("-cpu-shadow-cost") {
        let local = out.join("instrumented-rcc");
        std::fs::create_dir_all(&local).unwrap();
        for entry in std::fs::read_dir(&application_rcc).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|x| x == "rs") {
                std::fs::copy(&path, local.join(path.file_name().unwrap())).unwrap();
            }
        }
        let path = local.join("shadow.rs");
        let mut shadow = std::fs::read_to_string(&path).unwrap();
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
fn shadow_draw(p:&mut Pass,m:&Mesh,o:&mut Model,a:&Material,t:ShadowTriangle){signal(7,4);measured_body_shadow_draw(p,m,o,a,t);signal(8,4);}
"#;
        std::fs::write(path, shadow).unwrap();
        application_rcc = local;
    }
    let ir = resolver::compile_explicit_gpu_program_checked_with_sources(
        "Bloxorz RTT",
        &source,
        &options,
        &application_rcc,
        &sources.join("rcc"),
        &constants,
        &[sources.join("examples/bloxorz/rcc")],
        &capture,
        &rcc::frontend::rustc_check::RustcCheck::new(out.join("checked")),
        &mut |p| dependencies.push(p.to_owned()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(dependencies.contains(&sources.join("examples/bloxorz/rcc/bloxorz_core.rs")));
    let frontend_static_words = ir.debug.static_data_end;
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    std::fs::write(
        out.join("budget.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"static_words":frontend_static_words}))
            .unwrap(),
    )
    .unwrap();
    (program, resources, out)
}

#[path = "gpu_renderer_bloxorz_buttons.rs"]
mod buttons;
#[path = "gpu_renderer_bloxorz_cpu_emu.rs"]
mod cpu_emu;
#[path = "gpu_renderer_bloxorz_moving.rs"]
mod moving;
#[path = "gpu_renderer_bloxorz_orientation.rs"]
mod orientation;
#[path = "gpu_renderer_bloxorz_static_cache.rs"]
mod static_cache;

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

fn check_swap_bridge_failure(level: u16, route: &[u16]) {
    let moves = route
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        r#"
session_init();level_reset({level});SESSION[0u16]=PLAY;TRANSITION[0u16]=0;SESSION[3u16]=0;
let route:Buf<u16,{count}>=Buf::new([{moves}]);
let mut saved:Buf<u16,10>=Buf::new([0;10]);let mut n:u16=0;
while n+1<{count} {{
    let mut j:u16=0;while j<10 {{saved[j]=GAME[j];j+=1;}}
    check(session_move(route[n]));
    if route[n]==6 {{
        check(GAME[4u16]==0 && SESSION[0u16]==PLAY && GAME[8u16]==(saved[8u16]^1));
        session_tick(32768,0,0);j=0;while j<10 {{check(GAME[j]==saved[j]);j+=1;}}
        check(session_move(6));
    }}
    let mut ticks:u16=0;while SESSION[0u16]!=PLAY && ticks<40 {{session_tick(0,0,50);ticks+=1;}}
    check(SESSION[0u16]==PLAY && GAME[4u16]==0);SESSION[3u16]=0;n+=1;
}}
let mut j:u16=0;while j<10 {{saved[j]=GAME[j];j+=1;}}
let before=block_transform();let supported=idle_cube_transform();
let generation=LEVEL_GENERATION[0u16];signal(6,1);
check(route[n]==6 && session_move(6));
check(GAME[4u16]==2 && GAME[9u16]==1 && SESSION[0u16]==FALL && !fading());signal(6,2);
let at=if saved[8u16]==0 {{GAME[1u16]*LEVEL[0u16]+GAME[0u16]}}else{{GAME[7u16]*LEVEL[0u16]+GAME[6u16]}};
check(level_field(at,0)==5 && GAME[3u16]&level_field(at,1)==0);
check(GAME[8u16]==(saved[8u16]^1) && MOTION[12u16]==saved[8u16]);
let falling=block_transform();let idle=idle_cube_transform();
check(falling.position.x==before.position.x && falling.position.y==before.position.y && falling.position.z==before.position.z);
check(falling.scale.x==before.scale.x && falling.scale.y==before.scale.y && falling.scale.z==before.scale.z);
check(idle.position.x==supported.position.x && idle.position.y==supported.position.y && idle.position.z==supported.position.z);
session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,50);
let dropped=block_transform();let idle=idle_cube_transform();
check(dropped.position.y<before.position.y && dropped.position.x==before.position.x && dropped.position.z==before.position.z);
check(idle.position.x==supported.position.x && idle.position.y==supported.position.y && idle.position.z==supported.position.z);signal(6,3);
// Undo before the fade restores both identities, mask and move count exactly.
session_tick(32768,0,0);j=0;while j<10 {{check(GAME[j]==saved[j]);j+=1;}}
check(SESSION[0u16]==PLAY && !fading());check(session_move(6));
n=0;while n<12 {{session_tick(0,0,50);n+=1;}}
check(SESSION[0u16]==FALL && fading() && GAME[4u16]==2);signal(6,4);
n=0;while TRANSITION[1u16]<320 && n<8 {{session_tick(0,0,50);n+=1;}}
check(TRANSITION[1u16]==320 && LEVEL_GENERATION[0u16]==generation && GAME[4u16]==2);
session_presented();check(TRANSITION[0u16]==HOLD && SESSION[0u16]==ENTRY_OPEN);
check(LEVEL_GENERATION[0u16]==generation+1 && MOTION[6u16]=={level} && GAME[4u16]==0 && GAME[5u16]==0);
n=0;while n<10 {{session_tick(0,0,50);n+=1;}}check(TRANSITION[1u16]==380);session_presented();
n=0;while n<12 {{session_tick(0,0,50);n+=1;}}
check(!fading() && SESSION[0u16]==PLAY && session_move(route[0u16]));signal(6,5);
"#,
        count = route.len()
    );
    let (program, resources, out) = application(
        &format!("swap-bridge-failure-level{}", level + 1),
        Some(&body),
    );
    std::fs::write(out.join("probe-body.rs"), &body).unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    preload(cpu.physical_memory_mut(), &resources);
    let mut stage = 0;
    let mut steps = 0;
    let mut halted = false;
    for _ in 0..2_000_000 {
        steps += 1;
        match cpu.step().unwrap() {
            StepOutcome::Signaled(e) if e.signal_type == 6 => stage = e.value,
            StepOutcome::Halted { signal } => {
                assert_eq!(
                    signal,
                    0x600d,
                    "level{} swap terminal stage{stage}",
                    level + 1
                );
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded swap terminal timeout");
    assert_eq!(stage, 5);
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"level":level+1,"route":route,"steps":steps,"max_steps":2000000,"correct_unsupported_cube_falls_supported_cube_stays":true,"ordinary_swap_and_full_undo_checked":true,"failure_undo_before_fade_checked":true,"reset_only_after_actual_presentation_callback":true,"new_entry_and_move_checked":true,"gpu_executed":false})).unwrap()).unwrap();
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

fn world_determinant(words: &[u16]) -> f64 {
    let m: [[f64; 3]; 3] =
        std::array::from_fn(|r| std::array::from_fn(|c| scene::fixed(words, (r * 4 + c) * 2)));
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
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
fn actual_hud_cells(memory: &[u16], row: usize) -> Vec<u8> {
    (0..50)
        .map(|x| {
            let i = row * 50 + x;
            (memory[0xfbf0 * 64 + i / 2] >> ((i & 1) * 8)) as u8
        })
        .collect()
}
fn actual_hud_number(cells: &[u8]) -> u16 {
    let digits = cells
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect::<Vec<_>>();
    std::str::from_utf8(&digits).unwrap().parse().unwrap()
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
fn cached_veil_captures_each_boundary_once_and_discards_locked_input() {
    use cpu_v3_tang_nano_20k::{
        input_device::{ControllerSnapshot, InputMode},
        system_sim::{CpuV3SystemSim, VBlankMode},
    };
    let (program, resources, out) = application("web-cached-veil", None);
    let session = global(&program, "SESSION");
    let transition = global(&program, "TRANSITION");
    let motion = global(&program, "MOTION");
    let game = global(&program, "GAME");
    let hud = global(&program, "HUD");
    let mut observed_draws = 0;
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
    let mut static_replays = 0;
    let mut consumer_depth: Option<Vec<u16>> = None;
    let mut sources = 0;
    let mut source_dirty = false;
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
                    let mut depth_clear = None;
                    for c in code {
                        if c >> 28 == 3 || c >> 28 == 11 {
                            observed_draws += 1;
                        }
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
                                assert_eq!(
                                    local[render + 4] & 8,
                                    0,
                                    "veil must not write shared scene depth"
                                );
                            }
                            8 if c & 65536 == 0 => clear = Some(c as u16),
                            8 => depth_clear = Some(c as u16),
                            9 => invalidate = true,
                            12 => copy = true,
                            _ => {}
                        }
                    }
                    assert_eq!(
                        depth,
                        if target == 0x500000 {
                            0x540000
                        } else {
                            0x480000
                        }
                    );
                    if target == 0x500000 {
                        assert_eq!(depth_clear, Some(65535));
                        assert!(copy || clear == Some(0x2924));
                    } else if fixed {
                        consumers += 1;
                        assert_eq!(
                            invalidate, source_dirty,
                            "invalidate exactly after source writes"
                        );
                        source_dirty = false;
                        assert!(!copy);
                        assert_eq!(clear, Some(0x1107));
                        assert!(
                            depth_clear.is_none(),
                            "veil must preserve shared scene depth"
                        );
                        assert!(target == 0x400000 || target == 0x440000);
                        consumer_depth = Some(m[0x480000 / 2..0x480000 / 2 + 102400].to_vec());
                    } else if target == 0x600000 {
                        assert!(clear.is_none());
                        if copy {
                            static_replays += 1;
                            assert!(depth_clear.is_none(), "restoration loads the copied depth");
                        } else {
                            sources += 1;
                            source_dirty = true;
                            assert!(depth_clear.is_none());
                        }
                    } else {
                        assert!(target == 0x400000 || target == 0x440000);
                        if copy {
                            static_replays += 1;
                            assert!(depth_clear.is_none() && clear.is_none());
                        } else {
                            assert!(!copy && clear.is_none());
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
                if m[session] == 3 && m[session + 1] >= 300 {
                    connected = true;
                }
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
                if let Some(expected) = consumer_depth.take() {
                    assert_eq!(
                        expected,
                        &m[0x480000 / 2..0x480000 / 2 + 102400],
                        "veil changed shared scene depth"
                    );
                }
                assert_eq!(
                    m[hud + 1],
                    observed_draws,
                    "HUD must sum static/dynamic/veil/UI actual executions"
                );
                observed_draws = 0;
                assert_eq!(m[hud], 0);
                assert_eq!(&actual_hud_cells(m, 0)[..6], b"LEVEL ");
                if m[transition] != 0 {
                    assert_eq!(
                        source_images.last().unwrap().as_slice(),
                        &m[0x600000 / 2..0x632000 / 2],
                        "cached RTT must stay immutable between captures"
                    );
                }
                let phase = m[transition];
                let time = m[transition + 1];
                if phase == 0 {
                    let pixels = sim.render_active_framebuffer();
                    let mut current = vec![];
                    for y in region.1..region.3 {
                        current.extend_from_slice(&pixels[y * 400 + region.0..y * 400 + region.2]);
                    }
                    if let Some(expected) = &goal_pixels {
                        assert!(
                            expected == &current,
                            "goal/neighbor pixels changed during entry or the first move"
                        );
                        goal_checks += 1;
                    } else {
                        goal_pixels = Some(current);
                    }
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
                            if x < 64 && y < 16 {
                                continue;
                            }
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
                    let pixels = sim.render_active_framebuffer();
                    assert!(
                        pixels
                            .iter()
                            .enumerate()
                            .all(|(i, p)| (i % 400 < 64 && i / 400 < 16) || *p == 0x102039)
                    );
                    assert!(
                        pixels[..16 * 400]
                            .iter()
                            .enumerate()
                            .any(|(i, p)| i % 400 < 64 && *p != 0x102039),
                        "level HUD must remain visible over opaque veil"
                    );
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
            StepOutcome::Halted { signal } => panic!(
                "unexpected cached-veil halt {signal:#x}, moved={moved} restarted={restarted} boundary={boundary} held_play={held_play}"
            ),
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
    assert!(static_replays > sources);
    assert_eq!(
        sources, 3,
        "outgoing, incoming, then the next explicit restart"
    );
    assert!(consumers > sources);
    assert!(
        source_images.windows(2).any(|w| w[0] != w[1]),
        "outgoing and incoming boundaries must have different content"
    );
    assert_eq!(
        &sim.cpu().physical_memory_slice()[0x580000 / 2..0x5b2000 / 2],
        initial_background
    );
    assert!(sim.gpu().last_error().is_none());
    std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"functional_gpu_actual_display":true,"boundary_snapshots":sources,"consumers":consumers,"goal_neighbor_invariant_normal_frames":goal_checks,"initial_closed_entry_without_input":initial_closed,"full_half_veil_formula_matches":half,"opaque_color_matches":opaque,"boundary_held_dpad_start_triangle_suppressed":true,"fresh_start_accepted":accepted,"both_normal_back_targets":ordinary,"source_immutable":true,"scope":"instruction CPU + functional GPU/Display; explicit40ms host ticks; no board, scanout timing or native CPU performance claim"})).unwrap()).unwrap();
}

#[test]
fn web_first_two_stage_play_frames_use_functional_gpu_and_actual_display() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    for name in [
        "web-stage1-play",
        "web-stage2-play",
        "web-stage2-right-standing",
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
    let commands = &memory[0x9880 * 64..0x9880 * 64 + usize::from(command_words)];
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
            memory[0x9880 * 64..0x9880 * 64 + usize::from(command_words)]
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
        for _ in 0..10_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            match sim.step().unwrap() {
                StepOutcome::Signaled(e) if e.signal_type == 14 => {
                    push_words = usize::from(e.value)
                }
                StepOutcome::Signaled(e) if e.signal_type == 15 => {
                    let m = sim.cpu().physical_memory_slice();
                    commands = m[0x9880 * 64..0x9880 * 64 + usize::from(e.value)].to_vec();
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
        assert!(
            halted,
            "{name}: bounded cache invalidation scene timeout after{}frames",
            frames.len()
        );
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
        let (program, resources, out) = application(&name, None);
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
fn retained_static_scene_matches_fresh_recording_and_reuses_immutable_commands() {
    static_cache::retained_static_scene_regression();
}
#[test]
fn all_stage_loading_split_swap_merge_fragile_and_navigation_follow_reference_rules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let reference =
        std::fs::read_to_string(root.join("examples/bloxorz/assets/reference.txt")).unwrap();
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

#[test]
fn all_stages_and_new_presentations_render_with_real_cpu_gpu_and_display() {
    use cpu_v3_tang_nano_20k::{
        input_device::{ControllerSnapshot, InputMode},
        system_sim::{CpuV3SystemSim, VBlankMode},
    };
    let start = std::env::var("BLOXORZ_GALLERY_START").map_or(0, |v| v.parse::<usize>().unwrap());
    let expected_frames =
        std::env::var("BLOXORZ_GALLERY_COUNT").map_or(52 - start, |v| v.parse::<usize>().unwrap());
    let (program, resources, out) = application("all-stage-gallery", None);
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

/// Four real aperture phases and the two dynamic-neighbor fallback levels.
#[test]
#[ignore = "bounded production goal aperture and dynamic-neighbor qualification"]
fn goal_carrier_surface_ownership_diagnostic() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    assert!(std::env::var_os("BLOXORZ_CARRIER_PROBE").is_none());
    let extra_only = std::env::var("BLOXORZ_CARRIER_CASE_SET").is_ok_and(|v| {
        assert_eq!(v, "closing-and-fallback");
        true
    });
    for (variant, expected) in [
        ("carrier-play", [0u16, 0]),
        ("carrier-open", [4, 100]),
        ("carrier-drop", [5, 210]),
        ("carrier-close", [7, 100]),
        ("carrier-soft-fallback", [0, 0]),
        ("carrier-heavy-fallback", [0, 0]),
    ] {
        if extra_only
            && (variant == "carrier-play" || variant == "carrier-open" || variant == "carrier-drop")
        {
            continue;
        }
        let (program, resources, out) = application(&format!("opt-shadow-{variant}"), None);
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("assets.json")).unwrap()).unwrap();
        assert_eq!(
            catalog["goal_carrier"]["fallback_levels"],
            serde_json::json!([15, 17])
        );
        let carrier_records: Vec<u64> = catalog["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["name"].as_str().unwrap().starts_with("goal-carrier-"))
            .map(|m| m["record_sector"].as_u64().unwrap())
            .collect();

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
        let mut inputs = vec![];
        let mut count = 0u16;
        let mut halted = false;
        let mut steps = 0;
        for _ in 0..4_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_vblank();
            }
            steps += 1;
            let outcome=sim.step_observing(|write,memory|{
                if write.device==4&&write.channel==0{count=write.value;}
                if write.device==4&&write.channel==1 {
                    let file=format!("gpu-before-{}.bin",inputs.len());std::fs::write(out.join(&file),memory.iter().flat_map(|w|w.to_le_bytes()).collect::<Vec<_>>()).unwrap();
                    let base=usize::from(write.value)*64;let mut banks=[0usize;2];let mut carrier_apertures=vec![];let mut draws=0;let mut triangles=0;
                    for pair in memory[base..base+(usize::from(count)+1)*2].as_chunks::<2>().0 {
                        let code=u32::from(pair[0])|(u32::from(pair[1])<<16);
                        if code>>28==2 {let at=((code>>12)&65535) as usize*64;let header=u32::from(memory[at])|(u32::from(memory[at+1])<<16);banks[(code&1) as usize]=((header>>8)&127) as usize;if let Some(aperture)=carrier_records.iter().position(|r|*r==(at/64) as u64){carrier_apertures.push(aperture);}}
                        if code>>28==3 {draws+=1;triangles+=banks[(code&1) as usize];}
                    }
                    inputs.push(serde_json::json!({"memory":file,"base_bytes":u32::from(write.value)*128,"count_minus_one":count,"maximum_work":10000000,"submitted_draws":draws,"submitted_triangles":triangles,"carrier_apertures":carrier_apertures}));
                }true
            }).unwrap().unwrap();
            if let StepOutcome::Signaled(e) = outcome {
                if e.signal_type == 13 {
                    let at = usize::from(e.value);
                    assert!(
                        world_determinant(&sim.cpu().physical_memory_slice()[at..at + 24]) > 0.
                    );
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d, "{:?}", sim.gpu().last_error());
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded carrier pose timeout");
        assert!(sim.gpu().last_error().is_none());
        let selected: Vec<u64> = inputs
            .iter()
            .flat_map(|input| {
                input["carrier_apertures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
            })
            .collect();
        if variant.ends_with("fallback") {
            assert!(selected.is_empty());
        } else {
            assert_eq!(selected.len(), 1);
            match variant {
                "carrier-play" => assert_eq!(selected, [43]),
                "carrier-open" => assert_eq!(selected, [45]),
                "carrier-drop" => assert_eq!(selected, [47]),
                _ => assert!(selected[0] == 23 || selected[0] == 24),
            }
        }

        let m = sim.cpu().physical_memory_slice();
        let at = global(&program, "SESSION");
        assert_eq!(&m[at..at + 2], &expected);
        let game = global(&program, "GAME");
        let active = sim.display_state().active_base as usize;
        for (filename, base) in [
            ("color-physical.bin", active),
            ("depth-physical.bin", 0x480000 / 2),
        ] {
            std::fs::write(
                out.join(filename),
                m[base..base + 102400]
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        write_display_ppm(
            &out.join("frame-display.ppm"),
            &sim.render_active_framebuffer(),
        );
        std::fs::write(out.join("qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"variant":variant,"surface":"production-goal-carrier","level":if variant=="carrier-soft-fallback"{15}else if variant=="carrier-heavy-fallback"{17}else{2},"camera":"own stage eye/focal and conservative far","game":m[game..game+10],"session":expected,"steps":steps,"max_steps":4000000,"gpu_inputs":inputs,"active_color_byte_base":active*2,"display":"actual400x240 Display output","limits":"representative production states; all48 aperture meshes validated separately, not all33 whole-frame qualification; real hole geometry, no depth bias/Always override"})).unwrap()).unwrap();
    }
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
fn render_ms_uses_last_gpu_wait_excludes_present_and_handles_cold_and_wrap() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    use std::time::Duration;
    for fade in [false, true] {
        let name = if fade {
            "web-stage1-warm-render-time-fade"
        } else {
            "web-stage1-warm-render-time-normal"
        };
        let (program, resources, out) = application(name, None);
        let timing = global(&program, "FRAME_RENDER_MS");
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
        sim.cpu_mut().set_boot_window(&program.words);
        preload(sim.cpu_mut().physical_memory_mut(), &resources);
        let mut clock = 0u64;
        let mut starts = 0usize;
        let mut frames = vec![];
        let mut halted = false;
        let mut wrapped = false;
        let mut render_wrapped = false;
        for _ in 0..8_000_000 {
            if sim.waiting_for_vblank() {
                sim.advance_host_time(Duration::from_millis(50));
                clock += 50;
                sim.advance_vblank();
            }
            let mut started = false;
            let outcome = sim
                .step_observing(|w, _| {
                    if w.device == 4 && w.channel == 1 {
                        started = true;
                    }
                    true
                })
                .unwrap()
                .unwrap();
            if started {
                let delta = [3u64, 5, 7, 11][starts];
                sim.advance_host_time(Duration::from_millis(delta));
                let low_before = clock as u16;
                clock += delta;
                render_wrapped |= (clock as u16) < low_before;
                starts += 1;
            }
            if let StepOutcome::Signaled(e) = outcome {
                if e.signal_type == 8 && e.value == 0 {
                    let delta = 65534u16.wrapping_sub(clock as u16);
                    sim.advance_host_time(Duration::from_millis(u64::from(delta)));
                    clock += u64::from(delta);
                    wrapped = true;
                }
                if e.signal_type == 12 {
                    let expected_starts = if fade {
                        if frames.is_empty() {
                            4
                        } else {
                            1
                        }
                    } else if frames.is_empty() {
                        3
                    } else {
                        2
                    };
                    assert_eq!(starts, expected_starts);
                    let expected = if fade { 3u16 } else { 8u16 };
                    let m = sim.cpu().physical_memory_slice();
                    let row = actual_hud_cells(m, 2);
                    if frames.is_empty() {
                        assert_eq!(m[timing + 1], 0);
                    } else {
                        assert_eq!(&m[timing..timing + 2], &[expected, 1]);
                    }
                    if frames.len() < 2 {
                        assert_eq!(&row[3..8], b"-----");
                    } else {
                        assert_eq!(actual_hud_number(&row[3..8]), expected);
                    }
                    frames.push(serde_json::json!({"frame":frames.len()+1,"gpu_submissions":starts,"sample":m[timing..timing+2],"hud_ms_cells":row[3..8],"host_clock_mod65536":clock as u16,"present_wait_ms_excluded":50}));
                    starts = 0;
                }
            }
            if let StepOutcome::Halted { signal } = outcome {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(halted && wrapped && render_wrapped);
        assert_eq!(frames.len(), 3);
        assert!(sim.gpu().last_error().is_none());
        std::fs::write(out.join("timing-qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"fade":fade,"max_steps":8000000,"clock":"real simulator timer advanced at actual GPU starts and vblank boundaries; test-only signal8 before the real start read","frames":frames,"initial_interval_unknown":true,"mod65536_wrap_tested":true,"hardware_frame_time_claim":false})).unwrap()).unwrap();
    }
}

#[test]
fn same_heap_address_rebuilds_only_after_retirement_and_capture_reads_new_records() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let (program, resources, out) = application("web-stage1-warm-heap-reload-capture", None);
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
            assert_eq!(signal, 0x600d);
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

#[path = "gpu_renderer_bloxorz_batch_cost.rs"]
mod batch_cost;

#[path = "gpu_renderer_bloxorz_atlas.rs"]
mod atlas;
#[path = "gpu_renderer_bloxorz_fragile_batch.rs"]
mod fragile_batch;
#[path = "gpu_renderer_bloxorz_frame_schedule.rs"]
mod frame_schedule;
#[path = "gpu_renderer_bloxorz_pad_surface.rs"]
mod pad_surface;

#[test]
fn cached_veil_diagnostic_updates_transmittance_overlay_on_every_consumer() {
    use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
    let (program, resources, out) = application("web-stage1-warm-cached-diagnostic", None);
    let session = global(&program, "SESSION");
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait).with_functional_gpu();
    sim.cpu_mut().set_boot_window(&program.words);
    preload(sim.cpu_mut().physical_memory_mut(), &resources);
    let mut overlays = vec![];
    let mut snapshots = 0;
    let mut starts = 0;
    let mut halted = false;
    let mut frozen = None;
    for _ in 0..4_000_000 {
        if sim.waiting_for_vblank() {
            sim.advance_vblank();
        }
        match sim
            .step_observing(|w, _| {
                if w.device == 4 && w.channel == 1 {
                    starts += 1;
                }
                true
            })
            .unwrap()
            .unwrap()
        {
            StepOutcome::Signaled(e) if e.signal_type == 4 && e.value != 0 => {
                sim.advance_host_time(std::time::Duration::from_millis(40))
            }
            StepOutcome::Signaled(e) if e.signal_type == 11 => snapshots += 1,
            StepOutcome::Signaled(e) if e.signal_type == 12 => {
                let m = sim.cpu().physical_memory_slice();
                assert_eq!(m[session + 4], 1);
                let image = m[0x600000 / 2..0x632000 / 2].to_vec();
                if let Some(old) = &frozen {
                    assert_eq!(old, &image);
                } else {
                    frozen = Some(image);
                }
                overlays.push(actual_hud_number(&actual_hud_cells(m, 0)[..4]));
            }
            StepOutcome::Halted { signal } => {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
            _ => {}
        }
    }
    assert!(halted, "bounded4M diagnostic timeout");
    assert_eq!(snapshots, 1);
    assert_eq!(starts, 8, "one cache build precedes the boundary snapshot");
    assert_eq!(overlays, [1255, 1224, 1192, 1160, 1128]);
    assert!(!sim.display_state().swap_pending);
    assert!(sim.gpu().last_error().is_none());
    write_display_ppm(
        &out.join("half-diagnostic-display.ppm"),
        &sim.render_active_framebuffer(),
    );
    std::fs::write(out.join("diagnostic-qualification.json"),serde_json::to_vec_pretty(&serde_json::json!({"overlays":overlays,"snapshots":snapshots,"submissions":starts,"maximum_steps":4000000,"snapshot_immutable":true,"selected_128_halt_after_retired_present":true})).unwrap()).unwrap();
}
