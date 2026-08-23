//! Tang Nano 20K system integration for CpuV3.

pub mod boot;
pub mod hardware;
mod layout;

pub use cpu_v3::*;
pub use digital_design_hardware::*;
pub use digital_design_hardware_gowin::*;
pub use hardware::*;
pub use layout::*;
