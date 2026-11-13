//! Controller UI owns its atlas producer and constants.
use crate::applications::{Assets, Definition, ResourceNames, DEFAULT, RENDERER_ASSETS};
pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "dualshock2-ui",
    selection: Some("dualshock2_ui"),
    source: Some("examples/dualshock2_ui/rcc/main.rs"),
    owner: "examples/dualshock2_ui/application.rs",
    assets: Some(Assets {
        script: "examples/dualshock2_ui/tools/assets.py",
        directory: "ds2-ui-assets",
        constants: "ui_layout.rs",
        repack: "ds2-ui-repack.conf",
        names: ResourceNames::Fixed("DS2_UI_ATLAS"),
        manifest_sources: true,
        ..RENDERER_ASSETS
    }),
    ..DEFAULT
}];
