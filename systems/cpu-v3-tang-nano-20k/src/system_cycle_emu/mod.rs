//! One system clock/backing owner for the complete production cycle models.
//! Individual components never advance the shared memory service.
pub mod clock;
pub mod input;
pub mod memory;
pub mod profiling;
pub mod request_slot;
pub mod runner;
pub mod scanout;
pub mod startup;
mod system;
pub use system::{Error, Observation, Statistics, Step, System};
