use crate::applications::{Assets, Definition, ResourceNames, DEFAULT, RENDERER_ASSETS};

pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "demo-billboard",
    selection: Some("tree_explosion"),
    source: Some("examples/demo_billboard/rcc/main.rs"),
    previous_source: Some("examples/tree_explosion/rcc/main.rs"),
    gpu_api: true,
    owner: "examples/demo_billboard/application.rs",
    assets: Some(Assets {
        script: "examples/demo_billboard/tools/assets.py",
        directory: "tree-explosion-assets",
        constants: "tree_explosion_assets.rs",
        repack: "tree-explosion-repack.conf",
        names: ResourceNames::File("SCENE_ASSET_"),
        manifest_sources: true,
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
}];
