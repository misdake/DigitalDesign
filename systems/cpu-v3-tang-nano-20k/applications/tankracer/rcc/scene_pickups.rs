//! Pickup methods on the live collider, flight, membership and resource owners.
use crate::dsl_rt::*;
use crate::{
    attitude, effect_pool, integer, inventory, ordered, pickup_light, powerup, quad_pool,
    scene_parent, scene_query, spatial,
};

pub struct Frame {
    pub motion: powerup::Frame,
    pub has_vehicles: bool,
    pub has_player: bool,
    pub result: Array<u16>,
    pub auxiliary: scene_parent::Frame,
}
pub struct Collection {
    pub inventory: Array<u16>,
    pub amounts: Array<u16>,
    pub cooldown: Array<u16>,
    pub lights: pickup_light::State,
    pub context: pickup_light::Context,
    pub quads: Array<quad_pool::Pool>,
    pub lines: Array<effect_pool::Pool>,
    pub effects_page: u16,
    pub frame: fix32,
    pub follow: bool,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn at(v: Array<u16>, words: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(words as i16).as_u16_array() }
}
fn error(status: u16) -> fix32 {
    if status == 3 {
        return n(-3);
    }
    n(-1)
}
pub fn flight(actor: &scene_parent::Actor) -> powerup::State {
    powerup::State {
        core: actor.state,
        parachute: at(actor.state, 28),
    }
}
pub fn tick(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    grid: &mut spatial::Grid,
    actor: &mut scene_parent::Actor,
    frame: &Frame,
) -> fix32 {
    if !frame.has_vehicles {
        return n(-5);
    }
    if !frame.has_player {
        return n(-4);
    }
    if actor.kind != 3 || !actor.has_state {
        return n(-1);
    }
    let collider = actor.fragments.collider;
    let flags = at(actor.properties, 8);
    let f = flight(actor);
    let pose = powerup::Pose {
        basis: at(collider, 8),
        position: at(collider, 2),
    };
    let action = powerup::phase(&f, &pose, flags, &frame.motion, &mut state.map);
    // The source commits the external basis even when phase returns an error.
    let mut out = collider;
    let mut i = 0u16;
    while i < 18 {
        out[26 + i] = collider[8 + i];
        i += 1;
    }
    attitude::put(collider, 29, n(1));
    if action < n(0) {
        return action;
    }
    if action == n(1) {
        let status = ordered::append(
            &mut state.runtime.special,
            fix32::from_words(actor.fragments.id, 0),
        );
        if status != 0 {
            return error(status);
        }
        let row = at(state.lifetime.records, actor.fragments.id * 8);
        let mesh = attitude::get(actor.fragments.current_mesh, 0);
        let present = mesh.hi_bits() == 0 && mesh.lo_bits() < state.resources.count;
        let inserted = spatial::insert(
            grid,
            fix32::from_words(actor.fragments.id, 0),
            pose.position,
            present,
            attitude::get(collider, 28).lo_bits(),
        );
        let mut row = row;
        row[4u16] = inserted.cell.x as u16;
        row[5u16] = inserted.cell.z;
        row[6u16] = 1;
        if inserted.status != 0 {
            return error(inserted.status);
        }
    } else if action == n(2) {
        let status = scene_parent::auxiliary_tick(scene, state, grid, actor, &frame.auxiliary);
        if status != n(0) {
            return status;
        }
    }
    powerup::finish_frame(&f, flags, frame.motion.dt);
    attitude::put(frame.result, 0, n(0));
    n(0)
}
pub fn collect(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    grid: &mut spatial::Grid,
    actor: &mut scene_parent::Actor,
    hit: &scene_parent::Hit,
    collection: &mut Collection,
) -> fix32 {
    if actor.kind != 3 || !actor.has_state {
        return n(-1);
    }
    let status = inventory::collect(
        collection.inventory,
        attitude::get(actor.state, 26),
        collection.amounts,
    );
    if status != n(0) {
        return status;
    }
    let row = at(state.lifetime.records, actor.fragments.id * 8);
    let cell = spatial::Cell {
        x: row[4u16] as i16,
        z: row[5u16],
        present: row[6u16] != 0,
    };
    let status = spatial::remove(
        grid,
        fix32::from_words(actor.fragments.id, 0),
        &cell,
        state.lifetime.limit,
    );
    if status != 0 {
        return error(status);
    }
    let status = scene_parent::auxiliary_hit(scene, state, grid, actor, hit);
    if status != n(0) {
        return status;
    }
    let old = attitude::get(actor.properties, 4);
    let mut flags = old.lo_bits() & 65534;
    if old.lo_bits() & 4 == 0 {
        flags |= 4;
        let f = flight(actor);
        attitude::put(f.core, 10, n(1));
        let mut q = f.parachute;
        let c = actor.fragments.collider;
        let mut i = 0u16;
        while i < 18 {
            q[i] = c[8 + i];
            i += 1;
        }
        i = 0;
        while i < 6 {
            q[18 + i] = c[2 + i];
            i += 1;
        }
    }
    attitude::put(actor.properties, 4, fix32::from_words(flags, old.hi_bits()));
    // Audio and haptic feedback are successful external device operations.
    let start = pickup_light::Start {
        target: hit.auxiliary.vehicle,
        origin: at(actor.fragments.collider, 2),
        velocity: hit.auxiliary.velocity,
        frame: collection.frame,
        follow: collection.follow,
    };
    let saved = read_dseg_page(3);
    if collection.effects_page != 65535 && collection.effects_page != saved {
        unsafe {
            write_dseg_page(3, collection.effects_page);
        }
    }
    let status = pickup_light::start(
        &collection.lights,
        &start,
        &collection.context,
        &mut collection.quads[0u16],
        &collection.lines[0u16],
    );
    if collection.effects_page != 65535 && collection.effects_page != saved {
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    if status != n(0) {
        return status;
    }
    inventory::collected(collection.inventory, collection.cooldown);
    attitude::put(hit.result, 0, n(0));
    n(0)
}
