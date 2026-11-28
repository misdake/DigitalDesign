//! Mutable triangle fragments on caller-owned vertices, centers and face state.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, random, trig_lookup};

#[derive(Copy, Clone)]
pub struct State {
    // Four i32 fields: retired count, ground height, prepared, lifetime.
    pub header: Array<u16>,
    // Six i32 velocity/angular fields, i32 life, i16 extra vertex, u16 mode.
    // Sixteen words per face. Modes: pair head 0, pair tail 1, single 2.
    pub faces: Array<u16>,
    pub count: u16,
}
#[derive(Copy, Clone)]
pub struct Mesh {
    pub vertices: Array<i16>,
    pub vertex_count: u16,
    pub indices: Array<u16>,
    pub flags: Array<u16>,
    pub triangle_count: u16,
    // Three strided i16 center arrays; each length is 65535 when absent.
    pub centers: Array<i16>,
    pub center_stride: u16,
    pub center_lengths: Array<u16>,
    pub radius: Array<u16>,
}
#[derive(Copy, Clone)]
pub struct Config {
    pub gravity: fix32,
    // Five multiplier/shift pairs: bounce, friction, push, velocity, spin.
    // Multipliers are unsigned u16; shifts use the source low five bits.
    pub gains: Array<u16>,
}
pub struct Hit {
    pub velocity: Array<u16>,
    pub has_velocity: bool,
    pub basis: Array<u16>,
    pub has_basis: bool,
    pub height: fix32,
    pub paired_tail: u16,
    pub has_paired_tail: bool,
}
pub struct Step {
    pub position: Array<u16>,
    pub dt: fix32,
    pub limit: fix32,
    pub sine: Array<i16>,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn at(data: Array<u16>, words: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(words as i16).as_u16_array() }
}
fn exhausted(work: fix32, limit: fix32) -> bool {
    work.hi_bits() > limit.hi_bits()
        || (work.hi_bits() == limit.hi_bits() && work.lo_bits() >= limit.lo_bits())
}
fn draw(seed: Array<u16>) -> fix32 {
    let value = random::next(attitude::get(seed, 0));
    attitude::put(seed, 0, value);
    value
}
fn scatter_draw(seed: Array<u16>, angular: bool) -> fix32 {
    let sign = draw(seed);
    if !angular {
        let bits = (sign.lo_bits() >> 4) | (sign.hi_bits() << 12);
        return fix32::from_words(bits, 0) - n(32767);
    }
    let value = draw(seed);
    let bits = ((value.lo_bits() >> 5) | (value.hi_bits() << 11)) & 32767;
    let value = fix32::from_words(bits, 0) + n(20000);
    if sign.lo_bits() & 32 != 0 {
        return value;
    }
    -value
}
fn scatter(seed: Array<u16>, gain: Array<u16>, speed: fix32, angular: bool) -> fix32 {
    let multiplier = fix32::from_words(gain[0u16], 0);
    let first = integer::mul(integer::mul(scatter_draw(seed, angular), multiplier), speed);
    let second = integer::mul(integer::mul(scatter_draw(seed, angular), multiplier), speed);
    // The first product selects the sign branch; the second supplies its bits.
    if first < n(0) {
        return -integer::sar(-second, gain[1u16]);
    }
    integer::sar(second, gain[1u16])
}
fn scaled(value: fix32, gains: Array<u16>, pair: u16) -> fix32 {
    integer::trunc_shift(
        integer::mul(value, fix32::from_words(gains[pair * 2], 0)),
        gains[pair * 2 + 1],
    )
}
fn contains(indices: Array<u16>, index: u16) -> bool {
    indices[0u16] == index || indices[1u16] == index || indices[2u16] == index
}
pub fn activate(
    state: &State,
    mesh: &Mesh,
    config: &Config,
    hit: &Hit,
    seed: Array<u16>,
    limit: fix32,
) -> fix32 {
    if limit == n(0) || state.count != mesh.triangle_count {
        return n(-1);
    }
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let incoming = velocity.as_array();
    let mut i = 0u16;
    if hit.has_velocity {
        while i < 3 {
            attitude::put(
                incoming,
                i,
                integer::trunc_shift(attitude::get(hit.velocity, i), 6),
            );
            i += 1;
        }
    }
    if hit.has_basis {
        collision::transform(incoming, hit.basis, incoming, true);
    }
    let mut speed = n(100);
    if hit.has_velocity {
        speed = integer::sar(collision::distance(incoming), 6);
    }
    i = 0;
    while i < 3 {
        attitude::put(
            incoming,
            i,
            scaled(attitude::get(incoming, i), config.gains, 2),
        );
        i += 1;
    }
    attitude::put(state.header, 1, hit.height);
    let mut used = n(0);
    i = 0;
    while i < state.count {
        if exhausted(used, limit) {
            return n(-3);
        }
        used = used + n(1);
        let mut face = at(state.faces, i * 16);
        let mut j = 0u16;
        while j < 3 {
            let value =
                attitude::get(incoming, j) + scatter(seed, at(config.gains, 6), speed, false);
            attitude::put(face, j, value);
            j += 1;
        }
        j = 0;
        while j < 3 {
            attitude::put(face, 3 + j, scatter(seed, at(config.gains, 8), speed, true));
            j += 1;
        }
        let value = draw(seed);
        face[15u16] = 2;
        face[14u16] = 65535;
        attitude::put(
            face,
            6,
            fix32::from_words((value.lo_bits() >> 4) & 1023, 0) + attitude::get(state.header, 3),
        );
        i += 1;
    }
    let mut tail = hit.paired_tail;
    let mut tail_valid = hit.has_paired_tail;
    let mut flags = mesh.flags;
    i = 0;
    while i + 1 < state.count {
        if exhausted(used, limit) {
            return n(-3);
        }
        used = used + n(1);
        flags[i] |= 1;
        flags[i + 1] |= 1;
        let first = at(mesh.indices, i * 3);
        let second = at(mesh.indices, (i + 1) * 3);
        if contains(second, first[0u16])
            || contains(second, first[1u16])
            || contains(second, first[2u16])
        {
            let mut j = 0u16;
            while j < 3 {
                if !contains(first, second[j]) {
                    tail = second[j];
                    tail_valid = true;
                }
                j += 1;
            }
            let mut face = at(state.faces, i * 16);
            face[15u16] = 0;
            i += 1;
            face = at(state.faces, i * 16);
            face[15u16] = 1;
            if !tail_valid {
                return n(-4);
            }
            face[14u16] = tail;
        }
        i += 1;
    }
    let mut radius = mesh.radius;
    radius[0u16] = 5120;
    attitude::put(state.header, 0, n(0));
    n(0)
}
fn vertex(
    mesh: &Mesh,
    index: u16,
    step: Array<u16>,
    center: Array<u16>,
    rotation: Array<u16>,
) -> bool {
    if index >= mesh.vertex_count {
        return false;
    }
    let mut points = mesh.vertices;
    let base = index * 3;
    let mut i = 0u16;
    while i < 3 {
        points[base + i] = ((points[base + i] as u16) + attitude::get(step, i).lo_bits()) as i16;
        i += 1;
    }
    let vx = integer::from_i16(points[base]) - attitude::get(center, 0);
    let vy = integer::from_i16(points[base + 1]) - attitude::get(center, 1);
    let vz = integer::from_i16(points[base + 2]) - attitude::get(center, 2);
    let c = attitude::get(rotation, 0);
    let s = attitude::get(rotation, 1);
    let x = integer::trunc_shift(integer::mul(c, vx) - integer::mul(s, vz), 12);
    let z = integer::trunc_shift(integer::mul(c, vz) + integer::mul(s, vx), 12);
    let c = attitude::get(rotation, 2);
    let s = attitude::get(rotation, 3);
    let y = integer::trunc_shift(integer::mul(c, vy) + integer::mul(s, z), 12);
    let z = integer::trunc_shift(integer::mul(c, z) - integer::mul(s, vy), 12);
    let c = attitude::get(rotation, 4);
    let s = attitude::get(rotation, 5);
    let result_x = integer::trunc_shift(integer::mul(c, x) - integer::mul(s, y), 12);
    let result_y = integer::trunc_shift(integer::mul(c, y) + integer::mul(s, x), 12);
    points[base] = (attitude::get(center, 0) + result_x).lo_bits() as i16;
    points[base + 1] = (attitude::get(center, 1) + result_y).lo_bits() as i16;
    points[base + 2] = (attitude::get(center, 2) + z).lo_bits() as i16;
    true
}
// Returns 0 active, 1 complete, or the original negative error. All views remain
// authoritative on failure, including earlier face/vertex/center writes.
pub fn tick(state: &State, mesh: &Mesh, config: &Config, frame: &Step) -> fix32 {
    if frame.limit == n(0) {
        return n(-1);
    }
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let center: Buf<u16, 6> = Buf::new([0; 6]);
    let rotation: Buf<u16, 12> = Buf::new([0; 12]);
    let turn = rotation.as_array();
    let step = delta.as_array();
    let pivot = center.as_array();
    let mut flags = mesh.flags;
    let mut i = 0u16;
    let mut used = n(0);
    while i < state.count {
        if exhausted(used, frame.limit) {
            return n(-3);
        }
        used = used + n(1);
        let face = at(state.faces, i * 16);
        let paired = face[15u16] == 0;
        let life = attitude::get(face, 6);
        if life > n(0) {
            let life = life - frame.dt;
            attitude::put(face, 6, life);
            if life <= n(0) {
                attitude::put(state.header, 0, attitude::get(state.header, 0) + n(1));
                if i >= mesh.triangle_count {
                    return n(-1);
                }
                flags[i] |= 64;
                if paired {
                    attitude::put(state.header, 0, attitude::get(state.header, 0) + n(1));
                    if i + 1 >= mesh.triangle_count {
                        return n(-1);
                    }
                    flags[i + 1] |= 64;
                }
            } else {
                attitude::put(face, 1, attitude::get(face, 1) - config.gravity);
                if i >= mesh.triangle_count {
                    return n(-1);
                }
                let tri = at(mesh.indices, i * 3);
                let mut minimum = 32767i16;
                let mut j = 0u16;
                while j < 3 {
                    let v = tri[j];
                    if v >= mesh.vertex_count {
                        return n(-1);
                    }
                    let height = mesh.vertices[v * 3 + 1];
                    if height < minimum {
                        minimum = height;
                    }
                    j += 1;
                }
                if attitude::get(frame.position, 1) + integer::mul(integer::from_i16(minimum), n(8))
                    <= attitude::get(state.header, 1)
                    && attitude::get(face, 1) < n(0)
                {
                    j = 0;
                    while j < 6 {
                        let mut pair = 1u16;
                        if j & 1 != 0 {
                            pair = 0;
                        }
                        let mut value = scaled(attitude::get(face, j), config.gains, pair);
                        if j & 1 != 0 {
                            value = -value;
                        }
                        attitude::put(face, j, value);
                        j += 1;
                    }
                }
                j = 0;
                while j < 3 {
                    attitude::put(
                        step,
                        j,
                        integer::trunc_shift(integer::mul(attitude::get(face, j), frame.dt), 11),
                    );
                    if mesh.center_lengths[j] == 65535 || i >= mesh.center_lengths[j] {
                        return n(-1);
                    }
                    attitude::put(
                        pivot,
                        j,
                        integer::from_i16(mesh.centers[j * mesh.center_stride + i]),
                    );
                    j += 1;
                }
                // Bounce has already changed this face's angular velocity.
                // Reuse its six exact lookup values for all three/four vertices;
                // retain the Y, X, Z arithmetic and truncation inside vertex().
                j = 0;
                while j < 3 {
                    let mut axis = j;
                    if j == 0 {
                        axis = 1;
                    } else if j == 1 {
                        axis = 0;
                    }
                    let angle = integer::mul(frame.dt, attitude::get(face, 3 + axis));
                    let index = (angle.lo_bits() & 65532) >> 2;
                    attitude::put(turn, j * 2, trig_lookup::index(frame.sine, 4096 + index));
                    attitude::put(turn, j * 2 + 1, trig_lookup::index(frame.sine, index));
                    j += 1;
                }
                j = 0;
                while j < 3 {
                    if !vertex(mesh, tri[j], step, pivot, turn) {
                        return n(-1);
                    }
                    j += 1;
                }
                if paired {
                    if i + 1 >= state.count {
                        return n(-1);
                    }
                    let tail = at(state.faces, (i + 1) * 16);
                    if (tail[14u16] as i16) < 0 || !vertex(mesh, tail[14u16], step, pivot, turn) {
                        return n(-1);
                    }
                }
                let mut centers = mesh.centers;
                j = 0;
                while j < 3 {
                    let index = j * mesh.center_stride + i;
                    centers[index] =
                        ((centers[index] as u16) + attitude::get(step, j).lo_bits()) as i16;
                    j += 1;
                }
            }
        }
        i += 1;
        if paired {
            i += 1;
        }
    }
    if attitude::get(state.header, 0) == fix32::from_words(state.count, 0) {
        return n(1);
    }
    n(0)
}
