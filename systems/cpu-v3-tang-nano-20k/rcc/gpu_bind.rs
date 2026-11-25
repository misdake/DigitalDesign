//! Canonical CPU snapshots and native binding; no scene math or object handles.
use crate::dsl_rt::*;
mod gpu_packet;
use gpu_packet::*;
mod gpu_scene_math;
use gpu_scene_math::*;

// Private CPU-owned canonical spans. Forward stores retain overlap order.
fn gpu_bind_copy(c: Array<u16>, dst: u16, data: Array<u16>, src: u16, words: u16) {
    let mut to = c.as_ptr().add(dst as i16);
    let mut from = data.as_ptr().add(src as i16);
    let mut left = words;
    // A forward-overlapping destination observes the earlier scalar writes.
    // Other CPU-owned spans can copy eight raw words at a time.
    if to.addr() <= from.addr() || to.addr() - from.addr() >= words {
        while left >= 8 {
            vec4::export(vec4::import(from), to);
            to = to.add(8);
            from = from.add(8);
            left -= 8;
        }
    }
    while left != 0 {
        unsafe {
            to.write(0, from.read(0));
        }
        to = to.add(1);
        from = from.add(1);
        left -= 1;
    }
}
fn gpu_bind_world_prepare(model: Array<u16>, mut native: Array<u16>) {
    // Assemble two 64-byte sections from tightly packed source matrices.
    // Transform padding never becomes a GPU coefficient or material value.
    gpu_bind_copy(native, GPU_SCRATCH_INSTANCE, model, 0, 24);
    gpu_bind_copy(native, GPU_SCRATCH_INSTANCE + 32, model, 24, 18);
    native[GPU_SCRATCH_INSTANCE + 24] = 0;
    native[GPU_SCRATCH_INSTANCE + 25] = 1;
}

fn gpu_bind_equal(c: Array<u16>, dst: u16, data: Array<u16>, src: u16, words: u16) -> bool {
    let mut a = c.as_ptr().add(dst as i16);
    let mut b = data.as_ptr().add(src as i16);
    let mut left = words;
    while left >= 8 {
        let equal = unsafe {
            a.read(0) == b.read(0)
                && a.read(1) == b.read(1)
                && a.read(2) == b.read(2)
                && a.read(3) == b.read(3)
                && a.read(4) == b.read(4)
                && a.read(5) == b.read(5)
                && a.read(6) == b.read(6)
                && a.read(7) == b.read(7)
        };
        if !equal {
            return false;
        }
        a = a.add(8);
        b = b.add(8);
        left -= 8;
    }
    while left != 0 {
        let equal = unsafe { a.read(0) == b.read(0) };
        if !equal {
            return false;
        }
        a = a.add(1);
        b = b.add(1);
        left -= 1;
    }
    true
}
fn gpu_bind_desired(slot: u16, camera: bool, data: Array<u16>) -> bool {
    if slot >= GPU_RECORD_COUNT {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16 {
        return false;
    }
    if camera {
        if c[GPU_RECORD_VALID] & 8 == 0
            || !gpu_bind_equal(c, GPU_CB_CAMERA_REVISION, data, GPU_CAMERA_REVISION, 4)
        {
            gpu_bind_copy(c, GPU_CB_CAMERA, data, 0, 32);
            gpu_bind_copy(c, GPU_CB_CAMERA_REVISION, data, GPU_CAMERA_REVISION, 4);
            c[GPU_RECORD_VALID] =
                (c[GPU_RECORD_VALID] | GPU_BIND_SCENE_DIRTY | GPU_BIND_VIEW_DIRTY)
                    & (65535 ^ GPU_BIND_LIGHT_VALID);
        } else if !gpu_bind_equal(c, GPU_CB_CAMERA + GPU_CAMERA_FAR, data, GPU_CAMERA_FAR, 6) {
            // Far/rays can change without changing the View-matrix revision.
            gpu_bind_copy(c, GPU_CB_CAMERA + GPU_CAMERA_FAR, data, GPU_CAMERA_FAR, 6);
            c[GPU_RECORD_VALID] |= GPU_BIND_SCENE_DIRTY;
        }
        c[GPU_RECORD_VALID] |= 8;
    } else {
        if c[GPU_RECORD_VALID] & 16 == 0
            || !gpu_bind_equal(c, GPU_CB_STATE, data, 0, GPU_STATE_CONTROL)
        {
            // The snapshot itself is the dependency key; only changing its
            // light direction invalidates the already encoded direction.
            if c[GPU_RECORD_VALID] & 16 == 0 || !gpu_bind_equal(c, GPU_CB_STATE, data, 0, 6) {
                c[GPU_RECORD_VALID] &= 65535 ^ GPU_BIND_LIGHT_VALID;
            }
            c[GPU_RECORD_VALID] |= GPU_BIND_SCENE_DIRTY;
        }
        gpu_bind_copy(c, GPU_CB_STATE, data, 0, 16);
        c[GPU_RECORD_VALID] |= 16;
    }
    true
}
fn gpu_bind_view_changed(slot: u16) -> bool {
    let c = gpu_record_data(slot);
    c[GPU_CB_VIEW_WORD] == 0 || c[GPU_RECORD_VALID] & GPU_BIND_VIEW_DIRTY != 0
}
// Direction quantization belongs to the view, not to each model or material.
fn gpu_bind_prepare_scene(slot: u16, native: Array<u16>) -> bool {
    let c = gpu_record_data(slot);
    let desired = unsafe { c.as_ptr().add(GPU_CB_CAMERA as i16).as_u16_array() };
    if c[GPU_RECORD_VALID] & GPU_BIND_LIGHT_VALID != 0 {
        gpu_bind_copy(native, 2, c, GPU_CB_GLOBAL + 2, 3);
    } else {
        let state = unsafe { c.as_ptr().add(GPU_CB_STATE as i16).as_u16_array() };
        if !gpu_scene_encode_view_light(desired, state, native) {
            return false;
        }
    }
    gpu_bind_copy(native, 12, c, GPU_CB_GLOBAL + 12, 4);
    gpu_scene_pass_fields(desired, native);
    true
}
fn gpu_bind_scene_current(slot: u16) -> bool {
    let valid = gpu_record_data(slot)[GPU_RECORD_VALID];
    valid & (1 | GPU_BIND_SCENE_DIRTY) == 1
}
// Seven caller-local words: changes, borrowed Transform pointer, View/Scene offset,
// World offset, push end, command words and DRAW mode (zero for scene-only).
// Full View and its Scene tail are one mutually exclusive upload decision.
const GPU_BIND_PLAN_VIEW: u16 = 16;
fn gpu_bind_plan_fields(
    slot: u16,
    native: Array<u16>,
    model: Array<u16>,
    mut plan: Array<u16>,
    scene_only: bool,
) -> bool {
    if slot >= GPU_RECORD_COUNT {
        return false;
    }
    let c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16 {
        return false;
    }
    let part = c[GPU_GROUP_RECORDING] != 0;
    let valid = c[GPU_RECORD_VALID];
    let world = native[GPU_SCRATCH_WORLD_MODE];
    #[cfg(gpu_debug)]
    {
        if !scene_only && world != 1 && world != 3 {
            return false;
        }
    }
    let mut changes: u16 = 0;
    if !part && (valid & 1 == 0 || valid & GPU_BIND_SCENE_DIRTY != 0) {
        changes |= 1;
    }
    if !scene_only {
        let material_changed =
            valid & 2 == 0 || !gpu_bind_equal(c, GPU_CB_RENDER, native, GPU_SCRATCH_RENDER, 12);
        // DRAW reads uniform color directly. Only CP-consumed fields require a
        // render drain and SET_RENDER: texture/specular/raster and lighting kind.
        if material_changed
            && (valid & 2 == 0
                || !gpu_bind_equal(c, GPU_CB_RENDER, native, GPU_SCRATCH_RENDER, 8)
                || !gpu_bind_equal(c, GPU_CB_RENDER + 10, native, GPU_SCRATCH_RENDER + 10, 2))
        {
            changes |= 2;
        }
        let response_changed = valid & 4 == 0
            || !gpu_bind_equal(c, GPU_CB_RESPONSE, native, GPU_SCRATCH_INSTANCE + 26, 4);
        // Geometry writes live CP response registers. A Billboard skips that
        // update; an ordinary draw must drain before installing unknown/new data.
        if native[GPU_SCRATCH_BILLBOARD] == 0
            && (valid & GPU_BIND_RESPONSE_VALID == 0 || response_changed)
        {
            changes |= 8;
        }
        if material_changed
            || response_changed
            || !gpu_bind_equal(c, GPU_CB_MODEL_REVISION, model, GPU_MODEL_REVISION, 4)
        {
            changes |= 4;
        }
    }
    plan[GPU_BIND_PLAN_MODEL] = 0;
    plan[GPU_BIND_PLAN_VIEW_OFFSET] = 0;
    plan[GPU_BIND_PLAN_WORLD_OFFSET] = 0;
    plan[GPU_BIND_PLAN_DRAW_MODE] = 0;
    if !scene_only {
        plan[GPU_BIND_PLAN_MODEL] = model.as_ptr().addr();
        plan[GPU_BIND_PLAN_DRAW_MODE] = world;
    }
    let mut end = c[GPU_RECORD_PUSH_USED];
    let capacity = c[GPU_RECORD_PUSH_CAPACITY];
    let mut commands: u16 = 0;
    // Shading configuration changes retain the whole-pipeline drain boundary.
    if changes & 11 != 0 {
        commands += 2;
    }
    if !part && gpu_bind_view_changed(slot) {
        changes |= GPU_BIND_PLAN_VIEW;
    }
    if changes & (GPU_BIND_PLAN_VIEW | 1) != 0 {
        let mut words: u16 = 16;
        if changes & GPU_BIND_PLAN_VIEW != 0 {
            words = 64;
        }
        let at = gpu_record_align_push(end, words, words, capacity);
        if at == 65535 {
            return false;
        }
        plan[GPU_BIND_PLAN_VIEW_OFFSET] = at;
        end = at + words;
        commands += 2;
    }
    if changes & 4 != 0 {
        let at = gpu_record_align_push(end, 64, 64, capacity);
        if at == 65535 {
            return false;
        }
        plan[GPU_BIND_PLAN_WORLD_OFFSET] = at;
        end = at + 64;
        commands += 2;
    } else if !scene_only {
        if c[GPU_RECORD_INSTANCE_WORD] == 0 {
            return false;
        }
        plan[GPU_BIND_PLAN_WORLD_OFFSET] = c[GPU_RECORD_INSTANCE_WORD] - 1;
    }
    if changes & (GPU_BIND_PLAN_VIEW | 5) != 0 {
        commands += 2;
    }
    if changes & 1 != 0 {
        commands += 2;
    }
    if changes & 2 != 0 {
        commands += 2;
    }
    plan[GPU_BIND_PLAN_CHANGES] = changes;
    plan[GPU_BIND_PLAN_PUSH_END] = end;
    plan[GPU_BIND_PLAN_CODE_WORDS] = commands;
    true
}
fn gpu_bind_plan(slot: u16, native: Array<u16>, model: Array<u16>, plan: Array<u16>) -> bool {
    gpu_bind_plan_fields(slot, native, model, plan, false)
}
// One encoder serves ordinary draws, prepared recording and scene-only replay.
fn gpu_bind_code(slot: u16, plan: Array<u16>, mut code: Array<u16>) -> u16 {
    let c = gpu_record_data(slot);
    let changes = plan[GPU_BIND_PLAN_CHANGES];
    let mut at: u16 = 0;
    if changes & 11 != 0 {
        code[at] = 3;
        code[at + 1] = 0x4000;
        at += 2;
    }
    if changes & (GPU_BIND_PLAN_VIEW | 1) != 0 {
        let mut local: u16 = 95;
        let mut size: u16 = 0;
        if changes & GPU_BIND_PLAN_VIEW != 0 {
            local = 92;
            size = 2;
        }
        gpu_packet_dma(code, at, c, plan[GPU_BIND_PLAN_VIEW_OFFSET], local, size);
        at += 2;
    }
    if changes & 4 != 0 {
        gpu_packet_dma(
            code,
            at,
            c,
            plan[GPU_BIND_PLAN_WORLD_OFFSET],
            GPU_PACKET_WORLD_LOCAL,
            GPU_PACKET_MATRIX_SIZE,
        );
        at += 2;
    }
    if changes & (GPU_BIND_PLAN_VIEW | 5) != 0 {
        code[at] = 1;
        code[at + 1] = 0x4000;
        at += 2;
    }
    if changes & 1 != 0 {
        code[at] = 128 | 95;
        code[at + 1] = 0x5000;
        at += 2;
    }
    if changes & 2 != 0 {
        code[at] = 128 | 83;
        code[at + 1] = 0x6000;
        at += 2;
    }
    at
}
fn gpu_bind_payload(slot: u16, native: Array<u16>, plan: Array<u16>) -> bool {
    let mut c = gpu_record_data(slot);
    let changes = plan[GPU_BIND_PLAN_CHANGES];
    if changes & (GPU_BIND_PLAN_VIEW | 1) != 0 {
        let mut source = native;
        let mut words: u16 = 16;
        if changes & GPU_BIND_PLAN_VIEW != 0 {
            source = unsafe { native.as_ptr().add(GPU_SCRATCH_VIEW as i16).as_u16_array() };
            words = 64;
        }
        if !gpu_window_write_words(
            c[GPU_RECORD_PUSH_BASE],
            plan[GPU_BIND_PLAN_VIEW_OFFSET],
            source,
            words,
        ) {
            c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
            return false;
        }
    }
    if changes & 4 != 0 {
        let model = unsafe { Ptr::from_addr(plan[GPU_BIND_PLAN_MODEL]).as_u16_array() };
        gpu_bind_world_prepare(model, native);
        let instance = unsafe {
            native
                .as_ptr()
                .add(GPU_SCRATCH_INSTANCE as i16)
                .as_u16_array()
        };
        if !gpu_window_write_words(
            c[GPU_RECORD_PUSH_BASE],
            plan[GPU_BIND_PLAN_WORLD_OFFSET],
            instance,
            64,
        ) {
            c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
            return false;
        }
    }
    true
}
fn gpu_bind_commit(slot: u16, native: Array<u16>, plan: Array<u16>) {
    let mut c = gpu_record_data(slot);
    let changes = plan[GPU_BIND_PLAN_CHANGES];
    if changes & 1 != 0 {
        gpu_bind_copy(c, GPU_CB_GLOBAL, native, 0, 16);
        c[GPU_RECORD_GLOBAL_WORD] = if changes & GPU_BIND_PLAN_VIEW != 0 {
            plan[GPU_BIND_PLAN_VIEW_OFFSET] + 49
        } else {
            plan[GPU_BIND_PLAN_VIEW_OFFSET] + 1
        };
        c[GPU_RECORD_VALID] |= GPU_BIND_LIGHT_VALID;
        c[GPU_RECORD_GLOBAL_BLOCKS] += 1;
        c[GPU_RECORD_VALID] &= 65535 ^ GPU_BIND_SCENE_DIRTY;
    }
    if changes & 4 != 0 {
        gpu_bind_copy(c, GPU_CB_RENDER, native, GPU_SCRATCH_RENDER, 12);
        gpu_bind_copy(c, GPU_CB_RESPONSE, native, GPU_SCRATCH_INSTANCE + 26, 4);
        c[GPU_RECORD_INSTANCE_WORD] = plan[GPU_BIND_PLAN_WORLD_OFFSET] + 1;
        c[GPU_RECORD_INSTANCE_BLOCKS] += 1;
        let model = unsafe { Ptr::from_addr(plan[GPU_BIND_PLAN_MODEL]).as_u16_array() };
        gpu_bind_copy(c, GPU_CB_MODEL_REVISION, model, GPU_MODEL_REVISION, 4);
    }
    c[GPU_RECORD_VALID] |= changes & 7;
    if plan[GPU_BIND_PLAN_DRAW_MODE] != 0 && changes & 13 != 0 {
        if native[GPU_SCRATCH_BILLBOARD] == 0 {
            c[GPU_RECORD_VALID] |= GPU_BIND_RESPONSE_VALID;
        } else {
            c[GPU_RECORD_VALID] &= 65535 ^ GPU_BIND_RESPONSE_VALID;
        }
    }
    if changes & GPU_BIND_PLAN_VIEW != 0 {
        gpu_bind_copy(c, GPU_CB_GLOBAL + 12, native, 12, 4);
        c[GPU_CB_VIEW_WORD] = plan[GPU_BIND_PLAN_VIEW_OFFSET] + 1;
        c[GPU_CB_VIEW_BLOCKS] += 1;
        c[GPU_RECORD_VALID] &= 65535 ^ GPU_BIND_VIEW_DIRTY;
    }
}
fn gpu_bind_emit(slot: u16, native: Array<u16>, plan: Array<u16>) -> bool {
    let code = unsafe {
        native
            .as_ptr()
            .add(GPU_SCRATCH_COMMAND as i16)
            .as_u16_array()
    };
    if !gpu_bind_payload(slot, native, plan) {
        return false;
    }
    let words = gpu_bind_code(slot, plan, code);
    if !gpu_record_append_reserved(slot, code, words, code, 0, plan[GPU_BIND_PLAN_PUSH_END]) {
        return false;
    }
    gpu_bind_commit(slot, native, plan);
    true
}
fn gpu_bind_install(slot: u16, native: Array<u16>, model: Array<u16>) -> bool {
    let plan: Buf<u16, 7> = Buf::new([0; 7]);
    gpu_bind_plan(slot, native, model, plan.as_array())
        && gpu_record_preflight(
            slot,
            plan[GPU_BIND_PLAN_CODE_WORDS],
            plan[GPU_BIND_PLAN_PUSH_END],
        )
        && gpu_bind_emit(slot, native, plan.as_array())
}
