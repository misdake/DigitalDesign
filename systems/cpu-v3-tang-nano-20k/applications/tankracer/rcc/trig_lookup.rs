//! Low-memory oracle backend. The production build selects trig_asset_lookup.rs
//! for this same module name; no runtime callbacks or table reconstruction.
use crate::dsl_rt::*;
use crate::integer;
pub fn index(table: Array<i16>, index: u16) -> fix32 {
    integer::from_i16(table[index])
}
