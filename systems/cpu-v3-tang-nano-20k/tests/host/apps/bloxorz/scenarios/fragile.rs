//! Explicit fragile fixture inputs.
use super::*;

pub(in super::super) fn fragile_pad(
    atlas: Option<(u16, u16)>,
    frozen_assets: bool,
) -> ApplicationCase {
    let mut case = edited("renderer_game_frames", move |mut source| {
        source = with_frame_number(source);
        source = source
            .required_replace(
                "GAME_SESSION.begin();",
                "GAME_SESSION.begin();RENDER_STATE.hud.enabled=0;",
            )
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;")
            .required_replace(
                "GAME_SESSION.tick(buttons, held, elapsed);",
                "pad_surface_pose(number);",
            );
        source += r#"
fn pad_surface_ticks(total:u16){let mut remain=total;while remain!=0 {let dt=if remain>50 {50u16}else{remain};GAME_SESSION.tick(0,0,dt);remain-=dt;}}
fn pad_surface_pose(which:u16){
 let level:u16=if which<5 {0u16}else{if which<9 {1u16}else{if which==9 {14u16}else{if which==10 {16u16}else{if which==11 {3u16}else{7u16}}}}};
 GAME_SESSION.reset_level(level);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;RENDER_STATE.hud.enabled=0;
 if which==0 {GAME_SESSION.try_move(0);pad_surface_ticks(220);}
 else if which<5 {GAME_SESSION.animation.phase=AnimationPhase::EntryOpen;GAME_SESSION.animation.elapsed=0;if which==2 {pad_surface_ticks(100);}else if which==3 {pad_surface_ticks(390);}else if which==4 {pad_surface_ticks(580);}}
 else if which>=6 && which<=8 {GAME_SESSION.rules.x=13;GAME_SESSION.rules.z=2;GAME_SESSION.rules.orientation=2;GAME_SESSION.try_move(3);pad_surface_ticks(if which==6 {320u16}else{if which==7 {630u16}else{1020u16}});}
}
"#;

        source
    });
    case.atlas = atlas;
    case.assets_from_sources = frozen_assets;
    case
}

pub(in super::super) fn fragile_atlas(
    atlas: Option<(u16, u16)>,
    frozen_assets: bool,
) -> ApplicationCase {
    let mut case = edited("renderer_game_frames", move |mut source| {
        source = with_frame_number(source);
        source = source.required_replace("GAME_SESSION.begin();", "GAME_SESSION.begin();RENDER_STATE.hud.enabled=0;")
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;")
            .required_replace("GAME_SESSION.tick(buttons, held, elapsed);", "let stage:u16=if number<2 || number==6 {3u16}else{if number==2 {29u16}else{if number==3 {14u16}else{if number==4 {16u16}else{7u16}}}};if GAME_SESSION.motion.level!=stage {GAME_SESSION.reset_level(stage);}GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;if number==6 {GAME_SESSION.rules.x=3;GAME_SESSION.rules.z=0;GAME_SESSION.rules.orientation=1;GAME_SESSION.try_move(0);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,10);}else{GAME_SESSION.tick(0,0,0);}");

        source
    });
    case.atlas = atlas;
    case.assets_from_sources = frozen_assets;
    case
}

pub(in super::super) fn fragile_flow() -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        source = with_frame_number(source);
        source = source.required_replace("GAME_SESSION.begin();", "GAME_SESSION.begin();GAME_SESSION.reset_level(3);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.animation.held_lock=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;")
            .required_replace("let fragile_owner = fragile_begin();", "let fragile_owner = fragile_begin();signal(14,fragile_owner.addr());")
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;")
            .required_replace("GAME_SESSION.tick(buttons, held, elapsed);", "if number==1 {GAME_SESSION.rules.x=3;GAME_SESSION.rules.z=0;GAME_SESSION.rules.orientation=1;GAME_SESSION.try_move(0);let mut ticks:u16=0;while ticks<5 {GAME_SESSION.tick(0,0,50);ticks+=1;}}else if number==2 {GAME_SESSION.tick(8,0,0);GAME_SESSION.transition.elapsed=320;}else if number==3 || number==6 {GAME_SESSION.transition.elapsed=80;GAME_SESSION.tick(0,0,0);}else if number==4 || number==7 {GAME_SESSION.transition.elapsed=380;GAME_SESSION.tick(0,0,0);}else if number==5 {GAME_SESSION.tick(4096,0,0);GAME_SESSION.transition.elapsed=320;}else {GAME_SESSION.tick(buttons,held,elapsed);}");

        source
    })
}

pub(in super::super) fn fragile_representative(frozen_assets: bool) -> ApplicationCase {
    let mut case = edited("renderer_game_frames", move |mut source| {
        source = with_frame_number(source);
        source = source.required_replace("GAME_SESSION.begin();", "GAME_SESSION.begin();RENDER_STATE.hud.enabled=0;")
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;")
            .required_replace("GAME_SESSION.tick(buttons, held, elapsed);", "let stage:u16=if number==0 {7u16}else{if number==1 {14u16}else{if number==2 {16u16}else{if number==3 {29u16}else{0u16}}}};if GAME_SESSION.motion.level!=stage {GAME_SESSION.reset_level(stage);}GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;GAME_SESSION.tick(0,0,0);");

        source
    });

    case.assets_from_sources = frozen_assets;
    case
}

pub(in super::super) fn fragile_play(frozen_assets: bool) -> ApplicationCase {
    let mut case = edited("renderer_game_frames", move |mut source| {
        source = with_frame_number(source);
        source = source
            .required_replace("GAME_SESSION.begin();", "GAME_SESSION.begin();GAME_SESSION.reset_level(3);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.animation.held_lock=0;GAME_SESSION.animation.direction_stack=0;GAME_SESSION.transition.phase=TransitionPhase::Idle;RENDER_STATE.hud.enabled=0;")
            .required_replace("let elapsed = now - clock.previous;", "let elapsed:u16=0;")
            .required_replace("GAME_SESSION.tick(buttons, held, elapsed);", "if number==3 {GAME_SESSION.rules.x=3;GAME_SESSION.rules.z=0;GAME_SESSION.rules.orientation=1;GAME_SESSION.try_move(0);let mut ticks:u16=0;while ticks<5 {GAME_SESSION.tick(0,0,50);ticks+=1;}}else if number==4 {GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,50);GAME_SESSION.tick(0,0,40);}else if number==5 {GAME_SESSION.tick(32768,0,0);}else {GAME_SESSION.tick(buttons,held,elapsed);}");

        source
    });

    case.assets_from_sources = frozen_assets;
    case
}

pub(in super::super) fn fragile_ordinary() -> ApplicationCase {
    ApplicationCase {
        ..ApplicationCase::default()
    }
}

pub(in super::super) fn atlas_sizes(value: &str) -> (u16, u16) {
    let (main, marker) = value.split_once('x').expect("atlas dimensions");
    (main.parse().unwrap(), marker.parse().unwrap())
}
