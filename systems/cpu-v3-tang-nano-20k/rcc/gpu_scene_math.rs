//! Numeric preparation for the explicit scene API; no allocation or commands.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;

#[inline]
pub fn gpu_scene_fix_read(data: Array<u16>, at: u16) -> fix32 {
    fix32::load_words(data, at)
}

#[inline]
pub fn gpu_scene_fix_write(data: Array<u16>, at: u16, value: fix32) {
    fix32::store_words(data, at, value);
}

pub fn gpu_scene_zero(data: Array<u16>, count: u16) {
    let zero = vec4::new(fix32::zero(), fix32::zero(), fix32::zero(), fix32::zero());
    let mut next = data.as_ptr();
    let mut left = count;
    while left >= 8 {
        vec4::export(zero, next);
        next = next.add(8);
        left -= 8;
    }
    while left != 0 {
        unsafe {
            next.write(0, 0);
        }
        next = next.add(1);
        left -= 1;
    }
}

#[inline]
pub fn gpu_scene_vec_read(data: Array<u16>, at: u16) -> vec3 {
    vec3::load_words(data, at)
}

#[inline]
pub fn gpu_scene_vec_write(data: Array<u16>, at: u16, value: vec3) {
    vec3::store_words(data, at, value);
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
// With one nonnegative operand at most one, the Q16.16 product cannot
// overflow. Reject the both-large case before multiplying, including products
// that could otherwise wrap back into the representable coefficient range.
#[cfg(gpu_debug)]
pub fn gpu_scene_response_product_valid(a: fix32, b: fix32) -> bool {
    let zero = fix32::zero();
    let one = fix32::from_int(1);
    a >= zero && b >= zero && !(a > one && b > one) && a * b <= one
}
#[cfg(gpu_debug)]
pub fn gpu_scene_responses_valid(state: Array<u16>, ambient: fix32, directional: fix32) -> bool {
    gpu_scene_response_product_valid(gpu_scene_fix_read(state, GPU_STATE_AMBIENT), ambient)
        && gpu_scene_response_product_valid(
            gpu_scene_fix_read(state, GPU_STATE_DIRECTIONAL),
            directional,
        )
}
fn gpu_scene_render_values(
    state: Array<u16>,
    material: Array<u16>,
    view: Array<u16>,
    textured: bool,
    mut native: Array<u16>,
) -> bool {
    let mut base: u16 = 0;
    let mut exponent: u16 = 0;
    let mut texture_flags: u16 = 0;
    let mut fixed: u16 = 0;
    let mut filter: u16 = 0;
    let mut min_mip: u16 = 0;
    if textured {
        base = view[GPU_VIEW_BASE];
        exponent = view[GPU_VIEW_EXPONENT];
        texture_flags = (view[GPU_VIEW_FLAGS] & 1) | ((view[GPU_VIEW_FLAGS] >> 1) & 6);
        fixed = (view[GPU_VIEW_FLAGS] >> 1) & 1;
        filter = state[GPU_STATE_FILTER] & 255;
        min_mip = (view[GPU_VIEW_FLAGS] >> 4) & 3;
    }
    if fixed != 0 && filter > 1 {
        return false;
    }
    let start = GPU_SCRATCH_RENDER;
    native[start] = base;
    native[start + 1] = exponent
        | ((texture_flags & 1) << 4)
        | ((texture_flags & 6) << 7)
        | (filter << 5)
        | (fixed << 7)
        | (min_mip << 10);
    native[start + 2] = material[GPU_MATERIAL_SPECULAR];
    native[start + 3] = material[GPU_MATERIAL_SPECULAR + 1];
    native[start + 4] = state[GPU_STATE_CONTROL] | ((material[GPU_MATERIAL_FLAGS] & 1) << 5);
    if textured {
        // Material24 packs bias beside raster control, unlike NativeRender32.
        native[start + 4] |= state[GPU_STATE_FILTER] & 65280;
    }
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

pub fn gpu_scene_encode_view_light(
    camera: Array<u16>,
    state: Array<u16>,
    native: Array<u16>,
) -> bool {
    let light = gpu_scene_vec_read(state, GPU_STATE_LIGHT);
    let camera = camera.as_ptr().add(GPU_CAMERA_VIEW as i16);
    let row0 = vec4::import(camera);
    let x = -fdot(vec3::new(row0.x(), row0.y(), row0.z()), light);
    let row1 = vec4::import(camera.add(8));
    let y = fdot(vec3::new(row1.x(), row1.y(), row1.z()), light);
    let row2 = vec4::import(camera.add(16));
    let z = fdot(vec3::new(row2.x(), row2.y(), row2.z()), light);
    let view_light = vec3::new(x, y, z);
    gpu_scene_light_encode(view_light, native)
}

// Material owns texture/raster/color and the uncombined response. No Camera or
// light direction is retained in a prepared part.
pub fn gpu_scene_material_prepare(
    state: Array<u16>,
    material: Array<u16>,
    view: Array<u16>,
    textured: bool,
    mut native: Array<u16>,
) -> bool {
    if !gpu_scene_render_values(state, material, view, textured, native) {
        return false;
    }
    let render = GPU_SCRATCH_RENDER;
    native[render + 5] = 0;
    native[render + 6] = 0;
    native[render + 7] = 0;
    native[render + 8] = material[GPU_MATERIAL_RGBA];
    native[render + 9] = material[GPU_MATERIAL_RGBA + 1];
    native[render + 10] = 3 | (material[GPU_MATERIAL_SHININESS] << 2);
    native[render + 11] = 0;
    let instance = GPU_SCRATCH_INSTANCE;
    // Every other word has exactly one producer: World, normal, response or
    // Material. Only these four reserved words need explicit initialization.
    native[instance + 30] = 0;
    native[instance + 31] = 0;
    native[instance + 50] = 0;
    native[instance + 51] = 0;
    native[instance + 26] = material[GPU_MATERIAL_AMBIENT];
    native[instance + 27] = material[GPU_MATERIAL_AMBIENT + 1];
    native[instance + 28] = material[GPU_MATERIAL_DIRECTIONAL];
    native[instance + 29] = material[GPU_MATERIAL_DIRECTIONAL + 1];
    true
}

pub fn gpu_scene_pass_fields(desired: Array<u16>, mut native: Array<u16>) {
    let state = unsafe {
        desired
            .as_ptr()
            .add(GPU_DESIRED_STATE as i16)
            .as_u16_array()
    };
    native[0u16] = desired[GPU_CAMERA_FAR];
    native[1u16] = desired[GPU_CAMERA_FAR + 1];
    native[5u16] = desired[GPU_CAMERA_RAY];
    native[6u16] = desired[GPU_CAMERA_RAY + 1];
    native[7u16] = desired[GPU_CAMERA_RAY + 2];
    let mut i: u16 = 0;
    while i < 4 {
        native[8 + i] = state[GPU_STATE_AMBIENT + i];
        i += 1;
    }
}
// View owns camera projection conventions once; World remains camera-independent.
pub fn gpu_scene_view_prepare(desired: Array<u16>, native: Array<u16>) {
    let camera = desired.as_ptr();
    let output = native.as_ptr().add(GPU_SCRATCH_VIEW as i16);
    let a = vec4::import(camera);
    let b = vec4::import(camera.add(8));
    let c = vec4::import(camera.add(16));
    let k = -(gpu_scene_fix_read(desired, GPU_CAMERA_FOCAL) * fix32::from_words(256, 0));
    vec4::export(
        vec4::new(a.x() * k, a.y() * k, a.z() * k, a.w() * k),
        output,
    );
    vec4::export(
        vec4::new(b.x() * k, b.y() * k, b.z() * k, b.w() * k),
        output.add(8),
    );
    vec4::export(vec4::new(-c.x(), -c.y(), -c.z(), -c.w()), output.add(16));
    vec4::export(vec4::new(-a.x(), -a.y(), -a.z(), b.x()), output.add(24));
    vec4::export(vec4::new(b.y(), b.z(), c.x(), c.y()), output.add(32));
    let mut view = unsafe { output.as_u16_array() };
    gpu_scene_fix_write(view, 40, c.z());
    view[42u16] = 0;
    view[43u16] = 1;
    let mut i: u16 = 44;
    while i < 64 {
        view[i] = 0;
        i += 1;
    }
    // Billboard basis belongs to View, shared by every model and static part.
    // Keep the established horizontal-right rule without renormalizing XZ.
    let horizontal = vec3::new(a.x(), fix32::zero(), a.z());
    gpu_scene_fix_write(
        view,
        44,
        fdot(horizontal, vec3::new(a.x(), a.y(), a.z())) * k,
    );
    gpu_scene_fix_write(
        view,
        46,
        fdot(horizontal, vec3::new(b.x(), b.y(), b.z())) * k,
    );
    gpu_scene_fix_write(view, 60, -fdot(horizontal, vec3::new(c.x(), c.y(), c.z())));
    gpu_scene_fix_write(view, 62, k);
    let mut native = native;
    native[12u16] = view[60u16];
    native[13u16] = view[61u16];
    native[14u16] = view[62u16];
    native[15u16] = view[63u16];
    i = 0;
    while i < 12 {
        view[48 + i] = native[i];
        i += 1;
    }
}
