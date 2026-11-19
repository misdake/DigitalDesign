//! Canonical CPU snapshots and native binding; no scene math or object handles.
use crate::dsl_rt::*;
mod gpu_packet;
use gpu_packet::*;

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
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS {
        return false;
    }
    if camera {
        gpu_bind_copy(c, GPU_CB_CAMERA, data, 0, 32);
        c[GPU_RECORD_VALID] |= 8;
    } else {
        gpu_bind_copy(c, GPU_CB_STATE, data, 0, 16);
        c[GPU_RECORD_VALID] |= 16;
    }
    true
}
// Six caller-local words: changes, Global/Render/Instance offsets, push end, code words.
// A draw preflights this plan PLUS its complete meshlet command count before emit.
fn gpu_bind_plan(slot: u16, native: Array<u16>, mut plan: Array<u16>) -> bool {
    if slot >= 2 {
        return false;
    }
    let c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS {
        return false;
    }
    let valid = c[GPU_RECORD_VALID];
    let mut changes: u16 = 0;
    if valid & 1 == 0 || !gpu_bind_equal(c, GPU_CB_GLOBAL, native, GPU_SCRATCH_GLOBAL, 32) {
        changes |= 1;
    }
    if valid & 2 == 0 || !gpu_bind_equal(c, GPU_CB_RENDER, native, GPU_SCRATCH_RENDER, 16) {
        changes |= 2;
    }
    if valid & 4 == 0 || !gpu_bind_equal(c, GPU_CB_INSTANCE, native, GPU_SCRATCH_INSTANCE, 64) {
        changes |= 4;
    }
    plan[0u16] = changes;
    plan[1u16] = 0;
    plan[2u16] = 0;
    let mut end = c[GPU_RECORD_PUSH_USED];
    let capacity = c[GPU_RECORD_PUSH_CAPACITY];
    let mut commands: u16 = 0;
    if changes & 4 != 0 {
        commands = 4;
    }
    if c[GPU_GROUP_RECORDING] != 0 && c[GPU_GROUP_FIRST_GLOBAL] == 0 {
        // The first draw starts with no bindings. Its Global and Render payloads
        // are contiguous and their commands occupy exactly one copyable line.
        if changes != 7 {
            return false;
        }
        commands += 2;
    }
    if changes & 3 != 0 {
        commands += 2;
    }
    if changes & 1 != 0 {
        commands += 6;
    }
    if changes & 2 != 0 {
        commands += 6;
    }
    if changes & 4 != 0 && changes & 3 != 0 {
        // Mixed bindings use the scratch's native layout: Global, Render,
        // sixteen unused words, then Instance. One aligned 256-byte upload.
        let at = gpu_record_align_push(end, 128, 64, capacity);
        if at == 65535 {
            return false;
        }
        plan[1u16] = at;
        plan[2u16] = at + 32;
        plan[3u16] = at + 64;
        end = at + 128;
    } else {
        if changes & 1 != 0 {
            let at = gpu_record_align_push(end, 32, 32, capacity);
            if at == 65535 {
                return false;
            }
            plan[1u16] = at;
            end = at + 32;
        }
        if changes & 2 != 0 {
            let at = gpu_record_align_push(end, 16, 16, capacity);
            if at == 65535 {
                return false;
            }
            plan[2u16] = at;
            end = at + 16;
        }
        if changes & 4 != 0 {
            let at = gpu_record_align_push(end, 64, 64, capacity);
            if at == 65535 {
                return false;
            }
            plan[3u16] = at;
            end = at + 64;
        } else {
            if c[GPU_RECORD_INSTANCE_WORD] == 0 {
                return false;
            }
            plan[3u16] = c[GPU_RECORD_INSTANCE_WORD] - 1;
        }
    }
    plan[4u16] = end;
    plan[5u16] = commands;
    true
}
// Produce one native binding command span without touching external memory.
fn gpu_bind_code(slot: u16, plan: Array<u16>, mut code: Array<u16>) -> u16 {
    let c = gpu_record_data(slot);
    let changes = plan[0u16];
    let mut at: u16 = 0;
    if changes & 3 != 0 {
        code[0u16] = 3;
        code[1u16] = 0x4000;
        at = 2;
    }
    if changes & 1 != 0 {
        gpu_packet_dma(code, at, c, plan[1u16], 66, 1);
        code[at + 2] = 1;
        code[at + 3] = 0x4000;
        code[at + 4] = 66;
        code[at + 5] = 0x5000;
        at += 6;
    }
    if changes & 2 != 0 {
        gpu_packet_dma(code, at, c, plan[2u16], 68, 0);
        code[at + 2] = 1;
        code[at + 3] = 0x4000;
        code[at + 4] = 68;
        code[at + 5] = 0x6000;
        at += 6;
    }
    if c[GPU_GROUP_RECORDING] != 0 && c[GPU_GROUP_FIRST_GLOBAL] == 0 {
        code[at] = 0;
        code[at + 1] = 0;
        at += 2;
    }
    // Instance bytes remain resident across draws, target and UI commands.
    // Group boundaries clear VALID bit2 because their stream may replace them.
    if changes & 4 != 0 {
        gpu_packet_dma(code, at, c, plan[3u16], 80, 2);
        code[at + 2] = 1;
        code[at + 3] = 0x4000;
        at += 4;
    }
    at
}
fn gpu_bind_payload(slot: u16, native: Array<u16>, plan: Array<u16>) -> bool {
    let changes = plan[0u16];
    if changes == 0 {
        return true;
    }
    let mut source: u16 = 0;
    let mut start = plan[1u16];
    let mut words: u16 = 32;
    if changes == 2 {
        source = 32;
        start = plan[2u16];
        words = 16;
    }
    if changes == 3 {
        words = 48;
    }
    if changes == 4 {
        source = 64;
        start = plan[3u16];
        words = 64;
    }
    if changes > 4 {
        words = 128;
    }
    let payload = unsafe { native.as_ptr().add(source as i16).as_u16_array() };
    let mut c = gpu_record_data(slot);
    if !gpu_window_write_words(c[GPU_RECORD_PUSH_BASE], start, payload, words) {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    true
}
// Commit the CPU binding only after every external write in the operation succeeds.
fn gpu_bind_commit(slot: u16, native: Array<u16>, plan: Array<u16>) {
    let mut c = gpu_record_data(slot);
    let changes = plan[0u16];
    if c[GPU_GROUP_RECORDING] != 0 && c[GPU_GROUP_FIRST_GLOBAL] == 0 {
        c[GPU_GROUP_FIRST_GLOBAL] = plan[1u16] + 1;
    }
    if changes & 1 != 0 {
        gpu_bind_copy(c, GPU_CB_GLOBAL, native, GPU_SCRATCH_GLOBAL, 32);
        c[GPU_RECORD_GLOBAL_WORD] = plan[1u16] + 1;
        c[GPU_RECORD_GLOBAL_BLOCKS] += 1;
    }
    if changes & 2 != 0 {
        gpu_bind_copy(c, GPU_CB_RENDER, native, GPU_SCRATCH_RENDER, 16);
    }
    if changes & 4 != 0 {
        gpu_bind_copy(c, GPU_CB_INSTANCE, native, GPU_SCRATCH_INSTANCE, 64);
        c[GPU_RECORD_INSTANCE_WORD] = plan[3u16] + 1;
        c[GPU_RECORD_INSTANCE_BLOCKS] += 1;
    }
    c[GPU_RECORD_VALID] |= 7;
}
// Consume the immediately preceding plan without an intervening external append.
fn gpu_bind_emit(slot: u16, native: Array<u16>, plan: Array<u16>) -> bool {
    let code = unsafe { native.as_ptr().add(160).as_u16_array() };
    if !gpu_bind_payload(slot, native, plan) {
        return false;
    }
    let words = gpu_bind_code(slot, plan, code);
    if !gpu_record_append_reserved(slot, code, words, code, 0, plan[4u16]) {
        return false;
    }
    gpu_bind_commit(slot, native, plan);
    true
}
fn gpu_bind_install(slot: u16, native: Array<u16>) -> bool {
    let plan: Buf<u16, 6> = Buf::new([0; 6]);
    gpu_bind_plan(slot, native, plan.as_array())
        && gpu_record_preflight(slot, plan[5u16], plan[4u16])
        && gpu_bind_emit(slot, native, plan.as_array())
}
