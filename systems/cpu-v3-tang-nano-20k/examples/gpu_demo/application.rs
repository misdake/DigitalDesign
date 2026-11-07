//! Demo owns both legacy layouts and the ordinary public-API scene.
use crate::applications::{
    Assets, CapturePolicy, Definition, ResourceNames, DEFAULT, RENDERER_ASSETS,
};
const DEMO: Definition = Definition {
    name: "demo",
    selection: Some("gpu_demo"),
    source: Some("examples/gpu_demo/rcc/main.rs"),
    owner: "examples/gpu_demo/application.rs",
    assets: Some(Assets {
        script: "examples/gpu_demo/tools/assets.py",
        directory: "gpu-demo-assets",
        constants: "demo_assets.rs",
        repack: "demo-repack.conf",
        names: ResourceNames::File(""),
        inputs: &[
            "examples/gpu_demo/assets/font-8x16.txt",
            "examples/gpu_demo/assets/scene.json",
            "../../ip/gpu-v2/examples/vliw_integration/solid_scene.py",
            "../../ip/gpu-v2/examples/vliw_integration/material_scene.py",
            "../../ip/gpu-v2/examples/vliw_integration/assets/room-tiles.png",
        ],
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
};
pub const APPLICATIONS: &[Definition] = &[
    DEMO,
    Definition {
        name: "demo-column16",
        selection: Some("gpu_demo_column16"),
        configure: |context, application, _| {
            context.copy_asset(application, "rcc/gpu_layout.rs", "gpu-column-layout.rs")
        },
        ..DEMO
    },
    Definition {
        name: "handheld",
        selection: Some("gpu_demo_api_column16"),
        source: Some("examples/gpu_demo/rcc/public_main.rs"),
        gpu_api: true,
        capture: CapturePolicy::Disabled,
        owner: "examples/gpu_demo/application.rs",
        assets: Some(Assets {
            script: "examples/gpu_demo/tools/public_assets.py",
            manifest_sources: true,
            inputs: &[
                "examples/gpu_demo/tools/assets.py",
                "examples/gpu_demo/assets/font-8x16.txt",
            ],
            ..RENDERER_ASSETS
        }),
        ..DEFAULT
    },
];
