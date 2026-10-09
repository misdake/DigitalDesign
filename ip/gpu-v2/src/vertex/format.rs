//! Frozen counted stage formats. Oracle precision is independently configurable.
use audited::Fixed;
pub type Packed = Fixed<96, 0, false>;
pub type Position = Fixed<32, 16, true>;
pub type Clip = Fixed<32, 16, true>;
pub type ClipProduct = Fixed<64, 32, true>;
pub type ClipSum = Fixed<66, 32, true>;
pub type Normal = Fixed<16, 14, true>;
pub type NormalProduct = Fixed<32, 28, true>;
pub type NormalSum = Fixed<34, 28, true>;
pub type UvCode = Fixed<12, 0, false>;
pub type TransformedRow = Fixed<36, 0, false>;
