//! Caller-owned ordered ID storage. IDs retain all 32 source bits.
use crate::dsl_rt::*;
use crate::{attitude, integer};
pub struct List {
    pub items: Array<u16>,
    pub count: u16,
    pub capacity: u16,
}
pub struct Found {
    pub status: u16,
    pub index: u16,
    pub found: bool,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
// Status: 0 success, 2 invalid work limit, 3 work limit, 4 storage capacity.
pub fn find(list: &List, id: fix32, limit: fix32) -> Found {
    let mut result = Found {
        status: 0,
        index: 0,
        found: false,
    };
    if limit == n(0) {
        result.status = 2;
        return result;
    }
    let mut i = 0u16;
    while i < list.count {
        // Work limits are u32; every stored index fits one word.
        if limit.hi_bits() == 0 && i >= limit.lo_bits() {
            result.status = 3;
            return result;
        }
        if attitude::get(list.items, i) == id {
            result.index = i;
            result.found = true;
            return result;
        }
        i += 1;
    }
    result.index = list.count;
    if limit.hi_bits() == 0 && list.count >= limit.lo_bits() {
        result.status = 3;
    }
    result
}
pub fn append(list: &mut List, id: fix32) -> u16 {
    if list.count == list.capacity {
        return 4;
    }
    attitude::put(list.items, list.count, id);
    list.count += 1;
    0
}
pub fn prepend(list: &mut List, id: fix32) -> u16 {
    if list.count == list.capacity {
        return 4;
    }
    let mut i = list.count;
    while i > 0 {
        attitude::put(list.items, i, attitude::get(list.items, i - 1));
        i -= 1;
    }
    attitude::put(list.items, 0, id);
    list.count += 1;
    0
}
pub fn remove_at(list: &mut List, index: u16) {
    let mut i = index;
    while i + 1 < list.count {
        attitude::put(list.items, i, attitude::get(list.items, i + 1));
        i += 1;
    }
    list.count -= 1;
}
pub fn remove_first(list: &mut List, id: fix32, limit: fix32) -> u16 {
    let found = find(list, id, limit);
    if found.status != 0 {
        return found.status;
    }
    if found.found {
        remove_at(list, found.index);
    }
    0
}
