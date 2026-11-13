//! Explicit rendering fixture inputs.
use super::*;

pub(super) fn stage_source(
    mut source: String,
    level: u16,
    gradient: bool,
    standing: bool,
    warm: bool,
) -> String {
    let background = u16::from(gradient);
    let pose = if standing {
        "GAME[0u16]=13;GAME[1u16]=2;GAME[2u16]=0;"
    } else {
        ""
    };
    source = source.required_replace("session_init();", &format!("session_init();level_reset({level});SESSION[0u16]=PLAY;BACKGROUND[0u16]={background};{pose}"));
    source = source.required_replace(
        "run(3, false, 128);",
        if warm {
            "run(3, false, 128);"
        } else {
            "run(1, false, 128);"
        },
    );

    source
}

pub(in super::super) fn stage(
    level: u16,
    gradient: bool,
    standing: bool,
    warm: bool,
) -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = stage_source(source, level, gradient, standing, warm);
        source
    })
}

pub(in super::super) fn cached_veil() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = stage_source(source, 0, false, false, true);

        source = source.required_replace("run(3, false, 128);", "run(5, true, 128);");
        source = source.required_replace(
            "session_init();",
            "session_init();TRANSITION[0u16]=FADE_OUT;TRANSITION[1u16]=0;CAPTURE_STIMULUS[0u16]=3;",
        );

        source
    })
}

pub(in super::super) fn rerecord() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = stage_source(source, 0, false, false, true);

        source = source.required_replace(
            "if scene_generation != LEVEL_GENERATION[0u16]",
            "if true || scene_generation != LEVEL_GENERATION[0u16]",
        );

        source
    })
}

pub(in super::super) fn goal_close() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = source
            .required_replace("run(3, false, 128);", "run(5, false, 128);")
            .required_replace(
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

        source
    })
}

pub(in super::super) fn orientation() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = source
            .required_replace("run(3, false, 128);", "run(4, false, 128);")
            .required_replace(
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

        source
    })
}

pub(in super::super) fn goal_mid() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        let initialize = r#"fn initialize_goal_mid(){session_init();BACKGROUND[0u16]=0;
        let mut goal_tick:u16=0;while goal_tick<15 {session_tick(0,0,40);goal_tick+=1;}
        let goal_route:Buf<u16,7>=Buf::new([0,0,2,0,0,0,2]);let mut goal_move:u16=0;
        while goal_move<7 {session_move(goal_route[goal_move]);goal_tick=0;while goal_tick<5 {session_tick(0,0,50);goal_tick+=1;}goal_move+=1;}
        goal_tick=0;while goal_tick<4 {session_tick(0,0,50);goal_tick+=1;}
        goal_tick=0;while goal_tick<4 {session_tick(0,0,50);goal_tick+=1;}session_tick(0,0,10);
}
"#;
        source = source
            .required_replace("session_init();", "initialize_goal_mid();")
            .required_replace("run(3, false, 128);", "run(1, false, 128);")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;");
        source.push_str(initialize);

        source
    })
}

const HINT_ORDER_HOOK: &str = r#"
use crate::{BACKGROUND, HUD};
pub fn hint_order_frame(n:u16){
 if n==0 {level_reset(28);}
 BACKGROUND[0u16]=0;HUD[0u16]=0;TRANSITION[0u16]=0;
 let mut i:u16=0;while i<LEVEL[6u16] && !(level_field(i,0)==5 && (GAME[3u16]&level_field(i,1))==0) {i+=1;}
 if i==LEVEL[6u16] {halt(0xbad);}
 let half=fix32::from_words(32768,0);let one=fix32::from_int(1);
 SESSION[0u16]=FALL;SESSION[1u16]=0;MOTION[5u16]=0;
 FALL_STATE[0u16]=FallState{position:Vec3::new(fix32::from_int((i%LEVEL[0u16]) as i16)-fix32::from_int(LEVEL[0u16] as i16)*half+half,
 fix32::from_int(2)-fix32::from_int(n as i16),
 fix32::from_int((i/LEVEL[0u16]) as i16)-fix32::from_int(LEVEL[1u16] as i16)*half+half),velocity:Vec3::zero(),scale:Vec3::new(one,fix32::from_int(2),one),angle:fix32::zero(),spin:fix32::zero(),axis:0};
}
"#;

fn hint_order_module(source: &Path, target: &Path) {
    let session = std::fs::read_to_string(source.join("session.rs")).unwrap();
    std::fs::write(target.join("session.rs"), session + HINT_ORDER_HOOK).unwrap();
}

pub(in super::super) fn hint_order(early: bool) -> ApplicationCase {
    let mut case = edited("renderer_game_three_frames", move |mut source| {
        source = source
            .required_replace("run(3, false, 128);", "run(4, false, 128);")
            .required_replace(
                "session_tick(buttons, held, elapsed);",
                "hint_order_frame(number);",
            );

        if early {
            let start = source.rfind("                draw_bridge_group(").unwrap();
            let end = start + source[start..].find("                );").unwrap() + 19;
            let call = source[start..end].to_string();
            assert!(call.contains("true,"));
            source.replace_range(start..end, "");
            let at = source.find("                draw_block(").unwrap();
            source.insert_str(
                at,
                &format!(
                    "{call}\n                pass.set_camera(scene.camera.ptr());pass.set_state(scene.state.ptr());\n"
                ),
            );
        }

        source
    });
    case.edit_modules = Some(hint_order_module);
    case
}

pub(in super::super) fn gallery() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = source
            .required_replace("run(3, false, 128);", "run(52, false, 128);")
            .required_replace(
                "session_tick(buttons, held, elapsed);",
                "session_tick(buttons,held,0);",
            );
        source = source.required_replace("    loop {", "    loop { gallery_pose(number);");
        let start =
            std::env::var("BLOXORZ_GALLERY_START").map_or(0, |v| v.parse::<usize>().unwrap());
        let count = std::env::var("BLOXORZ_GALLERY_COUNT")
            .map_or(52 - start, |v| v.parse::<usize>().unwrap());
        assert!(count > 0 && start + count <= 52);
        source = source
            .required_replace(
                "gallery_pose(number);",
                &format!("gallery_pose(number+{start});"),
            )
            .required_replace(
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

        source
    })
}
