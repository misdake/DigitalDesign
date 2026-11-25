//! Adjacent collision cells through the standard source-asset build path.
use crate::applications::{Assets, BuildOption, Definition, OptionValues, ResourceNames, DEFAULT};

pub const APPLICATION: Definition = Definition {
    name: "bench-tankracer-terrain",
    selection: Some("tankracer_terrain"),
    source: Some("applications/tankracer/terrain/main.rs"),
    owner: "applications/tankracer/terrain_application.rs",
    gpu_api: true,
    ordinary_sources: &["applications/tankracer/rcc"],
    options: &[
        BuildOption::integer_constant("samples", "TERRAIN_QUERY_LIMIT", 0, 0, 1200),
        BuildOption::environment("reference", "TANKRACER_REFERENCE", OptionValues::InputFile),
        BuildOption::environment(
            "archive",
            "TANKRACER_TERRAIN_ARCHIVE",
            OptionValues::InputFile,
        ),
    ],
    inputs: |context| {
        context.inputs(&["applications/tankracer/tools/terrain_assets.rs"]);
        for key in ["TANKRACER_REFERENCE", "TANKRACER_TERRAIN_ARCHIVE"] {
            let path =
                std::path::PathBuf::from(std::env::var(key).expect("terrain source options"));
            (context.watch)(&path);
            if key == "TANKRACER_REFERENCE" {
                let root = path.parent().unwrap();
                for directory in ["crates/assets/src", "crates/game/src"] {
                    (context.watch)(&root.join(directory));
                }
            }
        }
    },
    assets: Some(Assets {
        script: "applications/tankracer/tools/terrain_assets.py",
        directory: "tankracer-terrain-assets",
        constants: "terrain_assets.rs",
        repack: "terrain-repack.conf",
        names: ResourceNames::File("TERRAIN_"),
        production_surface: false,
        manifest_sources: false,
        inputs: &[],
    }),
    ..DEFAULT
};
