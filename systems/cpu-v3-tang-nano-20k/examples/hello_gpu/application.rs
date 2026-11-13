//! A copyable first application: one colored triangle and the shared project font.
use crate::applications::{
    Assets, BuildOption, Definition, ResourceNames, DEFAULT, RENDERER_ASSETS,
};

pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "hello-gpu",
    selection: Some("hello_gpu"),
    source: Some("examples/hello_gpu/rcc/main.rs"),
    gpu_api: true,
    owner: "examples/hello_gpu/application.rs",
    options: &[BuildOption::integer_constant(
        "frames",
        "HELLO_FRAMES",
        0,
        0,
        65535,
    )],
    assets: Some(Assets {
        script: "examples/hello_gpu/tools/assets.py",
        directory: "hello-gpu-assets",
        constants: "hello_assets.rs",
        repack: "hello-repack.conf",
        names: ResourceNames::Fixed("HELLO_ATLAS"),
        manifest_sources: true,
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
}];
