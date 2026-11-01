//! Numeric preparation for the explicit scene API; no allocation or commands.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;

pub fn gpu_scene_fix_read(data: Array<u16>, at: u16) -> fix32 {
    fix32::from_words(data[at], data[at + 1])
}

pub fn gpu_scene_fix_write(mut data: Array<u16>, at: u16, value: fix32) {
    data[at] = value.lo_bits();
    data[at + 1] = value.hi_bits();
}

pub fn gpu_scene_zero(mut data: Array<u16>, count: u16) {
    let mut at: u16 = 0;
    while at < count {
        data[at] = 0;
        at += 1;
    }
}

pub fn gpu_scene_vec_read(data: Array<u16>, at: u16) -> vec3 {
    vec3::new(
        gpu_scene_fix_read(data, at),
        gpu_scene_fix_read(data, at + 2),
        gpu_scene_fix_read(data, at + 4),
    )
}

pub fn gpu_scene_vec_write(data: Array<u16>, at: u16, value: vec3) {
    gpu_scene_fix_write(data, at, value.x());
    gpu_scene_fix_write(data, at + 2, value.y());
    gpu_scene_fix_write(data, at + 4, value.z());
}

pub fn gpu_scene_normalize(raw: vec3) -> vec3 {
    let mut value = raw;
    let half = fix32::from_words(32768, 0);
    let large = fix32::from_int(16384);
    // Halve only large inputs, before abs/square. This includes -32768 without
    // discarding the smallest nonzero Q16.16 directions by unconditional scaling.
    if raw.x() > large
        || raw.x() < -large
        || raw.y() > large
        || raw.y() < -large
        || raw.z() > large
        || raw.z() < -large
    {
        value = value * half;
    }
    let mut largest = value.x().abs();
    if value.y().abs() > largest {
        largest = value.y().abs();
    }
    if value.z().abs() > largest {
        largest = value.z().abs();
    }
    if largest == fix32::zero() {
        return value;
    }
    let one = fix32::from_int(1);
    let two = fix32::from_int(2);
    while largest > one {
        value = value * half;
        largest = largest * half;
    }
    while largest < half {
        value = value * two;
        largest = largest * two;
    }
    value * frsqrt(fdot(value, value))
}

pub fn gpu_scene_cross(a: vec3, b: vec3) -> vec3 {
    vec3::new(
        a.y() * b.z() - a.z() * b.y(),
        a.z() * b.x() - a.x() * b.z(),
        a.x() * b.y() - a.y() * b.x(),
    )
}

// Called by native draw preparation after selecting its material and state.
// Preserve fix32 until the product; quantize exactly once to native Q8.
// Both-profile range checks are the minimum native encoding prerequisite.
pub fn gpu_scene_light_response(
    state: Array<u16>,
    material: Array<u16>,
    mut native: Array<u16>,
) -> bool {
    let ambient = gpu_scene_fix_read(state, GPU_STATE_AMBIENT)
        * gpu_scene_fix_read(material, GPU_MATERIAL_AMBIENT);
    let directional = gpu_scene_fix_read(state, GPU_STATE_DIRECTIONAL)
        * gpu_scene_fix_read(material, GPU_MATERIAL_DIRECTIONAL);
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    if ambient < zero || ambient > one || directional < zero || directional > one {
        return false;
    }
    let scale = fix32::from_int(256);
    native[GPU_SCRATCH_GLOBAL + 8] = (ambient * scale).round().to_int() as u16;
    native[GPU_SCRATCH_GLOBAL + 9] = (directional * scale).round().to_int() as u16;
    true
}

pub fn gpu_scene_render_prepare(
    state: Array<u16>,
    material: Array<u16>,
    view: Array<u16>,
    textured: bool,
    mut native: Array<u16>,
) -> bool {
    let mut base: u16 = 0;
    let mut exponent: u16 = 0;
    let mut mip: u16 = 0;
    let mut fixed: u16 = 0;
    let mut filter: u16 = 0;
    if textured {
        base = view[GPU_VIEW_BASE];
        exponent = view[GPU_VIEW_EXPONENT];
        mip = view[GPU_VIEW_FLAGS] & 1;
        fixed = (view[GPU_VIEW_FLAGS] >> 1) & 1;
        filter = state[GPU_STATE_FILTER];
    }
    if fixed != 0 && filter > 1 {
        return false;
    }
    let start = GPU_SCRATCH_RENDER;
    let mut at: u16 = 0;
    while at < 16 {
        native[start + at] = 0;
        at += 1;
    }
    native[start] = base;
    native[start + 1] = exponent | (mip << 4) | (filter << 5) | (fixed << 7);
    native[start + 2] = material[GPU_MATERIAL_SPECULAR];
    native[start + 3] = material[GPU_MATERIAL_SPECULAR + 1];
    native[start + 4] = state[GPU_STATE_CONTROL] | ((material[GPU_MATERIAL_FLAGS] & 1) << 5);
    true
}

struct GpuSceneWide {
    low: u16,
    high: u16,
}
fn gpu_scene_square(value: i16) -> GpuSceneWide {
    let mut magnitude = value as u16;
    if value < 0 {
        magnitude = (-value) as u16;
    }
    let lo = magnitude & 255;
    let hi = magnitude >> 8;
    let product = lo * lo;
    let cross = lo * hi;
    let low = product + (cross << 9);
    let mut carry: u16 = 0;
    if low < product {
        carry = 1;
    }
    GpuSceneWide {
        low: low,
        high: hi * hi + (cross >> 7) + carry,
    }
}
fn gpu_scene_unit_codes(x: i16, y: i16, z: i16) -> bool {
    if x < -16384 || x > 16384 || y < -16384 || y > 16384 || z < -16384 || z > 16384 {
        return false;
    }
    let a: GpuSceneWide = gpu_scene_square(x);
    let b: GpuSceneWide = gpu_scene_square(y);
    let c: GpuSceneWide = gpu_scene_square(z);
    let ab = a.low + b.low;
    let low = ab + c.low;
    let mut carry: u16 = 0;
    if ab < a.low {
        carry += 1;
    }
    if low < ab {
        carry += 1;
    }
    let high = a.high + b.high + c.high + carry;
    (high == 4095 && low >= 32768) || (high == 4096 && low <= 32768)
}
fn gpu_scene_q14_value(code: u16) -> fix32 {
    fix32::from_words(code << 2, ((code as i16) >> 14) as u16)
}
fn gpu_scene_light_encode(raw: vec3, mut native: Array<u16>) -> bool {
    // A unit-vector input with small fixed-point error fits well inside this
    // bound. Reject before Q14 scaling can overflow the signed fix32 format.
    let limit = fix32::from_words(32768, 1);
    if raw.x() < -limit
        || raw.x() > limit
        || raw.y() < -limit
        || raw.y() > limit
        || raw.z() < -limit
        || raw.z() > limit
    {
        return false;
    }
    let scale = fix32::from_int(16384);
    native[2u16] = (raw.x() * scale).round().to_int() as u16;
    native[3u16] = (raw.y() * scale).round().to_int() as u16;
    native[4u16] = (raw.z() * scale).round().to_int() as u16;
    let mut largest: u16 = 2;
    let mut at: u16 = 3;
    while at < 5 {
        if fix32::from_int(native[at] as i16).abs() > fix32::from_int(native[largest] as i16).abs()
        {
            largest = at;
        }
        at += 1;
    }
    let mut attempt: u16 = 0;
    while attempt < 16 {
        if gpu_scene_unit_codes(
            native[2u16] as i16,
            native[3u16] as i16,
            native[4u16] as i16,
        ) {
            return true;
        }
        let value = vec3::new(
            gpu_scene_q14_value(native[2u16]),
            gpu_scene_q14_value(native[3u16]),
            gpu_scene_q14_value(native[4u16]),
        );
        let mut delta: i16 = -1;
        if fdot(value, value) < fix32::from_int(1) {
            delta = 1;
        }
        let code = native[largest] as i16;
        if code < 0 {
            native[largest] = (code - delta) as u16;
        } else {
            native[largest] = (code + delta) as u16;
        }
        attempt += 1;
    }
    false
}

// Shared preparation preserves the original failure and scratch-write boundaries.
pub fn gpu_scene_draw_header(
    desired: Array<u16>,
    material: Array<u16>,
    view: Array<u16>,
    textured: bool,
    mut native: Array<u16>,
) -> bool {
    let state = unsafe {
        desired
            .as_ptr()
            .add(GPU_DESIRED_STATE as i16)
            .as_u16_array()
    };
    let light = gpu_scene_vec_read(state, GPU_STATE_LIGHT);
    let row0 = gpu_scene_vec_read(desired, GPU_CAMERA_VIEW);
    let row1 = gpu_scene_vec_read(desired, GPU_CAMERA_VIEW + 8);
    let row2 = gpu_scene_vec_read(desired, GPU_CAMERA_VIEW + 16);
    let view_light = vec3::new(-fdot(row0, light), fdot(row1, light), fdot(row2, light));
    gpu_scene_zero(native, 32);
    if !gpu_scene_light_encode(view_light, native) {
        return false;
    }
    if !gpu_scene_light_response(state, material, native) {
        return false;
    }
    if !gpu_scene_render_prepare(state, material, view, textured, native) {
        return false;
    }
    native[0u16] = desired[GPU_CAMERA_FAR];
    native[1u16] = desired[GPU_CAMERA_FAR + 1];
    native[5u16] = desired[GPU_CAMERA_RAY];
    native[6u16] = desired[GPU_CAMERA_RAY + 1];
    native[7u16] = desired[GPU_CAMERA_RAY + 2];
    native[10u16] = 3 | (material[GPU_MATERIAL_SHININESS] << 2);
    let instance = GPU_SCRATCH_INSTANCE;
    let mut at: u16 = 0;
    while at < 64 {
        native[instance + at] = 0;
        at += 1;
    }
    true
}
// Writes only the42 coefficient words, without material/padding or ownership state.
pub fn gpu_scene_matrix_prepare(desired: Array<u16>, model: Array<u16>, mut out: Array<u16>) {
    let k = gpu_scene_fix_read(desired, GPU_CAMERA_FOCAL) * fix32::from_words(256, 0);
    let mut row: u16 = 0;
    while row < 3 {
        let basis = gpu_scene_vec_read(desired, GPU_CAMERA_VIEW + row * 8);
        let mut factor = -k;
        if row == 2 {
            factor = -fix32::from_int(1);
        }
        let mut col: u16 = 0;
        while col < 4 {
            let world = vec3::new(
                gpu_scene_fix_read(model, GPU_MODEL_WORLD + col * 2),
                gpu_scene_fix_read(model, GPU_MODEL_WORLD + 8 + col * 2),
                gpu_scene_fix_read(model, GPU_MODEL_WORLD + 16 + col * 2),
            );
            let mut value = fdot(basis, world);
            if col == 3 {
                value = value + gpu_scene_fix_read(desired, GPU_CAMERA_VIEW + row * 8 + 6);
            }
            gpu_scene_fix_write(out, row * 8 + col * 2, value * factor);
            col += 1;
        }
        col = 0;
        while col < 3 {
            let normal = vec3::new(
                gpu_scene_fix_read(model, GPU_MODEL_NORMAL + col * 2),
                gpu_scene_fix_read(model, GPU_MODEL_NORMAL + 6 + col * 2),
                gpu_scene_fix_read(model, GPU_MODEL_NORMAL + 12 + col * 2),
            );
            let mut value = fdot(basis, normal);
            if row == 0 {
                value = -value;
            }
            gpu_scene_fix_write(out, 24 + (row * 3 + col) * 2, value);
            col += 1;
        }
        row += 1;
    }
}
pub fn gpu_scene_draw_finish(material: Array<u16>, mut native: Array<u16>) {
    let instance = GPU_SCRATCH_INSTANCE;
    native[instance + 43] = 1;
    native[instance + 44] = material[GPU_MATERIAL_RGBA];
    native[instance + 45] = material[GPU_MATERIAL_RGBA + 1];
}
// The pure six-argument path retains no cache or object identity.
pub fn gpu_scene_draw_prepare(
    desired: Array<u16>,
    model: Array<u16>,
    material: Array<u16>,
    view: Array<u16>,
    textured: bool,
    native: Array<u16>,
) -> bool {
    if !gpu_scene_draw_header(desired, material, view, textured, native) {
        return false;
    }
    let instance = unsafe {
        native
            .as_ptr()
            .add(GPU_SCRATCH_INSTANCE as i16)
            .as_u16_array()
    };
    gpu_scene_matrix_prepare(desired, model, instance);
    gpu_scene_draw_finish(material, native);
    true
}
