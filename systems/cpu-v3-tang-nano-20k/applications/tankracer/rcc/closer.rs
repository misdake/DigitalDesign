//! Closing-barrier polling and movement on live state and Stop queues.
use crate::dsl_rt::*;
use crate::{attitude, integer, inverse_basis, lifecycle, scene_world};

// Three i32 state fields: speed, flags, phase (0 initial, 1 waiting,
// 2 closing, 3 fragments). The scene owner handles fragment dispatch.
pub struct Step {
    pub dt: fix32,
    pub entity: fix32,
    pub limit: fix32,
}
pub struct Tables {
    pub sine: Array<i16>,
    pub inverse_cosine: Array<i16>,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn abs(value: fix32) -> fix32 {
    if value < n(0) {
        return -value;
    }
    value
}
fn exhausted(work: fix32, limit: fix32) -> bool {
    work.hi_bits() > limit.hi_bits()
        || (work.hi_bits() == limit.hi_bits() && work.lo_bits() >= limit.lo_bits())
}
pub fn poll(
    state: Array<u16>,
    entity: fix32,
    frame: fix32,
    position: Array<u16>,
    player: Array<u16>,
    has_player: bool,
) {
    if (entity.lo_bits() ^ frame.lo_bits()) & 7 != 0 {
        return;
    }
    let mut near = entity != n(0);
    if has_player {
        near = abs(attitude::get(player, 0) - attitude::get(position, 0))
            < fix32::from_words(40960, 0)
            && abs(attitude::get(player, 2) - attitude::get(position, 2))
                < fix32::from_words(40960, 0);
    }
    if near {
        attitude::put(state, 2, n(2));
    }
}
pub fn advance(
    state: Array<u16>,
    matrix: Array<u16>,
    runtime: &mut scene_world::Runtime,
    step: &Step,
    tables: &Tables,
) -> fix32 {
    if step.limit == n(0) {
        return n(-1);
    }
    let angles: Buf<u16, 8> = Buf::new([0; 8]);
    if !inverse_basis::angles(
        angles.as_array(),
        matrix,
        tables.sine,
        tables.inverse_cosine,
    ) {
        return n(-2);
    }
    let mut roll = attitude::get(angles.as_array(), 2);
    let mut work = n(0);
    while roll > n(32760) {
        if exhausted(work, step.limit) {
            return n(-3);
        }
        work = work + n(1);
        roll = roll - fix32::from_words(65520, 0);
    }
    while roll <= n(-32760) {
        if exhausted(work, step.limit) {
            return n(-3);
        }
        work = work + n(1);
        roll = roll + fix32::from_words(65520, 0);
    }
    let target = -roll;
    let acceleration = integer::sar(integer::mul(step.dt, n(1820)), 8);
    let mut speed = attitude::get(state, 0);
    let original_flags = attitude::get(state, 1);
    let mut flags = original_flags.lo_bits();
    if flags & 4 != 0 {
        speed = speed + acceleration;
    } else if speed < acceleration {
        flags |= 4;
        speed = acceleration - speed;
    } else {
        speed = speed - acceleration;
    }
    attitude::put(state, 0, speed);
    attitude::put(state, 1, fix32::from_words(flags, original_flags.hi_bits()));
    let masked = fix32::from_words(speed.lo_bits() & 65532, speed.hi_bits());
    let mut delta = integer::sar(integer::mul(step.dt, speed), 8);
    let mut stop = false;
    if flags & 4 != 0 {
        if target < n(0) && abs(target) < delta {
            delta = target;
            if masked > n(7280) {
                flags &= 65529;
                attitude::put(state, 0, integer::sar(speed, 2));
            } else {
                flags |= 1;
                stop = true;
            }
        } else if target >= n(0) && target < delta {
            delta = target;
            if masked != n(0) {
                flags = (flags & 65531) | 2;
                attitude::put(state, 0, integer::sar(speed, 2));
            } else {
                flags |= 1;
                stop = true;
            }
        } else if target < n(0) {
            delta = -delta;
        }
    } else if flags & 2 != 0 {
        delta = -delta;
    }
    attitude::put(state, 1, fix32::from_words(flags, original_flags.hi_bits()));
    if stop {
        let status = lifecycle::queue(
            &mut runtime.queues,
            &mut runtime.ticks,
            step.entity,
            false,
            step.limit,
        );
        if status == 3 {
            return n(-3);
        }
        if status != 0 {
            return n(-1);
        }
    }
    attitude::rotate(matrix, 0, 2, delta, tables.sine);
    // Rotation precedes normalization. Failed normalization keeps that rotation
    // but never publishes its partially normalized temporary rows.
    let normalized: Buf<u16, 18> = Buf::new([0; 18]);
    let pending = normalized.as_array();
    let mut i = 0u16;
    while i < 9 {
        attitude::put(pending, i, attitude::get(matrix, i));
        i += 1;
    }
    if !attitude::normalize(pending, 0) {
        return n(-2);
    }
    i = 0;
    while i < 9 {
        attitude::put(matrix, i, attitude::get(pending, i));
        i += 1;
    }
    n(0)
}
