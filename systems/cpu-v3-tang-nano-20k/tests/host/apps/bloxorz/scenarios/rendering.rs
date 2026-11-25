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
        "GAME_SESSION.rules.x=13;GAME_SESSION.rules.z=2;GAME_SESSION.rules.orientation=0;"
    } else {
        ""
    };
    source = source.required_replace("GAME_SESSION.begin();", &format!("GAME_SESSION.begin();GAME_SESSION.reset_level({level});GAME_SESSION.animation.phase=AnimationPhase::Play;RENDER_STATE.background=if {background}==0 {{BackgroundMode::Solid}} else {{BackgroundMode::Dithered}};{pose}"));
    source = source.required_replace("run(3);", if warm { "run(3);" } else { "run(1);" });

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

        source = source.required_replace("run(3);", "run(5);");
        source = source.required_replace(
            "GAME_SESSION.begin();",
            "GAME_SESSION.begin();GAME_SESSION.transition.phase=TransitionPhase::FadeOut;GAME_SESSION.transition.elapsed=0;",
        );

        source
    })
}

pub(in super::super) fn rerecord() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = stage_source(source, 0, false, false, true);

        source = source.required_replace(
            "if cache.generation != GAME_SESSION.generation",
            "if true || cache.generation != GAME_SESSION.generation",
        );

        source
    })
}

pub(in super::super) fn goal_close() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = with_frame_number(source);
        source = source
            .required_replace("run(3);", "run(5);")
            .required_replace(
                "GAME_SESSION.tick(buttons, held, elapsed);",
                "goal_close_frame(number);",
            );
        source += r#"
fn goal_close_frame(n:u16){
 if n==0 {
 GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;
 let route:Buf<u16,7>=Buf::new([0,0,2,0,0,0,2]);let mut i:u16=0;
 while i<7 {if !GAME_SESSION.try_move(route[i]) {halt(0xbad);}let mut k:u16=0;while k<5 {GAME_SESSION.tick(0,0,50);k+=1;}i+=1;}
 let mut ticks:u16=0;while GAME_SESSION.animation.phase!=AnimationPhase::GoalClose && ticks<100 {GAME_SESSION.tick(0,0,10);ticks+=1;}
 if GAME_SESSION.animation.phase!=AnimationPhase::GoalClose || GAME_SESSION.fading() {halt(0xbad);}
 } else {GAME_SESSION.tick(0,0,50);}
 RENDER_STATE.hud.enabled=0;RENDER_STATE.background=BackgroundMode::Solid;
 if n<4 && GAME_SESSION.fading() {halt(0xbad);}
 if n==4 && (!GAME_SESSION.fading() || GAME_SESSION.animation.elapsed!=200 || GAME_SESSION.block_visible()) {halt(0xbad);}
}
"#;

        source
    })
}

pub(in super::super) fn orientation() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = with_frame_number(source);
        source = source
            .required_replace("run(3);", "run(4);")
            .required_replace(
                "GAME_SESSION.tick(buttons, held, elapsed);",
                "orientation_frame(number);",
            );
        source += r#"
fn orientation_frame(n:u16){
 RENDER_STATE.background=BackgroundMode::Solid;RENDER_STATE.hud.enabled=0;
 if n==0 {GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;check(GAME_SESSION.try_move(0));}
 if n==2 {check(GAME_SESSION.try_move(1));}
 if n==0 || n==2 {GAME_SESSION.animation.elapsed=220;GAME_SESSION.motion.elapsed=220;}
 else {GAME_SESSION.tick(0,0,0);}
}
fn check(ok:bool){if !ok {halt(0xbad);}}
"#;

        source
    })
}

pub(in super::super) fn goal_mid() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        let initialize = r#"fn initialize_goal_mid(){GAME_SESSION.begin();RENDER_STATE.background=BackgroundMode::Solid;
        let mut goal_tick:u16=0;while goal_tick<15 {GAME_SESSION.tick(0,0,40);goal_tick+=1;}
        let goal_route:Buf<u16,7>=Buf::new([0,0,2,0,0,0,2]);let mut goal_move:u16=0;
        while goal_move<7 {GAME_SESSION.try_move(goal_route[goal_move]);goal_tick=0;while goal_tick<5 {GAME_SESSION.tick(0,0,50);goal_tick+=1;}goal_move+=1;}
        goal_tick=0;while goal_tick<4 {GAME_SESSION.tick(0,0,50);goal_tick+=1;}
        goal_tick=0;while goal_tick<4 {GAME_SESSION.tick(0,0,50);goal_tick+=1;}GAME_SESSION.tick(0,0,10);
}
"#;
        source = source
            .required_replace("GAME_SESSION.begin();", "initialize_goal_mid();")
            .required_replace("run(3);", "run(1);")
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;");
        source.push_str(initialize);

        source
    })
}

const HINT_ORDER_HOOK: &str = r#"
use crate::RENDER_STATE;
pub fn hint_order_frame(n:u16){
 if n==0 {GAME_SESSION.reset_level(28);}
 RENDER_STATE.background=BackgroundMode::Solid;RENDER_STATE.hud.enabled=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;
 let mut i:u16=0;while i<GAME_SESSION.level.cell_count && !(GAME_SESSION.level_field(i,0)==5 && (GAME_SESSION.rules.bridge_mask&GAME_SESSION.level_field(i,1))==0) {i+=1;}
 if i==GAME_SESSION.level.cell_count {halt(0xbad);}
 let half=fix32::from_words(32768,0);let one=fix32::from_int(1);
 GAME_SESSION.animation.phase=AnimationPhase::Fall;GAME_SESSION.animation.elapsed=0;GAME_SESSION.motion.moving=0;
 GAME_SESSION.fall=FallState{position:Vec3::new(fix32::from_int((i%GAME_SESSION.level.width) as i16)-fix32::from_int(GAME_SESSION.level.width as i16)*half+half,
 fix32::from_int(2)-fix32::from_int(n as i16),
 fix32::from_int((i/GAME_SESSION.level.width) as i16)-fix32::from_int(GAME_SESSION.level.height as i16)*half+half),velocity:Vec3::zero(),scale:Vec3::new(one,fix32::from_int(2),one),angle:fix32::zero(),spin:fix32::zero(),axis:0};
}
"#;

fn hint_order_module(source: &Path, target: &Path) {
    let session = std::fs::read_to_string(source.join("session.rs")).unwrap();
    std::fs::write(target.join("session.rs"), session + HINT_ORDER_HOOK).unwrap();
}

pub(in super::super) fn hint_order(early: bool) -> ApplicationCase {
    let mut case = edited("renderer_game_three_frames", move |mut source| {
        source = with_frame_number(source);
        source = source
            .required_replace("run(3);", "run(4);")
            .required_replace(
                "GAME_SESSION.tick(buttons, held, elapsed);",
                "hint_order_frame(number);",
            );

        if early {
            let hint =
                "    pass.draw_prepared(parts[15u16]);\n    DRAW_STATS.draw_calls += RENDER_STATE.bridges.transparent_draws;\n";
            let block = concat!(
                "    draw_block(\n",
                "        pass,\n",
                "        &mut scene.block_model,\n",
                "        &scene.block,\n",
                "        &scene.material,\n",
                "        atlas_view,\n",
                "    );",
            );
            source = source.required_replace(hint, "");
            source = source.required_replace(block, &format!("{hint}{block}"));
        }

        source
    });
    case.edit_modules = Some(hint_order_module);
    case
}

pub(in super::super) fn gallery() -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = source
            .required_replace("run(3);", "run(52);")
            .required_replace(
                "GAME_SESSION.tick(buttons, held, elapsed);",
                "GAME_SESSION.tick(buttons,held,0);",
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
            .required_replace("run(52);", &format!("run({count});"));
        source += r#"
fn gallery_ticks(total:u16){let mut remain=total;while remain!=0 {let dt=if remain>50 {50u16}else{remain};GAME_SESSION.tick(0,0,dt);remain-=dt;}}
fn gallery_pose(which:u16){
    RENDER_STATE.background=BackgroundMode::Solid;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;
    if which<33 {GAME_SESSION.reset_level(which);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;return;}
    if which>=46 {
        GAME_SESSION.reset_level(1);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;
        let sample=(which-46)%3;
        if which>=49 {RENDER_STATE.background=BackgroundMode::Dithered;}
        if sample<2 {GAME_SESSION.rules.bridge_mask=3;gallery_ticks(if sample==0 {130u16}else{260u16});}
        return;
    }
    if which>=42 {GAME_SESSION.reset_level(0);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;return;}
    let scene=which-33;
    if scene<7 {
        GAME_SESSION.reset_level(7);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.rules.x=2;GAME_SESSION.rules.z=4;GAME_SESSION.rules.orientation=1;
        GAME_SESSION.try_move(0);
        if scene==0 {gallery_ticks(110);return;}
        gallery_ticks(220);
        if scene==1 {gallery_ticks(35);return;}
        if scene==2 {gallery_ticks(140);return;}
        gallery_ticks(280);
        if scene==4 {GAME_SESSION.try_move(2);gallery_ticks(110);}
        if scene==5 {GAME_SESSION.try_move(6);}
        if scene==6 {let mut n:u16=0;while n<5 {GAME_SESSION.try_move(2);gallery_ticks(220);n+=1;}}
    } else {
        GAME_SESSION.reset_level(3);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.rules.x=3;GAME_SESSION.rules.z=0;GAME_SESSION.rules.orientation=1;GAME_SESSION.try_move(0);gallery_ticks(220);
        if scene==7 {gallery_ticks(70);}else{gallery_ticks(140);gallery_ticks(150);}
    }
}
"#;

        source
    })
}
