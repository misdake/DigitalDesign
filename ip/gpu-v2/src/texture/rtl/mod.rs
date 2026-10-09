//! Hand-written synthesizable RTL leaves for the texture sampler.
//!
//! These leaves mirror existing Rust references edge for edge; they are not
//! generated from sample traces. `color` mirrors `texture::emu::color` exactly.
pub mod cache;
pub mod coefficient;
pub mod color;
pub mod sampler;
