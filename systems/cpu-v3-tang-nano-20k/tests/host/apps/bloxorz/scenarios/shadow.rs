//! Explicit shadow fixture inputs.
use super::*;

pub(in super::super) fn frame_schedule(pose: &str, shadow_cost: bool) -> ApplicationCase {
    let pose = pose.to_owned();
    let mut case = edited("renderer_game_three_frames", move |mut source| {
        source = with_frame_number(source);
        let level = if pose.contains("split-lifetime") {
            7
        } else if pose.contains("fallback15") {
            14
        } else if pose.contains("fallback17") {
            16
        } else if pose.contains("goal") {
            1
        } else {
            0
        };
        let initial = if pose.contains("entry") {
            "AnimationPhase::EntryOpen"
        } else {
            "AnimationPhase::Play"
        };
        source = source.required_replace("GAME_SESSION.begin();", &format!("GAME_SESSION.begin();GAME_SESSION.reset_level({level});GAME_SESSION.animation.phase={initial};GAME_SESSION.animation.elapsed=0;RENDER_STATE.background=BackgroundMode::Solid;"));
        let tick = if pose.contains("split-lifetime") {
            "if number==0 {GAME_SESSION.rules.x=2;GAME_SESSION.rules.z=4;GAME_SESSION.rules.orientation=1;GAME_SESSION.try_move(0);let mut ticks:u16=0;while ticks<10 {GAME_SESSION.tick(0,0,50);ticks+=1;}}GAME_SESSION.tick(0,0,0);"
        } else if pose.contains("entry") {
            "GAME_SESSION.tick(buttons,held,0);GAME_SESSION.animation.phase=AnimationPhase::EntryOpen;GAME_SESSION.animation.elapsed=if number<2 {50u16}else{100u16};"
        } else if pose.contains("goal") || pose.contains("fallback") {
            "GAME_SESSION.tick(buttons,held,0);GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.animation.phase=if number<2 {GOAL_OPEN}else{GOAL_CLOSE};GAME_SESSION.animation.elapsed=100;"
        } else if pose.contains("moving") {
            "if number==0 {GAME_SESSION.try_move(0);}GAME_SESSION.tick(buttons,held,40);"
        } else {
            "GAME_SESSION.tick(buttons,held,0);"
        };
        source = source.required_replace("GAME_SESSION.tick(buttons, held, elapsed);", tick);

        source
    });
    case.edit_modules = shadow_cost.then_some(instrument_shadow_cost);
    case
}

pub(in super::super) fn shadow_cache(forced: bool) -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = source
            .required_replace("run(3);", "run(9);")
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;");
        source=source.required_replace("    loop {",r#"    loop {
        if number==0 {GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=4;GAME_SESSION.rules.z=4;GAME_SESSION.rules.orientation=0;GAME_SESSION.animation.phase=AnimationPhase::Play;RENDER_STATE.background=BackgroundMode::Solid;}
        if number==2 {GAME_SESSION.rules.x=5;}
        if number==3 {GAME_SESSION.rules.bridge_mask=1;}
        if number==4 {GAME_SESSION.reset_level(0);GAME_SESSION.rules.x=7;GAME_SESSION.rules.z=4;GAME_SESSION.animation.phase=AnimationPhase::Play;}
        if number==5 {GAME_SESSION.animation.phase=AnimationPhase::GoalDrop;GAME_SESSION.animation.elapsed=0;}
        if number==6 {GAME_SESSION.reset_level(0);GAME_SESSION.rules.x=7;GAME_SESSION.rules.z=4;GAME_SESSION.animation.phase=AnimationPhase::GoalDrop;GAME_SESSION.animation.elapsed=0;}
        if number==7 {GAME_SESSION.animation.phase=AnimationPhase::EntryOpen;GAME_SESSION.animation.elapsed=0;}
        if number==8 {GAME_SESSION.animation.phase=AnimationPhase::Play;}
"#);
        source=source.required_replace("gpu.submit(commands);","signal(14,commands.push_words());signal(15,commands.command_words());gpu.submit(commands);");
        if forced {
            source = source.required_replace("draw_shadow(", "shadow_invalidate();draw_shadow(");
        }

        source
    })
}

pub(in super::super) fn shadow_diagnostic(variant: &str) -> ApplicationCase {
    let owned = variant.to_owned();
    edited("renderer_game_three_frames", move |mut source| {
        let variant = owned.as_str();

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
        assert_eq!(source.matches("GAME_SESSION.begin();").count(), 1);
        if variant == "far64" || variant == "far64-always" {
            assert_eq!(source.matches("let far = level_far(which);").count(), 1);
            source = source.required_replace(
                "let far = level_far(which);",
                "let far = fix32::from_int(63);",
            );
        }
        source = source.required_replace("GAME_SESSION.begin();", "diagnostic_pose();");
        let setup = if variant == "goal-always" {
            "GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=13;GAME_SESSION.rules.z=2;GAME_SESSION.rules.orientation=2;GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.try_move(3);let mut tick:u16=0;while tick<63 {GAME_SESSION.tick(0,0,10);tick+=1;}"
        } else if variant == "carrier-play" {
            "GAME_SESSION.reset_level(1);GAME_SESSION.animation.phase=AnimationPhase::Play;"
        } else if variant == "carrier-open" {
            "GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=13;GAME_SESSION.rules.z=2;GAME_SESSION.rules.orientation=2;GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.try_move(3);let mut tick:u16=0;while tick<32 {GAME_SESSION.tick(0,0,10);tick+=1;}"
        } else if variant == "carrier-drop" {
            "GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=13;GAME_SESSION.rules.z=2;GAME_SESSION.rules.orientation=2;GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.try_move(3);let mut tick:u16=0;while tick<63 {GAME_SESSION.tick(0,0,10);tick+=1;}"
        } else if variant == "carrier-close" {
            "GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=13;GAME_SESSION.rules.z=2;GAME_SESSION.rules.orientation=2;GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.try_move(3);let mut tick:u16=0;while tick<102 {GAME_SESSION.tick(0,0,10);tick+=1;}"
        } else if variant == "carrier-soft-fallback" {
            "GAME_SESSION.reset_level(14);GAME_SESSION.animation.phase=AnimationPhase::Play;"
        } else if variant == "carrier-heavy-fallback" {
            "GAME_SESSION.reset_level(16);GAME_SESSION.animation.phase=AnimationPhase::Play;"
        } else if variant == "right-standing" {
            "GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=13;GAME_SESSION.rules.z=2;GAME_SESSION.rules.orientation=0;GAME_SESSION.animation.phase=AnimationPhase::Play;"
        } else if variant.starts_with("bridge-") {
            if variant == "bridge-on-always" {
                "GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=12;GAME_SESSION.rules.z=5;GAME_SESSION.rules.orientation=0;GAME_SESSION.rules.bridge_mask=2;GAME_SESSION.animation.phase=AnimationPhase::Play;"
            } else {
                "GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=12;GAME_SESSION.rules.z=5;GAME_SESSION.rules.orientation=0;GAME_SESSION.rules.bridge_mask=0;GAME_SESSION.animation.phase=AnimationPhase::Play;"
            }
        } else {
            "GAME_SESSION.reset_level(1);GAME_SESSION.rules.x=13;GAME_SESSION.rules.z=2;GAME_SESSION.rules.orientation=0;GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.try_move(2);let mut tick:u16=0;while tick<11 {GAME_SESSION.tick(0,0,10);tick+=1;}"
        };
        source.push_str(&format!(
            "\nfn diagnostic_pose(){{GAME_SESSION.begin();{setup}RENDER_STATE.background=BackgroundMode::Solid;}}\n"
        ));
        source = source
            .required_replace("run(3);", "run(1);")
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;");
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
            assert!(
                old.contains("depth: DepthFunc::Always")
                    || old.contains("depth: DepthFunc::LessEqual")
            );
            let updated = old
                .replace("depth: DepthFunc::Always", desired)
                .replace("depth: DepthFunc::LessEqual", desired);
            source.replace_range(shadow_begin..shadow_end, &updated);
        } else {
            assert!(old.contains("depth: DepthFunc::Always"));
        }
        if variant == "no-shadow" {
            assert_eq!(source.matches("a: 87,").count(), 1);
            source = source.required_replace("a: 87,", "a: 0,");
        } else if variant == "no-block" {
            source = source.required_replace("if GAME_SESSION.block_visible() {", "if false {");
        }

        source
    })
}

pub(in super::super) fn web_shadow(case_name: &str) -> ApplicationCase {
    let case_name = case_name.to_owned();
    edited("renderer_game_three_frames", move |mut source| {
        source = source
            .required_replace("    let shadow_owner = shadow_batch_begin();\n", "")
            .required_replace("        shadow_batch_frame();\n", "")
            .required_replace("    shadow_batch_end(shadow_owner);\n", "");

        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../examples/game_bloxorz/assets/web-shadow-reference.json"
        ))
        .unwrap();
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == case_name.as_str())
            .unwrap();
        source = source.required_replace(
            "GAME_SESSION.begin();",
            if case_name == "entry" {
                "GAME_SESSION.begin();GAME_SESSION.animation.phase=AnimationPhase::EntryRise;GAME_SESSION.animation.elapsed=190;RENDER_STATE.background=BackgroundMode::Solid;"
            } else {
                "GAME_SESSION.begin();GAME_SESSION.animation.phase=AnimationPhase::Play;RENDER_STATE.background=BackgroundMode::Solid;"
            },
        );
        source = source
            .required_replace("run(3);", "run(1);")
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;");
        let begin = source.find("fn prepare_block(").unwrap();
        let end = begin + source[begin..].find("\nfn draw_block(").unwrap();
        source.replace_range(begin..end, &shadow_reference_prepare(case));
        source=source.required_replace("gpu.submit(commands);","signal(15,commands.command_words());signal(14,commands.push_words());gpu.submit(commands);");

        source
    })
}

fn instrument_shadow_cost(original: &std::path::Path, local: &std::path::Path) {
    let path = original.join("shadow.rs");
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
    std::fs::write(local.join("shadow.rs"), shadow).unwrap();
}
