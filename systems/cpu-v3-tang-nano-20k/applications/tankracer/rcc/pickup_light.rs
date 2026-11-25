//! Two pickup light banks: allocation, activation projection, follow and fade.
use crate::dsl_rt::*;
use crate::{attitude, collision, effect_pool, integer, quad_pool, trig_lookup};

pub struct State {
    // Two banks, each 408 i32: header 18, ten rays of 22, ten rings of 17.
    pub banks: Array<u16>,
}
pub struct Start {
    pub target: fix32,
    pub origin: Array<u16>,
    pub velocity: Array<u16>,
    pub frame: fix32,
    pub follow: bool,
}
pub struct Context {
    // Camera position XYZ then its nine basis values.
    pub camera: Array<u16>,
    pub materials: Array<u16>,
    // Three i32 growth rates then ten RGB byte values in u16 lanes.
    pub tuning: Array<u16>,
    pub sine: Array<i16>,
}
pub struct Frame {
    pub target: fix32,
    pub position: Array<u16>,
    pub number: fix32,
    pub dt: fix32,
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
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn above(a: fix32, b: fix32) -> bool {
    a.hi_bits() > b.hi_bits() || (a.hi_bits() == b.hi_bits() && a.lo_bits() > b.lo_bits())
}
fn ray(b: Array<u16>, i: u16) -> Array<u16> {
    at(b, 36 + i * 44)
}
fn ring(b: Array<u16>, i: u16) -> Array<u16> {
    at(b, 476 + i * 34)
}
pub fn reset(state: &State) {
    let mut i = 0u16;
    while i < 1632 {
        let mut data = state.banks;
        data[i] = 0;
        i += 1;
    }
    let mut bank = 0u16;
    while bank < 2 {
        let b = at(state.banks, bank * 816);
        i = 0;
        while i < 10 {
            let r = ray(b, i);
            p(r, 5, quad_pool::released());
            p(r, 19, n(1));
            let r = ring(b, i);
            p(r, 0, quad_pool::released());
            p(r, 14, n(1));
            i += 1;
        }
        bank += 1;
    }
}
fn retire(
    records: Array<u16>,
    stride: u16,
    handle: u16,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    let mut i = 0u16;
    while i < 10 {
        let r = at(records, i * stride);
        let h = g(r, handle);
        if h != quad_pool::released() {
            if quad_pool::release(quads, lines, h) != 0 {
                return n(-1);
            }
            p(r, handle, quad_pool::released());
        }
        i += 1;
    }
    n(0)
}
fn color(mut quad: Array<u16>, tuning: Array<u16>, index: u16, life: fix32, scaled: bool) {
    let mut r = fix32::from_words(tuning[6 + index * 3], 0);
    let mut g = fix32::from_words(tuning[7 + index * 3], 0);
    let mut b = fix32::from_words(tuning[8 + index * 3], 0);
    if scaled {
        r = integer::sar(fmul_lo32(r, life), 17);
        g = integer::sar(fmul_lo32(g, life), 17);
        b = integer::sar(fmul_lo32(b, life), 17);
    }
    quad[26u16] = (quad[26u16] & 255) | ((r.lo_bits() & 255) << 8);
    quad[27u16] = (g.lo_bits() & 255) | ((b.lo_bits() & 255) << 8);
}
fn point(out: Array<u16>, origin: Array<u16>, radius: fix32, angle: u16, ctx: &Context) {
    let index = (angle & 65532) >> 2;
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    p(
        local.as_array(),
        0,
        integer::trunc_shift(
            fmul_lo32(radius, trig_lookup::index(ctx.sine, 4096 + index)),
            12,
        ),
    );
    p(
        local.as_array(),
        1,
        integer::trunc_shift(fmul_lo32(radius, trig_lookup::index(ctx.sine, index)), 12),
    );
    collision::transform(delta.as_array(), at(ctx.camera, 6), local.as_array(), false);
    let mut i = 0u16;
    while i < 3 {
        p(out, i, g(origin, i) + g(delta.as_array(), i));
        i += 1;
    }
}
fn far(angle: fix32, view: Array<u16>, previous: fix32, second: bool) -> fix32 {
    let x = abs(g(view, 0));
    let y = abs(g(view, 1));
    let z = g(view, 2);
    let mut radius = previous;
    if x < n(2048) {
        radius = z;
    } else if second
        || (angle > n(8190) && angle < n(24570))
        || (angle < n(-8190) && angle > n(-24570))
    {
        radius = z + x;
    } else if angle > n(-8190) && angle < n(8190) {
        radius = z - y;
    } else if angle < n(-24570) || angle > n(24570) {
        radius = z + y;
    }
    if radius < n(12288) {
        return n(12288);
    }
    radius
}
fn geometry(b: Array<u16>, ctx: &Context) {
    let mut origin = at(b, 16);
    if g(b, 2) != n(0) {
        origin = at(b, 22);
    }
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    let view: Buf<u16, 6> = Buf::new([0; 6]);
    let mut i = 0u16;
    while i < 3 {
        p(
            local.as_array(),
            i,
            g(origin, i) - fmul_lo32(g(ctx.camera, i), n(8)),
        );
        i += 1;
    }
    collision::transform(view.as_array(), at(ctx.camera, 6), local.as_array(), true);
    i = 0;
    while i < 10 {
        let r = ray(b, i);
        let q = at(r, 12);
        let a = g(r, 0).lo_bits();
        let c = g(r, 1).lo_bits();
        point(q, origin, g(b, 17), a, ctx);
        point(at(q, 18), origin, g(b, 17), c, ctx);
        let radius = far(integer::from_i16(a as i16), view.as_array(), g(q, 9), false);
        point(at(q, 6), origin, radius, a, ctx);
        let radius = far(integer::from_i16(c as i16), view.as_array(), g(q, 3), true);
        point(at(q, 12), origin, radius, c, ctx);
        i += 1;
    }
    let angles: Buf<u16, 10> = Buf::new([
        1820, 8372, 14924, 21476, 28028, 34580, 41132, 47684, 54236, 60788,
    ]);
    i = 0;
    while i < 10 {
        let r = ring(b, i);
        if g(r, 0) != quad_pool::released() {
            let q = at(r, 2);
            let a = angles[i];
            let mut j = i + 1;
            if j == 10 {
                j = 0;
            }
            let c = angles[j];
            point(q, origin, g(b, 15), c, ctx);
            point(at(q, 6), origin, g(b, 16), c, ctx);
            point(at(q, 12), origin, g(b, 16), a, ctx);
            point(at(q, 18), origin, g(b, 15), a, ctx);
        }
        i += 1;
    }
}
pub fn project(
    state: &State,
    target: fix32,
    frame: fix32,
    ctx: &Context,
    quads: &quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    let mut i = 0u16;
    while i < 2 {
        let b = at(state.banks, i * 816);
        if g(b, 4) != n(0) && g(b, 5) == target && g(b, 0) != n(0) {
            if g(b, 7) == frame {
                if g(b, 1) == n(1) {
                    return n(0);
                }
                p(b, 1, n(1));
            }
            geometry(b, ctx);
            let mut j = 0u16;
            while j < 10 {
                let r = ray(b, j);
                if quad_pool::replace(quads, lines, g(r, 5), at(r, 12)) != 0 {
                    return n(-1);
                }
                j += 1;
            }
            j = 0;
            while j < 10 {
                let r = ring(b, j);
                if g(r, 0) != quad_pool::released()
                    && quad_pool::replace(quads, lines, g(r, 0), at(r, 2)) != 0
                {
                    return n(-1);
                }
                j += 1;
            }
        }
        i += 1;
    }
    n(0)
}
pub fn start(
    state: &State,
    input: &Start,
    ctx: &Context,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    let mut selected = 0u16;
    let mut oldest = n(-1);
    let mut i = 0u16;
    while i < 2 {
        let b = at(state.banks, i * 816);
        let frame = g(b, 7);
        if above(oldest, frame) || oldest == n(-1) {
            selected = i;
            oldest = frame;
        }
        i += 1;
    }
    let b = at(state.banks, selected * 816);
    if g(b, 0) != n(0) {
        i = 0;
        while i < 10 {
            let r = ray(b, i);
            if quad_pool::release(quads, lines, g(r, 5)) != 0 {
                return n(-1);
            }
            p(r, 5, quad_pool::released());
            i += 1;
        }
        let status = retire(ring(b, 0), 34, 0, quads, lines);
        if status != n(0) {
            return status;
        }
    }
    p(b, 0, n(1));
    p(b, 4, n(1));
    p(b, 5, input.target);
    p(b, 7, input.frame);
    p(b, 1, n(0));
    i = 0;
    while i < 3 {
        p(b, 8 + i, g(input.origin, i));
        i += 1;
    }
    p(b, 2, n(0));
    p(b, 3, n(0));
    p(b, 6, fix32::from_words(0, 1));
    if input.follow {
        p(b, 2, n(1));
        let velocity: Buf<u16, 6> = Buf::new([0; 6]);
        i = 0;
        while i < 3 {
            p(b, 11 + i, g(b, 8 + i));
            p(
                velocity.as_array(),
                i,
                integer::sar(g(input.velocity, i), 6),
            );
            i += 1;
        }
        p(b, 14, collision::distance(velocity.as_array()));
    }
    p(b, 16, n(1500));
    p(b, 15, n(100));
    p(b, 17, n(100));
    let sectors: Buf<i16, 40> = Buf::new([
        7280, 10920, -1863, -5031, 10192, 11648, 3540, 2143, 11284, 16380, 7547, 3727, 27300,
        30940, -3960, -7221, 29848, 32760, -2003, -4798, 32760, -27316, 7920, 3261, -20946, -18034,
        -3820, -5870, -18216, -15304, -2888, -4938, -14030, -11482, 4938, 2143, -12210, -4930,
        10017, 4426,
    ]);
    i = 0;
    while i < 10 {
        let r = ray(b, i);
        let q = at(r, 12);
        let mut j = 0u16;
        while j < 2 {
            p(r, j, fix32::from_words(sectors[i * 4 + j] as u16, 0));
            p(r, 2 + j, integer::from_i16(sectors[i * 4 + 2 + j]));
            j += 1;
        }
        p(r, 4, fix32::from_words(i, 0));
        color(q, ctx.tuning, i, n(0), false);
        let mut uv = q;
        uv[28u16] = 13621;
        uv[29u16] = 2613;
        uv[30u16] = 2570;
        uv[31u16] = 13578;
        p(q, 12, g(ctx.materials, 0));
        p(r, 5, quad_pool::allocate(quads, lines, q));
        i += 1;
    }
    i = 0;
    while i < 10 {
        let r = ring(b, i);
        let q = at(r, 2);
        let mut style = q;
        style[26u16] = (style[26u16] & 255) | 32768;
        style[27u16] = 0;
        style[28u16] = 13621;
        style[29u16] = 2613;
        style[30u16] = 2570;
        style[31u16] = 13578;
        p(q, 12, g(ctx.materials, 1));
        p(r, 0, quad_pool::allocate(quads, lines, q));
        i += 1;
    }
    project(state, input.target, input.frame, ctx, quads, lines)
}
pub fn fade(
    state: &State,
    frame: &Frame,
    tuning: Array<u16>,
    quads: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    let target = frame.target;
    let position = frame.position;
    let dt = frame.dt;
    let mut i = 0u16;
    while i < 2 {
        let b = at(state.banks, i * 816);
        if g(b, 4) != n(0) && g(b, 5) == target && g(b, 0) != n(0) {
            if g(b, 7) == frame.number {
                return n(0);
            }
            if g(b, 2) != n(0) || g(b, 3) != n(0) {
                let direction: Buf<u16, 6> = Buf::new([0; 6]);
                let mut j = 0u16;
                while j < 3 {
                    p(direction.as_array(), j, g(position, j) - g(b, 11 + j));
                    j += 1;
                }
                collision::normalize(direction.as_array());
                j = 0;
                while j < 3 {
                    p(
                        b,
                        11 + j,
                        g(b, 11 + j)
                            + integer::sar(
                                fmul_lo32(
                                    integer::sar(
                                        fmul_lo32(g(direction.as_array(), j), g(b, 14)),
                                        12,
                                    ),
                                    dt,
                                ),
                                8,
                            ),
                    );
                    j += 1;
                }
                let mut speed = g(b, 14) - fmul_lo32(dt, n(128));
                if speed < n(0) {
                    speed = n(0);
                }
                p(b, 14, speed);
            }
            p(
                b,
                16,
                g(b, 16) + integer::sar(fmul_lo32(g(tuning, 0), dt), 8),
            );
            p(
                b,
                15,
                g(b, 15) + integer::sar(fmul_lo32(g(tuning, 1), dt), 8),
            );
            p(
                b,
                17,
                g(b, 17) + integer::sar(fmul_lo32(g(tuning, 2), dt), 8),
            );
            let mut j = 0u16;
            while j < 10 {
                let r = ray(b, j);
                let mut k = 0u16;
                while k < 2 {
                    p(
                        r,
                        k,
                        fix32::from_words(
                            (g(r, k) + integer::sar(fmul_lo32(g(r, k + 2), dt), 8)).lo_bits(),
                            0,
                        ),
                    );
                    k += 1;
                }
                let index = g(r, 4);
                if index.hi_bits() != 0 || index.lo_bits() >= 10 {
                    return n(-1);
                }
                color(at(r, 12), tuning, index.lo_bits(), g(b, 6), true);
                let mut next = index + n(1);
                if next == n(10) {
                    next = n(0);
                }
                p(r, 4, next);
                j += 1;
            }
            if g(b, 16) > n(25600) && g(b, 15) > n(25600) {
                let status = retire(ring(b, 0), 34, 0, quads, lines);
                if status != n(0) {
                    return status;
                }
            }
            p(b, 6, g(b, 6) - fmul_lo32(dt, n(192)));
            if g(b, 6) < n(1) {
                let status = retire(ray(b, 0), 44, 5, quads, lines);
                if status != n(0) {
                    return status;
                }
                let status = retire(ring(b, 0), 34, 0, quads, lines);
                if status != n(0) {
                    return status;
                }
                p(b, 0, n(0));
            }
        }
        i += 1;
    }
    n(0)
}
