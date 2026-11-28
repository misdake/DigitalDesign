//! Authored mesh fragments through the normal installer and table windows.
use crate::applications::{Assets, BuildOption, DEFAULT, Definition, OptionValues, ResourceNames};
pub const APPLICATION: Definition = Definition {
    name: "bench-tankracer-fragments",
    selection: Some("tankracer_fragments"),
    source: Some("applications/tankracer/fragments/main.rs"),
    owner: "applications/tankracer/fragments_application.rs",
    gpu_api: true,
    ordinary_sources: &["applications/tankracer/rcc", "applications/tankracer/tools"],
    options: &[
        BuildOption::environment("reference", "TANKRACER_REFERENCE", OptionValues::InputFile),
        BuildOption::environment(
            "scope",
            "TANKRACER_FRAGMENT_SCOPE",
            OptionValues::Choices(&["full", "parents", "residency", "storage"]),
        ),
    ],
    inputs: |context| {
        context.inputs(&[
            "applications/tankracer/tools/mesh_break_assets.py",
            "applications/tankracer/tools/mesh_break_oracle.rs",
            "applications/tankracer/tools/fragment_snapshot.rs",
            "applications/tankracer/tools/mesh_break_case.rs",
            "applications/tankracer/tools/mesh_store_assets.py",
            "applications/tankracer/tools/mesh_store_oracle.rs",
            "applications/tankracer/tools/mesh_store_case.rs",
            "applications/tankracer/tools/mesh_store_snapshot.rs",
            "applications/tankracer/tools/mesh_catalog_assets.rs",
            "applications/tankracer/tools/mesh_catalog_case.rs",
            "applications/tankracer/tools/auxiliary_hit_assets.py",
            "applications/tankracer/tools/auxiliary_hit_oracle.rs",
            "applications/tankracer/tools/auxiliary_hit_case.rs",
            "applications/tankracer/tools/terrain_assets.rs",
            "applications/tankracer/tools/scene_fragment_assets.py",
            "applications/tankracer/tools/scene_fragment_oracle.rs",
            "applications/tankracer/tools/scene_fragment_case.rs",
            "applications/tankracer/tools/fragment_motion_case.rs",
            "applications/tankracer/tools/parent_auxiliary_assets.py",
            "applications/tankracer/tools/parent_auxiliary_oracle.rs",
            "applications/tankracer/tools/parent_auxiliary_case.rs",
            "applications/tankracer/tools/mesh_storage_case.rs",
            "applications/tankracer/tools/scene_lifecycle_case.rs",
            "applications/tankracer/tools/scene_lifecycle_assets.rs",
            "applications/tankracer/tools/identity_assets.rs",
            "applications/tankracer/tools/identity_capture.rs",
            "applications/tankracer/tools/reference_bridge.py",
            "applications/tankracer/tools/reference_scene.rs",
            "applications/tankracer/tools/reference_meshes.rs",
            "applications/tankracer/tools/reference_tanks.rs",
            "applications/tankracer/tools/scene_export.rs",
            "applications/tankracer/tools/reference_world.rs",
            "applications/tankracer/tools/export_trig_assets.py",
            "applications/tankracer/rcc/trig_asset_lookup.rs",
        ]);
        let path = std::path::PathBuf::from(
            std::env::var("TANKRACER_REFERENCE").expect("fragment reference"),
        );
        (context.watch)(&path);
        let root = path.parent().unwrap();
        for name in [
            "src/frame_math.c",
            "src/frame_math_tables.h",
            "tools/export_rust_math.mjs",
            "crates/game/src",
            "crates/game/data",
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
        script: "applications/tankracer/tools/fragments_assets.py",
        directory: "tankracer-fragments-assets",
        constants: "fragments_assets.rs",
        repack: "fragments-repack.conf",
        names: ResourceNames::File("FRAGMENTS_"),
        inputs: &[],
        production_surface: false,
        manifest_sources: false,
    }),
    configure: |context, application, _| {
        if std::env::var("TANKRACER_FRAGMENT_SCOPE").as_deref() == Ok("parents") {
            application.source.entry = "parents_main";
        }
        if std::env::var("TANKRACER_FRAGMENT_SCOPE").as_deref() == Ok("residency") {
            application.source.entry = "residency_main";
        }
        if std::env::var("TANKRACER_FRAGMENT_SCOPE").as_deref() == Ok("storage") {
            application.source.entry = "storage_main";
        }
        // Initial CPU-resident private meshes stay in an 8 KiB heap. Auxiliary
        // geometry shares the level's external bank with its motion rows.
        application.source.heap_words = Some(0x1000);
        application
            .source
            .ordinary_dirs
            .insert(0, context.output.join("tankracer-fragments-assets"));
    },
    ..DEFAULT
};
