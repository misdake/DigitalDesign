//! CpuV3 processor IP: ISA, executable model, cache models, and RCC backend.

mod architecture;
mod hardware;
pub mod rcc_backend;

pub use architecture::*;
pub use hardware::*;
