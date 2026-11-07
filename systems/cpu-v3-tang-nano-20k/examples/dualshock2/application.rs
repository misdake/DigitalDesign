//! Controller diagnostic application; no generated graphics assets.
use crate::applications::{Definition, DEFAULT};
pub const APPLICATIONS: &[Definition] = &[Definition {
    name: "dualshock2",
    selection: Some("dualshock2"),
    source: Some("examples/dualshock2/rcc/main.rs"),
    owner: "examples/dualshock2/application.rs",
    ..DEFAULT
}];
