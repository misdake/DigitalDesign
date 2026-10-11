//! Hand-written synthesizable RTL leaves for the texture sampler.
//!
//! These leaves mirror existing Rust references edge for edge; they are not
//! generated from sample traces. `color` mirrors `texture::emu::color` exactly.
pub mod cache;
pub mod coefficient;
pub mod color;
pub mod compact_coefficient;
pub mod compact_coordinate;
pub mod compact_leaves;
pub mod context_store;
pub mod coordinate;
pub mod coordinate_store;
pub mod lod;
pub mod packet_store;
pub mod packet_store_ordered;
pub mod runtime_backend;
pub mod runtime_cache;
pub mod runtime_color;
pub mod runtime_packet_backend;
pub mod runtime_rows;
pub mod sampler;
pub mod uv_rows;
pub mod uv_store;
pub mod work;

pub mod runtime_frontend;
pub mod runtime_frontend_early;
