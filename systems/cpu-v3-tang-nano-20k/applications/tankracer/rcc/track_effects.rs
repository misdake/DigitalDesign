//! Track spray, joining ground marks and brake/reverse lights on live buffers.
use crate::dsl_rt::*;
use crate::{attitude, collision, effect_pool, integer, quad_pool};
pub struct Spray {
    // timer, unsigned side, count.
    pub header: Array<u16>,
    // Eight rows: position3, velocity3, signed life, size, signed floor,
    // rotation, signed kind, side. Narrow fields are extended to i32.
    pub particles: Array<u16>,
}
pub struct SprayInput {
    pub dt: fix32,
    pub track_speed: fix32,
    pub velocity: Array<u16>,
    pub basis: Array<u16>,
    pub materials: Array<u16>,
    pub kinds: Array<u16>,
    pub kind_stride: u16,
    pub contacts: Array<u16>,
    pub contact_stride: u16,
    pub visible: fix32,
}
pub struct SprayTuning {
    pub kinds: Array<i16>,
    // Six rows: unsigned spin, initial size, gain multiplier, gain shift.
    pub styles: Array<u16>,
}
pub struct MarksInput {
    pub player: bool,
    pub deformable: bool,
    pub mode: fix32,
    pub dt: fix32,
    pub lateral: fix32,
    pub slip: fix32,
    pub velocity: Array<u16>,
    pub materials: Array<u16>,
    pub travel: Array<u16>,
    pub kinds: Array<u16>,
    pub kind_stride: u16,
    pub contacts: Array<u16>,
    pub contact_stride: u16,
    pub tracks: Array<u16>,
    pub track_stride: u16,
}
pub struct LightInput {
    pub player: bool,
    pub disabled: bool,
    pub active: bool,
    pub visible: fix32,
    pub queued: bool,
    pub reverse_braking: bool,
    pub material: fix32,
    pub throttle: fix32,
    pub brake: fix32,
    pub forward: fix32,
    pub position: Array<u16>,
    pub basis: Array<u16>,
    pub has_basis: bool,
    pub materials: Array<u16>,
    pub heights: Array<u16>,
    pub height_stride: u16,
    pub offset: Array<u16>,
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
fn at(v: Array<u16>, w: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(w as i16).as_u16_array() }
}
fn narrow(v: fix32) -> fix32 {
    n(v.lo_bits() as i16)
}
fn word(v: fix32) -> fix32 {
    fix32::from_words(v.lo_bits(), 0)
}
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
fn contact(kind: fix32) -> bool {
    let v = kind.lo_bits() as i16;
    v == 0 || v == 1
}
fn style(t: &SprayTuning, kind: fix32) -> Array<u16> {
    at(t.styles, kind.lo_bits() * 8)
}
pub fn spray(s: &Spray, input: &SprayInput, t: &SprayTuning) -> fix32 {
    p(s.header, 2, n(0));
    let mut free = 65535u16;
    let mut i = 0u16;
    while i < 8 {
        let row = at(s.particles, i * 24);
        if g(row, 6) < n(1) {
            free = i;
            i += 1;
            continue;
        }
        p(s.header, 2, g(s.header, 2) + n(1));
        let kind = g(row, 10);
        if kind.hi_bits() != 0 || kind.lo_bits() >= 6 {
            return n(-1);
        }
        let config = style(t, kind);
        p(
            row,
            3,
            integer::trunc_shift(integer::mul(g(row, 3), n(940)), 10),
        );
        p(row, 4, g(row, 4) - n(100));
        p(
            row,
            5,
            integer::trunc_shift(integer::mul(g(row, 5), n(940)), 10),
        );
        let mut axis = 0u16;
        while axis < 3 {
            p(
                row,
                axis,
                g(row, axis) + integer::trunc_shift(integer::mul(input.dt, g(row, 3 + axis)), 8),
            );
            axis += 1;
        }
        p(
            row,
            6,
            narrow(g(row, 6) - narrow(integer::mul(input.dt, n(6)))),
        );
        p(row, 7, word(g(row, 7) + integer::mul(input.dt, n(2))));
        if g(row, 1) < g(row, 8) {
            p(row, 1, g(row, 8));
        }
        if g(config, 0) != n(0) {
            let mut angle = integer::mul(input.dt, narrow(g(config, 0)));
            if g(row, 11) != n(0) {
                angle = -angle;
            }
            p(row, 9, word(g(row, 9) + angle));
        }
        i += 1;
    }
    p(s.header, 0, g(s.header, 0) + input.dt);
    if free == 65535 || g(s.header, 0) < n(40) {
        return n(0);
    }
    let side = n(1) - g(s.header, 1);
    p(s.header, 1, side);
    if side.hi_bits() != 0 || side.lo_bits() >= 2 {
        return n(-1);
    }
    let material = g(input.materials, side.lo_bits());
    if material.hi_bits() != 0 || material.lo_bits() >= 16 {
        return n(-1);
    }
    let kind = n(t.kinds[material.lo_bits()]);
    let row = at(s.particles, free * 24);
    p(row, 10, kind);
    if kind == n(-1) || (kind == n(5) && input.visible != n(1)) {
        return n(0);
    }
    let magnitude = abs(input.track_speed) - fix32::from_words(50000, 0);
    let front = side.lo_bits() * 2;
    if magnitude < n(1)
        || (!contact(g(input.kinds, front * input.kind_stride))
            && !contact(g(input.kinds, (front + 1) * input.kind_stride)))
    {
        return n(0);
    }
    if kind.hi_bits() != 0 || kind.lo_bits() >= 6 {
        return n(-1);
    }
    let config = style(t, kind);
    let mut axis = 0u16;
    while axis < 3 {
        p(
            row,
            axis,
            g(input.contacts, front * input.contact_stride + axis),
        );
        axis += 1;
    }
    p(row, 11, side);
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    p(
        local.as_array(),
        2,
        -integer::trunc_shift(input.track_speed, 6),
    );
    collision::transform(at(row, 6), input.basis, local.as_array(), false);
    p(
        row,
        3,
        g(row, 3) + integer::trunc_shift(g(input.velocity, 0), 6),
    );
    let gain = integer::trunc_shift(
        integer::mul(magnitude, g(config, 2)),
        g(config, 3).lo_bits(),
    );
    p(row, 4, g(row, 4) + gain);
    p(row, 6, n(2048));
    p(
        row,
        5,
        g(row, 5) + integer::trunc_shift(g(input.velocity, 2), 6),
    );
    p(row, 8, narrow(g(row, 1)));
    p(row, 7, word(g(config, 1) + gain));
    p(s.header, 0, n(0));
    n(0)
}
fn mark_push(
    s: Array<u16>,
    quad: Array<u16>,
    pool: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    if g(s, 5) == n(68) {
        let head = g(s, 6);
        if head.hi_bits() != 0 || head.lo_bits() >= 68 {
            return n(-1);
        }
        if quad_pool::release(pool, lines, g(s, 20 + head.lo_bits())) != 0 {
            return n(-1);
        }
        let mut value = head + n(1);
        if value == n(68) {
            value = n(0);
        }
        p(s, 6, value);
        p(s, 5, g(s, 5) - n(1));
    }
    let handle = quad_pool::allocate(pool, lines, quad);
    if handle != n(-1) {
        let tail = g(s, 7);
        if tail.hi_bits() != 0 || tail.lo_bits() >= 68 {
            return n(-1);
        }
        p(s, 20 + tail.lo_bits(), handle);
        let mut value = tail + n(1);
        if value == n(68) {
            value = n(0);
        }
        p(s, 7, value);
        p(s, 5, g(s, 5) + n(1));
    }
    n(0)
}
// State: active2, movement3, count/head/tail, endpoints12, handles68 (352 B).
pub fn marks(
    s: Array<u16>,
    input: &MarksInput,
    pool: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    if !input.player || input.deformable || input.mode == n(2) || input.mode == n(3) {
        return n(0);
    }
    if abs(input.lateral) < fix32::from_words(14464, 1) && abs(input.slip) < n(25000) {
        p(s, 0, n(0));
        p(s, 1, n(0));
        return n(0);
    }
    let mut axis = 0u16;
    while axis < 3 {
        p(
            s,
            2 + axis,
            integer::trunc_shift(integer::mul(g(input.velocity, axis), input.dt), 14),
        );
        axis += 1;
    }
    let quad: Buf<u16, 32> = Buf::new([0; 32]);
    let q = quad.as_array();
    let mut bytes = q;
    bytes[26u16] = 1536;
    bytes[27u16] = 1542;
    bytes[28u16] = 257;
    bytes[29u16] = 319;
    bytes[30u16] = 16191;
    bytes[31u16] = 16129;
    let mut side = 0u16;
    while side < 2 {
        let old = g(s, side) != n(0);
        if g(input.materials, side) != n(1) {
            side += 1;
            continue;
        }
        let front = side * 2;
        let rear = front + 1;
        let mut travel = g(input.travel, front);
        if g(input.travel, rear) > travel {
            travel = g(input.travel, rear);
        }
        let eligible = travel >= fix32::from_words(40001, 0)
            && (contact(g(input.kinds, front * input.kind_stride))
                || contact(g(input.kinds, rear * input.kind_stride)));
        let endpoints = 8 + side * 6;
        axis = 0;
        while axis < 3 {
            p(q, axis, g(s, endpoints + axis));
            p(q, 3 + axis, g(s, endpoints + 3 + axis));
            axis += 1;
        }
        if eligible {
            p(s, side, n(1));
            let matrix = at(input.tracks, side * input.track_stride * 2);
            let mut height = g(input.contacts, rear * input.contact_stride + 1);
            if g(input.contacts, front * input.contact_stride + 1) < height {
                height = g(input.contacts, front * input.contact_stride + 1);
            }
            axis = 0;
            while axis < 3 {
                let direction = integer::trunc_shift(integer::mul(g(matrix, axis), n(200)), 10);
                let offset = integer::trunc_shift(g(matrix, 6 + axis), 3);
                let mut rear_point = g(input.contacts, rear * input.contact_stride + axis);
                if axis == 1 {
                    rear_point = height;
                }
                rear_point = rear_point + offset + g(s, 2 + axis);
                let mut edge = rear_point + direction;
                if side != 0 {
                    edge = rear_point - direction;
                }
                if axis == 1 {
                    edge = rear_point;
                }
                p(s, endpoints + axis, edge);
                p(s, endpoints + 3 + axis, rear_point);
                p(q, 6 + axis, rear_point);
                p(q, 9 + axis, edge);
                axis += 1;
            }
            if !old {
                side += 1;
                continue;
            }
        } else if old {
            p(s, side, n(0));
            axis = 0;
            while axis < 3 {
                p(q, 6 + axis, g(q, 3 + axis) + g(s, 2 + axis));
                p(q, 9 + axis, g(q, axis) + g(s, 2 + axis));
                axis += 1;
            }
        } else {
            side += 1;
            continue;
        }
        let status = mark_push(s, q, pool, lines);
        if status != n(0) {
            return status;
        }
        side += 1;
    }
    n(0)
}
fn release_light(
    s: Array<u16>,
    side: u16,
    pool: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    if g(s, 1 + side) != n(0) {
        if quad_pool::release(pool, lines, g(s, 3 + side)) != 0 {
            return n(-1);
        }
        p(s, 1 + side, n(0));
    }
    n(0)
}
fn release_lights(s: Array<u16>, pool: &mut quad_pool::Pool, lines: &effect_pool::Pool) -> fix32 {
    let status = release_light(s, 0, pool, lines);
    if status != n(0) {
        return status;
    }
    release_light(s, 1, pool, lines)
}
// State: brake-mode byte, active2 bytes, handles2, all extended i32 (20 B).
pub fn lights(
    s: Array<u16>,
    input: &LightInput,
    pool: &mut quad_pool::Pool,
    lines: &effect_pool::Pool,
) -> fix32 {
    if input.visible != n(1) || (!input.player && (!input.active || input.disabled)) {
        return release_lights(s, pool, lines);
    }
    let brake = input.brake != n(0);
    if (!brake && g(s, 0) != n(0)) || (brake && g(s, 0) == n(0)) || input.throttle >= n(0) {
        let status = release_lights(s, pool, lines);
        if status != n(0) {
            return status;
        }
    }
    if input.queued {
        return n(0);
    }
    let mut gray = 0u16;
    if !brake {
        if input.throttle >= n(0) {
            return n(0);
        }
        p(s, 0, n(0));
        gray = 128;
    } else if (input.forward > n(1000) && input.throttle == n(0))
        || (input.reverse_braking && input.forward < n(-1000))
    {
        p(s, 0, n(1));
    } else {
        return release_lights(s, pool, lines);
    }
    let mut height = n(0);
    let mut i = 0u16;
    while i < 4 {
        height = height + g(input.heights, i * input.height_stride);
        i += 1;
    }
    height = integer::sar(height, 2);
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    let v = local.as_array();
    let quad: Buf<u16, 32> = Buf::new([0; 32]);
    let q = quad.as_array();
    let mut bytes = q;
    p(q, 12, input.material);
    bytes[26u16] = 32769;
    bytes[27u16] = gray | (gray << 8);
    bytes[28u16] = 257;
    bytes[29u16] = 287;
    bytes[30u16] = 7967;
    bytes[31u16] = 7937;
    let mut side = 0u16;
    while side < 2 {
        if g(input.materials, side) != n(1) || g(input.position, 1) - height > n(2999) {
            let status = release_light(s, side, pool, lines);
            if status != n(0) {
                return status;
            }
            side += 1;
            continue;
        }
        let x = g(input.offset, 0);
        let mut x1 = -x - n(325);
        let mut x2 = -x + n(125);
        if side != 0 {
            x1 = x - n(125);
            x2 = x + n(325);
        }
        i = 0;
        while i < 4 {
            let mut px = x1;
            if i == 0 || i == 3 {
                px = x2;
            }
            let mut z = g(input.offset, 2);
            if i >= 2 {
                z = z + n(1200);
            }
            p(v, 0, px);
            p(v, 1, n(-1500));
            p(v, 2, z);
            if input.has_basis {
                collision::transform(v, input.basis, v, false);
            }
            let mut axis = 0u16;
            while axis < 3 {
                p(q, (3 - i) * 3 + axis, g(v, axis) + g(input.position, axis));
                axis += 1;
            }
            i += 1;
        }
        if g(s, 1 + side) == n(0) {
            p(s, 3 + side, quad_pool::allocate(pool, lines, q));
            p(s, 1 + side, n(1));
        } else {
            let handle = g(s, 3 + side);
            if (handle < n(0) || handle > n(160)) && handle != n(-1) {
                return n(-1);
            }
            i = 0;
            while i < 12 {
                quad_pool::put(pool, lines, handle, i, integer::sar(g(q, i), 3));
                i += 1;
            }
        }
        side += 1;
    }
    n(0)
}
