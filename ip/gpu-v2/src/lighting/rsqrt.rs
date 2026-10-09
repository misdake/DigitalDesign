//! Read-only table images for reproducing RSQRT storage/error qualification.
//! Working coefficients stay Q15; Q13 is a separate endpoint storage contract.
pub use super::format::{RSQRT_Q13_BIASES, RSQRT_Q13_RAW, RSQRT_RAW};
