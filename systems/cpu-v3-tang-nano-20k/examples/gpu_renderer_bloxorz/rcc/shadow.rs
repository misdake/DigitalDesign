//! Ground-clipped box shadow, emitted through immutable triangle Instances.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
mod session;
use session::*;

#[derive(Copy, Clone)]
struct ShadowScalar {
    value: fix32,
}

#[derive(Copy, Clone)]
struct ShadowTriangle {
    x: fix32,
    z: fix32,
    ax: fix32,
    az: fix32,
    bx: fix32,
    bz: fix32,
}
#[derive(Copy, Clone)]
struct ShadowKey {
    row0: Vec4,
    row1: Vec4,
    row2: Vec4,
    generation: u16,
    mask: u16,
    receivers: u16,
}
static SHADOW_VALID: Buf<u16, 1> = Buf::new([0]);
static SHADOW_KEY: Buf<ShadowKey, 1> = Buf::new([ShadowKey {
    row0: Vec4 {
        x: fix32::zero(),
        y: fix32::zero(),
        z: fix32::zero(),
        w: fix32::zero(),
    },
    row1: Vec4 {
        x: fix32::zero(),
        y: fix32::zero(),
        z: fix32::zero(),
        w: fix32::zero(),
    },
    row2: Vec4 {
        x: fix32::zero(),
        y: fix32::zero(),
        z: fix32::zero(),
        w: fix32::zero(),
    },
    generation: 0,
    mask: 0,
    receivers: 0,
}]);

pub fn shadow_invalidate() {
    SHADOW_VALID[0u16] = 0;
}
fn shadow_row_equal(a: Vec4, b: Vec4) -> bool {
    a.x == b.x && a.y == b.y && a.z == b.z && a.w == b.w
}
fn shadow_receivers() -> u16 {
    let mode = visual_mode();
    let mut flags: u16 = 0;
    if mode == ENTRY_OPEN || mode == ENTRY_RISE {
        flags = 1;
    }
    if mode > GOAL_OPEN && mode <= GOAL_CLOSE {
        flags |= 2;
    }
    flags
}

pub static SHADOW_INFO: Buf<u16, 3> = Buf::new([0; 3]);

static SHADOW_CORNERS: Buf<ShadowScalar, 24> = Buf::new(
    [ShadowScalar {
        value: fix32::zero(),
    }; 24],
);
static SHADOW_POINTS: Buf<ShadowScalar, 40> = Buf::new(
    [ShadowScalar {
        value: fix32::zero(),
    }; 40],
);
static SHADOW_HULL: Buf<ShadowScalar, 42> = Buf::new(
    [ShadowScalar {
        value: fix32::zero(),
    }; 42],
);
static SHADOW_CLIP_A: Buf<ShadowScalar, 48> = Buf::new(
    [ShadowScalar {
        value: fix32::zero(),
    }; 48],
);
static SHADOW_CLIP_B: Buf<ShadowScalar, 48> = Buf::new(
    [ShadowScalar {
        value: fix32::zero(),
    }; 48],
);

// Corners and projected candidates die after hull construction. Reuse those
// slots for4+6 triangle records, while hull and per-cell clipping remain live.
fn shadow_cache_memory(index: u16) -> Array<ShadowScalar> {
    if index < 4 {
        SHADOW_CORNERS.as_array()
    } else {
        SHADOW_POINTS.as_array()
    }
}
fn shadow_cache_offset(index: u16) -> u16 {
    if index < 4 {
        index * 6
    } else {
        (index - 4) * 6
    }
}
fn shadow_cache_store(index: u16, t: ShadowTriangle) {
    let mut a = shadow_cache_memory(index);
    let at = shadow_cache_offset(index);
    a[at].value = t.x;
    a[at + 1].value = t.z;
    a[at + 2].value = t.ax;
    a[at + 3].value = t.az;
    a[at + 4].value = t.bx;
    a[at + 5].value = t.bz;
}
fn shadow_cache_load(index: u16) -> ShadowTriangle {
    let a = shadow_cache_memory(index);
    let at = shadow_cache_offset(index);
    ShadowTriangle {
        x: a[at].value,
        z: a[at + 1].value,
        ax: a[at + 2].value,
        az: a[at + 3].value,
        bx: a[at + 4].value,
        bz: a[at + 5].value,
    }
}
fn shadow_turn(ax: fix32, az: fix32, bx: fix32, bz: fix32, cx: fix32, cz: fix32) -> fix32 {
    (bx - ax) * (cz - az) - (bz - az) * (cx - ax)
}
fn shadow_ratio(a: fix32, b: fix32) -> fix32 {
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    if a == zero {
        return zero;
    }
    if b == zero {
        return one;
    }
    let d = a - b;
    let mut t = zero;
    // Scale only genuinely tiny divisors. Scaling an ordinary divisor up to
    // hundreds would quantize its Q16 reciprocal too coarsely and lose cells.
    if d.abs() < fix32::from_words(256, 0) {
        let scale = fix32::from_int(256);
        t = (a * scale) * frcp(d * scale);
    } else {
        t = a * frcp(d);
    }
    if t < zero {
        return zero;
    }
    if t > one {
        return one;
    }
    t
}
fn shadow_project(world: Mat3x4) -> u16 {
    let half = fix32::from_words(32768, 0);
    let radius = fix32::from_words(32113, 0);
    let mut count: u16 = 0;
    let mut i: u16 = 0;
    while i < 8 {
        let x = if i & 4 == 0 { -radius } else { radius };
        let y = if i & 2 == 0 { -radius } else { radius };
        let z = if i & 1 == 0 { -radius } else { radius };
        let px = world.row0.x * x + world.row0.y * y + world.row0.z * z + world.row0.w;
        let py = world.row1.x * x + world.row1.y * y + world.row1.z * z + world.row1.w;
        let pz = world.row2.x * x + world.row2.y * y + world.row2.z * z + world.row2.w;
        SHADOW_CORNERS[i * 3].value = px;
        SHADOW_CORNERS[i * 3 + 1].value = py;
        SHADOW_CORNERS[i * 3 + 2].value = pz;
        if py >= fix32::zero() {
            SHADOW_POINTS[count * 2].value = px - half * py;
            SHADOW_POINTS[count * 2 + 1].value = pz - half * py;
            count += 1;
        }
        i += 1;
    }
    i = 0;
    while i < 8 {
        let mut axis: u16 = 0;
        while axis < 3 {
            let j = i ^ (1 << axis);
            if i < j {
                let ya = SHADOW_CORNERS[i * 3 + 1].value;
                let yb = SHADOW_CORNERS[j * 3 + 1].value;
                if (ya >= fix32::zero() && yb < fix32::zero())
                    || (ya < fix32::zero() && yb >= fix32::zero())
                {
                    let t = shadow_ratio(ya, yb);
                    SHADOW_POINTS[count * 2].value = SHADOW_CORNERS[i * 3].value
                        + (SHADOW_CORNERS[j * 3].value - SHADOW_CORNERS[i * 3].value) * t;
                    SHADOW_POINTS[count * 2 + 1].value = SHADOW_CORNERS[i * 3 + 2].value
                        + (SHADOW_CORNERS[j * 3 + 2].value - SHADOW_CORNERS[i * 3 + 2].value) * t;
                    count += 1;
                }
            }
            axis += 1;
        }
        i += 1;
    }
    // A half-space cut of an affine box has at most10 retained/crossing
    // vertices (including coincident plane vertices). The game's rigid/cardinal
    // roll transforms preserve this topology. Candidate storage also hosts cache.
    assert(count <= 10, 0x5a01);
    count
}
fn shadow_hull(count: u16) -> u16 {
    if count < 3 {
        return 0;
    }
    let mut i: u16 = 1;
    while i < count {
        let x = SHADOW_POINTS[i * 2].value;
        let z = SHADOW_POINTS[i * 2 + 1].value;
        let mut j = i;
        while j > 0
            && (SHADOW_POINTS[(j - 1) * 2].value > x
                || (SHADOW_POINTS[(j - 1) * 2].value == x
                    && SHADOW_POINTS[(j - 1) * 2 + 1].value > z))
        {
            SHADOW_POINTS[j * 2].value = SHADOW_POINTS[(j - 1) * 2].value;
            SHADOW_POINTS[j * 2 + 1].value = SHADOW_POINTS[(j - 1) * 2 + 1].value;
            j -= 1;
        }
        SHADOW_POINTS[j * 2].value = x;
        SHADOW_POINTS[j * 2 + 1].value = z;
        i += 1;
    }
    let mut size: u16 = 0;
    i = 0;
    while i < count {
        let x = SHADOW_POINTS[i * 2].value;
        let z = SHADOW_POINTS[i * 2 + 1].value;
        while size >= 2
            && shadow_turn(
                SHADOW_HULL[(size - 2) * 2].value,
                SHADOW_HULL[(size - 2) * 2 + 1].value,
                SHADOW_HULL[(size - 1) * 2].value,
                SHADOW_HULL[(size - 1) * 2 + 1].value,
                x,
                z,
            ) <= fix32::zero()
        {
            size -= 1;
        }
        assert(size < 21, 0x5a03);
        SHADOW_HULL[size * 2].value = x;
        SHADOW_HULL[size * 2 + 1].value = z;
        size += 1;
        i += 1;
    }
    let lower = size;
    i = count - 1;
    while i > 0 {
        i -= 1;
        let x = SHADOW_POINTS[i * 2].value;
        let z = SHADOW_POINTS[i * 2 + 1].value;
        while size > lower
            && shadow_turn(
                SHADOW_HULL[(size - 2) * 2].value,
                SHADOW_HULL[(size - 2) * 2 + 1].value,
                SHADOW_HULL[(size - 1) * 2].value,
                SHADOW_HULL[(size - 1) * 2 + 1].value,
                x,
                z,
            ) <= fix32::zero()
        {
            size -= 1;
        }
        assert(size < 21, 0x5a03);
        SHADOW_HULL[size * 2].value = x;
        SHADOW_HULL[size * 2 + 1].value = z;
        size += 1;
    }
    size - 1
}
fn shadow_distance(x: fix32, z: fix32, bound: fix32, side: u16) -> fix32 {
    if side == 0 {
        return x - bound;
    }
    if side == 1 {
        return bound - x;
    }
    if side == 2 {
        return z - bound;
    }
    bound - z
}
fn shadow_clip(
    input: Array<ShadowScalar>,
    mut output: Array<ShadowScalar>,
    count: u16,
    bound: fix32,
    side: u16,
) -> u16 {
    if count == 0 {
        return 0;
    }
    let mut size: u16 = 0;
    let mut i: u16 = 0;
    while i < count {
        let previous = if i == 0 { count - 1 } else { i - 1 };
        let a = shadow_distance(
            input[previous * 2].value,
            input[previous * 2 + 1].value,
            bound,
            side,
        );
        let b = shadow_distance(input[i * 2].value, input[i * 2 + 1].value, bound, side);
        let previous_inside = a >= fix32::zero();
        let current_inside = b >= fix32::zero();
        if (previous_inside && !current_inside) || (!previous_inside && current_inside) {
            let t = shadow_ratio(a, b);
            assert(size < 24, 0x5a02);
            output[size * 2].value =
                input[previous * 2].value + (input[i * 2].value - input[previous * 2].value) * t;
            output[size * 2 + 1].value = input[previous * 2 + 1].value
                + (input[i * 2 + 1].value - input[previous * 2 + 1].value) * t;
            size += 1;
        }
        if current_inside {
            assert(size < 24, 0x5a02);
            output[size * 2].value = input[i * 2].value;
            output[size * 2 + 1].value = input[i * 2 + 1].value;
            size += 1;
        }
        i += 1;
    }
    assert(size <= 24, 0x5a02);
    size
}
fn shadow_triangle(
    pass: &mut Pass,
    mesh: &Mesh,
    model: &mut Model,
    material: &Material,
    points: Array<ShadowScalar>,
    i: u16,
) {
    let x = points[0u16].value;
    let z = points[1u16].value;
    let ax = points[(i - 1) * 2].value - x;
    let az = points[(i - 1) * 2 + 1].value - z;
    let bx = points[i * 2].value - x;
    let bz = points[i * 2 + 1].value - z;
    if (ax * bz - az * bx).abs() < fix32::from_words(2, 0) {
        return;
    }
    let triangle: ShadowTriangle = ShadowTriangle {
        x: x,
        z: z,
        ax: ax,
        az: az,
        bx: bx,
        bz: bz,
    };
    let at = SHADOW_INFO[2u16];
    if at < 10 {
        shadow_cache_store(at, triangle);
    } else {
        shadow_invalidate();
    }
    shadow_draw(pass, mesh, model, material, triangle);
}
fn shadow_draw(
    pass: &mut Pass,
    mesh: &Mesh,
    model: &mut Model,
    material: &Material,
    t: ShadowTriangle,
) {
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    model.set_matrices(
        Mat3x4 {
            row0: Vec4 {
                x: t.ax,
                y: zero,
                z: t.bx,
                w: t.x,
            },
            row1: Vec4 {
                x: zero,
                y: one,
                z: zero,
                w: fix32::from_words(393, 0),
            },
            row2: Vec4 {
                x: t.az,
                y: zero,
                z: t.bz,
                w: t.z,
            },
        },
        Mat3x3 {
            row0: demo_point(one, zero, zero),
            row1: demo_point(zero, one, zero),
            row2: demo_point(zero, zero, one),
        },
    );
    let texture = demo_texture();
    let view = texture.view();
    pass.draw(mesh, model, material, &view);
    SHADOW_INFO[2u16] += 1;
}

pub fn draw_shadow(
    pass: &mut Pass,
    mesh: &Mesh,
    model: &mut Model,
    material: &Material,
    block: &Model,
) {
    if !block_visible() {
        shadow_invalidate();
        SHADOW_INFO[0u16] = 0;
        SHADOW_INFO[1u16] = 0;
        SHADOW_INFO[2u16] = 0;
        return;
    }
    let world = block.world();
    let mask = visual_mask();
    let receivers = shadow_receivers();
    let key = SHADOW_KEY[0u16];
    if SHADOW_VALID[0u16] != 0
        && key.generation == LEVEL_GENERATION[0u16]
        && key.mask == mask
        && key.receivers == receivers
        && shadow_row_equal(world.row0, key.row0)
        && shadow_row_equal(world.row1, key.row1)
        && shadow_row_equal(world.row2, key.row2)
    {
        let count = SHADOW_INFO[2u16];
        SHADOW_INFO[2u16] = 0;
        let mut i: u16 = 0;
        while i < count {
            shadow_draw(pass, mesh, model, material, shadow_cache_load(i));
            i += 1;
        }
        return;
    }
    SHADOW_VALID[0u16] = 1;
    SHADOW_KEY[0u16] = ShadowKey {
        row0: world.row0,
        row1: world.row1,
        row2: world.row2,
        generation: LEVEL_GENERATION[0u16],
        mask: mask,
        receivers: receivers,
    };
    SHADOW_INFO[0u16] = 0;
    SHADOW_INFO[1u16] = 0;
    SHADOW_INFO[2u16] = 0;
    let count = shadow_hull(shadow_project(world));
    SHADOW_INFO[0u16] = count;
    if count < 3 {
        return;
    }
    let mut min_x = SHADOW_HULL[0u16].value;
    let mut max_x = min_x;
    let mut min_z = SHADOW_HULL[1u16].value;
    let mut max_z = min_z;
    let mut i: u16 = 1;
    while i < count {
        let x = SHADOW_HULL[i * 2].value;
        let z = SHADOW_HULL[i * 2 + 1].value;
        if x < min_x {
            min_x = x;
        }
        if x > max_x {
            max_x = x;
        }
        if z < min_z {
            min_z = z;
        }
        if z > max_z {
            max_z = z;
        }
        i += 1;
    }
    let half = fix32::from_words(32768, 0);
    let one = fix32::from_int(1);
    let mode = visual_mode();
    i = 0;
    while i < LEVEL[6u16] {
        let x = rem_u16(i, LEVEL[0u16]);
        let z = div_u16(i, LEVEL[0u16]);
        let at = 8 + i * 8;
        let kind = LEVEL[at];
        let left = fix32::from_int(x as i16) - fix32::from_int(LEVEL[0u16] as i16) * half;
        let top = fix32::from_int(z as i16) - fix32::from_int(LEVEL[1u16] as i16) * half;
        let start = x == LEVEL[2u16] && z == LEVEL[3u16];
        let receiving = kind != 0
            && !(kind == 5 && mask & LEVEL[at + 1] == 0)
            && !(start && (mode == ENTRY_OPEN || mode == ENTRY_RISE))
            && !(kind == 6 && mode > GOAL_OPEN && mode <= GOAL_CLOSE);
        if receiving && left + one >= min_x && left <= max_x && top + one >= min_z && top <= max_z {
            let a = SHADOW_CLIP_A.as_array();
            let b = SHADOW_CLIP_B.as_array();
            let mut n = shadow_clip(SHADOW_HULL.as_array(), a, count, left, 0);
            n = shadow_clip(a, b, n, left + one, 1);
            n = shadow_clip(b, a, n, top, 2);
            n = shadow_clip(a, b, n, top + one, 3);
            if n >= 3 {
                SHADOW_INFO[1u16] += 1;
            }
            let mut triangle: u16 = 2;
            while triangle < n {
                shadow_triangle(pass, mesh, model, material, b, triangle);
                triangle += 1;
            }
        }
        i += 1;
    }
}
