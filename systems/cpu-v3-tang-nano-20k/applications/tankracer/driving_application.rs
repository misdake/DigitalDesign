//! Persistent real-map single-car qualification through the normal application installer.
use crate::applications::{Assets, BuildOption, Definition, OptionValues, ResourceNames, DEFAULT};
pub const APPLICATION: Definition = Definition {
    name: "bench-tankracer-driving",
    selection: Some("tankracer_driving"),
    source: Some("applications/tankracer/driving/main.rs"),
    owner: "applications/tankracer/driving_application.rs",
    gpu_api: true,
    ordinary_sources: &["applications/tankracer/rcc", "applications/tankracer/tools"],
    options: &[
        BuildOption::environment("reference", "TANKRACER_REFERENCE", OptionValues::InputFile),
        BuildOption::integer_constant("steps", "DRIVING_STEPS", 48, 1, 96),
    ],
    inputs: |context| {
        context.inputs(&[
            "applications/tankracer/tools/driving_oracle.rs",
            "applications/tankracer/tools/driving_ground.rs",
            "applications/tankracer/tools/driving_capture.rs",
            "applications/tankracer/tools/paired_driving_oracle.rs",
            "applications/tankracer/tools/scene_export.rs",
            "applications/tankracer/tools/deform_assets.rs",
            "applications/tankracer/tools/reference_world.rs",
            "applications/tankracer/tools/reference_bridge.py",
            "applications/tankracer/tools/reference_tanks.rs",
            "applications/tankracer/tools/reference_car.rs",
            "applications/tankracer/tools/impact_snapshot.rs",
            "applications/tankracer/tools/vehicle_oracle.rs",
            "applications/tankracer/tools/drive_oracle.rs",
            "applications/tankracer/tools/terrain_assets.rs",
            "applications/tankracer/tools/export_trig_assets.py",
            "applications/tankracer/rcc/trig_asset_lookup.rs",
        ]);
        let path = std::path::PathBuf::from(
            std::env::var("TANKRACER_REFERENCE").expect("driving reference"),
        );
        (context.watch)(&path);
        let root = path.parent().unwrap();
        for name in [
            "crates/game/src",
            "crates/assets/src",
            "assets/game/DATA/FASTFILE.DAT",
            "assets/game/LEVELS/ENGLAND/FASTFILE.DAT",
            "assets/game/TANKS/BRONZE/FASTFILE.DAT",
            "assets/game/TANKS/SILVER/FASTFILE.DAT",
            "assets/game/TANKS/GOLD/FASTFILE.DAT",
            "assets/game/INGAME/FASTFILE.DAT",
        ] {
            (context.watch)(&root.join(name));
        }
    },
    assets: Some(Assets {
        script: "applications/tankracer/tools/driving_assets.py",
        directory: "tankracer-driving-assets",
        constants: "driving_assets.rs",
        repack: "driving-repack.conf",
        names: ResourceNames::File("DRIVING_"),
        inputs: &[],
        production_surface: false,
        manifest_sources: false,
    }),
    configure: |context, application, _| {
        application
            .source
            .ordinary_dirs
            .insert(0, context.output.join("tankracer-driving-assets"));
    },
    ..DEFAULT
};
