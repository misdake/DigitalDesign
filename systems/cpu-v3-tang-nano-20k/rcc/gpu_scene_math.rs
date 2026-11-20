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

pub fn gpu_scene_zero(mut data: Array<u16>, count: u16) {
    let mut at: u16 = 0;
    while at < count {
        data[at] = 0;
        at += 1;
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
    let mut texture_flags: u16 = 0;
    let mut fixed: u16 = 0;
    let mut filter: u16 = 0;
    if textured {
        base = view[GPU_VIEW_BASE];
        exponent = view[GPU_VIEW_EXPONENT];
        texture_flags = (view[GPU_VIEW_FLAGS] & 1) | ((view[GPU_VIEW_FLAGS] >> 1) & 6);
        fixed = (view[GPU_VIEW_FLAGS] >> 1) & 1;
        filter = state[GPU_STATE_FILTER];
    }
    if fixed != 0 && filter > 1 {
        return false;
    }
    let start = GPU_SCRATCH_RENDER;
    let mut at: u16 = 5;
    while at < 16 {
        native[start + at] = 0;
        at += 1;
    }
    native[start] = base;
    native[start + 1] = exponent
        | ((texture_flags & 1) << 4)
        | ((texture_flags & 6) << 7)
        | (filter << 5)
        | (fixed << 7);
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

// Build final native fields; the coefficient producer owns Instance0..41.
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
    let mut padding: u16 = 11;
    while padding < 32 {
        native[padding] = 0;
        padding += 1;
    }
    if !gpu_scene_encode_view_light(desired, state, native) {
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
    let mut at: u16 = 42;
    while at < 64 {
        native[instance + at] = 0;
        at += 1;
    }
    true
}
// Writes exactly 42 coefficient words. Inputs and output must be disjoint.
// Transpose packed rows in F registers. Finish normals before loading world rows
// and form one camera basis at a time to bound live vectors and allocator spills.
pub fn gpu_scene_matrix_prepare(desired: Array<u16>, model: Array<u16>, out: Array<u16>) {
    let camera = desired.as_ptr();
    let source = model.as_ptr();
    let target = out.as_ptr();
    let a = vec4::import(camera);
    let b = vec4::import(camera.add(8));
    let c = vec4::import(camera.add(16));
    // The final overlapping load ends at word 41, never at cache word 42.
    let nx = vec4::import(source.add(24));
    let ny = vec4::import(source.add(30));
    let nz = vec4::import(source.add(34));
    let n0 = vec3::new(nx.x(), ny.x(), nz.y());
    let n1 = vec3::new(nx.y(), ny.y(), nz.z());
    let n2 = vec3::new(nx.z(), ny.z(), nz.w());
    let r0 = vec3::new(a.x(), a.y(), a.z());
    let n00 = -fdot(r0, n0);
    let n01 = -fdot(r0, n1);
    let n02 = -fdot(r0, n2);
    let r1 = vec3::new(b.x(), b.y(), b.z());
    let n10 = fdot(r1, n0);
    vec4::export(vec4::new(n00, n01, n02, n10), target.add(24));
    let n11 = fdot(r1, n1);
    let n12 = fdot(r1, n2);
    let r2 = vec3::new(c.x(), c.y(), c.z());
    let n20 = fdot(r2, n0);
    let n21 = fdot(r2, n1);
    vec4::export(vec4::new(n11, n12, n20, n21), target.add(32));
    gpu_scene_fix_write(out, 40, fdot(r2, n2));
    let k = -(gpu_scene_fix_read(desired, GPU_CAMERA_FOCAL) * fix32::from_words(256, 0));
    let x = vec4::import(source);
    let y = vec4::import(source.add(8));
    let z = vec4::import(source.add(16));
    let w0 = vec3::new(x.x(), y.x(), z.x());
    let w1 = vec3::new(x.y(), y.y(), z.y());
    let w2 = vec3::new(x.z(), y.z(), z.z());
    let w3 = vec3::new(x.w(), y.w(), z.w());
    let p0 = vec3::new(a.x(), a.y(), a.z());
    vec4::export(
        vec4::new(
            fdot(p0, w0) * k,
            fdot(p0, w1) * k,
            fdot(p0, w2) * k,
            (fdot(p0, w3) + a.w()) * k,
        ),
        target.add(0),
    );
    let p1 = vec3::new(b.x(), b.y(), b.z());
    vec4::export(
        vec4::new(
            fdot(p1, w0) * k,
            fdot(p1, w1) * k,
            fdot(p1, w2) * k,
            (fdot(p1, w3) + b.w()) * k,
        ),
        target.add(8),
    );
    let p2 = vec3::new(c.x(), c.y(), c.z());
    vec4::export(
        vec4::new(
            fdot(p2, w0) * -fix32::from_int(1),
            fdot(p2, w1) * -fix32::from_int(1),
            fdot(p2, w2) * -fix32::from_int(1),
            (fdot(p2, w3) + c.w()) * -fix32::from_int(1),
        ),
        target.add(16),
    );
}
pub fn gpu_scene_draw_finish(material: Array<u16>, mut native: Array<u16>) {
    let instance = GPU_SCRATCH_INSTANCE;
    native[instance + 43] = 1;
    native[instance + 44] = material[GPU_MATERIAL_RGBA];
    native[instance + 45] = material[GPU_MATERIAL_RGBA + 1];
}

pub fn gpu_scene_billboard_directions(desired: Array<u16>, native: Array<u16>, vertical: bool) {
    let k = -(gpu_scene_fix_read(desired, GPU_CAMERA_FOCAL) * fix32::from_words(256, 0));
    let directions = GPU_SCRATCH_INSTANCE + 48;
    if vertical {
        let camera = desired.as_ptr();
        let right = vec4::import(camera);
        let up = vec4::import(camera.add(8));
        let back = vec4::import(camera.add(16));
        let horizontal = vec3::new(right.x(), fix32::zero(), right.z());
        gpu_scene_fix_write(
            native,
            directions,
            fdot(horizontal, vec3::new(right.x(), right.y(), right.z())) * k,
        );
        gpu_scene_fix_write(
            native,
            directions + 4,
            fdot(horizontal, vec3::new(up.x(), up.y(), up.z())) * k,
        );
        gpu_scene_fix_write(
            native,
            directions + 8,
            -fdot(horizontal, vec3::new(back.x(), back.y(), back.z())),
        );
    } else {
        // Camera right/up become the X/Y basis before projection.
        gpu_scene_fix_write(native, directions, k);
        gpu_scene_fix_write(native, directions + 6, k);
    }
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
