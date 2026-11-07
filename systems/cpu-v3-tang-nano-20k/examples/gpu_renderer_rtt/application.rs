//! Render-to-texture variants require the production column surface.
use crate::applications::{Assets, Definition, DEFAULT, RENDERER_ASSETS};
const RTT: Definition = Definition {
    name: "rtt",
    selection: Some("gpu_renderer_rtt"),
    source: Some("examples/gpu_renderer_rtt/rcc/main.rs"),
    gpu_api: true,
    owner: "examples/gpu_renderer_rtt/application.rs",
    assets: Some(Assets {
        script: "examples/gpu_renderer_rtt/tools/assets.py",
        production_surface: true,
        inputs: &[
            "tools/gpu_texture_layout.py",
            "examples/gpu_renderer/tools/assets.py",
            "examples/gpu_demo/assets/font-8x16.txt",
        ],
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
};
pub const APPLICATIONS: &[Definition] = &[
    RTT,
    Definition {
        name: "rtt-bilinear",
        selection: Some("gpu_renderer_rtt_bilinear"),
        source: Some("examples/gpu_renderer_rtt/rcc/bilinear.rs"),
        ..RTT
    },
];
