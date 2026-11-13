//! Ground-clipped shadow with receiver-cell batches and exact-triangle fallback.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;

mod session;
use session::*;
mod render_stats;
use render_stats::*;

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
    if shadow_batch_enabled() {
        let mut data = shadow_batch_data();
        data[68u16] = 0;
        data[4292u16] = 0;
    }
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
    if FRACTURE[0u16] != 65535 && fragile_dropped(FRACTURE[0u16]) {
        flags |= 4 | (FRACTURE[0u16] << 3);
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
    let half = fix!(0.5);
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
            row0: Vec3::new(one, zero, zero),
            row1: Vec3::new(zero, one, zero),
            row2: Vec3::new(zero, zero, one),
        },
    );
    // The immutable shadow mesh carries white vertex colors and no texture UVs.
    pass.draw_colored(mesh.ptr(), model.ptr(), material.ptr());
    DRAW_CALLS[0u16] += mesh.meshlet_count();
    SHADOW_INFO[2u16] += 1;
}

pub fn draw_shadow(
    pass: &mut Pass,
    mesh: &Mesh,
    model: &mut Model,
    material: &Material,
    block: &Model,
) {
    if shadow_batch_enabled() {
        shadow_batch_select();
    }
    if !block_visible() {
        if shadow_batch_enabled() {
            let mut data = shadow_batch_slot();
            data[4u16] = 0;
        }
        shadow_invalidate();
        SHADOW_INFO[0u16] = 0;
        SHADOW_INFO[1u16] = 0;
        SHADOW_INFO[2u16] = 0;
        return;
    }
    let world = block.world();
    let mask = visual_mask();
    let receivers = shadow_receivers();
    if shadow_batch_enabled() {
        if shadow_batch_matches(world, mask, receivers) {
            shadow_batch_restore();
            shadow_batch_draw(pass, model, material);
            return;
        }
        SHADOW_VALID[0u16] = 0;
        shadow_batch_prepare(world, mask, receivers);
    }
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
    if !shadow_batch_enabled() {
        SHADOW_VALID[0u16] = 1;
        SHADOW_KEY[0u16] = ShadowKey {
            row0: world.row0,
            row1: world.row1,
            row2: world.row2,
            generation: LEVEL_GENERATION[0u16],
            mask: mask,
            receivers: receivers,
        };
    }
    SHADOW_INFO[0u16] = 0;
    SHADOW_INFO[1u16] = 0;
    SHADOW_INFO[2u16] = 0;
    let count = shadow_hull(shadow_project(world));
    SHADOW_INFO[0u16] = count;
    draw_shadow_hull(pass, mesh, model, material, count, mask);
}

// Projection/cache selection is complete; clip the shared hull to receiver cells.
fn draw_shadow_hull(
    pass: &mut Pass,
    mesh: &Mesh,
    model: &mut Model,
    material: &Material,
    count: u16,
    mask: u16,
) {
    if count < 3 {
        shadow_batch_finish(pass, model, material);
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
    let half = fix!(0.5);
    let one = fix32::from_int(1);
    let mode = visual_mode();
    // Inclusive overlap needs x >= min_x + half_width - 1 and
    // x <= max_x + half_width. Floor the lower bound conservatively so
    // exact shared boundaries retain the original clipping/admission order.
    let width = LEVEL[0u16];
    let height = LEVEL[1u16];
    let half_width = fix32::from_int(width as i16) * half;
    let half_height = fix32::from_int(height as i16) * half;
    let mut first_x = (min_x + half_width - one).to_int();
    let mut last_x = (max_x + half_width).to_int();
    let mut first_z = (min_z + half_height - one).to_int();
    let mut last_z = (max_z + half_height).to_int();
    if first_x < 0 {
        first_x = 0;
    }
    if first_z < 0 {
        first_z = 0;
    }
    if last_x >= width as i16 {
        last_x = width as i16 - 1;
    }
    if last_z >= height as i16 {
        last_z = height as i16 - 1;
    }
    let mut z = first_z;
    while z <= last_z {
        let top = fix32::from_int(z) - half_height;
        let mut x = first_x;
        i = z as u16 * width + x as u16;
        while x <= last_x && i < LEVEL[6u16] {
            let kind = level_field(i, 0);
            let left = fix32::from_int(x) - half_width;
            let start = x as u16 == LEVEL[2u16] && z as u16 == LEVEL[3u16];
            let receiving = kind != 0
                && !fragile_dropped(i)
                && !(kind == 5 && mask & level_field(i, 1) == 0)
                && !(start && (mode == ENTRY_OPEN || mode == ENTRY_RISE))
                && !(kind == 6 && mode > GOAL_OPEN && mode <= GOAL_CLOSE);
            if receiving
                && left + one >= min_x
                && left <= max_x
                && top + one >= min_z
                && top <= max_z
            {
                let mut points = SHADOW_HULL.as_array();
                let mut destination = SHADOW_CLIP_A.as_array();
                let mut n = count;
                if min_x < left {
                    n = shadow_clip(points, destination, n, left, 0);
                    points = destination;
                    destination =
                        if points.as_ptr().addr() == SHADOW_CLIP_A.as_array().as_ptr().addr() {
                            SHADOW_CLIP_B.as_array()
                        } else {
                            SHADOW_CLIP_A.as_array()
                        };
                }
                if max_x > left + one {
                    n = shadow_clip(points, destination, n, left + one, 1);
                    points = destination;
                    destination =
                        if points.as_ptr().addr() == SHADOW_CLIP_A.as_array().as_ptr().addr() {
                            SHADOW_CLIP_B.as_array()
                        } else {
                            SHADOW_CLIP_A.as_array()
                        };
                }
                if min_z < top {
                    n = shadow_clip(points, destination, n, top, 2);
                    points = destination;
                    destination =
                        if points.as_ptr().addr() == SHADOW_CLIP_A.as_array().as_ptr().addr() {
                            SHADOW_CLIP_B.as_array()
                        } else {
                            SHADOW_CLIP_A.as_array()
                        };
                }
                if max_z > top + one {
                    n = shadow_clip(points, destination, n, top + one, 3);
                    points = destination;
                }
                if n >= 3 {
                    SHADOW_INFO[1u16] += 1;
                }
                if shadow_batch_enabled() {
                    SHADOW_INFO[2u16] += shadow_batch_cell(points, n, left, top);
                } else {
                    let mut triangle: u16 = 2;
                    while triangle < n {
                        shadow_triangle(pass, mesh, model, material, points, triangle);
                        triangle += 1;
                    }
                }
            }
            i += 1;
            x += 1;
        }
        z += 1;
    }
    shadow_batch_finish(pass, model, material);
}
// Caller-owned receiver-cell buffers. Rebuild only after prior GPU retirement
// and CB reset; free only after the caller has dropped both command buffers.
const SHADOW_BATCH_DIAGNOSTICS: u16 = 0;
static SHADOW_BATCH_BASE: u16 = 0;
fn shadow_batch_data() -> Array<u16> {
    unsafe { Ptr::from_addr(SHADOW_BATCH_BASE).as_u16_array() }
}
fn shadow_batch_slot() -> Array<u16> {
    unsafe { Ptr::from_addr(shadow_batch_data()[1u16]).as_u16_array() }
}
pub fn shadow_batch_begin() -> Ptr {
    let owner = malloc(8575);
    let base = (owner.addr() + 63) & 65472;
    unsafe {
        addr_of(&SHADOW_BATCH_BASE).write(0, base);
    }
    let mut data = shadow_batch_data();
    let mut i: u16 = 0;
    while i < 8512 {
        data[i] = 0;
        i += 1;
    }
    owner
}
pub fn shadow_batch_frame() {
    shadow_batch_data()[0u16] = 0;
}
pub fn shadow_batch_end(owner: Ptr) {
    unsafe {
        addr_of(&SHADOW_BATCH_BASE).write(0, 0);
    }
    free(owner);
}
fn shadow_batch_enabled() -> bool {
    SHADOW_BATCH_BASE != 0
}
fn shadow_batch_select() {
    let mut data = shadow_batch_data();
    let slot = data[0u16];
    assert(slot < 2, 0x5b01);
    data[1u16] = SHADOW_BATCH_BASE + 64 + slot * 4224;
    data[0u16] = slot + 1;
}
fn shadow_batch_scalar(mut data: Array<u16>, at: u16, value: fix32, store: bool) -> bool {
    if store {
        data[at] = value.lo_bits();
        data[at + 1] = value.hi_bits();
        return true;
    }
    data[at] == value.lo_bits() && data[at + 1] == value.hi_bits()
}
fn shadow_batch_row(data: Array<u16>, at: u16, row: Vec4, store: bool) -> bool {
    shadow_batch_scalar(data, at, row.x, store)
        && shadow_batch_scalar(data, at + 2, row.y, store)
        && shadow_batch_scalar(data, at + 4, row.z, store)
        && shadow_batch_scalar(data, at + 6, row.w, store)
}
fn shadow_batch_matches(world: Mat3x4, mask: u16, receivers: u16) -> bool {
    let data = shadow_batch_slot();
    data[4u16] != 0
        && data[29u16] == LEVEL_GENERATION[0u16]
        && data[30u16] == mask
        && data[31u16] == receivers
        && shadow_batch_row(data, 5, world.row0, false)
        && shadow_batch_row(data, 13, world.row1, false)
        && shadow_batch_row(data, 21, world.row2, false)
}
fn shadow_batch_prepare(world: Mat3x4, mask: u16, receivers: u16) {
    let mut data = shadow_batch_slot();
    data[0u16] = 0;
    data[4u16] = 0;
    shadow_batch_row(data, 5, world.row0, true);
    shadow_batch_row(data, 13, world.row1, true);
    shadow_batch_row(data, 21, world.row2, true);
    data[29u16] = LEVEL_GENERATION[0u16];
    data[30u16] = mask;
    data[31u16] = receivers;
    if SHADOW_BATCH_DIAGNOSTICS != 0 {
        let mut i: u16 = 32;
        while i < 40 {
            data[i] = 0;
            i += 1;
        }
    }
}
fn shadow_batch_restore() {
    let data = shadow_batch_slot();
    SHADOW_INFO[0u16] = data[1u16];
    SHADOW_INFO[1u16] = data[2u16];
    SHADOW_INFO[2u16] = data[3u16];
}
fn shadow_batch_index(mut words: Array<u16>, base: u16, triangle: u16, fan: u16) {
    let at = base + (triangle / 3) * 4;
    let slot = triangle % 3;
    if slot == 0 {
        words[at] |= ((fan - 1) << 6) | (fan << 12);
        words[at + 1] |= fan >> 4;
    } else if slot == 1 {
        words[at + 1] |= ((fan - 1) << 8) | (fan << 14);
        words[at + 2] |= fan >> 2;
    } else {
        words[at + 2] |= (fan - 1) << 10;
        words[at + 3] |= fan;
    }
}
fn shadow_batch_cell(points: Array<ShadowScalar>, count: u16, left: fix32, top: fix32) -> u16 {
    if count < 3 {
        return 0;
    }
    assert(count <= 24, 0x5b02);
    let mut data = shadow_batch_slot();
    let record = data[0u16];
    assert(record < 16, 0x5b03);
    let offset = 128 + record * 256;
    let mut words = unsafe { data.as_ptr().add(offset as i16).as_u16_array() };
    let index_base = 64 + ((count * 6 + 3) >> 2) * 4;
    // Header plus every potentially fetched payload sector, including padding.
    let clear_words = 64 + ((index_base - 64 + (count / 3) * 4 + 63) >> 6) * 64;
    let mut i: u16 = 0;
    while i < clear_words {
        words[i] = 0;
        i += 1;
    }
    let scale = fix32::from_int(512);
    let quantum = fix32::from_words(128, 0);
    let half = fix!(0.5);
    i = 0;
    while i < count {
        let px = points[i * 2].value;
        let pz = points[i * 2 + 1].value;
        let x = ((px - left) * scale + half).to_int() as u16;
        let z = ((pz - top) * scale + half).to_int() as u16;
        assert(x < 1024 && z < 1024, 0x5b04);
        if SHADOW_BATCH_DIAGNOSTICS != 0 {
            let ex = (left + fix32::from_int(x as i16) * quantum - px)
                .abs()
                .lo_bits();
            let ez = (top + fix32::from_int(z as i16) * quantum - pz)
                .abs()
                .lo_bits();
            if ex > data[32u16] {
                data[32u16] = ex;
            }
            if ez > data[33u16] {
                data[33u16] = ez;
            }
            if ex != 0 || ez != 0 {
                data[34u16] += 1;
            }
        }
        let at = 64 + i * 6;
        words[at] = x;
        words[at + 1] = z << 4;
        words[at + 2] = 64512;
        words[at + 3] = 7;
        words[at + 4] = 65535;
        words[at + 5] = 255;
        i += 1;
    }
    if SHADOW_BATCH_DIAGNOSTICS != 0 {
        if count > data[35u16] {
            data[35u16] = count;
        }
        data[36u16] += count;
    }
    let mut triangles: u16 = 0;
    i = 2;
    while i < count {
        let x = points[0u16].value;
        let z = points[1u16].value;
        let ax = points[(i - 1) * 2].value - x;
        let az = points[(i - 1) * 2 + 1].value - z;
        let bx = points[i * 2].value - x;
        let bz = points[i * 2 + 1].value - z;
        if (ax * bz - az * bx).abs() >= fix32::from_words(2, 0) {
            shadow_batch_index(words, index_base, triangles, i);
            triangles += 1;
        }
        i += 1;
    }
    if triangles == 0 {
        return 0;
    }
    words[0u16] = count | (triangles << 8);
    words[1u16] = 20 | (((index_base - 64) / 4) << 8);
    words[4u16] = 16;
    words[8u16] = left.lo_bits();
    words[9u16] = left.hi_bits();
    words[10u16] = 393;
    words[11u16] = 0;
    words[12u16] = top.lo_bits();
    words[13u16] = top.hi_bits();
    words[14u16] = 263;
    data[64 + record * 2] = (shadow_batch_data()[1u16] + offset) >> 6;
    data[65 + record * 2] = ((index_base - 64 + ((triangles + 2) / 3) * 4 + 63) >> 6);
    data[0u16] = record + 1;
    triangles
}
fn shadow_batch_draw(pass: &mut Pass, model: &mut Model, material: &Material) {
    let data = shadow_batch_slot();
    if data[0u16] == 0 {
        return;
    }
    let sector = shadow_batch_data()[1u16] >> 6;
    let mesh = Mesh::from_native(MeshDesc {
        directory_sector: sector + 1,
        directory_word: 0,
        meshlet_count: data[0u16],
        storage: unsafe { GpuMemory::from_sectors(sector + 1, 65) },
    });
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    model.set_matrices(
        Mat3x4 {
            row0: Vec4 {
                x: one,
                y: zero,
                z: zero,
                w: zero,
            },
            row1: Vec4 {
                x: zero,
                y: one,
                z: zero,
                w: zero,
            },
            row2: Vec4 {
                x: zero,
                y: zero,
                z: one,
                w: zero,
            },
        },
        Mat3x3 {
            row0: Vec3::new(one, zero, zero),
            row1: Vec3::new(zero, one, zero),
            row2: Vec3::new(zero, zero, one),
        },
    );
    pass.draw_colored(mesh.ptr(), model.ptr(), material.ptr());
    DRAW_CALLS[0u16] += mesh.meshlet_count();
}
fn shadow_batch_finish(pass: &mut Pass, model: &mut Model, material: &Material) {
    if !shadow_batch_enabled() {
        return;
    }
    let mut data = shadow_batch_slot();
    data[1u16] = SHADOW_INFO[0u16];
    data[2u16] = SHADOW_INFO[1u16];
    data[3u16] = SHADOW_INFO[2u16];
    data[4u16] = 1;
    shadow_batch_draw(pass, model, material);
}
