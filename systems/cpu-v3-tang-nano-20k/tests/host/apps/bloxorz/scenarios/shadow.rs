//! Explicit shadow fixture inputs.
use super::*;

pub(in super::super) fn frame_schedule(pose: &str, shadow_cost: bool) -> ApplicationCase {
    let pose = pose.to_owned();
    let mut case = edited("renderer_game_three_frames", move |mut source| {
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
            "ENTRY_OPEN"
        } else {
            "PLAY"
        };
        source = source.required_replace("session_init();", &format!("session_init();level_reset({level});SESSION[0u16]={initial};SESSION[1u16]=0;BACKGROUND[0u16]=0;"));
        let tick = if pose.contains("split-lifetime") {
            "if number==0 {GAME[0u16]=2;GAME[1u16]=4;GAME[2u16]=1;session_move(0);let mut ticks:u16=0;while ticks<10 {session_tick(0,0,50);ticks+=1;}}session_tick(0,0,0);"
        } else if pose.contains("entry") {
            "session_tick(buttons,held,0);SESSION[0u16]=ENTRY_OPEN;SESSION[1u16]=if number<2 {50u16}else{100u16};"
        } else if pose.contains("goal") || pose.contains("fallback") {
            "session_tick(buttons,held,0);TRANSITION[0u16]=0;SESSION[0u16]=if number<2 {GOAL_OPEN}else{GOAL_CLOSE};SESSION[1u16]=100;"
        } else if pose.contains("moving") {
            "if number==0 {session_move(0);}session_tick(buttons,held,40);"
        } else {
            "session_tick(buttons,held,0);"
        };
        source = source.required_replace("session_tick(buttons, held, elapsed);", tick);

        source
    });
    case.edit_modules = shadow_cost.then_some(instrument_shadow_cost);
    case.hook = true;
    case
}

pub(in super::super) fn shadow_cache(forced: bool) -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = source
            .required_replace("run(3, false, 128);", "run(9, false, 128);")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;");
        source=source.required_replace("    loop {",r#"    loop {
        if number==0 {level_reset(1);GAME[0u16]=4;GAME[1u16]=4;GAME[2u16]=0;SESSION[0u16]=PLAY;BACKGROUND[0u16]=0;}
        if number==2 {GAME[0u16]=5;}
        if number==3 {GAME[3u16]=1;}
        if number==4 {level_reset(0);GAME[0u16]=7;GAME[1u16]=4;SESSION[0u16]=PLAY;}
        if number==5 {SESSION[0u16]=GOAL_DROP;SESSION[1u16]=0;}
        if number==6 {level_reset(0);GAME[0u16]=7;GAME[1u16]=4;SESSION[0u16]=GOAL_DROP;SESSION[1u16]=0;}
        if number==7 {SESSION[0u16]=ENTRY_OPEN;SESSION[1u16]=0;}
        if number==8 {SESSION[0u16]=PLAY;}
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
        assert_eq!(source.matches("session_init();").count(), 1);
        if variant == "far64" || variant == "far64-always" {
            assert_eq!(source.matches("let far = level_far(which);").count(), 1);
            source = source.required_replace(
                "let far = level_far(which);",
                "let far = fix32::from_int(63);",
            );
        }
        source = source.required_replace("session_init();", "diagnostic_pose();");
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
            .required_replace("run(3, false, 128);", "run(1, false, 128);")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;");
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
            source = source.required_replace("if block_visible() {", "if false {");
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
            "../../../../../examples/gpu_renderer_bloxorz/assets/web-shadow-reference.json"
        ))
        .unwrap();
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == case_name.as_str())
            .unwrap();
        source = source.required_replace(
            "session_init();",
            if case_name == "entry" {
                "session_init();SESSION[0u16]=ENTRY_RISE;SESSION[1u16]=190;BACKGROUND[0u16]=0;"
            } else {
                "session_init();SESSION[0u16]=PLAY;BACKGROUND[0u16]=0;"
            },
        );
        source = source
            .required_replace("run(3, false, 128);", "run(1, false, 128);")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;");
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
