//! Immediate debris calls on the same car, RNG and scenery owners as driving.
use crate::dsl_rt::*;
use crate::{attitude, blast, integer, ordered, scene_parent, shatter};

fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn at(data: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset as i16).as_u16_array() }
}
fn error(status: u16) -> fix32 {
    if status == 3 {
        return n(-3);
    }
    if status == 4 {
        return n(-4);
    }
    n(-1)
}
fn hit_inner(
    state: &mut scene_parent::State,
    actor: &scene_parent::Actor,
    call: &scene_parent::Hit,
) -> fix32 {
    if call.auxiliary.row[7u16] & 8 == 0 {
        return n(-1);
    }
    let debris = &actor.debris[0u16];
    let incoming: Buf<u16, 6> = Buf::new([0; 6]);
    let status = shatter::begin(
        debris,
        incoming.as_array(),
        call.auxiliary.velocity,
        state.lifetime.limit,
        call.sine,
    );
    if status < n(0) {
        return status;
    }
    let entity = attitude::get(actor.fragments.collider, 0);
    if status == n(0) {
        attitude::put(call.auxiliary.result, 0, entity);
        return n(0);
    }
    if attitude::get(debris.header, 0).lo_bits() & 4 != 0 {
        if call.current >= state.car_count {
            return n(-1);
        }
        let current = state.cars[call.current].slot;
        let origin = at(actor.fragments.collider, 2);
        let mut slot = 0u16;
        while slot < 9 {
            let mut index = state.slots[slot];
            if slot == current {
                index = call.current;
            }
            if index != 65535 {
                if index >= state.car_count {
                    return n(-1);
                }
                let car = &mut state.cars[index];
                if shatter::affected(at(car.motion, 18), origin, attitude::get(car.blast, 3)) {
                    if call.mode != n(0) && call.mode != n(1) {
                        return n(-5);
                    }
                    let status = blast::racer(car, origin, n(4), state.runtime.seed);
                    if status < n(0) {
                        return status;
                    }
                    // Feedback is an immediate successful external device call.
                }
            }
            slot += 1;
        }
    }
    let status = shatter::activate(
        debris,
        at(actor.fragments.collider, 2),
        incoming.as_array(),
        state.runtime.seed,
        state.lifetime.limit,
        call.sine,
    );
    if status != n(0) {
        return status;
    }
    let id = fix32::from_words(actor.fragments.id, 0);
    let status = ordered::remove_first(&mut state.runtime.ordinary, id, state.lifetime.limit);
    if status != 0 {
        return error(status);
    }
    let found = ordered::find(&state.runtime.special, id, state.lifetime.limit);
    if found.status != 0 {
        return error(found.status);
    }
    if !found.found {
        let status = ordered::append(&mut state.runtime.special, id);
        if status != 0 {
            return error(status);
        }
    }
    attitude::put(call.auxiliary.result, 0, entity);
    n(0)
}
pub fn hit(
    state: &mut scene_parent::State,
    actor: &scene_parent::Actor,
    call: &scene_parent::Hit,
) -> fix32 {
    let saved = read_dseg_page(3);
    if actor.fragments.motion_page != 65535 {
        unsafe {
            write_dseg_page(3, actor.fragments.motion_page);
        }
    }
    let status = hit_inner(state, actor, call);
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
pub fn active(actor: &scene_parent::Actor) -> bool {
    let saved = read_dseg_page(3);
    if actor.fragments.motion_page != 65535 {
        unsafe {
            write_dseg_page(3, actor.fragments.motion_page);
        }
    }
    let active = attitude::get(actor.debris[0u16].header, 0).lo_bits() & 1 != 0;
    unsafe {
        write_dseg_page(3, saved);
    }
    active
}
pub fn retained(actor: &scene_parent::Actor) -> bool {
    let saved = read_dseg_page(3);
    if actor.fragments.motion_page != 65535 {
        unsafe {
            write_dseg_page(3, actor.fragments.motion_page);
        }
    }
    let retained = attitude::get(actor.debris[0u16].header, 0).lo_bits() & 16 != 0;
    unsafe {
        write_dseg_page(3, saved);
    }
    retained
}
pub fn tick(
    state: &mut scene_parent::State,
    actor: &scene_parent::Actor,
    frame: &scene_parent::Frame,
) -> fix32 {
    let saved = read_dseg_page(3);
    if actor.fragments.motion_page != 65535 {
        unsafe {
            write_dseg_page(3, actor.fragments.motion_page);
        }
    }
    let step = shatter::Frame {
        entity: frame.auxiliary.entity,
        dt: frame.auxiliary.dt,
        group: frame.group,
        limit: state.lifetime.limit,
        sine: frame.auxiliary.sine,
        angle: frame.angle,
    };
    let status = shatter::tick(&actor.debris[0u16], &step, state.runtime.seed);
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
