//! Diagnostic profiler owns its manifest-selected short cases.
use crate::applications::{
    Assets, CapturePolicy, Definition, SourceSpecialization, DEFAULT, RENDERER_ASSETS,
};
pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "gpu-profiler",
    selection: Some("gpu_profiler_board"),
    source: Some("examples/gpu_profiler_board/rcc/main.rs"),
    gpu_api: true,
    capture: CapturePolicy::Diagnostic,
    owner: "examples/gpu_profiler_board/application.rs",
    assets: Some(Assets {
        script: "examples/gpu_profiler_board/tools/assets.py",
        inputs: &["examples/gpu_demo/assets/font-8x16.txt"],
        ..RENDERER_ASSETS
    }),
    configure: |context, application, manifest| match manifest["profiler_short_case"]
        .as_str()
        .unwrap_or("none")
    {
        "none" => {}
        "rtt" => {
            context.production_surface(manifest);
            application.project.application = "examples/gpu_profiler_board/rcc/rtt.rs".into();
            application.source.ordinary_dirs =
                vec![context.root.join("examples/gpu_renderer_rtt/rcc")];
            context.inputs(&[
                "tools/gpu_texture_layout.py",
                "examples/gpu_renderer/tools/assets.py",
            ]);
        }
        "memcpy" => application.source.specialization = SourceSpecialization::ProfilerMemcpy,
        _ => panic!("unsupported profiler short case"),
    },
    ..DEFAULT
}];
