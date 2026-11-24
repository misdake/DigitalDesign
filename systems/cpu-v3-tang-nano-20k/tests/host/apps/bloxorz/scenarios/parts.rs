//! Explicit parts fixture inputs.
use super::*;

pub(in super::super) fn button_parts(fresh: bool) -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        source = with_frame_number(source);
        source = source
            .required_replace(
                "GAME_SESSION.begin();",
                "GAME_SESSION.begin();GAME_SESSION.reset_level(28);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;RENDER_STATE.hud.enabled=0;",
            )
            .required_replace(
                "GAME_SESSION.tick(buttons, held, elapsed);",
                "button_test_pose(number);",
            )
            .required_replace(
                "let mut recording = parts[part].record();",
                "if part>0 && part<14 {signal(14,part);}let mut recording = parts[part].record();",
            );
        if fresh {
            source = source.required_replace(
                "sync_bridge_group(parts.as_array());",
                "sync_bridge_group(parts.as_array());let mut i:u16=0;while i<16{let p=parts[i];p.invalidate();i+=1;}",
            );
        }
        source += r#"
fn button_test_pose(n:u16){
 if n==9 || n==14 {GAME_SESSION.reset_level(32);GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;GAME_SESSION.rules.bridge_mask=0;}
 if n==0 {GAME_SESSION.rules.bridge_mask=0;}
 let mut i:u16=0;let mut bit:u16=0;
 while i<GAME_SESSION.level.cell_count && bit==0 {let k=GAME_SESSION.level_field(i,0);if k==3 || k==4 {bit=GAME_SESSION.level_field(i,1);}i+=1;}
 if n==2 || n==11 {GAME_SESSION.rules.bridge_mask=bit;}
 if n==16 {
  let mut cell:u16=0;while GAME_SESSION.level_field(cell,0)!=3 {cell+=1;}
  GAME_SESSION.level_words()[9+cell*2]=(GAME_SESSION.level_words()[9+cell*2]&4095)|32768;
  GAME_SESSION.generation+=1;GAME_SESSION.rules.bridge_mask=256;
 }
 if n==7 {GAME_SESSION.rules.bridge_mask=0;}
 GAME_SESSION.tick(0,0,if n==3 || n==12 || n==17 {50u16}else{if n==5 {210u16}else{0u16}});
}
"#;

        source
    })
}

pub(in super::super) fn static_cache(level: u16) -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        assert!((1..=33).contains(&level));
        source = source.required_replace(
            "GAME_SESSION.begin();",
            &format!(
                "GAME_SESSION.begin();GAME_SESSION.reset_level({});GAME_SESSION.animation.phase=AnimationPhase::Play;GAME_SESSION.animation.elapsed=0;RENDER_STATE.hud.enabled=0;",
                level - 1
            ),
        );
        source = source.required_replace(
            "GAME_SESSION.tick(buttons, held, elapsed);",
            "GAME_SESSION.tick(buttons, held, 0);",
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
