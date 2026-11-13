//! Benchmark owns its scene-dependent source inputs.
use crate::applications::{
    Assets, BuildOption, Definition, OptionValues, ResourceNames, DEFAULT, RENDERER_ASSETS,
};
pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "gpu-bench",
    selection: Some("gpu_bench"),
    source: Some("examples/gpu_bench/rcc/main.rs"),
    owner: "examples/gpu_bench/application.rs",
    options: &[BuildOption {
        flag: "scene",
        variable: "GPU_BENCH_SCENE",
        values: OptionValues::Choices(&["micro", "full"]),
    }],
    inputs: |context| {
        println!("cargo:rerun-if-env-changed=GPU_BENCH_SCENE");
        if std::env::var("GPU_BENCH_SCENE").as_deref() == Ok("full") {
            context.inputs(&[
                "examples/gpu_demo/tools/assets.py",
                "tools/gpu_mesh_layout.py",
                "examples/gpu_demo/assets/scene.json",
                "../../ip/gpu-v2/examples/vliw_integration/material_scene.py",
                "../../ip/gpu-v2/examples/vliw_integration/assets/room-tiles.png",
            ]);
        }
    },
    assets: Some(Assets {
        script: "examples/gpu_bench/tools/assets.py",
        directory: "gpu-bench-assets",
        constants: "bench_assets.rs",
        repack: "bench-repack.conf",
        names: ResourceNames::File("GPU_"),
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
}];
