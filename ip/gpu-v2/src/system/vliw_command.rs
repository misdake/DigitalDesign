//! Compatibility path for the command ABI and production factory.
//! New callers use `system::command`; this module owns no parallel implementation.
pub use super::command::abi::*;
pub use super::command::{verilog, MODULE};
