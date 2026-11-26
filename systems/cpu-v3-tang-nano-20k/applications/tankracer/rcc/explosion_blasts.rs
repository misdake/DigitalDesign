//! Two shared blast slots and derived additive geometry on live effect pools.
use crate::dsl_rt::*;
use crate::{attitude, collision, effect_pool, integer, projectile, quad_pool, trig_lookup};

pub struct State {
    // i32: four signed times, two next slots, two unsigned counts, total.
    pub core: Array<u16>,
    // Two 40-word rows: inner/outer, origin XYZ, owner presence/ID, flipped,
    // twelve handles. Two 138-word rings follow: life, origin XYZ, owner
    // presence/ID, kind/alpha/rotation, twelve five-i32 ray records.
    pub bands: Array<u16>,
    pub rings: Array<u16>,
}
pub struct Context {
    // Material presence, band material, three ring materials (five i32).
    pub resources: Array<u16>,
    pub camera: Array<u16>,
    pub sine: Array<i16>,
    pub seed: Array<u16>,
}
pub struct Start {
    pub origin: Array<u16>,
    pub owner: fix32,
    pub time: fix32,
    pub outer: fix32,
    pub flipped: bool,
    pub kind: u16,
}
pub struct Step {
    pub slot: fix32,
    pub dt: fix32,
}
pub struct Outcome {
    pub status: fix32,
    pub slot: fix32,
    pub has_released: bool,
    pub owner: fix32,
    pub ring: bool,
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
fn result(status: i16) -> Outcome {
    Outcome {
        status: n(status),
        slot: n(-1),
        has_released: false,
        owner: n(0),
        ring: false,
    }
}
pub fn view(data: Array<u16>) -> State {
    State {
        core: data,
        bands: at(data, 18),
        rings: at(data, 98),
    }
}
pub fn release(
    state: &State,
    ring: bool,
    slot: fix32,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> Outcome {
    if slot != n(0) && slot != n(1) {
        return result(-4);
    }
    let mut row = at(state.bands, slot.lo_bits() * 40);
    let mut owner = 5u16;
    if ring {
        row = at(state.rings, slot.lo_bits() * 138);
        owner = 4;
    }
    if g(row, owner) == n(0) {
        return result(-4);
    }
    let entity = g(row, owner + 1);
    let mut i = 0u16;
    while i < 12 {
        let mut handle = g(row, 8 + i);
        if ring {
            handle = g(row, 9 + i * 5);
        }
        if quad_pool::release(quads, lines, handle) != 0 {
            return result(-1);
        }
        i += 1;
    }
    let mut kind = 0u16;
    if ring {
        kind = 1;
    }
    p(state.core, 4 + kind, slot);
    p(state.core, kind * 2 + slot.lo_bits(), n(0));
    p(state.core, 6 + kind, g(state.core, 6 + kind) - n(1));
    p(state.core, 8, g(state.core, 8) - n(1));
    Outcome {
        status: n(0),
        slot,
        has_released: true,
        owner: entity,
        ring,
    }
}
fn oldest(state: &State, only: i16) -> u16 {
    let mut time = n(0);
    let mut selected = 4u16;
    let mut kind = 0u16;
    while kind < 2 {
        if only < 0 || only as u16 == kind {
            let mut slot = 0u16;
            while slot < 2 {
                let t = g(state.core, kind * 2 + slot);
                if t != n(0) && (time == n(0) || t < time) {
                    time = t;
                    selected = kind * 2 + slot;
                }
                slot += 1;
            }
        }
        kind += 1;
    }
    selected
}
fn allocate(
    state: &State,
    ring: bool,
    time: fix32,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> Outcome {
    let mut kind = 0u16;
    if ring {
        kind = 1;
    }
    let mut out = result(0);
    let mut slot = n(0);
    if g(state.core, 8) == n(2) && g(state.core, 6 + kind) == n(2) {
        let selected = oldest(state, kind as i16);
        if selected == 4 {
            return result(-4);
        }
        slot = fix32::from_words(selected & 1, 0);
        out = release(state, ring, slot, quads, lines);
        if out.status != n(0) {
            return out;
        }
        p(state.core, 4 + kind, n(-1));
        p(state.core, kind * 2 + slot.lo_bits(), time);
    } else {
        if g(state.core, 8) == n(2) {
            let selected = oldest(state, -1);
            if selected == 4 {
                return result(-4);
            }
            out = release(
                state,
                selected >= 2,
                fix32::from_words(selected & 1, 0),
                quads,
                lines,
            );
            if out.status != n(0) {
                return out;
            }
        }
        slot = g(state.core, 4 + kind);
        if slot != n(0) && slot != n(1) {
            return result(-4);
        }
        p(state.core, kind * 2 + slot.lo_bits(), time);
        p(state.core, 4 + kind, n(-1));
        let mut i = 0u16;
        while i < 2 {
            if g(state.core, kind * 2 + i) == n(0) {
                p(state.core, 4 + kind, fix32::from_words(i, 0));
                break;
            }
            i += 1;
        }
    }
    p(state.core, 6 + kind, g(state.core, 6 + kind) + n(1));
    p(state.core, 8, g(state.core, 8) + n(1));
    out.slot = slot;
    out
}
fn point(out: Array<u16>, origin: Array<u16>, x: fix32, y: fix32, angle: u16, ctx: &Context) {
    let index = (angle & 65532) >> 2;
    let sine = trig_lookup::index(ctx.sine, index);
    let cosine = trig_lookup::index(ctx.sine, 4096 + index);
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    p(
        local.as_array(),
        0,
        integer::trunc_shift(fmul_lo32(x, cosine) - fmul_lo32(y, sine), 12),
    );
    p(
        local.as_array(),
        1,
        integer::trunc_shift(fmul_lo32(x, sine) + fmul_lo32(y, cosine), 12),
    );
    collision::transform(delta.as_array(), ctx.camera, local.as_array(), false);
    let mut i = 0u16;
    while i < 3 {
        p(out, i, g(origin, i) + g(delta.as_array(), i));
        i += 1;
    }
}
fn band_quad(mut out: Array<u16>, row: Array<u16>, index: u16, initial: bool, ctx: &Context) {
    let inner = g(row, 0);
    let mut outer = inner - integer::sar(inner, 2);
    if initial {
        outer = n(8);
    }
    let first = index * 5461;
    let second = (index + 1) * 5461;
    point(out, at(row, 4), inner, n(0), first, ctx);
    point(at(out, 6), at(row, 4), outer, n(0), first, ctx);
    point(at(out, 12), at(row, 4), outer, n(0), second, ctx);
    point(at(out, 18), at(row, 4), inner, n(0), second, ctx);
    let flipped = g(row, 7) != n(0);
    let mut extra = 15u16;
    if initial || flipped {
        extra = 25;
    }
    // Only bits 8..15 survive the final u8 cast of the unsigned expression.
    let alpha = (((g(row, 1) - inner).lo_bits() >> 8) + extra) & 255;
    let mut red = alpha;
    let mut blue = 100u16;
    if flipped {
        red = 100;
        blue = alpha;
    }
    p(out, 12, g(ctx.resources, 1));
    out[26u16] = 1 | (red << 8);
    out[27u16] = red | (blue << 8);
    out[28u16] = 0;
    out[29u16] = 16128;
    out[30u16] = 16191;
    out[31u16] = 63;
}
fn ring_quad(mut out: Array<u16>, row: Array<u16>, ray: Array<u16>, project: bool, ctx: &Context) {
    let kind = g(row, 6).lo_bits();
    let value = integer::sar(g(row, 0), 2);
    let mut red = value;
    let mut green = n(127);
    let mut blue = value;
    if kind == 1 {
        red = n(127);
        green = g(row, 0);
        if project {
            green = value + n(60);
        }
        blue = green;
    } else if kind == 2 {
        green = value;
        blue = n(127);
    } else {
        if red > n(100) {
            red = n(100);
        }
        blue = red;
    }
    let mut i = 0u16;
    while i < 4 {
        let mut x = n(0);
        if i < 2 {
            x = g(ray, 1);
        }
        let mut y = g(ray, 2);
        if i == 1 || i == 2 {
            y = -y;
        }
        point(at(out, i * 6), at(row, 2), x, y, g(ray, 4).lo_bits(), ctx);
        i += 1;
    }
    p(out, 12, g(ctx.resources, 1 + kind));
    out[26u16] = 1 | ((red.lo_bits() & 255) << 8);
    out[27u16] = (green.lo_bits() & 255) | ((blue.lo_bits() & 255) << 8);
    out[28u16] = 16128;
    out[29u16] = 0;
    out[30u16] = 31;
    out[31u16] = 16159;
}
pub fn band(
    state: &State,
    start: &Start,
    ctx: &Context,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> Outcome {
    if g(ctx.resources, 0) == n(0) {
        return result(-5);
    }
    let mut out = allocate(state, false, start.time, quads, lines);
    if out.status != n(0) {
        return out;
    }
    let row = at(state.bands, out.slot.lo_bits() * 40);
    let mut data = row;
    let mut i = 0u16;
    while i < 40 {
        data[i] = 0;
        i += 1;
    }
    p(row, 0, n(10));
    p(row, 1, start.outer);
    i = 0;
    while i < 3 {
        p(row, 2 + i, g(start.origin, i));
        i += 1;
    }
    p(row, 5, n(1));
    p(row, 6, start.owner);
    if start.flipped {
        p(row, 7, n(1));
    }
    let quad: Buf<u16, 32> = Buf::new([0; 32]);
    i = 0;
    while i < 12 {
        band_quad(quad.as_array(), row, i, true, ctx);
        let handle = quad_pool::allocate(quads, lines, quad.as_array());
        p(row, 8 + i, handle);
        if handle == n(-1) {
            let mut j = 0u16;
            while j < i {
                if quad_pool::release(quads, lines, g(row, 8 + j)) != 0 {
                    return result(-1);
                }
                j += 1;
            }
            out.slot = n(-1);
            return out;
        }
        i += 1;
    }
    out
}
pub fn ring(
    state: &State,
    start: &Start,
    ctx: &Context,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> Outcome {
    if start.kind < 1 || start.kind > 3 {
        return result(-1);
    }
    if g(ctx.resources, 0) == n(0) {
        return result(-5);
    }
    let mut out = allocate(state, true, start.time, quads, lines);
    if out.status != n(0) {
        return out;
    }
    let row = at(state.rings, out.slot.lo_bits() * 138);
    p(row, 0, n(250));
    let mut i = 0u16;
    while i < 3 {
        p(row, 1 + i, g(start.origin, i));
        i += 1;
    }
    p(row, 4, n(1));
    p(row, 5, start.owner);
    p(row, 6, fix32::from_words(start.kind, 0));
    let quad: Buf<u16, 32> = Buf::new([0; 32]);
    i = 0;
    while i < 12 {
        let mut width = n(200);
        if start.kind == 2 {
            width = fix32::from_words(projectile::random_word(ctx.seed) & 511, 0) + g(row, 0);
        }
        let radius = fix32::from_words(projectile::random_word(ctx.seed) & 8191, 0)
            + fmul_lo32(g(row, 0), n(4));
        let mut angle = projectile::random_word(ctx.seed);
        if start.kind != 2 {
            angle = angle & 32767;
        }
        let ray = at(row, 18 + i * 10);
        p(ray, 0, n(0));
        p(ray, 1, radius);
        p(ray, 2, width);
        p(ray, 3, n(0));
        p(ray, 4, fix32::from_words(angle, 0));
        ring_quad(quad.as_array(), row, ray, false, ctx);
        let handle = quad_pool::allocate(quads, lines, quad.as_array());
        p(ray, 0, handle);
        if handle == n(-1) {
            let mut j = 0u16;
            while j < i {
                if quad_pool::release(quads, lines, g(row, 9 + j * 5)) != 0 {
                    return result(-1);
                }
                j += 1;
            }
            out.slot = n(-1);
            return out;
        }
        i += 1;
    }
    out
}
pub fn tick_band(
    state: &State,
    step: &Step,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> Outcome {
    if step.slot != n(0) && step.slot != n(1) {
        return result(-4);
    }
    let row = at(state.bands, step.slot.lo_bits() * 40);
    p(row, 0, g(row, 0) + fmul_lo32(step.dt, n(45)));
    if g(row, 1) < g(row, 0) {
        return release(state, false, step.slot, quads, lines);
    }
    result(0)
}
pub fn tick_ring(
    state: &State,
    step: &Step,
    seed: Array<u16>,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> Outcome {
    if step.slot != n(0) && step.slot != n(1) {
        return result(-4);
    }
    let row = at(state.rings, step.slot.lo_bits() * 138);
    if g(row, 0) <= step.dt {
        return release(state, true, step.slot, quads, lines);
    }
    let life = g(row, 0) - step.dt;
    p(row, 0, life);
    p(
        row,
        7,
        fix32::from_words(integer::sar(life, 1).lo_bits() & 255, 0),
    );
    p(row, 8, fix32::from_words(projectile::random_word(seed), 0));
    let mut i = 0u16;
    while i < 12 {
        let ray = at(row, 18 + i * 10);
        p(ray, 3, n(0));
        let mut angle = projectile::random_word(seed);
        if g(row, 6) != n(2) {
            angle = angle & 32767;
        }
        p(ray, 4, fix32::from_words(angle, 0));
        let mut width = n(200);
        if g(row, 6) != n(2) {
            width = fix32::from_words(projectile::random_word(seed) & 511, 0) + life;
        }
        p(ray, 2, width);
        p(
            ray,
            1,
            fix32::from_words(projectile::random_word(seed) & 8191, 0) + fmul_lo32(life, n(4)),
        );
        i += 1;
    }
    result(0)
}
// Caller supplies space for 48 raw quads (1,536 words). Projection is read-only:
// allocated lights retain their initial geometry and the seed is unchanged.
pub fn project(state: &State, ctx: &Context, out: Array<u16>) -> fix32 {
    if g(ctx.resources, 0) == n(0) {
        return n(-5);
    }
    let mut count = 0u16;
    let mut slot = 0u16;
    while slot < 2 {
        if g(state.core, slot) != n(0) {
            let mut i = 0u16;
            while i < 12 {
                band_quad(
                    at(out, count * 32),
                    at(state.bands, slot * 40),
                    i,
                    false,
                    ctx,
                );
                count += 1;
                i += 1;
            }
        }
        if g(state.core, 2 + slot) != n(0) {
            let row = at(state.rings, slot * 138);
            let mut i = 0u16;
            while i < 12 {
                ring_quad(at(out, count * 32), row, at(row, 18 + i * 10), true, ctx);
                count += 1;
                i += 1;
            }
        }
        slot += 1;
    }
    fix32::from_words(count, 0)
}
