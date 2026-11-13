//! The core SDK demonstration owns its pages, font styles and deterministic entry.
use crate::applications::{
    Assets, BuildOption, CapturePolicy, Definition, OptionValues, ResourceNames, DEFAULT,
    RENDERER_ASSETS,
};

pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "core-demo",
    selection: Some("core_demo"),
    source: Some("examples/core_demo/rcc/main.rs"),
    gpu_api: true,
    capture: CapturePolicy::Disabled,
    owner: "examples/core_demo/application.rs",
    options: &[
        BuildOption {
            flag: "page",
            variable: "CORE_DEMO_PAGE",
            values: OptionValues::Integer {
                minimum: 0,
                maximum: 4,
            },
        },
        BuildOption {
            flag: "frames",
            variable: "CORE_DEMO_FRAMES",
            values: OptionValues::Integer {
                minimum: 0,
                maximum: 65535,
            },
        },
    ],
    inputs: |_| {
        println!("cargo:rerun-if-env-changed=CORE_DEMO_PAGE");
        println!("cargo:rerun-if-env-changed=CORE_DEMO_FRAMES");
    },
    configure: |_, application, _| {
        for (name, maximum) in [("CORE_DEMO_PAGE", 4), ("CORE_DEMO_FRAMES", 65535)] {
            let value: u16 = std::env::var(name)
                .unwrap_or_else(|_| "0".into())
                .parse()
                .expect("numeric core demo option");
            assert!(value <= maximum, "invalid {name}");
            application
                .constants
                .push_str(&format!("pub const {name}:u16={value};\n"));
        }
    },
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
