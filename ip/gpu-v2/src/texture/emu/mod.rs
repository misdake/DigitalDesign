//! Independent runtime arithmetic blocks; preparation/cache composition is separate.
pub mod cache;
pub mod coefficient;
#[cfg(test)]
mod coefficient_hybrid;
pub mod color;
pub mod coordinate;
pub mod derivative;
pub mod lod;
pub mod refill_bridge;
pub mod sampler;
