//! Complete initial ENGLAND collision metadata through normal immutable assets.
use crate::applications::{Assets, BuildOption, Definition, OptionValues, ResourceNames, DEFAULT};
pub const APPLICATION: Definition = Definition {
    name: "bench-tankracer-scene",
    selection: Some("tankracer_scene"),
    source: Some("applications/tankracer/scene/main.rs"),
    owner: "applications/tankracer/scene_application.rs",
    gpu_api: true,
    ordinary_sources: &["applications/tankracer/rcc", "applications/tankracer/scene"],
    options: &[
        BuildOption::environment("reference", "TANKRACER_REFERENCE", OptionValues::InputFile),
        BuildOption::integer_constant("samples", "SCENE_QUERY_LIMIT", 0, 0, 1200),
    ],
    inputs: |context| {
        context.inputs(&[
            "applications/tankracer/tools/scene_assets.rs",
            "applications/tankracer/tools/scene_export.rs",
            "applications/tankracer/tools/scene_hits.rs",
            "applications/tankracer/tools/scene_bounce.rs",
            "applications/tankracer/tools/terrain_assets.rs",
            "applications/tankracer/tools/export_trig_assets.py",
            "applications/tankracer/rcc/trig_asset_lookup.rs",
        ]);
        let path = std::path::PathBuf::from(
            std::env::var("TANKRACER_REFERENCE").expect("scene reference"),
        );
        (context.watch)(&path);
        let root = path.parent().unwrap();
        for name in [
            "crates/game/src",
            "crates/assets/src",
            "src/frame_math.c",
            "src/frame_math_tables.h",
            "assets/game/DATA/FASTFILE.DAT",
            "assets/game/INGAME/FASTFILE.DAT",
            "assets/game/TANKS/BRONZE/FASTFILE.DAT",
            "assets/game/TANKS/SILVER/FASTFILE.DAT",
            "assets/game/TANKS/GOLD/FASTFILE.DAT",
            "assets/game/LEVELS/ENGLAND/FASTFILE.DAT",
        ] {
            (context.watch)(&root.join(name));
        }
    },
    assets: Some(Assets {
        script: "applications/tankracer/tools/scene_assets.py",
        directory: "tankracer-scene-assets",
        constants: "scene_assets.rs",
        repack: "scene-repack.conf",
        names: ResourceNames::File("SCENE_"),
        inputs: &[],
        production_surface: false,
        manifest_sources: false,
    }),
    configure: |context, application, _| {
        application
            .source
            .ordinary_dirs
            .insert(0, context.output.join("tankracer-scene-assets"));
    },
    ..DEFAULT
};
