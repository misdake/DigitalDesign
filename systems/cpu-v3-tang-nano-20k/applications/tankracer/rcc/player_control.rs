//! Sampled player controls and the state changes before parent Tank dispatch.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer};

pub struct Input {
    // Same seven columns as drive::Frame 19..26.
    pub drive: Array<u16>,
    // Start-held and fast-start bytes, as two i32 columns.
    pub state: Array<u16>,
    // The prefix owns countdown throttle independently of the input flags.
    pub countdown: Array<u16>,
    pub analog_latch: Array<u16>,
}
pub struct Brake {
    // These fields retain their existing owners.
    pub saved_turn: Array<u16>,
    pub reverse_braking: Array<u16>,
    pub drive: Array<u16>,
    pub max_turn: Array<u16>,
    pub yaw_lock: Array<u16>,
}
pub struct Collision {
    // impact_accumulator and collision_size, as shared by scenery_contact.
    pub counters: Array<u16>,
    pub ground_delta: Array<u16>,
    pub tank_delta: Array<u16>,
    pub obstacle_impulse: Array<u16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn narrow(v: fix32) -> fix32 {
    n(v.lo_bits() as i16)
}
fn mul(a: fix32, b: fix32) -> fix32 {
    integer::mul(a, b)
}
fn min(a: fix32, b: fix32) -> fix32 {
    if a < b {
        return a;
    }
    b
}
fn max(a: fix32, b: fix32) -> fix32 {
    if a > b {
        return a;
    }
    b
}
fn abs(a: fix32) -> fix32 {
    if a < n(0) {
        return -a;
    }
    a
}
fn rate(value: fix32, force: fix32, dt: fix32) -> fix32 {
    let amount = mul(force, dt);
    if value > amount {
        return value - amount;
    }
    if value < -amount {
        return value + amount;
    }
    n(0)
}
fn press(state: Array<u16>, stage: fix32) {
    if stage == n(1) {
        p(state, 0, n(1));
    } else if stage == n(2) && g(state, 0) == n(0) {
        p(state, 1, n(1));
    }
}
// Frame: dt, driving, signed player slot, speed, start stage, analog,
// analog-longitudinal, twin tracks, left track, right track.
// Tuning: push throttle/brake/steer, signed return throttle/brake, return steer.
// Keys bits: left, right, throttle, brake, auxiliary; axes: steering/longitudinal.
// Device sampling is external. Return bits: driving, auxiliary-down.
pub fn input(s: &Input, f: Array<u16>, t: Array<u16>, mut keys: u16, axes: Array<u16>) -> u16 {
    let dt = g(f, 0);
    if g(f, 2) != n(0) {
        keys = 0;
    }
    let analog = g(f, 5) != n(0);
    let longitudinal = analog && g(f, 6) != n(0);
    let stage = g(f, 4);
    let mut latch = s.analog_latch;
    if g(f, 1) == n(0) {
        if stage != n(4) {
            let throttle = keys & 4 != 0;
            let brake = keys & 8 != 0;
            if throttle {
                p(s.countdown, 0, g(s.countdown, 0) + mul(g(t, 0), dt));
                press(s.state, stage);
            } else {
                p(s.countdown, 0, rate(g(s.countdown, 0), g(t, 3), dt));
                if stage >= n(1) && stage < n(3) && !analog {
                    p(s.state, 0, n(0));
                }
            }
            if brake {
                p(s.drive, 1, g(s.drive, 1) + mul(g(t, 1), dt));
            } else {
                p(s.drive, 1, rate(g(s.drive, 1), g(t, 4), dt));
            }
            if longitudinal && !throttle && !brake {
                let axis = g(axes, 1);
                if axis != n(0) {
                    p(s.countdown, 0, mul(axis, n(-2)));
                    if axis < n(-16384) {
                        press(s.state, stage);
                    } else if axis < n(0) && stage >= n(1) && stage < n(3) {
                        p(s.state, 0, n(0));
                    }
                }
            }
            p(
                s.countdown,
                0,
                min(max(g(s.countdown, 0), n(0)), fix32::from_words(65535, 0)),
            );
        }
        return 0;
    }
    let speed = g(f, 3);
    if analog && g(f, 7) != n(0) {
        let left = g(f, 8);
        let right = g(f, 9);
        let opposite = -(left + right);
        p(s.drive, 2, right - left);
        p(s.drive, 0, n(0));
        p(s.drive, 1, n(0));
        if opposite > n(0) && speed < n(0) {
            p(s.drive, 1, opposite);
        } else if opposite <= n(0) && speed > n(0) {
            p(s.drive, 1, -opposite);
        } else {
            p(s.drive, 0, opposite);
        }
    } else {
        let reverse = (keys & 8 != 0 && speed == n(0)) || speed < n(0);
        let mut push = keys & 4 != 0;
        let mut stop = keys & 8 != 0;
        let mut force = g(t, 0);
        if reverse {
            push = keys & 8 != 0;
            stop = keys & 4 != 0;
            force = -force;
        }
        if push {
            p(s.drive, 0, g(s.drive, 0) + mul(force, dt));
        } else {
            p(s.drive, 0, rate(g(s.drive, 0), g(t, 3), dt));
        }
        if stop {
            p(s.drive, 1, g(s.drive, 1) + mul(g(t, 1), dt));
        } else {
            p(s.drive, 1, rate(g(s.drive, 1), g(t, 4), dt));
        }
        if longitudinal && !push && !stop {
            let value = g(axes, 1);
            if speed > n(0) && value > n(0) {
                p(s.drive, 1, mul(value, n(2)));
            } else if speed < n(0) && value < n(0) {
                p(s.drive, 1, mul(value, n(-2)));
            } else if value != n(0) {
                p(s.drive, 0, mul(value, n(-2)));
            }
        }
        if analog {
            let axis = g(axes, 0);
            if axis != n(0) {
                latch[0u16] = 1;
            }
            if latch[0u16] != 0 {
                p(s.drive, 2, mul(axis, n(2)));
            }
        }
        if keys & 1 != 0 {
            p(s.drive, 2, g(s.drive, 2) - mul(g(t, 2), dt));
            latch[0u16] = 0;
        } else if keys & 2 != 0 {
            p(s.drive, 2, g(s.drive, 2) + mul(g(t, 2), dt));
            latch[0u16] = 0;
        } else if latch[0u16] == 0 {
            let steer = g(s.drive, 2);
            let step = mul(g(t, 5), dt);
            if steer > n(0) {
                p(s.drive, 2, max(steer - step, n(0)));
            } else if steer < n(0) {
                p(s.drive, 2, min(steer + step, n(0)));
            }
        }
    }
    let bound = fix32::from_words(65535, 0);
    p(s.drive, 0, min(max(g(s.drive, 0), -bound), bound));
    p(s.drive, 1, min(max(g(s.drive, 1), n(0)), bound));
    p(s.drive, 2, min(max(g(s.drive, 2), -bound), bound));
    if keys & 16 != 0 {
        return 3;
    }
    1
}
fn turret_integrate(state: Array<u16>, angle: Array<u16>, dt: fix32) {
    let before = rate(g(state, 0), n(1), dt);
    let mut velocity = before - integer::sar(mul(before, dt), 7);
    if (before < n(0) && velocity > n(0)) || (before > n(0) && velocity < n(0)) {
        velocity = n(0);
    }
    p(state, 0, velocity);
    let value = g(angle, 0).lo_bits() as i16;
    let delta = integer::sar(mul(dt, velocity), 6).lo_bits() as i16;
    p(angle, 0, n(value + delta));
}
// State: velocity, cooldown, centering, moving, signed center-lock, pressed2.
// Keys bits: turret left/right. Angle is the signed i16 value in a raw i32 pose column.
pub fn turret(state: Array<u16>, angle: Array<u16>, keys: u16, dt: fix32) {
    p(state, 5, n(0));
    p(state, 6, n(0));
    p(state, 1, g(state, 1) - dt);
    let mut i = 0u16;
    while i < 2 {
        if keys & (1u16 << i) != 0 && g(state, 1) < n(0) {
            let mut force = mul(n(22), dt);
            if i == 1 {
                force = -force;
            }
            p(state, 0, g(state, 0) + force);
            p(state, 5 + i, n(1));
            p(state, 2, n(0));
        }
        i += 1;
    }
    if g(state, 5) != n(0)
        && g(state, 6) != n(0)
        && g(state, 2) == n(0)
        && (g(angle, 0).lo_bits() as i16) != 0
        && g(state, 4) < n(1)
    {
        p(state, 2, n(1));
        p(state, 1, n(600));
    }
    let before = g(angle, 0).lo_bits() as i16;
    if g(state, 2) == n(0) {
        turret_integrate(state, angle, dt);
    } else if before != 0 {
        let mut force = mul(n(44), dt);
        if before > 0 {
            force = -force;
        }
        p(state, 0, g(state, 0) + force);
        turret_integrate(state, angle, dt);
        if (before < 0 && (g(angle, 0).lo_bits() as i16) > 0)
            || (before > 0 && (g(angle, 0).lo_bits() as i16) < 0)
        {
            p(angle, 0, n(0));
            p(state, 0, n(0));
            p(state, 2, n(0));
        }
    }
    let mut moving = n(0);
    if g(state, 0) != n(0) {
        moving = n(1);
    }
    p(state, 3, moving);
}
// Reference: unsigned slide multiplier, signed rotation. Frame: dt/speed/mode/
// signed temporary timer/auxiliary-down. The drive fields remain shared with input.
pub fn restore(s: &Brake, dt: fix32, mode: fix32, reference: Array<u16>) {
    let slide = g(reference, 0);
    let rotation = g(reference, 1);
    if g(s.drive, 5) != slide {
        let value = narrow(g(s.drive, 5) + mul(dt, n(2)));
        p(s.drive, 5, value);
        if value > slide {
            p(s.drive, 5, narrow(slide));
        }
    }
    let current = g(s.drive, 6);
    let delta = narrow(integer::sar(dt, 2));
    if current < rotation {
        p(s.drive, 6, narrow(current + delta));
        if g(s.drive, 6) > rotation {
            p(s.drive, 6, rotation);
        }
    } else if current > rotation {
        p(s.drive, 6, narrow(current - delta));
        if g(s.drive, 6) < rotation {
            p(s.drive, 6, rotation);
        }
    }
    let turn = g(s.saved_turn, 0);
    if g(s.max_turn, 0) != turn {
        p(s.max_turn, 0, max(g(s.max_turn, 0) - mul(dt, n(4)), turn));
    }
    if g(s.drive, 5) == slide && g(s.drive, 6) == rotation && g(s.max_turn, 0) == turn {
        p(s.drive, 4, n(0));
        if mode == n(4) {
            p(s.yaw_lock, 0, n(2000));
        }
    }
}
pub fn brake(s: &Brake, f: Array<u16>, reference: Array<u16>) {
    let dt = g(f, 0);
    let mode = g(f, 2);
    let down = g(f, 4) != n(0);
    if g(f, 3) == n(0) {
        if down && abs(g(f, 1)) > fix32::from_words(53392, 3) {
            p(s.drive, 5, max(narrow(g(s.drive, 5) - mul(dt, n(2))), n(0)));
            let mut rotation = n(0);
            if mode == n(4) {
                rotation = n(400);
            } else if mode == n(5) {
                rotation = n(200);
            }
            p(s.drive, 6, rotation);
            if g(s.drive, 4) == n(0) {
                p(s.saved_turn, 0, narrow(g(s.max_turn, 0)));
                p(s.max_turn, 0, n(310));
                p(s.drive, 4, n(1));
            }
        } else if g(s.drive, 4) != n(0) {
            restore(s, dt, mode, reference);
        }
        if down {
            p(s.drive, 1, fix32::from_words(40000, 0));
            p(s.drive, 0, n(0));
        }
        let mut reverse = n(0);
        if down {
            reverse = n(1);
        }
        p(s.reverse_braking, 0, reverse);
    } else if g(s.drive, 4) != n(0) {
        p(s.drive, 6, g(reference, 1));
        p(s.drive, 5, narrow(g(reference, 0)));
        p(s.max_turn, 0, g(s.saved_turn, 0));
        p(s.drive, 4, n(0));
    }
}
pub fn collision_decay(s: &Collision, dt: fix32, race_active: bool) {
    if !race_active {
        return;
    }
    let mut marker = g(s.counters, 1);
    if marker != n(0) {
        marker = max(marker - dt, n(0));
    }
    let mut i = 0u16;
    while i < 3 {
        let mut change = g(s.obstacle_impulse, 0);
        if i == 0 {
            change = collision::distance(s.ground_delta);
        } else if i == 1 {
            change = collision::distance(s.tank_delta);
        }
        if change > n(30000) {
            marker = marker + integer::sar(change - n(30000), 11);
        }
        i += 1;
    }
    p(s.counters, 1, min(marker, n(1250)));
    let impact = g(s.counters, 0);
    if impact != n(0) {
        let mut value = n(0);
        if impact >= dt {
            value = impact - dt;
        }
        p(s.counters, 0, value);
    }
}
