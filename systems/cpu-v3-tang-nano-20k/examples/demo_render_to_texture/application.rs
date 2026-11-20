//! Render-to-texture variants require the production column surface.
use crate::applications::{Assets, Definition, DEFAULT, RENDERER_ASSETS};
const RTT: Definition = Definition {
    name: "demo-render-to-texture",
    selection: Some("gpu_renderer_rtt"),
    source: Some("examples/demo_render_to_texture/rcc/main.rs"),
    previous_source: Some("examples/gpu_renderer_rtt/rcc/main.rs"),
    gpu_api: true,
    owner: "examples/demo_render_to_texture/application.rs",
    assets: Some(Assets {
        script: "examples/demo_render_to_texture/tools/assets.py",
        production_surface: true,
        manifest_sources: true,
        inputs: &[
            "tools/gpu_texture_layout.py",
            "applications/rendering/tools/minimal_assets.py",
        ],
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
};
pub const APPLICATIONS: &[Definition] = &[
    RTT,
    Definition {
        name: "demo-render-to-texture-bilinear",
        selection: Some("gpu_renderer_rtt_bilinear"),
        source: Some("examples/demo_render_to_texture/rcc/bilinear.rs"),
        previous_source: Some("examples/gpu_renderer_rtt/rcc/bilinear.rs"),
        ..RTT
    },
];
