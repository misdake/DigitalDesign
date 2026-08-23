//! Executable specification for the next-generation G16 instruction set.
//!
//! This module intentionally lives beside the v2.6 implementation while the
//! compiler and hardware are migrated. It is self-contained: no source or
//! runtime dependency on the exploratory `design_model` project is required.
//! The normative migration notes and local encoding changes are in `spec.md`.

mod cache;
mod encoding;
mod sdram;
mod sim;

pub use digital_design_ip_common::PhysicalWordAddress;

pub use cache::*;
pub use encoding::*;
pub use sdram::*;
pub use sim::*;
