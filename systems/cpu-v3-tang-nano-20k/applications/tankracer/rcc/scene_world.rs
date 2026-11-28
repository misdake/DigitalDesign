//! Live collider geometry and ordered memberships shared by updates and queries.
use crate::dsl_rt::*;
use crate::{attitude, collision, lifecycle, object_query, ordered, spatial};
pub const DYNAMIC_ROWS: u16 = 215;
pub struct Runtime {
    pub ticks: lifecycle::Ticks,
    pub slot: Array<u16>,
    pub seed: Array<u16>,
    pub queues: lifecycle::Queues,
    pub ordinary: ordered::List,
    pub special: ordered::List,
}
pub struct World {
    // Contiguous 60-word collider rows, with the object_query::Collider layout.
    pub colliders: Array<u16>,
    // 16 words per object: class, weight, threshold, activity, flags (i32),
    // followed by the six signed mesh bounds (i16). Methods mutate these rows.
    pub properties: Array<u16>,
    pub count: u16,
    // Fixed rows retain their existing addresses. Appended rows occupy one
    // 32-KiB bank in global-ID order: collider[60], properties[16].
    pub fixed_count: u16,
    pub dynamic_page: u16,
    pub dynamic_capacity: u16,
    pub grid: spatial::Grid,
    pub deformables: ordered::List,
}
fn transfer(world: &World, index: u16, data: Array<u16>, properties: bool, write: bool) -> bool {
    if index >= world.count {
        return false;
    }
    let mut count = 60u16;
    let mut row = world.colliders;
    let mut offset = index * 60;
    if properties {
        count = 16;
        row = world.properties;
        offset = index * 16;
    }
    let mut saved = 0u16;
    let mut dynamic = false;
    if index >= world.fixed_count {
        let local = index - world.fixed_count;
        if local >= world.dynamic_capacity || local >= DYNAMIC_ROWS {
            return false;
        }
        saved = read_dseg_page(3);
        dynamic = true;
        if saved != world.dynamic_page {
            unsafe {
                write_dseg_page(3, world.dynamic_page);
            }
        }
        row = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
        offset = local * 76;
        if properties {
            offset += 60;
        }
    }
    let mut i = 0u16;
    let mut destination = data;
    let mut rows = row;
    if write {
        while i < count {
            rows[offset + i] = data[i];
            i += 1;
        }
    } else {
        while i < count {
            destination[i] = row[offset + i];
            i += 1;
        }
    }
    if dynamic && read_dseg_page(3) != saved {
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    true
}
pub fn load_collider(world: &World, index: u16, out: Array<u16>) -> bool {
    transfer(world, index, out, false, false)
}
pub fn store_collider(world: &World, index: u16, data: Array<u16>) -> bool {
    transfer(world, index, data, false, true)
}
pub fn load_properties(world: &World, index: u16, out: Array<u16>) -> bool {
    transfer(world, index, out, true, false)
}
pub fn store_properties(world: &World, index: u16, data: Array<u16>) -> bool {
    transfer(world, index, data, true, true)
}
// Source order selects the first hit. Every candidate reads current geometry;
// moving, rotating or removing an earlier object is visible to the next query.
pub fn query(world: &World, q: &object_query::Query, out: &mut object_query::Hit) -> u16 {
    spatial::select(&world.grid);
    let mut row = ordered::List {
        items: world.deformables.items,
        count: world.deformables.count,
        capacity: world.deformables.capacity,
    };
    if !q.deformable {
        row = spatial::candidates(&world.grid, q.start);
    }
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let d = delta.as_array();
    let mut i = 0u16;
    while i < 3 {
        attitude::put(d, i, attitude::get(q.end, i) - attitude::get(q.start, i));
        i += 1;
    }
    let length = collision::distance(d);
    let mut collider = object_query::collider_zero();
    let raw = unsafe { view_of(&mut collider).as_ptr().as_u16_array() };
    i = 0;
    while i < row.count {
        let id = attitude::get(row.items, i);
        if id.hi_bits() != 0 || id.lo_bits() >= world.count {
            return 4;
        }
        let index = id.lo_bits();
        let mut status = 0u16;
        if index < world.fixed_count {
            // The fixed rows use Collider's 60-word layout. Narrow phase only
            // reads the row and keeps its mapping, so borrow the resident data.
            let colliders: Array<object_query::Collider> = unsafe {
                world
                    .colliders
                    .as_ptr()
                    .as_array::<object_query::Collider>()
            };
            status = object_query::obstacle_candidate(&colliders[index], index, q, out, d, length);
        } else {
            // Dynamic rows borrow DSEG3, where the candidate list also lives.
            // Keep this snapshot until the narrow phase returns to that list.
            if !load_collider(world, index, raw) {
                return 4;
            }
            status = object_query::obstacle_candidate(&collider, index, q, out, d, length);
        }
        if status != 0 {
            return status;
        }
        i += 1;
    }
    0
}
// Pose publication writes the corresponding row in world.colliders.
// Call spatial::remove/insert at the original method's membership boundaries.
