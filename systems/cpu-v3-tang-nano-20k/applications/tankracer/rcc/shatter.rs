//! Debris activation and motion on persistent integer rows.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, random, trig_lookup};

pub struct State {
    // flags, retired(u16), ground, gravity, friction, direction, offset XYZ.
    pub header: Array<u16>,
    // 21 mutable i32 fields: position XYZ, velocity XYZ, basis[9], angular
    // XYZ, previous tilt, flags, sprite angle. Render translation stays cold.
    pub pieces: Array<u16>,
    // Three source words per piece: i16 velocity/spin gains and u16 radius.
    pub info: Array<u16>,
    pub count: u16,
    // position XYZ, initial size, cycles, growth, timer, rotation, cycle,
    // size XY, UV[4], sheet size, cell size. Narrow fields are widened raw.
    pub animation: Array<u16>,
    pub has_animation: bool,
    pub has_sprite: bool,
    pub sprite_radius: fix32,
}
pub struct Frame {
    pub entity: fix32,
    pub dt: fix32,
    pub group: fix32,
    pub limit: fix32,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
pub fn empty() -> State {
    let words = unsafe { Ptr::from_addr(0).as_u16_array() };
    State {
        header: words,
        pieces: words,
        info: words,
        count: 0,
        animation: words,
        has_animation: false,
        has_sprite: false,
        sprite_radius: fix32::zero(),
    }
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn at(data: Array<u16>, word: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(word as i16).as_u16_array() }
}
fn g(data: Array<u16>, index: u16) -> fix32 {
    attitude::get(data, index)
}
fn p(data: Array<u16>, index: u16, value: fix32) {
    attitude::put(data, index, value);
}
fn magnitude(value: fix32) -> fix32 {
    if value < n(0) {
        return -value;
    }
    value
}
fn bits(data: Array<u16>, index: u16, set: u16, clear: u16) {
    let value = g(data, index);
    p(
        data,
        index,
        fix32::from_words((value.lo_bits() & (65535 - clear)) | set, value.hi_bits()),
    );
}
fn draw(seed: Array<u16>) -> fix32 {
    let value = random::next(g(seed, 0));
    p(seed, 0, value);
    value
}
fn random_word(seed: Array<u16>) -> u16 {
    let value = draw(seed);
    (value.lo_bits() >> 4) | (value.hi_bits() << 12)
}
fn exhausted(index: u16, limit: fix32) -> bool {
    limit.hi_bits() == 0 && index >= limit.lo_bits()
}
// All debris basis constructions have zero roll. Keep the source's arithmetic
// Q12 shifts, including negation before the XZ product's shift.
fn basis(out: Array<u16>, pitch: fix32, yaw: fix32, sine: Array<i16>) {
    let x = (pitch.lo_bits() & 65532) >> 2;
    let y = (yaw.lo_bits() & 65532) >> 2;
    let cx = trig_lookup::index(sine, 4096 + x);
    let sx = trig_lookup::index(sine, x);
    let cy = trig_lookup::index(sine, 4096 + y);
    let sy = trig_lookup::index(sine, y);
    p(out, 0, cy);
    p(out, 1, n(0));
    p(out, 2, sy);
    p(out, 3, integer::sar(fmul_lo32(sy, sx), 12));
    p(out, 4, cx);
    p(out, 5, -integer::sar(fmul_lo32(cy, sx), 12));
    p(out, 6, integer::sar(-fmul_lo32(cx, sy), 12));
    p(out, 7, sx);
    p(out, 8, integer::sar(fmul_lo32(cy, cx), 12));
}
pub fn affected(position: Array<u16>, source: Array<u16>, timer: fix32) -> bool {
    magnitude(g(position, 0) - g(source, 0)) < n(20480)
        && magnitude(g(position, 2) - g(source, 2)) < n(20480)
        && timer <= n(0)
}
// 1 captures an activation, 0 is already active, negative is the source error.
// The caller runs immediate blast callbacks before activate, using this saved
// incoming vector and re-reading the live state on resume.
pub fn begin(
    state: &State,
    incoming: Array<u16>,
    velocity: Array<u16>,
    limit: fix32,
    sine: Array<i16>,
) -> fix32 {
    if limit == n(0) {
        return n(-1);
    }
    let flags = g(state.header, 0).lo_bits();
    if flags & 1 != 0 {
        return n(0);
    }
    p(state.header, 1, n(0));
    bits(state.header, 0, 1025, 0);
    let mut i = 0u16;
    while i < 3 {
        p(incoming, i, n(0));
        i += 1;
    }
    if flags & 64 != 0 {
        let matrix: Buf<u16, 18> = Buf::new([0; 18]);
        basis(matrix.as_array(), n(0), g(state.header, 5), sine);
        p(
            incoming,
            2,
            integer::trunc_shift(collision::distance(velocity), 6),
        );
        collision::transform(incoming, matrix.as_array(), incoming, false);
    } else if flags & 2 == 0 {
        i = 0;
        while i < 3 {
            p(incoming, i, integer::trunc_shift(g(velocity, i), 6));
            i += 1;
        }
    }
    n(1)
}
pub fn activate(
    state: &State,
    origin: Array<u16>,
    incoming: Array<u16>,
    seed: Array<u16>,
    limit: fix32,
    sine: Array<i16>,
) -> fix32 {
    if limit == n(0) {
        return n(-1);
    }
    if state.has_animation {
        let a = state.animation;
        let mut i = 0u16;
        while i < 3 {
            p(a, i, g(state.header, 6 + i) + g(origin, i));
            i += 1;
        }
        p(a, 7, fix32::from_words(random_word(seed), 0));
        p(a, 9, g(a, 3));
        p(a, 10, g(a, 3));
        p(a, 8, n(0));
        p(a, 6, n(0));
        bits(state.header, 0, 8, 0);
    }
    let vector: Buf<u16, 6> = Buf::new([0; 6]);
    let v = vector.as_array();
    let matrix: Buf<u16, 18> = Buf::new([0; 18]);
    let mut index = 0u16;
    while index < state.count {
        if exhausted(index, limit) {
            return n(-3);
        }
        let piece = at(state.pieces, index * 42);
        let info = at(state.info, index * 3);
        let flags = g(state.header, 0).lo_bits();
        let mut i = 0u16;
        while i < 3 {
            p(v, i, n(0));
            i += 1;
        }
        if flags & 2 != 0 {
            i = 0;
            while i < 3 {
                let mut value = fix32::from_words(random_word(seed) & 8191, 0);
                if i != 1 {
                    value = value - n(4096);
                }
                p(v, i, value);
                p(piece, 3 + i, value);
                if flags & 2048 != 0 {
                    if i == 1 {
                        value = integer::sar(fmul_lo32(value, n(3)), 2);
                    } else {
                        value = integer::sar(value, 1);
                    }
                    p(piece, 3 + i, value);
                }
                i += 1;
            }
        } else if flags & 256 == 0 {
            i = 0;
            while i < 3 {
                p(
                    v,
                    i,
                    integer::trunc_shift(
                        fmul_lo32(g(incoming, i), integer::from_i16(info[0u16] as i16)),
                        7,
                    ),
                );
                i += 1;
            }
            let mut pitch = n(0);
            let mut yaw = n(0);
            if flags & 64 != 0 {
                pitch = fix32::from_words(random_word(seed) & 16383, 0) + n(4096);
                yaw = fix32::from_words(random_word(seed) & 32767, 0) - n(16384);
            } else {
                pitch = fix32::from_words(random_word(seed) & 4095, 0);
                yaw = fix32::from_words(random_word(seed) & 16383, 0) - n(8192);
            }
            basis(matrix.as_array(), pitch, yaw, sine);
            collision::transform(v, matrix.as_array(), v, false);
            i = 0;
            while i < 3 {
                p(piece, 3 + i, g(v, i));
                i += 1;
            }
        }
        if flags & 256 == 0 {
            i = 0;
            while i < 3 {
                p(
                    piece,
                    i,
                    g(piece, i) + g(piece, 3 + i) + g(state.header, 6 + i),
                );
                i += 1;
            }
            let amount = integer::sar(
                fmul_lo32(
                    fix32::from_words(random_word(seed) & 127, 0)
                        + integer::from_i16(info[1u16] as i16),
                    collision::distance(v),
                ),
                6,
            );
            if flags & 128 != 0 {
                let mut value = g(piece, 16);
                if draw(seed).lo_bits() & 16 != 0 {
                    value = value + amount;
                } else {
                    value = value - amount;
                }
                p(piece, 16, value);
            } else {
                let axis = random_word(seed) % 6;
                let slot = 15 + axis % 3;
                let mut value = g(piece, slot);
                if axis < 3 {
                    value = value + amount;
                } else {
                    value = value - amount;
                }
                p(piece, slot, value);
            }
        }
        bits(piece, 19, 1, 0);
        index += 1;
    }
    n(0)
}
fn approach(data: Array<u16>, index: u16, amount: fix32) {
    let mut value = g(data, index);
    if value < n(0) {
        value = value + amount;
        if value > n(0) {
            value = n(0);
        }
    } else if value > n(0) {
        value = value - amount;
        if value < n(0) {
            value = n(0);
        }
    }
    p(data, index, value);
}
fn spin(piece: Array<u16>, seed: Array<u16>) {
    let mut i = 0u16;
    while i < 3 {
        p(
            piece,
            15 + i,
            fix32::from_words(random_word(seed) & 8191, 0) - n(4096),
        );
        i += 1;
    }
}
fn retire(state: &State) {
    let count = g(state.header, 1).lo_bits() + 1;
    p(state.header, 1, fix32::from_words(count, 0));
}
pub fn tick(state: &State, frame: &Frame, seed: Array<u16>) -> fix32 {
    if frame.limit == n(0) {
        return n(-1);
    }
    let friction = integer::trunc_shift(fmul_lo32(g(state.header, 4), frame.dt), 8);
    let mut index = 0u16;
    while index < state.count {
        if exhausted(index, frame.limit) {
            return n(-3);
        }
        let piece = at(state.pieces, index * 42);
        let info = at(state.info, index * 3);
        let flags = g(piece, 19).lo_bits();
        if flags & 2 != 0 {
            index += 1;
            continue;
        }
        if flags & 1 != 0 {
            p(
                piece,
                4,
                g(piece, 4) - integer::trunc_shift(fmul_lo32(g(state.header, 3), frame.dt), 8),
            );
        } else if flags & 4 == 0 {
            approach(piece, 3, friction);
            approach(piece, 5, friction);
            let mut i = 0u16;
            while i < 3 {
                approach(piece, 15 + i, friction);
                i += 1;
            }
        }
        let mut i = 0u16;
        while i < 3 {
            p(
                piece,
                i,
                g(piece, i) + integer::trunc_shift(fmul_lo32(g(piece, 3 + i), frame.dt), 8),
            );
            i += 1;
        }
        let mut radius = fmul_lo32(fix32::from_words(info[2u16], 0), n(8));
        if g(state.header, 0).lo_bits() & 128 != 0 {
            p(
                piece,
                20,
                g(piece, 20) + integer::trunc_shift(fmul_lo32(g(piece, 16), frame.dt), 8),
            );
            if !state.has_sprite {
                return n(-1);
            }
            radius = state.sprite_radius;
        } else {
            i = 0;
            while i < 3 {
                attitude::rotate(
                    piece,
                    6,
                    i,
                    integer::trunc_shift(fmul_lo32(g(piece, 15 + i), frame.dt), 8),
                    frame.sine,
                );
                i += 1;
            }
        }
        if flags & 1 != 0 {
            let floor = radius + g(state.header, 2);
            if g(piece, 1) < floor {
                if flags & 8 != 0 {
                    i = 0;
                    while i < 3 {
                        p(piece, 3 + i, n(0));
                        i += 1;
                    }
                    let old = g(piece, 19);
                    p(
                        piece,
                        19,
                        fix32::from_words((flags & 65534) | 4, old.hi_bits()),
                    );
                } else {
                    p(piece, 1, floor);
                    let value = magnitude(g(piece, 4));
                    if fix32::from_words(value.lo_bits() & 65534, value.hi_bits()) <= n(2048) {
                        p(piece, 4, n(0));
                        bits(piece, 19, 0, 1);
                    } else {
                        p(piece, 4, integer::sar(value, 1));
                        spin(piece, seed);
                        let yaw = fix32::from_words(random_word(seed) & 4095, 0) - n(2048);
                        let matrix: Buf<u16, 18> = Buf::new([0; 18]);
                        basis(matrix.as_array(), n(0), yaw, frame.sine);
                        collision::transform(at(piece, 6), matrix.as_array(), at(piece, 6), false);
                    }
                }
            }
        } else if flags & 4 != 0 {
            let m = at(piece, 12);
            let tilt = attitude::angle(frame.angle, g(m, 7), attitude::length2(g(m, 6), g(m, 8)));
            if tilt == n(-1) {
                return n(-2);
            }
            if g(piece, 18) == n(-1) || tilt >= g(piece, 18) {
                p(piece, 18, tilt);
            } else {
                bits(piece, 19, 2, 0);
                retire(state);
                let yaw = attitude::angle(frame.angle, g(m, 6), g(m, 8));
                if yaw == n(-1) {
                    return n(-2);
                }
                basis(m, fix32::from_words(49156, 65535), -yaw, frame.sine);
            }
        } else if collision::distance(at(piece, 6)) <= n(1024) {
            retire(state);
            bits(piece, 19, 2, 0);
        } else if fix32::from_words(frame.entity.lo_bits() & 3, 0) == frame.group
            && random_word(seed) % 3 == 0
        {
            spin(piece, seed);
            let pitch = -fix32::from_words(random_word(seed) & 8191, 0);
            let yaw = fix32::from_words(random_word(seed) & 4095, 0) - n(2048);
            let matrix: Buf<u16, 18> = Buf::new([0; 18]);
            basis(matrix.as_array(), pitch, yaw, frame.sine);
            collision::transform(at(piece, 6), matrix.as_array(), at(piece, 6), false);
            bits(piece, 19, 1, 0);
        }
        index += 1;
    }
    if g(state.header, 0).lo_bits() & 8 != 0 {
        if !state.has_animation {
            return n(-1);
        }
        let a = state.animation;
        animate(a, frame.dt);
        if g(a, 8) == g(a, 4) {
            bits(state.header, 0, 0, 8);
        } else {
            let amount = g(a, 5);
            let mut change = integer::sar(fmul_lo32(frame.dt, amount), 7);
            if amount <= n(0) {
                change = -integer::sar(-fmul_lo32(frame.dt, amount), 7);
            }
            p(a, 9, g(a, 9) + change);
            p(a, 10, g(a, 10) + change);
        }
    }
    if g(state.header, 1).lo_bits() == state.count && g(state.header, 0).lo_bits() & 8 == 0 {
        return n(1);
    }
    n(0)
}
pub fn animate(a: Array<u16>, dt: fix32) {
    let timer = g(a, 6).lo_bits() + dt.lo_bits();
    p(a, 6, integer::from_i16(timer as i16));
    if (timer as i16) <= 60 {
        return;
    }
    let end = integer::from_i16(g(a, 15).lo_bits() as i16)
        - integer::from_i16(g(a, 16).lo_bits() as i16)
        - n(1);
    let cell = g(a, 16).lo_bits() & 255;
    p(a, 6, n(0));
    if g(a, 11) < end {
        p(
            a,
            11,
            fix32::from_words((g(a, 11).lo_bits() + cell) & 255, 0),
        );
        p(
            a,
            13,
            fix32::from_words((g(a, 13).lo_bits() + cell) & 255, 0),
        );
    } else if g(a, 12) < end {
        p(a, 11, n(0));
        p(
            a,
            12,
            fix32::from_words((g(a, 12).lo_bits() + cell) & 255, 0),
        );
        p(a, 13, fix32::from_words((cell + 255) & 255, 0));
        p(
            a,
            14,
            fix32::from_words((g(a, 14).lo_bits() + cell) & 255, 0),
        );
    } else {
        p(a, 8, fix32::from_words(g(a, 8).lo_bits() + 1, 0));
        p(a, 11, n(0));
        p(a, 12, n(0));
        p(a, 13, fix32::from_words((cell + 255) & 255, 0));
        p(a, 14, g(a, 13));
    }
}
