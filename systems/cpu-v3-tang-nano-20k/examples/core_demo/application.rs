//! The core SDK demonstration owns its pages, font styles and deterministic entry.
use crate::applications::{
    Assets, BuildOption, Definition, ResourceNames, DEFAULT, RENDERER_ASSETS,
};

pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "core-demo",
    selection: Some("core_demo"),
    source: Some("examples/core_demo/rcc/main.rs"),
    gpu_api: true,
    owner: "examples/core_demo/application.rs",
    options: &[
        BuildOption::integer_constant("page", "CORE_DEMO_PAGE", 0, 0, 6),
        BuildOption::integer_constant("frames", "CORE_DEMO_FRAMES", 0, 0, 65535),
    ],
    assets: Some(Assets {
        script: "examples/core_demo/tools/assets.py",
        directory: "core-demo-assets",
        constants: "core_assets.rs",
        repack: "core-demo-repack.conf",
        names: ResourceNames::Fixed("CORE_ATLAS"),
        manifest_sources: true,
        inputs: &["tools/gpu_layout.py"],
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
}];
