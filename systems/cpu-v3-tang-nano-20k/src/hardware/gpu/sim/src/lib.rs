//! GPU v2 cycle and numerical model.
//!
//! All clocked state is explicit. Numerical work uses integer fixed-point
//! values and named rounding/overflow operations. This crate has no RTL,
//! board, system-emulator, or production-GPU dependency.

pub mod dma;
pub mod dualwide;
pub mod events;
pub mod explore;
pub mod fixed;
pub mod format;
pub mod framebuffer;
pub mod micro_audit;
pub mod microkernel;
pub mod normal_contract;
pub mod probe;
pub mod result_store;
pub mod scratchpad;
pub mod setup;
pub mod stream96;
pub mod timing;
pub mod transform;
pub mod vertex;
