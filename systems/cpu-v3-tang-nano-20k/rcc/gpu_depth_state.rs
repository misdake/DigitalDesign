//! CPU-only depth-content provenance; exact aliases share a physical plane key.
use crate::dsl_rt::*;
mod gpu_arena;
use gpu_arena::*;

static GPU_DEPTH_HEAD: u16 = 0;
const GPU_DEPTH_STATE: u16 = 2;
const GPU_DEPTH_NEXT: u16 = 14;
const GPU_DEPTH_ANY: u16 = 5;
const GPU_DEPTH_WRITING: u16 = 6;
const GPU_DEPTH_REF: u16 = 32768;
const GPU_DEPTH_SEEN: u16 = 16;
const GPU_DEPTH_LOAD: u16 = 32;
const GPU_DEPTH_CLEAR: u16 = 64;

fn gpu_depth_state_init() {
    unsafe {
        addr_of(&GPU_DEPTH_HEAD).write(0, 0);
    }
}
fn gpu_depth_planes_disjoint(a: u16, b: u16) -> bool {
    if a < b {
        b - a >= 1600
    } else {
        a - b >= 1600
    }
}
fn gpu_depth_surface_register(handle: u16) -> bool {
    let mut data = gpu_arena_data(handle);
    let mut other = GPU_DEPTH_HEAD;
    let mut state: u16 = 0;
    while other != 0 {
        let entry = gpu_arena_data(other);
        if data[0u16] == entry[0u16] {
            state = entry[GPU_DEPTH_STATE];
            if state == GPU_DEPTH_WRITING {
                state = 0;
            }
        } else if !gpu_depth_planes_disjoint(data[0u16], entry[0u16]) {
            return false;
        }
        other = entry[GPU_DEPTH_NEXT];
    }
    data[GPU_DEPTH_STATE] = state;
    data[GPU_DEPTH_NEXT] = GPU_DEPTH_HEAD;
    unsafe {
        addr_of(&GPU_DEPTH_HEAD).write(0, handle);
    }
    true
}
fn gpu_depth_surface_unregister(handle: u16) -> bool {
    let mut other = GPU_DEPTH_HEAD;
    let mut previous: u16 = 0;
    while other != 0 {
        let mut data = gpu_arena_data(other);
        if other == handle {
            if previous == 0 {
                unsafe {
                    addr_of(&GPU_DEPTH_HEAD).write(0, data[GPU_DEPTH_NEXT]);
                }
            } else {
                gpu_arena_data(previous)[GPU_DEPTH_NEXT] = data[GPU_DEPTH_NEXT];
            }
            data[GPU_DEPTH_STATE] = 0;
            return true;
        }
        previous = other;
        other = data[GPU_DEPTH_NEXT];
    }
    false
}
fn gpu_depth_descriptor_matches(flags: u16, first: u16, count: u16) -> bool {
    gpu_depth_live_plane(flags, first, count) != 0
}
fn gpu_depth_live_plane(flags: u16, first: u16, count: u16) -> u16 {
    if flags & GPU_DEPTH_REF == 0 || count < 1600 {
        return 0;
    }
    let mut other = GPU_DEPTH_HEAD;
    while other != 0 {
        let data = gpu_arena_data(other);
        if data[0u16] == first {
            return other;
        }
        other = data[GPU_DEPTH_NEXT];
    }
    0
}
fn gpu_depth_plane_state(first: u16, state: u16) {
    let mut other = GPU_DEPTH_HEAD;
    while other != 0 {
        let mut data = gpu_arena_data(other);
        if data[0u16] == first {
            data[GPU_DEPTH_STATE] = state;
        }
        other = data[GPU_DEPTH_NEXT];
    }
}
fn gpu_depth_input_compatible(flags: u16, first: u16, count: u16) -> bool {
    if flags & GPU_DEPTH_LOAD == 0 {
        return true;
    }
    let shift = (flags >> 2) & 3;
    if flags & GPU_DEPTH_REF == 0 {
        // Explicit legacy raw/preimage facts have no live descriptor reference.
        return shift == 0;
    }
    let handle = gpu_depth_live_plane(flags, first, count);
    if handle == 0 {
        return false;
    }
    let state = gpu_arena_data(handle)[GPU_DEPTH_STATE];
    state == shift + 1 || state == GPU_DEPTH_ANY
}
fn gpu_depth_content_start(flags: u16, first: u16, count: u16) {
    if flags & (GPU_DEPTH_SEEN | GPU_DEPTH_CLEAR) != 0
        && gpu_depth_descriptor_matches(flags, first, count)
    {
        gpu_depth_plane_state(first, GPU_DEPTH_WRITING);
    }
}
fn gpu_depth_content_complete(flags: u16, first: u16, count: u16) {
    if !gpu_depth_descriptor_matches(flags, first, count) {
        return;
    }
    // Single GPU owner: only aliases present at START carry WRITING. A new
    // descriptor created during this writer starts unknown and blocks its
    // completion from certifying a new resource lifetime.
    let mut other = GPU_DEPTH_HEAD;
    while other != 0 {
        let data = gpu_arena_data(other);
        if data[0u16] == first && data[GPU_DEPTH_STATE] != GPU_DEPTH_WRITING {
            return;
        }
        other = data[GPU_DEPTH_NEXT];
    }
    if flags & GPU_DEPTH_SEEN != 0 {
        gpu_depth_plane_state(first, ((flags >> 2) & 3) + 1);
    } else if flags & GPU_DEPTH_CLEAR != 0 {
        // A complete raw clear contains no geometry encoded with a prior k.
        gpu_depth_plane_state(first, GPU_DEPTH_ANY);
    }
}
fn gpu_depth_capture_shift(flags: u16, first: u16, count: u16) -> u16 {
    if flags & GPU_DEPTH_SEEN != 0 {
        return (flags >> 2) & 3;
    }
    let handle = gpu_depth_live_plane(flags, first, count);
    if handle != 0 {
        let state = gpu_arena_data(handle)[GPU_DEPTH_STATE];
        if state > 0 && state < 5 {
            return state - 1;
        }
    }
    0
}
