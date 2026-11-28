//! First-map flock flight: ordered birds, shared RNG and live wing vertices.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, projectile};

pub struct Model {
    pub vertices: Array<i16>,
    pub page: u16,
    pub low: u16,
    pub high: u16,
}
pub struct State {
    // phase, unsigned stage, timer, target3, eight velocity3, wings_up2.
    pub flight: Array<u16>,
    // Eight rows: position3, basis9, speed, active. Each row is 28 words.
    pub birds: Array<u16>,
    pub models: Array<Model>,
    pub model_count: u16,
    pub range: fix32,
}
pub struct Frame {
    pub origin: Array<u16>,
    pub player: Array<u16>,
    pub entity: fix32,
    pub number: fix32,
    pub dt: fix32,
    pub seed: Array<u16>,
    pub tables: projectile::Tables,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn g(values: Array<u16>, index: u16) -> fix32 {
    attitude::get(values, index)
}
fn p(values: Array<u16>, index: u16, value: fix32) {
    attitude::put(values, index, value);
}
fn at(values: Array<u16>, word: u16) -> Array<u16> {
    unsafe { values.as_ptr().add(word as i16).as_u16_array() }
}
pub fn select(model: &Model) -> u16 {
    let previous = read_dseg_page(3);
    if model.page != 65535 && model.page != previous {
        unsafe {
            write_dseg_page(3, model.page);
        }
    }
    previous
}
// Model preparation and terrain grounding precede this boundary. Every draw
// initializes one authored bird field; models and range retain their metadata.
pub fn initialize(
    state: &State,
    origin: Array<u16>,
    maneuver: i16,
    seed: Array<u16>,
    sine: Array<i16>,
) {
    let mut flight = state.flight;
    let mut i = 0u16;
    while i < 64 {
        flight[i] = 0;
        i += 1;
    }
    p(flight, 31, n(1));
    i = 0;
    while i < 8 {
        let bird = at(state.birds, i * 28);
        let x = fix32::from_words(projectile::random_word(seed) & 8191, 0) - n(4096);
        let z = fix32::from_words(projectile::random_word(seed) & 8191, 0) - n(4096);
        let yaw = fix32::from_words(projectile::random_word(seed), 0);
        let speed = fix32::from_words(projectile::random_word(seed) & 63, 0) + n(maneuver);
        p(bird, 0, g(origin, 0) + x);
        p(bird, 1, g(origin, 1) + n(250));
        p(bird, 2, g(origin, 2) + z);
        projectile::basis(at(bird, 6), n(0), yaw, sine);
        p(bird, 12, speed);
        p(bird, 13, n(1));
        i += 1;
    }
}
// Zero continues, one requests Stop after publication, negative is a source error.
pub fn tick(state: &State, frame: &Frame) -> fix32 {
    let f = state.flight;
    if g(f, 0) == n(0) {
        if (frame.entity.lo_bits() ^ frame.number.lo_bits()) & 7 != 0 {
            return n(0);
        }
        let mut axis = 0u16;
        while axis < 3 {
            let mut delta = g(frame.player, axis) - g(frame.origin, axis);
            if delta < n(0) {
                delta = -delta;
            }
            if delta >= state.range {
                return n(0);
            }
            axis += 2;
        }
        p(f, 1, n(0));
        p(f, 3, g(frame.origin, 0) - g(frame.player, 0));
        p(f, 5, g(frame.origin, 2) - g(frame.player, 2));
        p(f, 0, n(1));
        p(f, 4, n(0));
        collision::normalize(at(f, 6));
        p(f, 3, integer::mul(g(f, 3), n(2)));
        p(f, 4, n(2048));
        p(f, 5, integer::mul(g(f, 5), n(2)));
        p(f, 2, n(1000));
        let mut bird = 0u16;
        while bird < 8 {
            if projectile::random_word(frame.seed) % 3 == 0 {
                p(at(state.birds, bird * 28), 13, n(0));
            }
            bird += 1;
        }
        return n(0);
    }
    if g(f, 0) != n(1) {
        return n(-5);
    }
    let stage = g(f, 1);
    if stage.hi_bits() != 0 || stage.lo_bits() > 1 {
        return n(0);
    }
    let mut stop = n(0);
    if g(f, 2) < frame.dt {
        if stage == n(1) {
            stop = n(1);
        } else {
            p(f, 1, n(1));
            p(f, 2, n(3000));
            let yaw = fix32::from_words(projectile::random_word(frame.seed), 0);
            let pitch = fix32::from_words(projectile::random_word(frame.seed) & 2047, 0) - n(1024);
            let basis: Buf<u16, 18> = Buf::new([0; 18]);
            projectile::basis(basis.as_array(), pitch, yaw, frame.tables.sine);
            let mut axis = 0u16;
            while axis < 3 {
                p(
                    f,
                    3 + axis,
                    integer::sar(integer::mul(g(basis.as_array(), 6 + axis), n(8192)), 12),
                );
                axis += 1;
            }
            let mut bird = 0u16;
            while bird < 8 {
                p(at(state.birds, bird * 28), 13, n(0));
                bird += 1;
            }
        }
    } else {
        p(f, 2, g(f, 2) - frame.dt);
    }
    let mut index = 0u16;
    while index < 8 {
        let bird = at(state.birds, index * 28);
        if g(bird, 13) != n(0) {
            if projectile::random_word(frame.seed) % 3 == 0 {
                p(bird, 13, n(0));
            }
        } else {
            let velocity = at(f, 12 + index * 6);
            let mut axis = 0u16;
            while axis < 3 {
                let old = g(velocity, axis);
                let goal = g(f, 3 + axis);
                let speed = g(bird, 12);
                let mut next = old;
                if old < goal {
                    next = old + speed;
                } else if old > goal {
                    next = old - speed;
                }
                if (old < goal && next > goal) || (old > goal && next < goal) {
                    next = goal;
                }
                p(velocity, axis, next);
                p(
                    bird,
                    axis,
                    g(bird, axis) + integer::trunc_shift(integer::mul(frame.dt, next), 8),
                );
                axis += 1;
            }
            let yaw = attitude::angle(frame.tables.angle, -g(velocity, 0), g(velocity, 2));
            if yaw == n(-1) {
                return n(-2);
            }
            let mut pitch = attitude::angle(frame.tables.angle, g(velocity, 1), g(velocity, 2));
            if pitch == n(-1) {
                return n(-2);
            }
            if pitch > n(16384) {
                pitch = fix32::from_words(32768, 0) - pitch;
            } else if pitch < n(-16384) {
                pitch = -fix32::from_words(32768, 0) - pitch;
            }
            projectile::basis(at(bird, 6), pitch, yaw, frame.tables.sine);
        }
        index += 1;
    }
    index = 0;
    while index < state.model_count {
        let model = &state.models[index];
        let up = g(f, 30 + index) != n(0);
        let previous = select(model);
        let mut vertices = model.vertices;
        let mut flip = false;
        let mut delta = integer::mul(frame.dt, n(-2));
        if up {
            delta = integer::mul(frame.dt, n(2));
        }
        let low = model.low * 3 + 1;
        let high = model.high * 3 + 1;
        vertices[low] = (integer::from_i16(vertices[low]) + delta).lo_bits() as i16;
        vertices[high] = (integer::from_i16(vertices[high]) + delta).lo_bits() as i16;
        let y = vertices[low];
        if (up && y > 50) || (!up && y < -50) {
            let mut next = -50i16;
            if up {
                next = 50;
            }
            vertices[low] = next;
            vertices[high] = next;
            flip = true;
        }
        unsafe {
            write_dseg_page(3, previous);
        }
        if flip {
            let mut flag = n(1);
            if up {
                flag = n(0);
            }
            p(f, 30 + index, flag);
        }
        index += 1;
    }
    stop
}
