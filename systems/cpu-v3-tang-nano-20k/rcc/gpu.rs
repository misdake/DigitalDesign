//! Compatibility import for the active single-buffer snapshot UI API.
//! New resource applications use gpu_api; DS2 UI imports gpu_legacy explicitly.
mod gpu_legacy;
pub use gpu_legacy::*;
