//! Original entity IDs and free links in caller-owned contiguous storage.
use crate::dsl_rt::*;
use crate::{attitude, integer};

pub struct State {
    // Six i32 values: capacity, free head, high water, record count,
    // sequential allocation, next requested ID.
    pub header: Array<u16>,
    // Six u16 words per slot: flags, class, component ID (u32), free link (u32).
    // Flags: record present (1), class present (2), Handle kind in bits 2..4.
    // Handle kinds: 0 none, 1 obstacle, 2 vehicle, 3 camera, 4 finish owner.
    pub rows: Array<u16>,
    // Reserved slots; growth returns capacity status before crossing this
    // backing span. At most 5461 six-word rows fit a signed pointer offset.
    pub storage_capacity: u16,
}
pub struct Entry {
    // 0 success, 2 invalid limit, 3 work limit, 4 capacity, 6 missing record.
    pub status: u16,
    pub id: fix32,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn entry(status: u16, id: fix32) -> Entry {
    Entry {
        status: status,
        id: id,
    }
}
fn valid(state: &State, id: fix32) -> bool {
    id.hi_bits() == 0 && id.lo_bits() < attitude::get(state.header, 0).lo_bits()
}
fn row(state: &State, id: fix32) -> Array<u16> {
    unsafe {
        state
            .rows
            .as_ptr()
            .add((id.lo_bits() * 6) as i16)
            .as_u16_array()
    }
}
pub fn present(state: &State, id: fix32) -> bool {
    valid(state, id) && row(state, id)[0u16] & 1 != 0
}
pub fn class(state: &State, id: fix32) -> fix32 {
    if !present(state, id) {
        return n(-1);
    }
    let r = row(state, id);
    if r[0u16] & 2 == 0 {
        return n(-1);
    }
    fix32::from_words(r[1u16], 0)
}
pub fn handle_kind(state: &State, id: fix32) -> u16 {
    if !present(state, id) {
        return 0;
    }
    (row(state, id)[0u16] >> 2) & 7
}
pub fn handle(state: &State, id: fix32) -> fix32 {
    let kind = handle_kind(state, id);
    if kind == 0 || kind > 2 {
        return n(0);
    }
    attitude::get(row(state, id), 1)
}
pub fn bind_class(state: &State, id: fix32, class: u16) -> u16 {
    if !present(state, id) {
        return 6;
    }
    let mut r = row(state, id);
    r[1u16] = class;
    r[0u16] |= 2;
    0
}
pub fn bind_handle(state: &State, id: fix32, kind: u16, index: fix32) -> u16 {
    if !present(state, id) {
        return 6;
    }
    let mut r = row(state, id);
    r[0u16] = (r[0u16] & 3) | (kind << 2);
    attitude::put(r, 1, index);
    0
}
pub fn next_free(state: &State, id: fix32) -> fix32 {
    attitude::get(row(state, id), 2)
}
pub fn reset(state: &State, capacity: u16) -> u16 {
    if capacity < 2 || capacity > 5461 || capacity > state.storage_capacity {
        return 4;
    }
    let previous = attitude::get(state.header, 0);
    let mut rows = state.rows;
    let mut i = 0u16;
    if previous != fix32::from_words(capacity, 0) {
        while i < capacity * 6 {
            rows[i] = 0;
            i += 1;
        }
    }
    i = 1;
    while i < capacity {
        let mut next = fix32::from_words(i + 1, 0);
        if i + 1 == capacity {
            next = n(0);
        }
        attitude::put(row(state, fix32::from_words(i, 0)), 2, next);
        i += 1;
    }
    attitude::put(state.header, 0, fix32::from_words(capacity, 0));
    attitude::put(state.header, 1, n(1));
    attitude::put(state.header, 2, n(1));
    attitude::put(state.header, 3, n(0));
    0
}
pub fn grow(state: &State) -> u16 {
    let old = attitude::get(state.header, 0).lo_bits();
    if old > 5333 || state.storage_capacity < 128 || old > state.storage_capacity - 128 {
        return 4;
    }
    let count = old + 128;
    let mut rows = state.rows;
    let mut i = old * 6;
    while i < count * 6 {
        rows[i] = 0;
        i += 1;
    }
    i = old;
    while i < count {
        let mut next = fix32::from_words(i + 1, 0);
        if i + 1 == count {
            next = n(0);
        }
        attitude::put(row(state, fix32::from_words(i, 0)), 2, next);
        i += 1;
    }
    attitude::put(state.header, 0, fix32::from_words(count, 0));
    if attitude::get(state.header, 1) == n(0) {
        attitude::put(state.header, 1, fix32::from_words(old, 0));
        attitude::put(state.header, 2, fix32::from_words(old, 0));
    }
    0
}
pub fn unlink_free(state: &State, id: fix32, limit: fix32) -> Entry {
    if limit == n(0) {
        return entry(2, n(0));
    }
    if !valid(state, id) {
        return entry(6, n(0));
    }
    let mut current = attitude::get(state.header, 1);
    if current == id {
        attitude::put(state.header, 1, next_free(state, current));
        return entry(0, id);
    }
    let mut next = next_free(state, current);
    let mut work = n(0);
    while next != n(0) && next != id {
        if work.hi_bits() > limit.hi_bits()
            || (work.hi_bits() == limit.hi_bits() && work.lo_bits() >= limit.lo_bits())
        {
            return entry(3, n(0));
        }
        work = work + n(1);
        current = next;
        next = next_free(state, next);
    }
    if next != n(0) || id == n(0) {
        attitude::put(row(state, current), 2, next_free(state, next));
    }
    if next == n(0) {
        return entry(0, n(0));
    }
    entry(0, id)
}
pub fn new_record(state: &State, requested: fix32, limit: fix32) -> Entry {
    if limit == n(0) {
        return entry(2, n(0));
    }
    if requested < n(0) {
        return entry(4, n(0));
    }
    let mut id = requested;
    if requested == n(0) {
        if attitude::get(state.header, 1) == n(0) {
            let status = grow(state);
            if status != 0 {
                return entry(status, n(0));
            }
        }
        id = attitude::get(state.header, 1);
        attitude::put(state.header, 1, next_free(state, id));
    } else {
        let mut work = n(0);
        while requested >= attitude::get(state.header, 0) {
            if work.hi_bits() > limit.hi_bits()
                || (work.hi_bits() == limit.hi_bits() && work.lo_bits() >= limit.lo_bits())
            {
                return entry(3, n(0));
            }
            work = work + n(1);
            let status = grow(state);
            if status != 0 {
                return entry(status, n(0));
            }
        }
        if !present(state, id) {
            let found = unlink_free(state, id, limit);
            if found.status != 0 {
                return found;
            }
        }
    }
    let mut r = row(state, id);
    r[0u16] = 1;
    if attitude::get(state.header, 2) < id {
        attitude::put(state.header, 2, id);
    }
    attitude::put(state.header, 3, attitude::get(state.header, 3) + n(1));
    entry(0, id)
}
pub fn create_record(state: &State, limit: fix32) -> Entry {
    let mut requested = n(0);
    if attitude::get(state.header, 4) != n(0) {
        requested = attitude::get(state.header, 5);
        attitude::put(state.header, 5, requested + n(1));
    }
    new_record(state, requested, limit)
}
pub fn detach(state: &State, id: fix32) -> u16 {
    if !valid(state, id) {
        return 6;
    }
    let mut r = row(state, id);
    r[0u16] = 0;
    0
}
pub fn release_id(state: &State, id: fix32) -> u16 {
    let status = detach(state, id);
    if status != 0 {
        return status;
    }
    attitude::put(row(state, id), 2, attitude::get(state.header, 1));
    attitude::put(state.header, 1, id);
    0
}
// The factory calls this only after both class destructor methods succeed.
pub fn finish_destroy(state: &State) {
    attitude::put(state.header, 3, attitude::get(state.header, 3) - n(1));
}
