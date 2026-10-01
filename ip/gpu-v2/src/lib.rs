//! Reusable GPU v2 IP. Each component owns its ports and model implementations.
//! Board integration belongs to a final system outside this crate.
#![forbid(unsafe_code)]

pub mod lighting;
pub mod system;
