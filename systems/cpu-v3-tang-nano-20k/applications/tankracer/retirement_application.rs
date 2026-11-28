//! Ordinary scene lifetime through the normal installer and immutable assets.
use crate::applications::{Assets, BuildOption, Definition, OptionValues, ResourceNames, DEFAULT};
pub const APPLICATION: Definition = Definition {
    name: "bench-tankracer-retirement",
    selection: Some("tankracer_retirement"),
    source: Some("applications/tankracer/retirement/main.rs"),
    owner: "applications/tankracer/retirement_application.rs",
    gpu_api: true,
    ordinary_sources: &["applications/tankracer/rcc", "applications/tankracer/tools"],
    options: &[BuildOption::environment(
        "reference",
        "TANKRACER_REFERENCE",
        OptionValues::InputFile,
    )],
    inputs: |context| {
        context.inputs(&[
            "applications/tankracer/tools/scene_lifecycle_oracle.rs",
            "applications/tankracer/tools/scene_lifecycle_assets.rs",
            "applications/tankracer/tools/scene_lifecycle_case.rs",
            "applications/tankracer/tools/identity_assets.rs",
            "applications/tankracer/tools/identity_capture.rs",
            "applications/tankracer/tools/reference_bridge.py",
            "applications/tankracer/tools/reference_scene.rs",
            "applications/tankracer/tools/reference_tanks.rs",
            "applications/tankracer/tools/reference_world.rs",
            "applications/tankracer/tools/scene_export.rs",
        ]);
        let path = std::path::PathBuf::from(
            std::env::var("TANKRACER_REFERENCE").expect("retirement reference"),
        );
        (context.watch)(&path);
        let root = path.parent().unwrap();
        for name in [
            "crates/game/src",
            "crates/assets/src",
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
        script: "applications/tankracer/tools/retirement_assets.py",
        directory: "tankracer-retirement-assets",
        constants: "retirement_assets.rs",
        repack: "retirement-repack.conf",
        names: ResourceNames::File("RETIREMENT_"),
        inputs: &[],
        production_surface: false,
        manifest_sources: false,
    }),
    configure: |context, application, _| {
        application
            .source
            .ordinary_dirs
            .insert(0, context.output.join("tankracer-retirement-assets"));
    },
    ..DEFAULT
};
