//! Benchmark owns its scene-dependent source inputs.
use crate::applications::{
    Assets, BuildOption, Definition, OptionValues, ResourceNames, DEFAULT, RENDERER_ASSETS,
};
pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "bench-gpu",
    selection: Some("gpu_bench"),
    source: Some("examples/bench_gpu/rcc/main.rs"),
    previous_source: Some("examples/gpu_bench/rcc/main.rs"),
    owner: "examples/bench_gpu/application.rs",
    options: &[BuildOption::environment(
        "scene",
        "GPU_BENCH_SCENE",
        OptionValues::Choices(&["micro", "full"]),
    )],
    inputs: |context| {
        if std::env::var("GPU_BENCH_SCENE").as_deref() == Ok("full") {
            context.inputs(&[
                "examples/demo_box_rabbit/tools/assets.py",
                "tools/gpu_mesh_layout.py",
                "examples/demo_box_rabbit/assets/scene.json",
                "../../ip/gpu-v2/examples/vliw_integration/material_scene.py",
                "../../ip/gpu-v2/examples/vliw_integration/assets/room-tiles.png",
            ]);
        }
    },
    assets: Some(Assets {
        script: "examples/bench_gpu/tools/assets.py",
        directory: "gpu-bench-assets",
        constants: "bench_assets.rs",
        repack: "bench-repack.conf",
        names: ResourceNames::File("GPU_"),
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
}];
