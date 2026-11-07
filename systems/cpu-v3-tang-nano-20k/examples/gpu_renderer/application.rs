//! Renderer variants share the same asset producer and public API policy.
use crate::applications::{Assets, Definition, DEFAULT, RENDERER_ASSETS};
const RENDERER: Definition = Definition {
    name: "renderer",
    selection: Some("gpu_renderer"),
    source: Some("examples/gpu_renderer/rcc/main.rs"),
    gpu_api: true,
    assets: Some(Assets {
        inputs: &["examples/gpu_demo/assets/font-8x16.txt"],
        ..RENDERER_ASSETS
    }),
    owner: "examples/gpu_renderer/application.rs",
    ..DEFAULT
};
pub const APPLICATIONS: &[Definition] = &[
    RENDERER,
    Definition {
        name: "renderer-changes",
        selection: Some("gpu_renderer_changes"),
        source: Some("examples/gpu_renderer/rcc/changes.rs"),
        ..RENDERER
    },
    Definition {
        name: "renderer-members",
        selection: Some("gpu_renderer_members"),
        source: Some("examples/gpu_renderer/rcc/members.rs"),
        ..RENDERER
    },
];
