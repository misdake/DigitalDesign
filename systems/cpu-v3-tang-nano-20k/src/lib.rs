//! Tang Nano 20K system integration for CpuV3.

pub mod boot;
pub mod cpu_subsystem;
pub mod debugger;
pub mod display;
mod display_device;
mod gpu_device;
#[path = "../rcc/gpu_layout.rs"]
mod gpu_layout;
pub mod hardware;
pub mod host_io;
pub mod input_device;
mod layout;
mod observation_device;
pub mod profiling_capture;
pub mod runtime_diagnostics;
pub mod system_cycle_emu;
pub mod system_emu;
pub mod system_sim;

// System internals share the lower-layer types through this crate root. New
// examples should still import each lower layer explicitly so ownership stays
// visible at composition sites.
pub(crate) use cpu_v3::*;
pub(crate) use digital_design_hardware::*;
pub use display_device::*;
pub use gpu_device::*;
pub use hardware::*;
pub use layout::*;
pub use observation_device::ObservationDevice;
