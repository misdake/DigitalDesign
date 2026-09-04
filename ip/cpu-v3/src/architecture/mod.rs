//! Executable specification for the next-generation CpuV3 instruction set.
//!
//! This module intentionally lives beside the v2.6 implementation while the
//! compiler and hardware are migrated. It is self-contained: no source or
//! runtime dependency on the exploratory `design_model` project is required.
//! The normative ISA and migration notes are in [`../../docs/isa.md`](../../docs/isa.md).

mod cache;
mod encoding;
mod fpu;
mod sdram;
mod sim;

pub use digital_design_ip_common::PhysicalWordAddress;

pub use cache::*;
pub use encoding::*;
pub use fpu::*;
pub use sdram::*;
pub use sim::*;
