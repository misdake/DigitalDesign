//! Reusable GPU v2 IP. Each component owns its ports and model implementations.
//! Board integration belongs to a final system outside this crate.
#![forbid(unsafe_code)]

pub mod command_processor;
pub mod frontend;
pub mod lighting;
pub mod scratchpad;
pub mod system;
pub mod triangle;
pub mod vertex;
