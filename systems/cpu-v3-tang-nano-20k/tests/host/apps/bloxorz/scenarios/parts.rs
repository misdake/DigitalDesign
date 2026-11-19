//! Explicit parts fixture inputs.
use super::*;

pub(in super::super) fn button_parts(fresh: bool) -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        source = with_frame_number(source);
        source = source
            .required_replace(
                "session_init();",
                "session_init();level_reset(28);SESSION[0u16]=PLAY;SESSION[1u16]=0;HUD[0u16]=0;",
            )
            .required_replace(
                "session_tick(buttons, held, elapsed);",
                "button_test_pose(number);",
            )
            .required_replace(
                "pass.begin_prepared_part(part);",
                "if part>0 && part<14 {signal(14,part);}pass.begin_prepared_part(part);",
            );
        if fresh {
            source = source.required_replace(
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

        source
    })
}

pub(in super::super) fn static_cache(level: u16) -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        assert!((1..=33).contains(&level));
        source = source.required_replace(
            "session_init();",
            &format!(
                "session_init();level_reset({});SESSION[0u16]=PLAY;SESSION[1u16]=0;HUD[0u16]=0;",
                level - 1
            ),
        );
        source = source.required_replace(
            "session_tick(buttons, held, elapsed);",
            "session_tick(buttons, held, 0);",
        );

        source
    })
}

pub(in super::super) fn cache_level() -> u16 {
    let level = std::env::var("BLOXORZ_CACHE_LEVEL")
        .unwrap_or_else(|_| "29".into())
        .parse()
        .unwrap();
    assert!((1..=33).contains(&level));
    level
}
