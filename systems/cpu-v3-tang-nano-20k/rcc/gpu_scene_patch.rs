//! CPU-owned scene/group and target-only bindings. No START or publication.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_record;
use gpu_record::*;
mod gpu_execute;
use gpu_execute::*;
mod gpu_packet;
use gpu_packet::*;
mod gpu_bind;
use gpu_bind::*;
mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_model_cache;
use gpu_model_cache::*;
mod gpu_arena;
use gpu_arena::*;
mod gpu_memcpy;
use gpu_memcpy::*;
mod gpu_present;
use gpu_present::*;

// Identity matters even when two unrelated draws deduplicate to equal bytes.
fn gpu_patch_note(slot: u16, field: u16, identity: u16) {
    let mut c = gpu_record_data(slot);
    if c[field] == 0 {
        c[field] = identity;
    } else if c[field] != identity {
        c[field] = 65535;
    }
}
fn gpu_patch_drop(field: u16, identity: u16) {
    let mut slot: u16 = 0;
    while slot < 2 {
        let mut c = gpu_record_data(slot);
        // Once sealed, the group is fixed and retains no source descriptor.
        // Do not mutate metadata of a prepared, running or leased CB on Drop.
        if (c[GPU_RECORD_PHASE] == GPU_RECORD_READY || c[GPU_RECORD_PHASE] == GPU_RECORD_PASS)
            && c[field] == identity
        {
            c[field] = 65535;
        }
        slot += 1;
    }
}
fn gpu_patch_owned(slot: u16) -> bool {
    if slot >= 2 || GPU_EXECUTION[1u16] == slot + 1 {
        return false;
    }
    let c = gpu_record_data(slot);
    c[GPU_RECORD_PHASE] == GPU_RECORD_READY || c[GPU_RECORD_PHASE] == GPU_RECORD_SEALED
}
fn gpu_patch_span(c: Array<u16>, at: u16, words: u16) -> bool {
    at != 0
        && at - 1 <= c[GPU_RECORD_PUSH_USED]
        && words <= c[GPU_RECORD_PUSH_USED] - (at - 1)
        && c[GPU_RECORD_PUSH_USED] <= c[GPU_RECORD_PUSH_CAPACITY]
}
fn gpu_patch_group(c: Array<u16>) -> bool {
    c[GPU_RECORD_VALID] & GPU_RECORD_HAS_COPY == 0
        && c[GPU_RECORD_TARGET_BLOCKS] == 1
        && c[GPU_RECORD_GLOBAL_BLOCKS] == 1
        && c[GPU_RECORD_INSTANCE_BLOCKS] == 1
        && c[GPU_RECORD_UI_WORD] == 0
        && (c[GPU_RECORD_GENERATION_LOW] | c[GPU_RECORD_GENERATION_HIGH]) != 0
        && c[GPU_RECORD_SCENE_MODEL] != 0
        && c[GPU_RECORD_SCENE_MODEL] != 65535
        && c[GPU_RECORD_SCENE_MATERIAL] != 0
        && c[GPU_RECORD_SCENE_MATERIAL] != 65535
        && c[GPU_RECORD_SCENE_CAMERA] != 0
        && c[GPU_RECORD_SCENE_CAMERA] != 65535
        && gpu_patch_span(c, c[GPU_RECORD_TARGET_WORD], 16)
        && gpu_patch_span(c, c[GPU_RECORD_GLOBAL_WORD], 32)
        && gpu_patch_span(c, c[GPU_RECORD_INSTANCE_WORD], 64)
}
fn gpu_patch_binding(slot: u16) -> bool {
    if !gpu_patch_owned(slot) {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if !gpu_patch_group(c) || c[GPU_RECORD_FACT_COUNT] > GPU_RECORD_FACT_LIMIT {
        return false;
    }
    let facts = gpu_record_capture_facts(slot);
    let mut color: u16 = 0;
    let mut depth: u16 = 0;
    let mut i: u16 = 0;
    while i < c[GPU_RECORD_FACT_COUNT] {
        let at = i * 4;
        if facts[at + 2] == GPU_FACT_COLOR {
            if color != 0 || facts[at] != c[GPU_RECORD_COLOR] || facts[at + 3] != 3 {
                return false;
            }
            color = i + 1;
        }
        if facts[at + 2] == GPU_FACT_DEPTH {
            if depth != 0 || facts[at] != c[GPU_RECORD_DEPTH] || facts[at + 3] != 3 {
                return false;
            }
            depth = i + 1;
        }
        i += 1;
    }
    if color == 0 || depth == 0 || !gpu_record_seal(slot) {
        return false;
    }
    c[GPU_RECORD_COLOR_FACT] = color;
    c[GPU_RECORD_DEPTH_FACT] = depth;
    true
}
fn gpu_patch_matches(slot: u16, low: u16, high: u16) -> bool {
    if !gpu_patch_owned(slot) {
        return false;
    }
    let c = gpu_record_data(slot);
    gpu_patch_group(c)
        && c[GPU_RECORD_GENERATION_LOW] == low
        && c[GPU_RECORD_GENERATION_HIGH] == high
}
fn gpu_patch_targets(slot: u16, color: Array<u16>, depth: Array<u16>) -> bool {
    let c = gpu_record_data(slot);
    let facts = gpu_record_capture_facts(slot);
    let ci = c[GPU_RECORD_COLOR_FACT];
    let di = c[GPU_RECORD_DEPTH_FACT];
    if c[GPU_RECORD_FACT_COUNT] > GPU_RECORD_FACT_LIMIT
        || ci == 0
        || di == 0
        || ci > c[GPU_RECORD_FACT_COUNT]
        || di > c[GPU_RECORD_FACT_COUNT]
        || facts[(ci - 1) * 4 + 2] != GPU_FACT_COLOR
        || facts[(di - 1) * 4 + 2] != GPU_FACT_DEPTH
        || facts[(ci - 1) * 4] != c[GPU_RECORD_COLOR]
        || facts[(di - 1) * 4] != c[GPU_RECORD_DEPTH]
        || facts[(ci - 1) * 4 + 1] != color[1u16]
        || facts[(di - 1) * 4 + 1] != depth[1u16]
        || facts[(ci - 1) * 4 + 3] != 3
        || facts[(di - 1) * 4 + 3] != 3
        || gpu_record_overlap(color[0u16], color[1u16], depth[0u16], depth[1u16])
    {
        return false;
    }
    let mut i: u16 = 0;
    while i < GPU_EXECUTION[GPU_EXEC_CONFIGURED] {
        let other = gpu_record_data(i);
        if !gpu_packet_target_valid(other, color[0u16], depth[0u16])
            || gpu_record_overlap(
                color[0u16],
                color[1u16],
                other[GPU_RECORD_COMMAND_BASE],
                other[GPU_RECORD_COMMAND_CAPACITY] >> 6,
            )
            || gpu_record_overlap(
                color[0u16],
                color[1u16],
                other[GPU_RECORD_PUSH_BASE],
                other[GPU_RECORD_PUSH_CAPACITY] >> 6,
            )
            || gpu_record_overlap(
                depth[0u16],
                depth[1u16],
                other[GPU_RECORD_COMMAND_BASE],
                other[GPU_RECORD_COMMAND_CAPACITY] >> 6,
            )
            || gpu_record_overlap(
                depth[0u16],
                depth[1u16],
                other[GPU_RECORD_PUSH_BASE],
                other[GPU_RECORD_PUSH_CAPACITY] >> 6,
            )
        {
            return false;
        }
        i += 1;
    }
    i = 0;
    while i < c[GPU_RECORD_FACT_COUNT] {
        let at = i * 4;
        if i + 1 != ci
            && i + 1 != di
            && (gpu_record_overlap(color[0u16], color[1u16], facts[at], facts[at + 1])
                || gpu_record_overlap(depth[0u16], depth[1u16], facts[at], facts[at + 1]))
        {
            return false;
        }
        i += 1;
    }
    true
}
fn gpu_patch_scene(
    slot: u16,
    camera: Array<u16>,
    model: Array<u16>,
    color: Array<u16>,
    depth: Array<u16>,
) -> bool {
    if !gpu_patch_owned(slot) || !gpu_patch_targets(slot, color, depth) {
        return false;
    }
    let mut c = gpu_record_data(slot);
    let mut i: u16 = 24;
    while i < 32 {
        if camera[i] != c[GPU_CB_CAMERA + i] {
            return false;
        }
        i += 1;
    }
    let mut native = gpu_native_scratch();
    gpu_bind_copy(native, 0, c, GPU_CB_GLOBAL, 32);
    gpu_bind_copy(native, GPU_SCRATCH_INSTANCE, c, GPU_CB_INSTANCE, 64);
    gpu_bind_copy(native, GPU_SCRATCH_MATH, camera, 0, 32);
    gpu_bind_copy(native, GPU_SCRATCH_MATH + 32, c, GPU_CB_STATE, 16);
    let desired = unsafe { native.as_ptr().add(GPU_SCRATCH_MATH as i16).as_u16_array() };
    let state = unsafe { c.as_ptr().add(GPU_CB_STATE as i16).as_u16_array() };
    let light = gpu_scene_vec_read(state, GPU_STATE_LIGHT);
    let row0 = gpu_scene_vec_read(desired, 0);
    let row1 = gpu_scene_vec_read(desired, 8);
    let row2 = gpu_scene_vec_read(desired, 16);
    let view_light = vec3::new(-fdot(row0, light), fdot(row1, light), fdot(row2, light));
    if !gpu_scene_light_encode(view_light, native) {
        return false;
    }
    let revision = unsafe {
        camera
            .as_ptr()
            .add(GPU_CAMERA_REVISION as i16)
            .as_u16_array()
    };
    gpu_model_matrix_prepare(
        desired,
        model,
        native,
        gpu_arena_data(model[GPU_MODEL_DERIVED]),
        revision,
    );
    i = GPU_SCRATCH_TARGET;
    while i < GPU_SCRATCH_INSTANCE {
        native[i] = 0;
        i += 1;
    }
    native[GPU_SCRATCH_TARGET] = (color[0u16] >> 2) | (depth[0u16] << 12);
    native[GPU_SCRATCH_TARGET + 1] = depth[0u16] >> 4;
    let instance = unsafe {
        native
            .as_ptr()
            .add(GPU_SCRATCH_INSTANCE as i16)
            .as_u16_array()
    };
    let target = unsafe {
        native
            .as_ptr()
            .add(GPU_SCRATCH_TARGET as i16)
            .as_u16_array()
    };
    if !gpu_window_write_words(
        c[GPU_RECORD_PUSH_BASE],
        c[GPU_RECORD_GLOBAL_WORD] - 1,
        native,
        32,
    ) || !gpu_window_write_words(
        c[GPU_RECORD_PUSH_BASE],
        c[GPU_RECORD_INSTANCE_WORD] - 1,
        instance,
        42,
    ) || !gpu_window_write_words(
        c[GPU_RECORD_PUSH_BASE],
        c[GPU_RECORD_TARGET_WORD] - 1,
        target,
        16,
    ) {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    gpu_bind_copy(c, GPU_CB_CAMERA, camera, 0, 32);
    gpu_bind_copy(c, GPU_CB_CAMERA_REVISION, camera, GPU_CAMERA_REVISION, 4);
    gpu_bind_copy(c, GPU_CB_GLOBAL, native, 0, 32);
    gpu_bind_copy(c, GPU_CB_INSTANCE, native, GPU_SCRATCH_INSTANCE, 42);
    let mut facts = gpu_record_capture_facts(slot);
    facts[(c[GPU_RECORD_COLOR_FACT] - 1) * 4] = color[0u16];
    facts[(c[GPU_RECORD_DEPTH_FACT] - 1) * 4] = depth[0u16];
    c[GPU_RECORD_COLOR] = color[0u16];
    c[GPU_RECORD_DEPTH] = depth[0u16];
    true
}

// Target-only bindings never alter any camera, model or material payload.
fn gpu_target_shape(c: Array<u16>) -> bool {
    if c[GPU_RECORD_TARGET_BLOCKS] != 1
        || c[GPU_RECORD_UI_WORD] != 0
        || (c[GPU_RECORD_GENERATION_LOW] | c[GPU_RECORD_GENERATION_HIGH]) == 0
        || !gpu_patch_span(c, c[GPU_RECORD_TARGET_WORD], 16)
        || c[GPU_RECORD_COPY_COUNT] > 1
    {
        return false;
    }
    if c[GPU_RECORD_COPY_COUNT] == 0 {
        return c[GPU_RECORD_VALID] & GPU_RECORD_HAS_COPY == 0;
    }
    c[GPU_RECORD_VALID] & GPU_RECORD_HAS_COPY != 0
        && gpu_patch_span(c, c[GPU_RECORD_COPY_WORD], 16)
        && c[GPU_RECORD_COPY_WORD] < c[GPU_RECORD_TARGET_WORD]
        && c[GPU_RECORD_TARGET_WORD] - c[GPU_RECORD_COPY_WORD] >= 16
        && c[GPU_RECORD_COPY_DESTINATION] == c[GPU_RECORD_COLOR]
        && gpu_memcpy_native_valid(
            c[GPU_RECORD_COPY_SOURCE],
            c[GPU_RECORD_COLOR],
            GPU_API_PLANE_SECTORS,
        )
}
fn gpu_target_binding(slot: u16) -> bool {
    if !gpu_patch_owned(slot) {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if !gpu_target_shape(c) || c[GPU_RECORD_FACT_COUNT] > GPU_RECORD_FACT_LIMIT {
        return false;
    }
    let facts = gpu_record_capture_facts(slot);
    let mut color: u16 = 0;
    let mut depth: u16 = 0;
    let mut source = false;
    let mut i: u16 = 0;
    while i < c[GPU_RECORD_FACT_COUNT] {
        let at = i * 4;
        if facts[at + 2] == GPU_FACT_COLOR {
            if facts[at + 3] & GPU_FACT_WRITE != 0 {
                if color != 0 || facts[at] != c[GPU_RECORD_COLOR] || facts[at + 3] != 3 {
                    return false;
                }
                color = i + 1;
            } else if facts[at + 3] != GPU_FACT_READ {
                return false;
            }
            if facts[at] == c[GPU_RECORD_COPY_SOURCE]
                && facts[at + 3] == GPU_FACT_READ
                && facts[at + 1] >= GPU_API_PLANE_SECTORS
            {
                source = true;
            }
        }
        if facts[at + 2] == GPU_FACT_DEPTH {
            if depth != 0 || facts[at] != c[GPU_RECORD_DEPTH] || facts[at + 3] != 3 {
                return false;
            }
            depth = i + 1;
        }
        i += 1;
    }
    if color == 0
        || depth == 0
        || (c[GPU_RECORD_COPY_COUNT] != 0 && !source)
        || !gpu_record_seal(slot)
    {
        return false;
    }
    c[GPU_RECORD_COLOR_FACT] = color;
    c[GPU_RECORD_DEPTH_FACT] = depth;
    true
}
fn gpu_target_matches(slot: u16, low: u16, high: u16) -> bool {
    if !gpu_patch_owned(slot) {
        return false;
    }
    let c = gpu_record_data(slot);
    gpu_target_shape(c)
        && c[GPU_RECORD_GENERATION_LOW] == low
        && c[GPU_RECORD_GENERATION_HIGH] == high
}
// Observation only: no poll/retire, no waiting and no sticky Display mutation.
fn gpu_target_front_safe(color: Array<u16>, depth: Array<u16>) -> bool {
    let snapshot: Buf<u16, 4> = Buf::new([0; 4]);
    let status = gpu_present_capture_state(snapshot.as_array());
    if status == 65535 || status == 1 {
        return false;
    }
    if status == 2 {
        if GPU_PRESENT[1u16] != 0 || snapshot[0u16] != 0 {
            return false;
        }
        let front = (snapshot[1u16] >> 6) | (snapshot[2u16] << 10);
        if gpu_record_overlap(color[0u16], color[1u16], front, GPU_API_PLANE_SECTORS)
            || gpu_record_overlap(depth[0u16], depth[1u16], front, GPU_API_PLANE_SECTORS)
        {
            return false;
        }
    }
    true
}
fn gpu_target_retarget(slot: u16, color: Array<u16>, depth: Array<u16>) -> bool {
    if !gpu_patch_owned(slot) {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if !gpu_target_shape(c)
        || !gpu_patch_targets(slot, color, depth)
        || !gpu_target_front_safe(color, depth)
    {
        return false;
    }
    let native = gpu_native_scratch();
    let copy = unsafe { native.as_ptr().add(16).as_u16_array() };
    if c[GPU_RECORD_COPY_COUNT] != 0
        && !gpu_memcpy_native(
            copy,
            c[GPU_RECORD_COPY_SOURCE],
            color[0u16],
            GPU_API_PLANE_SECTORS,
        )
    {
        return false;
    }
    let mut target = native;
    let mut i: u16 = 0;
    while i < 16 {
        target[i] = 0;
        i += 1;
    }
    target[0u16] = (color[0u16] >> 2) | (depth[0u16] << 12);
    target[1u16] = depth[0u16] >> 4;
    if !gpu_window_write_words(
        c[GPU_RECORD_PUSH_BASE],
        c[GPU_RECORD_TARGET_WORD] - 1,
        target,
        16,
    ) || (c[GPU_RECORD_COPY_COUNT] != 0
        && !gpu_window_write_words(
            c[GPU_RECORD_PUSH_BASE],
            c[GPU_RECORD_COPY_WORD] - 1,
            copy,
            16,
        ))
    {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    let mut facts = gpu_record_capture_facts(slot);
    facts[(c[GPU_RECORD_COLOR_FACT] - 1) * 4] = color[0u16];
    facts[(c[GPU_RECORD_DEPTH_FACT] - 1) * 4] = depth[0u16];
    c[GPU_RECORD_COLOR] = color[0u16];
    c[GPU_RECORD_DEPTH] = depth[0u16];
    if c[GPU_RECORD_COPY_COUNT] != 0 {
        c[GPU_RECORD_COPY_DESTINATION] = color[0u16];
    }
    true
}
