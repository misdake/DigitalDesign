#![allow(clippy::type_complexity)]

extern crate self as digital_design_hardware;

mod gowin;
mod inout;
mod module;
mod project;
mod resources;
mod target;
mod testing;

pub mod components;

pub use components::*;
pub use digital_design_hardware_macros::{Hardware, ModuleIo};
pub use gowin::*;
pub use inout::*;
pub use module::*;
pub use project::*;
pub use resources::components::*;
pub use resources::*;
pub use target::*;
pub use testing::*;
