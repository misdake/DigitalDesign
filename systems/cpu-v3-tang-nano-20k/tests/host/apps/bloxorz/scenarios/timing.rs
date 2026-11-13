//! Explicit timing fixture inputs.
use super::*;

pub(in super::super) fn cpu_shadow() -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        source = source
            .required_replace(
                "session_init();",
                "session_init();level_reset(28);SESSION[0u16]=PLAY;SESSION[1u16]=0;HUD[0u16]=0;",
            )
            .required_replace(
                "session_tick(buttons, held, elapsed);",
                "session_tick(buttons, held, if number==2 {17u16}else{0u16});",
            );

        source
    })
}

pub(in super::super) fn moving() -> ApplicationCase {
    edited("renderer_game_frames", move |mut source| {
        source = source.required_replace(
            "session_init();",
            "session_init();level_reset(28);SESSION[0u16]=PLAY;SESSION[1u16]=0;HUD[0u16]=0;",
        );
        // Physical Device5 input and the real millisecond clock drive the roll.

        source
    })
}

pub(in super::super) fn render_time(fade: bool) -> ApplicationCase {
    edited("renderer_game_three_frames", move |mut source| {
        source = stage_source(source, 0, false, false, true);

        source = source.required_replace(
            "session_init();",
            if fade {
                "session_init();HUD[0u16]=1;TRANSITION[0u16]=FADE_OUT;TRANSITION[1u16]=160;"
            } else {
                "session_init();HUD[0u16]=1;"
            },
        );
        assert_eq!(
            source
                .matches("render_start = time_milliseconds();")
                .count(),
            1
        );
        source = source.required_replace(
            "render_start = time_milliseconds();",
            "signal(8,number);render_start = time_milliseconds();",
        );

        source
    })
}

pub(in super::super) fn cpu_cost(legacy: bool) -> ApplicationCase {
    let case = edited("renderer_game_three_frames", move |mut source| {
        source = stage_source(source, 0, false, false, true);

        assert_eq!(source.matches("level_reset(0);").count(), 1);
        source = source.required_replace("level_reset(0);", "level_reset(7);");
        let mode = if legacy { 2 } else { 1 };
        source = source.required_replace(
            "pub fn renderer_game_three_frames() {",
            &format!("pub fn renderer_game_three_frames() {{GROUND_BATCH_MODE[0u16]={mode};"),
        );
        source = source.required_replace(
            "ground_rebuild();",
            "signal(9,0);ground_rebuild();signal(10,0);",
        );

        source
    });

    case
}
