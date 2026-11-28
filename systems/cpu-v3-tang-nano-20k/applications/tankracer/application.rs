//! Reproducible static-map rendering on the production 20K SDK.
use crate::applications::{
    Assets, BuildOption, Context, DEFAULT, Definition, OptionValues, Prepared, RENDERER_ASSETS,
};
use serde_json::Value;
use std::path::Path;

const CAMERA_INPUT: &str = "TANKRACER_CAMERA_INPUT";
const MAP_INPUT: &str = "TANKRACER_MAP_INPUT";
const ROUTE_INPUT: &str = "TANKRACER_ROUTE_INPUT";

fn inputs(context: &mut Context<'_>) {
    let repository = context.root.join("../..").canonicalize().unwrap();
    if std::env::var_os(MAP_INPUT).is_none() {
        std::env::set_var(
            MAP_INPUT,
            context
                .root
                .join("applications/tankracer/assets/england/scene-grid-atlas.zip"),
        );
    }
    if std::env::var_os(ROUTE_INPUT).is_none() {
        std::env::set_var(
            ROUTE_INPUT,
            context
                .root
                .join("applications/tankracer/assets/england/route.json"),
        );
    }
    for (flag, variable) in [
        ("map", MAP_INPUT),
        ("camera", CAMERA_INPUT),
        ("route", ROUTE_INPUT),
    ] {
        let path = std::env::var_os(variable).unwrap_or_else(|| panic!("missing --{flag}"));
        let path = Path::new(&path).canonicalize().unwrap();
        assert!(
            path.starts_with(&repository),
            "--{flag} must be staged within the repository"
        );
        (context.watch)(&path);
    }
}

fn configure(_: &mut Context<'_>, application: &mut Prepared, manifest: &Value) {
    for (key, filename) in [("route", "route.json"), ("route_ring", "route-ring.json")] {
        if let Some(file) = manifest[key]["file"].as_str() {
            assert_eq!(file, filename, "unexpected route sidecar name");
            application
                .extra_assets
                .push(format!("tankracer-static-assets/{file}"));
        }
    }
}

const STATIC_APPLICATION: Definition = Definition {
    name: "bench-tankracer-static",
    selection: Some("tankracer_static"),
    source: Some("applications/tankracer/rcc/main.rs"),
    gpu_api: true,
    owner: "applications/tankracer/application.rs",
    inputs,
    configure,
    options: &[
        BuildOption::environment(
            "policy",
            "TANKRACER_POLICY",
            OptionValues::Choices(&["full", "cull"]),
        ),
        BuildOption::environment(
            "candidates",
            "TANKRACER_CANDIDATES",
            OptionValues::Choices(&["clusters", "route"]),
        ),
        BuildOption::environment("map", MAP_INPUT, OptionValues::InputFile),
        BuildOption::environment("camera", CAMERA_INPUT, OptionValues::InputFile),
        BuildOption::environment("route", ROUTE_INPUT, OptionValues::InputFile),
        BuildOption::environment(
            "recording",
            "TANKRACER_RECORDING",
            OptionValues::Choices(&["always", "reuse", "prepared"]),
        ),
        BuildOption::environment(
            "prepared-selection",
            "TANKRACER_PREPARED_SELECTION",
            OptionValues::Choices(&[
                "plans",
                "coarse-sphere",
                "coarse-sphere-shared",
                "coarse-sphere-incremental",
            ]),
        ),
        BuildOption::environment(
            "range-limit",
            "TANKRACER_RANGE_LIMIT",
            OptionValues::Choices(&["0", "4", "6", "8"]),
        ),
        BuildOption::environment(
            "range-layout",
            "TANKRACER_RANGE_LAYOUT",
            OptionValues::Choices(&["route", "lanes", "stable"]),
        ),
        BuildOption::environment(
            "range-cell",
            "TANKRACER_RANGE_CELL",
            OptionValues::Choices(&["16", "32"]),
        ),
    ],
    assets: Some(Assets {
        script: "applications/tankracer/tools/static_assets.py",
        directory: "tankracer-static-assets",
        constants: "tankracer_static.rs",
        repack: "tankracer-static-repack.conf",
        manifest_sources: true,
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
};

#[path = "driving_application.rs"]
mod driving;
#[path = "fragments_application.rs"]
mod fragments;
#[path = "physics_application.rs"]
mod physics;
#[path = "retirement_application.rs"]
mod retirement;
#[path = "scene_application.rs"]
mod scene;
#[path = "terrain_application.rs"]
mod terrain;

pub const APPLICATIONS: &[Definition] = &[
    STATIC_APPLICATION,
    physics::APPLICATION,
    terrain::APPLICATION,
    driving::APPLICATION,
    scene::APPLICATION,
    retirement::APPLICATION,
    fragments::APPLICATION,
];
