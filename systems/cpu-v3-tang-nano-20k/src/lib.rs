//! Tang Nano 20K system integration for CpuV3.

pub mod boot;
pub mod debugger;
pub mod display;
mod display_device;
pub mod hardware;
mod layout;

// System internals share the lower-layer types through this crate root. New
// examples should still import each lower layer explicitly so ownership stays
// visible at composition sites.
pub(crate) use cpu_v3::*;
pub(crate) use digital_design_hardware::*;
pub(crate) use digital_design_hardware_gowin::*;
pub use display_device::*;
pub use hardware::*;
pub use layout::*;
