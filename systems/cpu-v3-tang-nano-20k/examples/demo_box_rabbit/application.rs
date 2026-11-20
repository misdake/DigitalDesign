//! Public API applications using the classic controller scene.
use crate::applications::{Assets, Definition, DEFAULT, RENDERER_ASSETS};
pub const APPLICATIONS: &[Definition] = &[
    Definition {
        name: "demo-rgba4444",
        selection: Some("gpu_demo_rgba4444"),
        source: Some("examples/demo_box_rabbit/rcc/rgba_main.rs"),
        previous_source: Some("examples/gpu_demo/rcc/rgba_main.rs"),
        gpu_api: true,
        owner: "examples/demo_box_rabbit/application.rs",
        assets: Some(Assets {
            script: "examples/demo_box_rabbit/tools/rgba_assets.py",
            directory: "rgba4444-assets",
            manifest_sources: true,
            inputs: &[
                "examples/demo_box_rabbit/tools/public_assets.py",
                "examples/demo_box_rabbit/tools/assets.py",
            ],
            ..RENDERER_ASSETS
        }),
        ..DEFAULT
    },
    Definition {
        name: "demo-box-rabbit",
        selection: Some("gpu_demo_api_column16"),
        source: Some("examples/demo_box_rabbit/rcc/public_main.rs"),
        previous_source: Some("examples/gpu_demo/rcc/public_main.rs"),
        gpu_api: true,
        owner: "examples/demo_box_rabbit/application.rs",
        assets: Some(Assets {
            script: "examples/demo_box_rabbit/tools/public_assets.py",
            manifest_sources: true,
            inputs: &["examples/demo_box_rabbit/tools/assets.py"],
            ..RENDERER_ASSETS
        }),
        ..DEFAULT
    },
];
