//! Vehicle prepare phase, before sampling, power and synchronous collisions.
use crate::dsl_rt::*;
use crate::{attitude, effect_pool, entities, integer, random, sampling, scene_query, spin};

pub struct State {
    // alternating sign, countdown throttle/RPM, target, temporary timer/slot,
    // restore multiplier/shift and current sidegrip multiplier/shift.
    pub core: Array<u16>,
    pub spin: Array<u16>,
    // The four existing physical boost columns: speed, power, loss, i16 timer.
    pub boost: Array<u16>,
    // camera velocity/timer, target entity, active, applied and phase.
    pub boost_control: Array<u16>,
    pub auxiliary: Array<sampling::Auxiliary>,
}
pub struct Shared {
    // enabled, visible, position XYZ, angular rate and nine Q12 matrix values.
    pub view: Array<u16>,
    pub camera: Array<u16>,
    pub spin: Array<u16>,
    // texture frame/delta, source UV[2], current UV[2], feedback[3].
    pub boost: Array<u16>,
    // available slot, four timestamps and four rows of nine line handles.
    pub temporary: Array<u16>,
    pub pool: Array<effect_pool::Pool>,
    pub seed: Array<u16>,
    pub slot: Array<u16>,
}
pub struct Frame {
    pub dt: fix32,
    pub number: fix32,
    pub active: bool,
    pub player: bool,
    pub entity: fix32,
    pub camera: Array<u16>,
    pub sine: Array<i16>,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(data: Array<u16>, i: u16) -> fix32 {
    attitude::get(data, i)
}
fn p(data: Array<u16>, i: u16, value: fix32) {
    attitude::put(data, i, value);
}
fn smaller(a: fix32, b: fix32) -> fix32 {
    if a < b {
        return a;
    }
    b
}
fn larger(a: fix32, b: fix32) -> fix32 {
    if a > b {
        return a;
    }
    b
}
fn advance_view(view: Array<u16>, frame: &Frame) -> fix32 {
    let mut near = g(view, 0) != n(0);
    let mut axis = 0u16;
    while near && axis < 3 {
        let coordinate = integer::sar(g(view, 2 + axis), 3);
        let camera = g(frame.camera, axis);
        let mut distance = coordinate - camera;
        if distance < n(0) {
            distance = camera - coordinate;
        }
        near = distance < n(19200);
        axis += 2;
    }
    if !near {
        p(view, 1, n(0));
        return n(0);
    }
    let angle = integer::sar(fmul_lo32(g(view, 5), frame.dt), 8);
    if angle != n(0) {
        attitude::rotate(view, 6, 1, angle, frame.sine);
    }
    if frame.number.lo_bits() & 31 == 0 && !attitude::normalize(view, 6) {
        return n(-2);
    }
    p(view, 1, n(1));
    n(0)
}
fn apply(state: &State, config: Array<u16>, player: bool) {
    let b = state.boost;
    p(
        b,
        0,
        smaller(g(b, 0) + g(config, 1), fix32::from_words(41248, 7)),
    );
    p(b, 3, n(5000));
    let mut index = 4u16;
    if player {
        index = 2;
    }
    p(b, 1, g(config, index));
    p(b, 2, g(config, index + 1));
}
fn decay(state: &State, frame: &Frame) {
    let b = state.boost;
    if g(b, 3) > n(0) {
        let timer = (g(b, 3).lo_bits() as i16) - (frame.dt.lo_bits() as i16);
        p(b, 3, integer::from_i16(timer));
        p(b, 1, larger(g(b, 1) - fmul_lo32(g(b, 2), frame.dt), n(0)));
        if timer < 1 && (!frame.player || g(state.boost_control, 5) == n(8)) {
            p(b, 0, n(0));
            p(b, 3, n(0));
            p(b, 1, n(0));
            p(state.boost_control, 3, n(0));
        }
    }
}
fn boost_step(
    state: &State,
    shared: &Shared,
    frame: &Frame,
    config: Array<u16>,
    scene: &scene_query::Scene,
    identity: &entities::State,
) -> fix32 {
    let b = state.boost_control;
    let camera = shared.camera;
    let dt = frame.dt;
    if g(b, 3) == n(0) {
        return n(0);
    }
    if !frame.player {
        if g(b, 4) == n(0) {
            p(b, 4, n(1));
            apply(state, config, false);
        }
    } else if g(b, 5).lo_bits() & 8 == 0 {
        let phase = g(b, 5).lo_bits();
        if phase & 1 != 0 {
            if g(camera, 2) < n(-7199) {
                if g(camera, 0) < n(1) {
                    p(camera, 0, g(camera, 0) + dt);
                }
                if g(camera, 1) > n(2000) {
                    p(camera, 1, g(camera, 1) - dt);
                }
                let mut velocity = g(b, 0) - fmul_lo32(dt, n(32));
                velocity = larger(velocity, n(100));
                if g(camera, 2) < n(-7799) {
                    velocity = smaller(g(b, 0) + fmul_lo32(dt, n(16)), n(5000));
                }
                p(b, 0, velocity);
                p(
                    camera,
                    2,
                    g(camera, 2) + integer::sar(fmul_lo32(dt, velocity), 8),
                );
            } else {
                p(b, 0, g(b, 0) - fmul_lo32(dt, n(8)));
                p(
                    camera,
                    2,
                    g(camera, 2) + integer::sar(fmul_lo32(dt, g(b, 0)), 8),
                );
                p(b, 1, g(b, 1) + dt);
                if g(b, 1) >= n(200) {
                    if g(b, 4) == n(0) {
                        let target = g(b, 2);
                        let class = entities::class(identity, target);
                        if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
                            return n(-1);
                        }
                        let saved = g(shared.slot, 0);
                        p(shared.slot, 0, n(28));
                        let method = scene.method(class.lo_bits(), 28);
                        if method.status == 4 {
                            return n(-1);
                        }
                        if method.status != 2 {
                            if method.status == 1 {
                                if method.id != 76 || target != frame.entity {
                                    return n(-5);
                                }
                                apply(state, config, true);
                            }
                            p(shared.slot, 0, saved);
                        }
                        p(b, 4, n(1));
                    }
                    if g(b, 1) >= n(350) {
                        p(b, 5, n(2));
                        p(b, 1, n(0));
                    }
                }
            }
        } else if phase & 2 != 0 {
            p(b, 0, larger(g(b, 0) - fmul_lo32(dt, n(128)), n(-15000)));
            p(camera, 0, larger(g(camera, 0) - dt, n(-1638)));
            p(camera, 1, smaller(g(camera, 1), n(5500)));
            p(
                camera,
                2,
                g(camera, 2) + integer::sar(fmul_lo32(dt, g(b, 0)), 8),
            );
            if g(camera, 2) < n(-8199) {
                p(b, 5, n(4));
            }
        } else if phase & 4 != 0 {
            if g(b, 0) != n(0) {
                p(b, 0, smaller(g(b, 0) + fmul_lo32(dt, n(128)), n(0)));
                p(
                    camera,
                    2,
                    g(camera, 2) + integer::sar(fmul_lo32(dt, g(b, 0)), 8),
                );
            } else {
                p(b, 5, n(8));
            }
        }
    }
    if g(b, 4) != n(0) && g(shared.boost, 0) != frame.number {
        p(shared.boost, 0, frame.number);
        let value = random::next(g(shared.seed, 0));
        p(shared.seed, 0, value);
        let delta = (value.lo_bits() >> 4) & 63;
        p(shared.boost, 1, fix32::from_words(delta, 0));
        let mut i = 0u16;
        while i < 2 {
            let uv = g(shared.boost, 2 + i).lo_bits() - delta;
            p(shared.boost, 4 + i, fix32::from_words(uv, 0));
            i += 1;
        }
    }
    n(0)
}
fn expire(shared: &Shared, slot: fix32) -> fix32 {
    if slot < n(0) || slot >= n(4) {
        return n(-1);
    }
    let mut pools = shared.pool;
    let pool = &mut pools[0u16];
    let mut i = 0u16;
    while i < 9 {
        let handle = g(shared.temporary, 5 + slot.lo_bits() * 9 + i);
        if effect_pool::release(pool, handle) != 0 {
            return n(-1);
        }
        i += 1;
    }
    p(shared.temporary, 0, slot);
    p(shared.temporary, 1 + slot.lo_bits(), n(0));
    n(0)
}
// Config: first gear limit, turbo increment, player power/loss, peer power/loss.
pub fn prepare(
    state: &State,
    shared: &Shared,
    frame: &Frame,
    config: Array<u16>,
    scene: &scene_query::Scene,
    identity: &entities::State,
) -> fix32 {
    let core = state.core;
    let dt = frame.dt;
    if frame.player {
        let status = advance_view(shared.view, frame);
        if status != n(0) {
            return status;
        }
    }
    p(core, 0, -g(core, 0));
    if !frame.active {
        let addition = integer::sar(
            fmul_lo32(integer::sar(g(core, 1), 5), integer::sar(dt, 1)),
            1,
        );
        let rpm = larger(g(core, 2) + addition - fmul_lo32(dt, n(256)), n(0));
        p(core, 2, smaller(rpm, g(config, 0) - n(10)));
    }
    let mut auxiliary = state.auxiliary;
    auxiliary[0u16].cached = n(0);
    auxiliary[1u16].cached = n(0);
    p(core, 3, n(-1));
    decay(state, frame);
    if g(state.spin, 7) > n(0) {
        let tuning: Buf<u16, 12> =
            Buf::new([27955, 0, 16307, 0, 931, 0, 1, 2049, 1, 2305, 5, 3585]);
        let status = spin::step(
            state.spin,
            dt,
            tuning.as_array(),
            fix32::from_words(0, 1),
            shared.spin,
            shared.seed,
        );
        if status != n(0) {
            return status;
        }
    }
    let status = boost_step(state, shared, frame, config, scene, identity);
    if status != n(0) {
        return status;
    }
    let timer = integer::from_i16(g(core, 4).lo_bits() as i16);
    if timer != n(0) {
        if dt < timer {
            p(core, 4, integer::from_i16((timer - dt).lo_bits() as i16));
        } else {
            p(core, 4, n(0));
            p(core, 8, g(core, 6));
            p(core, 9, g(core, 7));
            let status = expire(shared, integer::from_i16(g(core, 5).lo_bits() as i16));
            if status != n(0) {
                return status;
            }
        }
    }
    n(0)
}
