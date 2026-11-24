//! Explicit timing fixture inputs.
use super::*;

pub(in super::super) fn cpu_shadow() -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        source = with_frame_number(source);
        source = source
            .required_replace(
                "GAME_SESSION.begin();",
                "GAME_SESSION.begin();GAME_SESSION.reset_level(28);GAME_SESSION.animation.phase=PLAY;GAME_SESSION.animation.elapsed=0;RENDER_STATE.hud.enabled=0;",
            )
            .required_replace(
                "GAME_SESSION.tick(buttons, held, elapsed);",
                "GAME_SESSION.tick(buttons, held, if number==2 {17u16}else{0u16});",
            );

        source
    })
}

pub(in super::super) fn moving() -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        source = source.required_replace(
            "GAME_SESSION.begin();",
            "GAME_SESSION.begin();GAME_SESSION.reset_level(28);GAME_SESSION.animation.phase=PLAY;GAME_SESSION.animation.elapsed=0;RENDER_STATE.hud.enabled=0;",
        );
        // Physical Device5 input and the real millisecond clock drive the roll.

        source
    })
}

pub(in super::super) fn render_time(fade: bool) -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = stage_source(source, 0, false, false, true);

        source = source.required_replace(
            "GAME_SESSION.begin();",
            if fade {
                "GAME_SESSION.begin();RENDER_STATE.hud.enabled=1;GAME_SESSION.transition.phase=FADE_OUT;GAME_SESSION.transition.elapsed=160;"
            } else {
                "GAME_SESSION.begin();RENDER_STATE.hud.enabled=1;"
            },
        );
        source = source.required_replace("run(3);", "run(4);");

        source
    })
}

pub(in super::super) fn cpu_cost(legacy: bool) -> ApplicationCase {
    let case = edited("renderer_game_three_frames", move |mut source| {
        source = stage_source(source, 0, false, false, true);

        assert_eq!(source.matches("GAME_SESSION.reset_level(0);").count(), 1);
        source = source.required_replace(
            "GAME_SESSION.reset_level(0);",
            "GAME_SESSION.reset_level(7);",
        );
        let mode = if legacy { 2 } else { 1 };
        source = source.required_replace(
            "pub fn renderer_game_three_frames() {",
            &format!("pub fn renderer_game_three_frames() {{unsafe{{addr_of(&GROUND_BATCH_MODE).write(0,{mode});}}"),
        );
        source = source.required_replace(
            "ground_rebuild();",
            "signal(9,0);ground_rebuild();signal(10,0);",
        );

        source
    });

    case
}
