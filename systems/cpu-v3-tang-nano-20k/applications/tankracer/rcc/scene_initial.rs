//! Scene construction fields from decoded parameters and the shared game RNG.
use crate::dsl_rt::*;
use crate::{
    attitude, collision, deformation, emitter, flock, integer, inverse_basis, projectile,
    trig_lookup,
};

fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn at(values: Array<u16>, word: u16) -> Array<u16> {
    unsafe { values.as_ptr().add(word as i16).as_u16_array() }
}

// Decoded row: kind, vertex count, glow count, remove-present, add/remove i32,
// basis9/position3, maneuver i16, visibility distance, debris count/sprite mode,
// authored-piece-position presence, then local glow/color and piece positions.
// Kinds: generic, Crush, Flock, Bomber, WayPoint, Powerup. The last two use
// their own deterministic constructors and do not enter the generic lap path.
// The output owns the actual generated fields, including every glow/vertex/bird.
pub fn random_fields(
    out: Array<u16>,
    input: Array<u16>,
    seed: Array<u16>,
    sine: Array<i16>,
    angle: Array<i16>,
) -> u16 {
    let kind = input[0u16];
    if kind > 5 {
        halt(0x7e99);
    }
    if kind == 4 || kind == 5 {
        return 0;
    }
    visibility(
        out,
        attitude::get(input, 2),
        attitude::get(input, 3),
        input[3u16] != 0,
        seed,
    );
    attitude::put(out, 0, fix32::from_words(input[33u16], 0));
    let next = 8 + input[34u16] * 48;
    debris(
        at(out, 8),
        input[34u16],
        input[35u16] != 0,
        at(input, 26),
        seed,
        sine,
    );
    if input[36u16] != 0 {
        debris_positions(
            at(out, 8),
            at(input, 37 + input[2u16] * 12),
            input[34u16],
            at(input, 8),
            sine,
            angle,
        );
    }
    if kind == 1 {
        let rows = unsafe { at(out, next).as_ptr().as_array::<deformation::Record>() };
        let mut records = rows;
        let count = input[2u16];
        let mut i = 0u16;
        while i < count {
            let record = &mut records[i];
            let values = at(input, 37 + i * 12);
            let mut axis = 0u16;
            while axis < 3 {
                attitude::put(
                    record.position.as_array(),
                    axis,
                    attitude::get(values, axis),
                );
                axis += 1;
            }
            record.red = attitude::get(values, 3);
            record.green = attitude::get(values, 4);
            record.blue = attitude::get(values, 5);
            i += 1;
        }
        glows(rows, count, at(input, 26), at(input, 8), seed);
        let flags = next + count * 22;
        vertex_flags(at(out, flags), input[1u16], seed);
        return flags + input[1u16];
    }
    if kind == 2 {
        let state = flock::State {
            flight: at(out, next),
            birds: at(out, next + 64),
            models: unsafe { Ptr::from_addr(0).as_array::<flock::Model>() },
            model_count: 0,
            range: fix32::zero(),
        };
        flock::initialize(&state, at(input, 26), input[32u16] as i16, seed, sine);
        return next + 288;
    }
    if kind == 3 {
        emitter::initialize_timer(at(out, next), seed);
        return next + 2;
    }
    next
}

pub fn basis(out: Array<u16>, pitch: u16, yaw: u16, roll: u16, sine: Array<i16>) {
    if roll == 0 {
        projectile::basis(
            out,
            fix32::from_words(pitch, 0),
            fix32::from_words(yaw, 0),
            sine,
        );
        return;
    }
    let cx = trig_lookup::index(sine, 4096 + ((pitch & 65532) >> 2));
    let sx = trig_lookup::index(sine, (pitch & 65532) >> 2);
    let cy = trig_lookup::index(sine, 4096 + ((yaw & 65532) >> 2));
    let sy = trig_lookup::index(sine, (yaw & 65532) >> 2);
    let cz = trig_lookup::index(sine, 4096 + ((roll & 65532) >> 2));
    let sz = trig_lookup::index(sine, (roll & 65532) >> 2);
    let u = integer::sar(fmul_lo32(sy, sx), 12);
    let v = integer::sar(fmul_lo32(cy, sx), 12);
    attitude::put(
        out,
        0,
        integer::sar(fmul_lo32(cy, cz) - fmul_lo32(u, sz), 12),
    );
    attitude::put(out, 1, integer::sar(-fmul_lo32(cx, sz), 12));
    attitude::put(
        out,
        2,
        integer::sar(fmul_lo32(sy, cz) + fmul_lo32(v, sz), 12),
    );
    attitude::put(
        out,
        3,
        integer::sar(fmul_lo32(cy, sz) + fmul_lo32(u, cz), 12),
    );
    attitude::put(out, 4, integer::sar(fmul_lo32(cx, cz), 12));
    attitude::put(
        out,
        5,
        integer::sar(fmul_lo32(sz, sy) - fmul_lo32(v, cz), 12),
    );
    attitude::put(out, 6, integer::sar(-fmul_lo32(cx, sy), 12));
    attitude::put(out, 7, sx);
    attitude::put(out, 8, integer::sar(fmul_lo32(cy, cx), 12));
}

// Piece rows match shatter's 21 mutable i32 columns followed by translation3.
// Missing model handles retain this initialization, as do ordinary pieces.
pub fn debris(
    out: Array<u16>,
    count: u16,
    sprite: bool,
    origin: Array<u16>,
    seed: Array<u16>,
    sine: Array<i16>,
) {
    let mut i = 0u16;
    while i < count {
        let row = at(out, i * 48);
        let mut column = 0u16;
        while column < 24 {
            attitude::put(row, column, n(0));
            column += 1;
        }
        column = 0;
        while column < 3 {
            attitude::put(row, column, attitude::get(origin, column));
            column += 1;
        }
        if sprite {
            attitude::put(row, 20, fix32::from_words(projectile::random_word(seed), 0));
            attitude::put(
                row,
                16,
                fix32::from_words(projectile::random_word(seed) & 255, 0) - n(128),
            );
        } else {
            let pitch = projectile::random_word(seed);
            let yaw = projectile::random_word(seed);
            let roll = projectile::random_word(seed);
            basis(at(row, 12), pitch, yaw, roll, sine);
            column = 0;
            while column < 3 {
                attitude::put(
                    row,
                    15 + column,
                    fix32::from_words(projectile::random_word(seed) & 255, 0) - n(128),
                );
                column += 1;
            }
        }
        i += 1;
    }
}

// Authored :terrys positions replace the initial orientation after its genuine
// construction. Hinged pieces derive yaw from their local X/Z; the other pieces
// inherit the object's basis. The six initial draws remain owned by each piece.
fn debris_positions(
    out: Array<u16>,
    positions: Array<u16>,
    count: u16,
    pose: Array<u16>,
    sine: Array<i16>,
    angle: Array<i16>,
) {
    let mut i = 0u16;
    while i < count {
        let row = at(out, i * 48);
        let local = at(positions, i * 6);
        let hinged = attitude::get(local, 1) == n(0);
        if hinged {
            let yaw = attitude::angle(angle, attitude::get(local, 0), attitude::get(local, 2));
            if yaw == n(-1) {
                halt(0x7e9c);
            }
            projectile::basis(at(row, 12), n(0), n(32760) - yaw, sine);
            attitude::put(row, 19, n(8));
        } else {
            let mut column = 0u16;
            while column < 9 {
                attitude::put(row, 6 + column, attitude::get(pose, column));
                column += 1;
            }
        }
        let mut axis = 0u16;
        while axis < 3 {
            attitude::put(
                row,
                axis,
                attitude::get(local, axis) + attitude::get(pose, 9 + axis),
            );
            attitude::put(row, 15 + axis, n(0));
            axis += 1;
        }
        if hinged {
            attitude::put(row, 15, n(8000));
        }
        attitude::put(row, 18, n(-1));
        i += 1;
    }
}

// Parameters have already been decoded: absent add is zero; remove retains
// its presence bit because an authored zero removes on the wrapped lap -1.
// Four i32 values: distance, add lap, remove lap, entered.
pub fn visibility(out: Array<u16>, add: fix32, remove: fix32, has_remove: bool, seed: Array<u16>) {
    let mut lap = add;
    if lap == n(-99) {
        lap = fix32::from_words(projectile::random_word(seed) % 3 + 1, 0);
    }
    attitude::put(out, 0, n(150));
    attitude::put(out, 1, n(0));
    attitude::put(out, 2, n(0));
    attitude::put(out, 3, n(1));
    if lap == n(2) || lap == n(3) {
        attitude::put(out, 1, lap - n(1));
        attitude::put(out, 3, n(0));
    }
    if has_remove {
        attitude::put(out, 2, integer::from_i16((remove - n(1)).lo_bits() as i16));
    }
}

pub fn glows(
    records: Array<deformation::Record>,
    count: u16,
    origin: Array<u16>,
    basis: Array<u16>,
    seed: Array<u16>,
) {
    let mut records = records;
    let mut i = 0u16;
    while i < count {
        let r = &mut records[i];
        let world = r.world.as_array();
        collision::transform(world, basis, r.position.as_array(), false);
        let mut axis = 0u16;
        while axis < 3 {
            attitude::put(
                world,
                axis,
                attitude::get(world, axis) + attitude::get(origin, axis),
            );
            axis += 1;
        }
        r.state = fix32::from_words(projectile::random_word(seed) & 1, 0);
        r.timer = fix32::from_words(projectile::random_word(seed) & 511, 0);
        i += 1;
    }
}

pub fn vertex_flags(mut flags: Array<u16>, count: u16, seed: Array<u16>) {
    let mut i = 0u16;
    while i < count {
        let mut value = 0u16;
        if projectile::random_word(seed) & 1 != 0 {
            value = 4;
            if projectile::random_word(seed) & 1 != 0 {
                value |= 1;
            }
            if projectile::random_word(seed) & 1 != 0 {
                value |= 2;
            }
        }
        flags[i] = value;
        i += 1;
    }
}

// Output i32 columns: basis[9], position[3], angles[3], displacement[3],
// target angles[3], variant, then two (rate, multiplier, shift) resize rows.
// Basis failure precedes the RNG draw and leaves the output untouched.
pub fn camera(
    out: Array<u16>,
    pose: Array<u16>,
    near: Array<u16>,
    seed: Array<u16>,
    sine: Array<i16>,
    inverse_cosine: Array<i16>,
) -> bool {
    let angles: Buf<u16, 8> = Buf::new([0; 8]);
    let a = angles.as_array();
    if !inverse_basis::angles(a, pose, sine, inverse_cosine) {
        return false;
    }
    let variant = projectile::random_word(seed) % 3;
    let mut i = 0u16;
    while i < 12 {
        attitude::put(out, i, attitude::get(pose, i));
        i += 1;
    }
    i = 0;
    while i < 3 {
        let angle = attitude::get(a, i);
        let mut target = angle;
        let mut displacement = attitude::get(near, i);
        if i == 1 {
            displacement = displacement - n(5000);
            if variant != 0 {
                displacement = attitude::get(near, i) + n(3000);
            }
            if variant == 1 {
                target = target + fix32::from_words(65520, 0);
            }
            if variant == 2 {
                target = target - fix32::from_words(65520, 0);
            }
        }
        if i == 2 {
            displacement = displacement - n(25000);
            if variant != 0 {
                displacement = attitude::get(near, i) - n(10000);
            }
            attitude::put(out, 11, attitude::get(pose, 11) + displacement);
        }
        attitude::put(out, 12 + i, angle);
        attitude::put(out, 15 + i, displacement);
        attitude::put(out, 18 + i, target);
        i += 1;
    }
    attitude::put(out, 21, fix32::from_words(variant, 0));
    attitude::put(out, 22, n(1));
    attitude::put(out, 23, n(1));
    attitude::put(out, 24, n(10));
    attitude::put(out, 25, n(1));
    attitude::put(out, 26, n(1));
    attitude::put(out, 27, n(9));
    if variant != 0 {
        attitude::put(out, 22, n(2));
        attitude::put(out, 24, n(9));
        attitude::put(out, 27, n(10));
    }
    true
}
