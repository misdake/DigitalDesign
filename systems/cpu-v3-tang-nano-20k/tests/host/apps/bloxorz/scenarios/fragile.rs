//! Explicit fragile fixture inputs.
use super::*;

pub(in super::super) fn fragile_pad(
    atlas: Option<(u16, u16)>,
    frozen_assets: bool,
) -> ApplicationCase {
    let mut case = edited("renderer_game_frames", move |mut source| {
        source = source
            .required_replace("session_init();", "session_init();HUD[0u16]=0;")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .required_replace(
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
        source = source.required_replace("session_init();", "session_init();HUD[0u16]=0;")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .required_replace("session_tick(buttons, held, elapsed);", "let stage:u16=if number<2 || number==6 {3u16}else{if number==2 {29u16}else{if number==3 {14u16}else{if number==4 {16u16}else{7u16}}}};if MOTION[6u16]!=stage {level_reset(stage);}SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;if number==6 {GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;session_move(0);session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,10);}else{session_tick(0,0,0);}");

        source
    });
    case.atlas = atlas;
    case.assets_from_sources = frozen_assets;
    case
}

pub(in super::super) fn fragile_flow() -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        source = source.required_replace("session_init();", "session_init();level_reset(3);SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;TRANSITION[0u16]=0;")
            .required_replace("let fragile_owner = fragile_begin();", "let fragile_owner = fragile_begin();signal(14,fragile_owner.addr());")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .required_replace("session_tick(buttons, held, elapsed);", "if number==1 {GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;session_move(0);let mut ticks:u16=0;while ticks<5 {session_tick(0,0,50);ticks+=1;}}else if number==2 {session_tick(8,0,0);TRANSITION[1u16]=320;}else if number==3 || number==6 {TRANSITION[1u16]=80;session_tick(0,0,0);}else if number==4 || number==7 {TRANSITION[1u16]=380;session_tick(0,0,0);}else if number==5 {session_tick(4096,0,0);TRANSITION[1u16]=320;}else {session_tick(buttons,held,elapsed);}");

        source
    })
}

pub(in super::super) fn fragile_representative(frozen_assets: bool) -> ApplicationCase {
    let mut case = edited("renderer_game_frames", move |mut source| {
        source = source.required_replace("session_init();", "session_init();HUD[0u16]=0;")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .required_replace("session_tick(buttons, held, elapsed);", "let stage:u16=if number==0 {7u16}else{if number==1 {14u16}else{if number==2 {16u16}else{if number==3 {29u16}else{0u16}}}};if MOTION[6u16]!=stage {level_reset(stage);}SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;session_tick(0,0,0);");

        source
    });

    case.assets_from_sources = frozen_assets;
    case
}

pub(in super::super) fn fragile_play(frozen_assets: bool) -> ApplicationCase {
    let mut case = edited("renderer_game_frames", move |mut source| {
        source = source
            .required_replace("session_init();", "session_init();level_reset(3);SESSION[0u16]=PLAY;SESSION[1u16]=0;SESSION[3u16]=0;SESSION[8u16]=0;TRANSITION[0u16]=0;HUD[0u16]=0;")
            .required_replace("let elapsed = now - previous;", "let elapsed:u16=0;")
            .required_replace("session_tick(buttons, held, elapsed);", "if number==3 {GAME[0u16]=3;GAME[1u16]=0;GAME[2u16]=1;session_move(0);let mut ticks:u16=0;while ticks<5 {session_tick(0,0,50);ticks+=1;}}else if number==4 {session_tick(0,0,50);session_tick(0,0,50);session_tick(0,0,40);}else if number==5 {session_tick(32768,0,0);}else {session_tick(buttons,held,elapsed);}");

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
