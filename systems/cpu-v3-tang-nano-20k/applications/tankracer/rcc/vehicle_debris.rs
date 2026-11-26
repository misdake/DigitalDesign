//! Two shared debris banks; bank theft immediately updates the previous owner.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, random};
pub struct Owners {
    // entity, active, cooldown, previous-plane. Stride is in i32 columns.
    pub rows: Array<u16>,
    pub count: u16,
    pub stride: u16,
}
pub struct Input {
    pub dt: fix32,
    pub player: bool,
    pub position: Array<u16>,
    pub velocity: Array<u16>,
    pub has_plane: fix32,
    pub plane_distance: fix32,
    pub materials: Array<u16>,
    pub contacts: Array<u16>,
    pub contact_stride: u16,
    pub camera_position: Array<u16>,
    pub camera_forward: Array<u16>,
}
pub struct Context {
    pub kinds: Array<i16>,
    pub seed: Array<u16>,
    pub impact_accumulator: Array<u16>,
    // Sound-draw presence and value belong to the shared world, not the car.
    pub sound: Array<u16>,
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
fn owner(o: &Owners, i: u16) -> Array<u16> {
    at(o.rows, i * o.stride * 2)
}
fn bank(state: Array<u16>, player: bool) -> Array<u16> {
    if player {
        return at(state, 182);
    }
    state
}
fn draw(c: &Context) -> fix32 {
    let v = random::next(g(c.seed, 0));
    p(c.seed, 0, v);
    v
}
fn jitter(c: &Context, mask: u16) -> fix32 {
    let v = draw(c);
    fix32::from_words(((v.lo_bits() >> 4) | (v.hi_bits() << 12)) & mask, 0)
}
fn color(c: &Context, surface: bool, component: u16) -> fix32 {
    draw(c);
    draw(c);
    draw(c);
    let v = draw(c);
    let mut factor = n(30);
    let mut mask = 4095u16;
    let mut shift = 8u16;
    if surface {
        mask = 16383;
        shift = 6;
        factor = n(33);
        if component == 1 {
            factor = n(18);
        } else if component == 2 {
            factor = n(10);
        }
    }
    let sample = fix32::from_words(integer::sar(v, shift).lo_bits() & mask, 0);
    fix32::from_words(
        integer::sar(
            integer::mul(fix32::from_words(65535, 0) - sample, factor),
            16,
        )
        .lo_bits()
            & 255,
        0,
    )
}
// Each 91-column bank: eight [velocity3,position3,color3,active,size] rows,
// followed by floor, owner presence, owner entity. Total 728 B.
// Vehicle borrows the active, cooldown and previous-plane columns directly.
pub fn clear(state: Array<u16>, player: bool, vehicle: Array<u16>) {
    if g(vehicle, 0) == n(0) {
        return;
    }
    let b = bank(state, player);
    let mut i = 0u16;
    while i < 8 {
        p(b, i * 11 + 9, n(0));
        i += 1;
    }
    p(b, 89, n(0));
    p(b, 90, n(0));
    p(vehicle, 0, n(0));
    p(vehicle, 1, n(0));
}
pub fn step(state: Array<u16>, owners: &Owners, own: u16, input: &Input, c: &Context) -> fix32 {
    let b = bank(state, input.player);
    if own >= owners.count {
        return n(-1);
    }
    let vehicle = owner(owners, own);
    let entity = g(vehicle, 0);
    if g(vehicle, 1) != n(0) {
        let gravity = integer::trunc_shift(integer::mul(input.dt, n(5000)), 8);
        let mut active = false;
        let mut i = 0u16;
        while i < 8 {
            let row = at(b, i * 22);
            if g(row, 9) != n(0) {
                p(row, 1, g(row, 1) - gravity);
                let mut axis = 0u16;
                while axis < 3 {
                    p(
                        row,
                        3 + axis,
                        g(row, 3 + axis)
                            + integer::trunc_shift(integer::mul(input.dt, g(row, axis)), 8),
                    );
                    axis += 1;
                }
                if g(row, 4) < g(b, 88) {
                    p(row, 9, n(0));
                } else {
                    active = true;
                }
            }
            i += 1;
        }
        if !active {
            p(b, 89, n(0));
            p(b, 90, n(0));
            p(vehicle, 1, n(0));
        }
    }
    let speed = collision::distance(input.velocity);
    let mut timer = g(vehicle, 2) - input.dt;
    if timer < n(0) {
        timer = n(0);
    }
    p(vehicle, 2, timer);
    let mut vy = g(input.velocity, 1);
    if vy < n(0) {
        vy = -vy;
    }
    let fast = speed.hi_bits() > 1 || (speed.hi_bits() == 1 && speed.lo_bits() > 36864);
    if input.has_plane != n(0)
        && timer == n(0)
        && !(g(vehicle, 3) != n(0) && (g(vehicle, 1) != n(0) || vy < fix32::from_words(36865, 1)))
        && fast
    {
        let sound = draw(c);
        p(c.sound, 0, n(1));
        p(c.sound, 1, sound);
        let surfaces: Buf<u16, 2> = Buf::new([0; 2]);
        let mut surface = surfaces.as_array();
        let mut side = 0u16;
        while side < 2 {
            let material = g(input.materials, side);
            if material.hi_bits() != 0 || material.lo_bits() >= 16 {
                return n(-1);
            }
            if c.kinds[material.lo_bits()] == 1i16 {
                surface[side] = 1;
            }
            side += 1;
        }
        p(vehicle, 1, n(1));
        p(vehicle, 2, n(500));
        p(c.impact_accumulator, 0, g(c.impact_accumulator, 0) + n(100));
        let mut dot = n(0);
        let mut axis = 0u16;
        while axis < 3 {
            dot = dot
                + integer::mul(
                    g(input.position, axis) - integer::mul(g(input.camera_position, axis), n(8)),
                    g(input.camera_forward, axis),
                );
            axis += 1;
        }
        if input.player || dot >= n(0) {
            if g(b, 89) != n(0) {
                let previous = g(b, 90);
                let mut found = false;
                let mut i = 0u16;
                while i < owners.count {
                    let row = owner(owners, i);
                    if g(row, 0) == previous {
                        p(row, 1, n(0));
                        found = true;
                        break;
                    }
                    i += 1;
                }
                if !found {
                    return n(-1);
                }
            }
            p(vehicle, 1, n(1));
            p(b, 89, n(1));
            p(b, 90, entity);
            p(b, 88, integer::mul(input.plane_distance, n(-8)));
            let mut i = 0u16;
            while i < 8 {
                let row = at(b, i * 22);
                side = i & 1;
                axis = 0;
                while axis < 3 {
                    p(row, 6 + axis, color(c, surface[side] != 0, axis));
                    axis += 1;
                }
                p(
                    row,
                    3,
                    g(input.contacts, side * input.contact_stride) + jitter(c, 2047) - n(1024),
                );
                p(
                    row,
                    5,
                    g(input.contacts, side * input.contact_stride + 2) + jitter(c, 2047) - n(1024),
                );
                p(row, 4, g(b, 88));
                axis = 0;
                while axis < 3 {
                    if axis != 1 {
                        p(
                            row,
                            axis,
                            integer::trunc_shift(g(input.velocity, axis), 7) + jitter(c, 2047)
                                - n(1024)
                                + integer::mul(g(row, 3 + axis) - g(input.position, axis), n(2)),
                        );
                    }
                    axis += 1;
                }
                let negative = -g(input.velocity, 1);
                let mut vertical = integer::sar(negative, 7);
                if negative < n(0) {
                    vertical = -integer::sar(g(input.velocity, 1), 7);
                }
                let s = fix32::from_words(
                    (speed.lo_bits() >> 8) | (speed.hi_bits() << 8),
                    speed.hi_bits() >> 8,
                );
                p(row, 1, jitter(c, 1023) + s + vertical);
                p(row, 9, n(1));
                p(row, 10, jitter(c, 511) + n(500));
                i += 1;
            }
        }
    }
    p(vehicle, 3, input.has_plane);
    n(0)
}
