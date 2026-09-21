//! Reusable CpuV3 revision 0.8 processor core with physical-memory and device ports.
//!
//! See [`../../docs/hardware-architecture.md`](../../docs/hardware-architecture.md) for the
//! current Stage 12 microarchitecture and fitted-cache boundary.

mod cache;
mod core;
mod fetch;
mod fpu;

pub use cache::*;
pub use core::*;
pub use fetch::*;
pub use fpu::*;

#[cfg(test)]
mod tests;
