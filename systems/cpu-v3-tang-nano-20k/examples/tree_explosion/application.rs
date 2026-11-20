use crate::applications::{Assets, Definition, ResourceNames, DEFAULT, RENDERER_ASSETS};

pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "tree-explosion",
    selection: Some("tree_explosion"),
    source: Some("examples/tree_explosion/rcc/main.rs"),
    gpu_api: true,
    owner: "examples/tree_explosion/application.rs",
    assets: Some(Assets {
        script: "examples/tree_explosion/tools/assets.py",
        directory: "tree-explosion-assets",
        constants: "tree_explosion_assets.rs",
        repack: "tree-explosion-repack.conf",
        names: ResourceNames::File("SCENE_ASSET_"),
        manifest_sources: true,
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
}];
