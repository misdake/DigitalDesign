//! Damage/traction smoke; severe slip synchronously emits into the live impact pool.
use crate::dsl_rt::*;
use crate::{attitude, collision, impact, integer, random};
pub struct Input {
    pub dt: fix32,
    pub player: bool,
    pub special: bool,
    pub velocity: Array<u16>,
    pub lateral: fix32,
    pub slip: fix32,
    pub penetration: fix32,
    pub materials: Array<u16>,
    pub contacts: Array<u16>,
    pub contact_stride: u16,
    pub corners: Array<u16>,
    pub corner_stride: u16,
    // Each pose is matrix9 and position3; the two views may be discontiguous.
    pub turret: Array<u16>,
    pub body: Array<u16>,
}
fn n(x: i16) -> fix32 {
    integer::from_i16(x)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn at(v: Array<u16>, i: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(i as i16).as_u16_array() }
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn narrow(v: fix32) -> fix32 {
    n(v.lo_bits() as i16)
}
fn word(v: fix32) -> fix32 {
    fix32::from_words(v.lo_bits(), 0)
}
fn rotation(seed: Array<u16>) -> fix32 {
    let v = random::next(g(seed, 0));
    p(seed, 0, v);
    fix32::from_words((v.lo_bits() >> 4) | (v.hi_bits() << 12), 0)
}
fn velocity(row: Array<u16>, input: &Input, rising: bool) {
    let mut axis = 0u16;
    while axis < 3 {
        let mut value = integer::trunc_shift(g(input.velocity, axis), 6);
        if rising && axis == 1 {
            value = value + n(800);
        }
        p(row, 3 + axis, value);
        axis += 1;
    }
}
fn ordinary(row: Array<u16>, input: &Input, side: u16, life: fix32, seed: Array<u16>) {
    velocity(row, input, false);
    let mut value = narrow(life);
    p(row, 6, value);
    p(row, 8, rotation(seed));
    if value > n(2048) {
        value = n(2048);
    }
    p(row, 6, value);
    p(row, 7, n(8000));
    let mut axis = 0u16;
    while axis < 3 {
        p(
            row,
            axis,
            g(input.contacts, side * 2 * input.contact_stride + axis),
        );
        axis += 1;
    }
    let mut direction = n(0);
    if side == 0 {
        direction = n(1);
    }
    p(row, 9, direction);
    p(row, 10, n(1));
}
// Header: timer, unsigned side, force, count, damage2, sockets2, damage timers2.
// Then fifteen [position3,velocity3,life,size,rotation,direction,expanding] rows.
// All narrow fields are extended in their canonical i32 columns: 700 B total.
pub fn step(
    state: Array<u16>,
    input: &Input,
    sockets: Array<u16>,
    impacts: &mut impact::State,
    vehicle: &mut impact::Vehicle,
    c: &mut impact::Context,
) -> fix32 {
    let mut force = g(state, 2);
    if input.penetration > force {
        force = input.penetration;
    }
    if force != n(0) {
        force = integer::decay(integer::rate_step(force, n(800), input.dt), 10, input.dt);
    }
    p(state, 2, force);
    p(state, 3, n(0));
    let mut free = 65535u16;
    let mut damaged0 = 65535u16;
    let mut damaged1 = 65535u16;
    let mut i = 0u16;
    while i < 15 {
        let row = at(state, 20 + i * 22);
        if g(row, 6) < n(1) {
            if g(state, 4) != n(0) && damaged0 == 65535 {
                damaged0 = i;
            } else if g(state, 5) != n(0) && damaged1 == 65535 {
                damaged1 = i;
            } else if free == 65535 {
                free = i;
            }
            i += 1;
            continue;
        }
        p(state, 3, g(state, 3) + n(1));
        p(
            row,
            3,
            integer::trunc_shift(integer::mul(g(row, 3), n(940)), 10),
        );
        p(
            row,
            5,
            integer::trunc_shift(integer::mul(g(row, 5), n(940)), 10),
        );
        p(row, 4, g(row, 4) + n(20));
        let mut axis = 0u16;
        while axis < 3 {
            p(
                row,
                axis,
                g(row, axis) + integer::trunc_shift(integer::mul(g(row, 3 + axis), input.dt), 8),
            );
            axis += 1;
        }
        p(
            row,
            6,
            narrow(g(row, 6) - narrow(integer::mul(input.dt, n(2)))),
        );
        if g(row, 10) != n(0) {
            p(row, 7, word(g(row, 7) + integer::mul(input.dt, n(15))));
        }
        let mut angle = integer::mul(input.dt, n(40));
        if g(row, 9) != n(0) {
            angle = -angle;
        }
        p(row, 8, word(g(row, 8) + angle));
        i += 1;
    }
    p(state, 0, g(state, 0) + input.dt);
    p(state, 8, g(state, 8) + input.dt);
    p(state, 9, g(state, 9) + input.dt);
    let mut body = 0u16;
    while body < 2 {
        let mut slot = damaged0;
        if body == 1 {
            slot = damaged1;
        }
        if slot != 65535 {
            let mut threshold = n(300);
            if g(state, 4 + body) > n(8) {
                threshold = n(150);
            }
            if g(state, 8 + body) >= threshold {
                p(state, 8 + body, n(0));
                let row = at(state, 20 + slot * 22);
                velocity(row, input, true);
                p(row, 8, rotation(c.seed));
                p(row, 6, n(2048));
                p(row, 7, n(5600));
                let socket = g(state, 6 + body);
                if socket.hi_bits() != 0 || socket.lo_bits() >= 6 {
                    return n(-1);
                }
                let mut pose = input.turret;
                if body != 0 {
                    pose = input.body;
                }
                collision::transform(
                    row,
                    pose,
                    at(sockets, (body * 18 + socket.lo_bits() * 3) * 2),
                    false,
                );
                let mut axis = 0u16;
                while axis < 3 {
                    p(row, axis, g(row, axis) + g(pose, 9 + axis));
                    axis += 1;
                }
                p(row, 10, n(0));
            } else if free == 65535 {
                free = slot;
            }
        }
        body += 1;
    }
    if free == 65535 || g(state, 0) < n(70) || (input.special && input.player) {
        return n(0);
    }
    let row = at(state, 20 + free * 22);
    let value = input.lateral;
    if (value < fix32::from_words(30536, 65535) && g(input.materials, 0) == n(1))
        || (value > fix32::from_words(35000, 0) && g(input.materials, 1) == n(1))
    {
        let mut side = 0u16;
        if value > fix32::from_words(35000, 0) {
            side = 1;
        }
        ordinary(
            row,
            input,
            side,
            integer::trunc_shift(abs(value) - fix32::from_words(35000, 0), 5),
            c.seed,
        );
        p(state, 0, n(0));
        if !input.player
            || (side == 1 && value < fix32::from_words(64465, 1))
            || (side == 0 && value > fix32::from_words(1071, 65534))
        {
            return n(0);
        }
        if collision::distance(input.velocity) < fix32::from_words(53393, 3) {
            return n(0);
        }
        let mut contact = side * 2;
        if vehicle.forward >= n(0) {
            contact += 1;
        }
        let mut kind = n(1);
        if side != 0 {
            kind = n(2);
        }
        let status = impact::emit(
            impacts,
            vehicle,
            g(input.corners, contact * input.corner_stride),
            kind,
            c,
        );
        if status < 2 {
            return n(0);
        }
        if status == 2 {
            return n(-2);
        }
        if status == 3 {
            return n(-1);
        }
        return n(-4);
    }
    let magnitude = abs(input.slip);
    if force < fix32::from_words(2305, 61) && magnitude < n(1) {
        return n(0);
    }
    let side = n(1) - g(state, 1);
    p(state, 1, side);
    if side.hi_bits() != 0 || side.lo_bits() >= 2 {
        return n(-1);
    }
    if g(input.materials, side.lo_bits()) != n(1) {
        return n(0);
    }
    let mut life = integer::sar(magnitude, 4);
    if magnitude < n(1) {
        life = integer::trunc_shift(force - fix32::from_words(2304, 61), 11);
    }
    ordinary(row, input, side.lo_bits(), life, c.seed);
    p(state, 0, n(0));
    n(0)
}
