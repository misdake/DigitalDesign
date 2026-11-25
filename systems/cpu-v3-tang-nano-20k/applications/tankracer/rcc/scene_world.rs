//! Live collider geometry and ordered memberships shared by updates and queries.
use crate::dsl_rt::*;
use crate::{attitude, collision, lifecycle, object_query, ordered, spatial};
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
    pub grid: spatial::Grid,
    pub deformables: ordered::List,
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
    let mut raw = unsafe { view_of(&mut collider).as_ptr().as_u16_array() };
    i = 0;
    while i < row.count {
        let id = attitude::get(row.items, i);
        if id.hi_bits() != 0 || id.lo_bits() >= world.count {
            return 4;
        }
        let index = id.lo_bits();
        let mut j = 0u16;
        while j < 60 {
            raw[j] = world.colliders[index * 60 + j];
            j += 1;
        }
        let status = object_query::obstacle_candidate(&collider, index, q, out, d, length);
        if status != 0 {
            return status;
        }
        i += 1;
    }
    0
}
// Pose publication writes the corresponding row in world.colliders.
// Call spatial::remove/insert at the original method's membership boundaries.
