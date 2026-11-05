//! Canonical CPU snapshots and native binding; no scene math or object handles.
use crate::dsl_rt::*;
mod gpu_packet;
use gpu_packet::*;

// Private CPU-owned canonical spans. Forward stores retain overlap order.
fn gpu_bind_copy(c: Array<u16>, dst: u16, data: Array<u16>, src: u16, words: u16) {
    let mut to = c.as_ptr().add(dst as i16);
    let mut from = data.as_ptr().add(src as i16);
    let mut left = words;
    while left >= 8 {
        unsafe {
            to.write(0, from.read(0));
            to.write(1, from.read(1));
            to.write(2, from.read(2));
            to.write(3, from.read(3));
            to.write(4, from.read(4));
            to.write(5, from.read(5));
            to.write(6, from.read(6));
            to.write(7, from.read(7));
        }
        to = to.add(8);
        from = from.add(8);
        left -= 8;
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
    let mut commands: u16 = 4;
    if c[GPU_GROUP_RECORDING] != 0 && c[GPU_GROUP_FIRST_GLOBAL] == 0 {
        // First Global/Render binding occupies exactly one copyable line.
        commands += 2;
    }
    if changes & 3 != 0 {
        commands += 2;
    }
    if changes & 1 != 0 {
        let at = gpu_record_align_push(end, 32, 32, capacity);
        if at == 65535 {
            return false;
        }
        plan[1u16] = at;
        end = at + 32;
        commands += 6;
    }
    if changes & 2 != 0 {
        let at = gpu_record_align_push(end, 16, 16, capacity);
        if at == 65535 {
            return false;
        }
        plan[2u16] = at;
        end = at + 16;
        commands += 6;
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
    plan[4u16] = end;
    plan[5u16] = commands;
    gpu_record_preflight(slot, commands, end)
}
// kind0=Global64, kind1=Render32, kind2=Instance128.
fn gpu_bind_block(slot: u16, native: Array<u16>, plan: Array<u16>, kind: u16, drain: bool) -> bool {
    let c = gpu_record_data(slot);
    let mut local: u16 = 66;
    let mut size: u16 = 1;
    let mut src: u16 = GPU_SCRATCH_GLOBAL;
    let mut count: u16 = 32;
    let mut alignment: u16 = 32;
    let mut opcode: u16 = 0x5000;
    if kind == 1 {
        local = 68;
        size = 0;
        src = GPU_SCRATCH_RENDER;
        count = 16;
        alignment = 16;
        opcode = 0x6000;
    }
    if kind == 2 {
        local = 80;
        size = 2;
        src = GPU_SCRATCH_INSTANCE;
        count = 64;
        alignment = 64;
        if plan[0u16] & 4 == 0 {
            count = 0;
            alignment = 1;
        }
    }
    let mut code: Buf<u16, 8> = Buf::new([0; 8]);
    let mut i: u16 = 0;
    if drain {
        code[0u16] = 3;
        code[1u16] = 0x4000;
        i = 2;
    }
    gpu_packet_dma(code.as_array(), i, c, plan[kind + 1], local, size);
    code[i + 2] = 1;
    code[i + 3] = 0x4000;
    i += 4;
    if kind != 2 {
        code[i] = local;
        code[i + 1] = opcode;
        i += 2;
    }
    let payload = unsafe { native.as_ptr().add(src as i16).as_u16_array() };
    gpu_record_append(slot, code.as_array(), i, payload, count, alignment)
}
// Consume the immediately preceding local plan; no intervening append is permitted.
fn gpu_bind_emit(slot: u16, native: Array<u16>, plan: Array<u16>) -> bool {
    if !gpu_record_preflight(slot, plan[5u16], plan[4u16]) {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS {
        return false;
    }
    let changes = plan[0u16];
    if changes & 1 != 0 && !gpu_bind_block(slot, native, plan, 0, true) {
        return false;
    }
    if changes & 2 != 0 && !gpu_bind_block(slot, native, plan, 1, changes & 1 == 0) {
        return false;
    }
    if c[GPU_GROUP_RECORDING] != 0 && c[GPU_GROUP_FIRST_GLOBAL] == 0 {
        let nop: Buf<u16, 2> = Buf::new([0; 2]);
        if changes & 3 != 3 || !gpu_record_append(slot, nop.as_array(), 2, nop.as_array(), 0, 1) {
            return false;
        }
        c[GPU_GROUP_FIRST_GLOBAL] = plan[1u16] + 1;
        c[GPU_GROUP_FIRST_RENDER] = plan[2u16] + 1;
    }
    if !gpu_bind_block(slot, native, plan, 2, false) {
        return false;
    }
    // Commit CPU last-bound values only after all backing writes succeeded.
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
    true
}
fn gpu_bind_install(slot: u16, native: Array<u16>) -> bool {
    let plan: Buf<u16, 6> = Buf::new([0; 6]);
    gpu_bind_plan(slot, native, plan.as_array()) && gpu_bind_emit(slot, native, plan.as_array())
}
