//! Bloxorz owns ordinary/capture entry points and shared game modules.
use crate::applications::{
    Assets, BuildOption, Definition, OptionValues, DEFAULT, RENDERER_ASSETS,
};
const GAME: Definition = Definition {
    name: "bloxorz",
    selection: Some("gpu_renderer_bloxorz"),
    source: Some("examples/gpu_renderer_bloxorz/rcc/main.rs"),
    gpu_api: true,
    ordinary_sources: &["applications/bloxorz/rcc"],
    owner: "examples/gpu_renderer_bloxorz/application.rs",
    options: &[
        BuildOption::environment(
            "start-level",
            "BLOXORZ_START_LEVEL",
            OptionValues::Integer {
                minimum: 1,
                maximum: 33,
            },
        ),
        BuildOption::environment(
            "start-phase",
            "BLOXORZ_START_PHASE",
            OptionValues::Choices(&["entry", "play"]),
        ),
    ],
    assets: Some(Assets {
        script: "examples/gpu_renderer_bloxorz/tools/assets.py",
        production_surface: true,
        manifest_sources: true,
        inputs: &[
            "tools/gpu_texture_layout.py",
            "examples/gpu_renderer/tools/assets.py",
            "applications/bloxorz/tools/batch-scene.py",
            "applications/bloxorz/tools/static-scene.py",
            "applications/bloxorz/assets/reference.txt",
            "applications/bloxorz/geometry.rs",
            "examples/gpu_demo/tools/assets.py",
        ],
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
};
pub const APPLICATIONS: &[Definition] = &[
    GAME,
    Definition {
        name: "bloxorz-capture",
        selection: Some("gpu_renderer_bloxorz_capture"),
        entry: "renderer_game_capture",
        ..GAME
    },
    Definition {
        name: "bloxorz-capture-full",
        selection: Some("gpu_renderer_bloxorz_capture_full"),
        entry: "renderer_game_capture_full",
        ..GAME
    },
];
