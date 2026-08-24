//! Gowin toolchain integration, primitives, and concrete FPGA targets.

pub use digital_design_hardware::*;
pub use digital_design_hardware_common::*;

pub mod primitives;
pub mod targets;
mod toolchain;

pub use primitives::*;
pub use targets::*;
pub use toolchain::*;
