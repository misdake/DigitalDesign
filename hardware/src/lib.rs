#![allow(clippy::type_complexity)]

extern crate self as digital_design_hardware;

mod gowin;
mod module;
mod project;
mod resources;
mod target;
mod testing;

pub mod examples;

pub use digital_design_hardware_macros::ModuleIo;
pub use gowin::*;
pub use module::*;
pub use project::*;
pub use resources::*;
pub use target::*;
pub use testing::*;
