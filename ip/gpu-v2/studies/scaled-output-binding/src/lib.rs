//! Finite research binding, never an executable GPU datapath or physical timing claim.
#![forbid(unsafe_code)]

pub mod binding;
pub mod reference;
pub mod window;

pub use window::{prepare, Spec, Window, SPECS};
