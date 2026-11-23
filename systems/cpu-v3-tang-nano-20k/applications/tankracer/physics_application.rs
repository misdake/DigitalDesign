//! Bounded physical-chain benchmark: standard source-declared immutable assets.
use crate::applications::{Assets, BuildOption, Definition, OptionValues, ResourceNames, DEFAULT};
pub const APPLICATION: Definition = Definition {
    name: "bench-tankracer-physics",
    selection: Some("tankracer_physics"),
    source: Some("applications/tankracer/native/main.rs"),
    owner: "applications/tankracer/physics_application.rs",
    gpu_api: true,
    ordinary_sources: &["applications/tankracer/rcc"],
    options: &[
        BuildOption::environment("reference", "TANKRACER_REFERENCE", OptionValues::InputFile),
        BuildOption::environment("fixtures", "TANKRACER_FIXTURES", OptionValues::InputFile),
        BuildOption::integer_constant("repeats", "PHYSICS_REPEATS", 2, 1, 2),
    ],
    inputs: |context| {
        context.inputs(&[
            "applications/tankracer/tools/export_trig_assets.py",
            "applications/tankracer/rcc/trig_asset_lookup.rs",
        ]);
        for key in ["TANKRACER_REFERENCE", "TANKRACER_FIXTURES"] {
            println!("cargo:rerun-if-env-changed={key}");
            let path = std::path::PathBuf::from(
                std::env::var(key).expect("physics reference/fixtures options"),
            );
            (context.watch)(&path);
            if key == "TANKRACER_REFERENCE" {
                let root = path.parent().unwrap();
                for rel in [
                    "src/frame_math.c",
                    "src/frame_math_tables.h",
                    "tools/export_rust_math.mjs",
                    "crates/game/src/math.rs",
                    "crates/game/data/sine.bin",
                    "crates/game/data/angle.bin",
                ] {
                    (context.watch)(&root.join(rel));
                }
            }
        }
    },
    assets: Some(Assets {
        script: "applications/tankracer/tools/native_assets.py",
        directory: "tankracer-physics-assets",
        constants: "physics_assets.rs",
        repack: "physics-repack.conf",
        names: ResourceNames::File("PHYSICS_"),
        inputs: &[],
        production_surface: false,
        manifest_sources: false,
    }),
    configure: |context, application, _| {
        application
            .source
            .ordinary_dirs
            .insert(0, context.output.join("tankracer-physics-assets"));
    },
    ..DEFAULT
};
