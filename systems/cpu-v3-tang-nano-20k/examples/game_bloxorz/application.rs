//! Bloxorz binds the game entry point to its shared modules and assets.
use crate::applications::{
    Assets, BuildOption, Definition, OptionValues, DEFAULT, RENDERER_ASSETS,
};
pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "game-bloxorz",
    selection: Some("gpu_renderer_bloxorz"),
    source: Some("examples/game_bloxorz/rcc/main.rs"),
    previous_source: Some("examples/gpu_renderer_bloxorz/rcc/main.rs"),
    gpu_api: true,
    ordinary_sources: &["applications/bloxorz/rcc"],
    owner: "examples/game_bloxorz/application.rs",
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
        script: "examples/game_bloxorz/tools/assets.py",
        production_surface: true,
        manifest_sources: true,
        inputs: &[
            "tools/gpu_texture_layout.py",
            "applications/rendering/tools/minimal_assets.py",
            "applications/bloxorz/tools/batch-scene.py",
            "applications/bloxorz/tools/static-scene.py",
            "applications/bloxorz/assets/reference.txt",
            "applications/bloxorz/geometry.rs",
            "examples/demo_box_rabbit/tools/assets.py",
        ],
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
}];
