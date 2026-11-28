//! Four shared temporary line effects, with direct rows and the shared RNG/pool.
use crate::dsl_rt::*;
use crate::{attitude, effect_pool, integer, random, trig_lookup};

pub struct State {
    // available, four timestamps, then four rows of nine u32 line handles.
    pub core: Array<u16>,
    // Twelve 16-word rows: first XYZ, last XYZ, four packed color bytes, phases2.
    pub rows: Array<u16>,
}
pub struct Context {
    pub pool: Array<effect_pool::Pool>,
    pub seed: Array<u16>,
    pub sine: Array<i16>,
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
fn at(v: Array<u16>, words: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(words as i16).as_u16_array() }
}
fn roll(seed: Array<u16>) -> fix32 {
    let value = random::next(g(seed, 0));
    p(seed, 0, value);
    value
}
fn word(seed: Array<u16>) -> u16 {
    let value = roll(seed);
    (value.lo_bits() >> 4) | (value.hi_bits() << 12)
}
fn small(seed: Array<u16>) -> u16 {
    let value = roll(seed);
    ((value.lo_bits() >> 13) | (value.hi_bits() << 3)) & 127
}
fn less_unsigned(a: fix32, b: fix32) -> bool {
    if a.hi_bits() == b.hi_bits() {
        return a.lo_bits() < b.lo_bits();
    }
    a.hi_bits() < b.hi_bits()
}
fn endpoint(out: Array<u16>, position: Array<u16>, yaw: u16, pitch: u16, sine: Array<i16>) {
    let radius = integer::sar(
        fmul_lo32(
            trig_lookup::index(sine, 4096 + ((pitch & 16380) >> 2)),
            n(2700),
        ),
        12,
    );
    p(
        out,
        0,
        g(position, 0)
            + integer::sar(
                fmul_lo32(
                    trig_lookup::index(sine, 4096 + ((yaw & 65532) >> 2)),
                    radius,
                ),
                12,
            ),
    );
    p(
        out,
        1,
        g(position, 1)
            + integer::sar(
                fmul_lo32(trig_lookup::index(sine, (pitch & 16380) >> 2), n(2700)),
                12,
            )
            - n(1000),
    );
    p(
        out,
        2,
        g(position, 2)
            + integer::sar(
                fmul_lo32(trig_lookup::index(sine, (yaw & 65532) >> 2), radius),
                12,
            ),
    );
}
fn endpoints(row: Array<u16>, position: Array<u16>, middle: Array<u16>, tick: bool, ctx: &Context) {
    let yaw = word(ctx.seed);
    let pitch = word(ctx.seed) & 16383;
    endpoint(row, position, yaw, pitch, ctx.sine);
    let mut yaw = 0u16;
    let mut pitch = 0u16;
    if tick {
        pitch = word(ctx.seed) & 16383;
        yaw = word(ctx.seed);
    } else {
        yaw = word(ctx.seed);
        pitch = word(ctx.seed) & 16383;
    }
    endpoint(at(row, 6), position, yaw, pitch, ctx.sine);
    let mut i = 0u16;
    while i < 3 {
        p(middle, i, integer::sar(g(row, i) + g(row, 3 + i), 1));
        i += 1;
    }
}
fn jitter(out: Array<u16>, point: Array<u16>, seed: Array<u16>) {
    let mut i = 0u16;
    while i < 3 {
        p(
            out,
            i,
            g(point, i) + fix32::from_words(word(seed) & 1023, 0) - n(512),
        );
        i += 1;
    }
}
fn pair_colors(mut row: Array<u16>, mut colors: Array<u16>, seed: Array<u16>) {
    let first = small(seed) + 20;
    colors[0u16] = first | (first << 8);
    row[12u16] = (row[12u16] & 65280) | first;
    let last = (word(seed) & 127) + 20;
    colors[2u16] = last | (last << 8);
    let blue = small(seed) + 70;
    colors[1u16] = (colors[1u16] & 65280) | blue;
    row[12u16] = (row[12u16] & 255) | (blue << 8);
    colors[3u16] = (colors[3u16] & 65280) | ((word(seed) & 127) + 70);
}
fn last_colors(mut row: Array<u16>, mut colors: Array<u16>, seed: Array<u16>) {
    let last = (word(seed) & 127) + 20;
    colors[2u16] = last | (last << 8);
    row[13u16] = (row[13u16] & 65280) | last;
    let blue = (word(seed) & 127) + 70;
    colors[3u16] = (colors[3u16] & 65280) | blue;
    row[13u16] = (row[13u16] & 255) | (blue << 8);
}
fn copy(mut out: Array<u16>, from: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        out[i] = from[i];
        i += 1;
    }
}
pub fn expire(state: &State, slot: i16, ctx: &Context) -> fix32 {
    if slot < 0 || slot >= 4 {
        return n(-1);
    }
    let mut pools = ctx.pool;
    let pool = &mut pools[0u16];
    let mut i = 0u16;
    while i < 9 {
        if effect_pool::release(pool, g(state.core, 5 + (slot as u16) * 9 + i)) != 0 {
            return n(-1);
        }
        i += 1;
    }
    p(state.core, 0, n(slot));
    p(state.core, 1 + slot as u16, n(0));
    n(0)
}
pub fn reset(state: &State) {
    let mut i = 1u16;
    while i < 5 {
        p(state.core, i, n(0));
        i += 1;
    }
}
pub fn start(
    state: &State,
    timer: i16,
    slot: Array<u16>,
    position: Array<u16>,
    time: fix32,
    ctx: &Context,
) -> fix32 {
    if timer != 0 {
        let selected = g(slot, 0).lo_bits() as i16;
        if selected < 0 || selected >= 4 {
            return n(-1);
        }
        p(state.core, 1 + selected as u16, time);
        return n(0);
    }
    let available = g(state.core, 0);
    let mut selected = available.lo_bits();
    if available == n(-1) {
        selected = 0;
        let mut i = 1u16;
        while i < 4 {
            if less_unsigned(g(state.core, 1 + i), g(state.core, 1 + selected)) {
                selected = i;
            }
            i += 1;
        }
        let status = expire(state, selected as i16, ctx);
        if status != n(0) {
            return status;
        }
        p(state.core, 0, n(-1));
        p(state.core, 1 + selected, time);
    } else {
        p(state.core, 0, n(-1));
        if available.hi_bits() != 0 || selected >= 4 {
            return n(-1);
        }
        p(state.core, 1 + selected, time);
        let mut i = 0u16;
        while i < 4 {
            if g(state.core, 1 + i) == n(0) {
                p(state.core, 0, fix32::from_words(i, 0));
                break;
            }
            i += 1;
        }
    }
    p(slot, 0, fix32::from_words(selected, 0));
    let colors: Buf<u16, 4> = Buf::new([0, 256, 0, 0]);
    let middle: Buf<u16, 6> = Buf::new([0; 6]);
    let points: Buf<u16, 12> = Buf::new([0; 12]);
    let mut pools = ctx.pool;
    let pool = &mut pools[0u16];
    let mut row_index = 0u16;
    while row_index < 3 {
        let row = at(state.rows, selected * 48 + row_index * 16);
        endpoints(row, position, middle.as_array(), false, ctx);
        let mut line = 0u16;
        while line < 2 {
            copy(points.as_array(), row, 6);
            jitter(at(points.as_array(), 6), middle.as_array(), ctx.seed);
            pair_colors(row, colors.as_array(), ctx.seed);
            p(
                state.core,
                5 + selected * 9 + row_index * 3 + line,
                effect_pool::allocate(pool, points.as_array(), colors.as_array()),
            );
            line += 1;
        }
        last_colors(row, colors.as_array(), ctx.seed);
        p(
            state.core,
            5 + selected * 9 + row_index * 3 + 2,
            effect_pool::allocate(pool, row, colors.as_array()),
        );
        row_index += 1;
    }
    n(0)
}
pub fn tick(state: &State, timer: i16, slot: i16, position: Array<u16>, ctx: &Context) -> fix32 {
    if timer == 0 {
        return n(0);
    }
    if slot < 0 || slot >= 4 {
        return n(-1);
    }
    let selected = slot as u16;
    let colors: Buf<u16, 4> = Buf::new([0, 256, 0, 0]);
    let middle: Buf<u16, 6> = Buf::new([0; 6]);
    let points: Buf<u16, 12> = Buf::new([0; 12]);
    let mut pools = ctx.pool;
    let pool = &mut pools[0u16];
    let mut row_index = 0u16;
    while row_index < 3 {
        let row = at(state.rows, selected * 48 + row_index * 16);
        endpoints(row, position, middle.as_array(), true, ctx);
        copy(points.as_array(), row, 6);
        jitter(at(points.as_array(), 6), middle.as_array(), ctx.seed);
        pair_colors(row, colors.as_array(), ctx.seed);
        let handle = 5 + selected * 9 + row_index * 3;
        if effect_pool::update(
            pool,
            g(state.core, handle),
            points.as_array(),
            colors.as_array(),
        ) != 0
        {
            return n(-1);
        }
        jitter(points.as_array(), middle.as_array(), ctx.seed);
        let first = (word(ctx.seed) & 127) + 20;
        let mut values = colors.as_array();
        values[0u16] = first | (first << 8);
        values[1u16] = (values[1u16] & 65280) | ((word(ctx.seed) & 127) + 70);
        if effect_pool::update(
            pool,
            g(state.core, handle + 1),
            points.as_array(),
            colors.as_array(),
        ) != 0
        {
            return n(-1);
        }
        copy(at(points.as_array(), 6), at(row, 6), 6);
        last_colors(row, colors.as_array(), ctx.seed);
        if effect_pool::update(
            pool,
            g(state.core, handle + 2),
            points.as_array(),
            colors.as_array(),
        ) != 0
        {
            return n(-1);
        }
        let mut r = row;
        r[14u16] = word(ctx.seed) & 511;
        r[15u16] = word(ctx.seed) & 511;
        row_index += 1;
    }
    n(0)
}
