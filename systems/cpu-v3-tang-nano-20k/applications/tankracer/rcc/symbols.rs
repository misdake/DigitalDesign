//! Shared authored entity/class bindings, indexed by original interned IDs.
use crate::dsl_rt::*;
use crate::{attitude, integer};
fn at(data: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset as i16).as_u16_array() }
}
pub fn entity(data: Array<u16>, id: fix32) -> fix32 {
    if id.hi_bits() != 0 || id.lo_bits() >= data[0u16] {
        return integer::from_i16(-1);
    }
    attitude::get(at(data, 1 + id.lo_bits() * 4), 0)
}
pub fn resolve(data: Array<u16>, binding: Array<u16>) -> fix32 {
    let value = attitude::get(at(binding, 1), 0);
    if binding[0u16] == 0 {
        return value;
    }
    if binding[0u16] == 1 {
        return entity(data, value);
    }
    integer::from_i16(-1)
}
