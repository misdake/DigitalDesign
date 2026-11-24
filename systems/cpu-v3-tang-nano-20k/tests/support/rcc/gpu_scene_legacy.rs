fn gpu_legacy_render_values(
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
    let start: u16 = 32;
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

// Legacy precomposed numeric reference, appended only by the bounded numeric tests.
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
    if !gpu_legacy_render_values(state, material, view, textured, native) {
        return false;
    }
    let mut i: u16 = 5;
    while i < 16 {
        native[32 + i] = 0;
        i += 1;
    }
    true
}

pub fn gpu_scene_pass_header(desired: Array<u16>, mut native: Array<u16>) -> bool {
    let state = unsafe {
        desired
            .as_ptr()
            .add(GPU_DESIRED_STATE as i16)
            .as_u16_array()
    };
    if !gpu_scene_encode_view_light(desired, state, native) {
        return false;
    }
    gpu_scene_pass_fields(desired, native);
    true
}

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
    if !gpu_scene_pass_header(desired, native)
        || !gpu_scene_light_response(state, material, native)
        || !gpu_scene_render_prepare(state, material, view, textured, native)
    {
        return false;
    }
    native[10u16] = 3 | (material[GPU_MATERIAL_SHININESS] << 2);
    let mut i: u16 = 11;
    while i < 32 {
        native[i] = 0;
        i += 1;
    }
    i = 42;
    while i < 64 {
        native[GPU_SCRATCH_INSTANCE + i] = 0;
        i += 1;
    }
    true
}

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
    native[GPU_SCRATCH_INSTANCE + 43] = 1;
    native[GPU_SCRATCH_INSTANCE + 44] = material[GPU_MATERIAL_RGBA];
    native[GPU_SCRATCH_INSTANCE + 45] = material[GPU_MATERIAL_RGBA + 1];
}

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
