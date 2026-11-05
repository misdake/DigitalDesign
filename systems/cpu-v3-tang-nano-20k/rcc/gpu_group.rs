//! Slot-private prepared parts. Commands, snapshots and facts keep separate lives.
use crate::dsl_rt::*;
mod gpu_bind;
use gpu_bind::*;
mod gpu_record;
use gpu_record::*;
mod gpu_storage;
use gpu_storage::*;
mod gpu_schema;
use gpu_schema::*;

// Upper half of the original command reservation, never fetched by the GPU.
// Header: code words, fact count, native flag, actual push end, zeros.
// Entry Global/Render48 at16; exit48 at64; facts64x4 at128.
fn gpu_group_transfer(
    c: Array<u16>,
    group: u16,
    word: u16,
    data: Array<u16>,
    count: u16,
    read: bool,
) -> bool {
    if group >= GPU_GROUP_COUNT
        || word > GPU_GROUP_META_WORDS
        || count > GPU_GROUP_META_WORDS - word
    {
        return false;
    }
    let mut at: u16 = 0;
    while at < count {
        let mut words = count - at;
        if words > 64 {
            words = 64;
        }
        let part = unsafe { data.as_ptr().add(at as i16).as_u16_array() };
        if !gpu_storage_copy_owned(
            part,
            c[GPU_GROUP_TEMPLATE] + 256,
            group * GPU_GROUP_META_WORDS + word + at,
            words,
            read,
        ) {
            return false;
        }
        at += words;
    }
    true
}
fn gpu_group_push_begin(group: u16) -> u16 {
    if group == 0 {
        0
    } else {
        8192 + (group - 1) * 1024
    }
}
fn gpu_group_push_end(group: u16) -> u16 {
    if group == 0 {
        8192
    } else {
        8192 + group * 1024
    }
}
fn gpu_group_enable(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_READY
        || c[GPU_RECORD_COMMAND_USED] != 0
        || c[GPU_RECORD_PUSH_USED] != 0
        || c[GPU_GROUP_TEMPLATE] != 0
        || c[GPU_RECORD_COMMAND_BASE] == 0
        || c[GPU_RECORD_COMMAND_BASE] & 255 != 0
        || c[GPU_RECORD_COMMAND_CAPACITY] != 32768
        || c[GPU_RECORD_PUSH_CAPACITY] != 32768
    {
        return false;
    }
    c[GPU_GROUP_TEMPLATE] = c[GPU_RECORD_COMMAND_BASE];
    c[GPU_RECORD_COMMAND_BASE] += 128;
    c[GPU_RECORD_COMMAND_CAPACITY] = 8192;
    c[GPU_GROUP_PUSH_END] = GPU_GROUP_PUSH_PREFIX;
    c[GPU_RECORD_PUSH_USED] = GPU_GROUP_PUSH_PREFIX;
    true
}
fn gpu_group_clear(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if !gpu_record_generation_available()
        || c[GPU_GROUP_TEMPLATE] == 0
        || c[GPU_GROUP_RECORDING] != 0
        || (c[GPU_RECORD_PHASE] != GPU_RECORD_READY && c[GPU_RECORD_PHASE] != GPU_RECORD_SEALED)
    {
        return false;
    }
    c[GPU_GROUP_VALID] = 0;
    c[GPU_GROUP_WORDS] = 0;
    c[GPU_GROUP_FACT_COUNT] = 0;
    gpu_record_reset(slot)
}
fn gpu_group_invalidate(slot: u16, group: u16) -> bool {
    if slot >= 2 || group >= GPU_GROUP_COUNT {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_GROUP_TEMPLATE] == 0
        || c[GPU_GROUP_RECORDING] != 0
        || (c[GPU_RECORD_PHASE] != GPU_RECORD_READY && c[GPU_RECORD_PHASE] != GPU_RECORD_SEALED)
        || (c[GPU_RECORD_PHASE] == GPU_RECORD_READY && c[GPU_RECORD_COMMAND_USED] != 0)
    {
        return false;
    }
    c[GPU_GROUP_VALID] &= (1u16 << group) ^ 65535;
    true
}
fn gpu_group_part_ready(slot: u16, group: u16) -> bool {
    slot < 2
        && group < GPU_GROUP_COUNT
        && gpu_record_data(slot)[GPU_GROUP_VALID] & (1u16 << group) != 0
}
fn gpu_group_ready(slot: u16) -> bool {
    gpu_group_part_ready(slot, 0)
}
fn gpu_group_pad(slot: u16) -> bool {
    let c = gpu_record_data(slot);
    let words = (16 - (c[GPU_RECORD_COMMAND_USED] & 15)) & 15;
    let zero: Buf<u16, 16> = Buf::new([0; 16]);
    gpu_record_append(slot, zero.as_array(), words, zero.as_array(), 0, 1)
}
fn gpu_group_boundary(mut c: Array<u16>) {
    // Preserve the true last Global/Render, never Instance or desired objects.
    c[GPU_RECORD_VALID] &= 65507;
    c[GPU_RECORD_UI_CLEAN] = 0;
    c[GPU_RECORD_SCENE_MODEL] = 65535;
    c[GPU_RECORD_SCENE_MATERIAL] = 65535;
    c[GPU_RECORD_SCENE_CAMERA] = 65535;
}
fn gpu_group_part_begin(slot: u16, group: u16) -> bool {
    if slot >= 2 || group >= GPU_GROUP_COUNT {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS
        || c[GPU_GROUP_TEMPLATE] == 0
        || c[GPU_GROUP_RECORDING] != 0
        || c[GPU_RECORD_VALID] & 24 != 24
        || c[GPU_GROUP_REFERENCED] & (1u16 << group) != 0
        || !gpu_group_pad(slot)
    {
        return false;
    }
    c[GPU_GROUP_VALID] &= (1u16 << group) ^ 65535;
    c[GPU_GROUP_START] = c[GPU_RECORD_COMMAND_USED];
    c[GPU_GROUP_SAVED_PUSH] = c[GPU_RECORD_PUSH_USED];
    c[GPU_GROUP_SAVED_CAPACITY] = c[GPU_RECORD_PUSH_CAPACITY];
    c[GPU_RECORD_PUSH_USED] = gpu_group_push_begin(group);
    c[GPU_RECORD_PUSH_CAPACITY] = gpu_group_push_end(group);
    c[GPU_GROUP_FIRST_GLOBAL] = 0;
    c[GPU_GROUP_FIRST_RENDER] = 0;
    c[GPU_GROUP_RECORDING] = group + 1;
    c[GPU_RECORD_VALID] &= 65528;
    true
}
fn gpu_group_begin(slot: u16) -> bool {
    gpu_group_part_begin(slot, 0)
}
fn gpu_group_end(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS || c[GPU_GROUP_RECORDING] == 0 || !gpu_group_pad(slot)
    {
        return false;
    }
    let group = c[GPU_GROUP_RECORDING] - 1;
    let words = c[GPU_RECORD_COMMAND_USED] - c[GPU_GROUP_START];
    if words > GPU_GROUP_CODE_WORDS
        || !gpu_window_copy_lines(
            c[GPU_RECORD_COMMAND_BASE],
            c[GPU_GROUP_START],
            c[GPU_GROUP_TEMPLATE],
            group * GPU_GROUP_CODE_WORDS,
            words,
        )
    {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    let scratch = gpu_native_scratch();
    let mut count: u16 = 0;
    let native = c[GPU_GROUP_FIRST_GLOBAL] != 0;
    if native {
        let render = unsafe { scratch.as_ptr().add(32).as_u16_array() };
        let last = unsafe { c.as_ptr().add(GPU_CB_GLOBAL as i16).as_u16_array() };
        if !gpu_window_read_words(
            c[GPU_RECORD_PUSH_BASE],
            c[GPU_GROUP_FIRST_GLOBAL] - 1,
            scratch,
            32,
        ) || !gpu_window_read_words(
            c[GPU_RECORD_PUSH_BASE],
            c[GPU_GROUP_FIRST_RENDER] - 1,
            render,
            16,
        ) || !gpu_group_transfer(c, group, 16, scratch, 48, false)
            || !gpu_group_transfer(c, group, 64, last, 48, false)
        {
            c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
            return false;
        }
        let facts = gpu_record_capture_facts(slot);
        let mut saved = scratch;
        let mut i: u16 = 0;
        while i < c[GPU_RECORD_FACT_COUNT] {
            let at = i * 4;
            if facts[at + 2] == GPU_FACT_MESH || facts[at + 2] == GPU_FACT_TEXTURE {
                let mut n: u16 = 0;
                while n < 4 {
                    saved[count * 4 + n] = facts[at + n];
                    n += 1;
                }
                count += 1;
            }
            i += 1;
        }
    }
    let mut header: Buf<u16, 8> = Buf::new([0; 8]);
    header[0u16] = words;
    header[1u16] = count;
    header[2u16] = if native { 1u16 } else { 0u16 };
    header[3u16] = c[GPU_RECORD_PUSH_USED];
    if !gpu_group_transfer(c, group, 128, scratch, count * 4, false)
        || !gpu_group_transfer(c, group, 0, header.as_array(), 8, false)
    {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    c[GPU_GROUP_WORDS] = words;
    c[GPU_GROUP_FACT_COUNT] = count;
    c[GPU_RECORD_PUSH_USED] = c[GPU_GROUP_SAVED_PUSH];
    c[GPU_RECORD_PUSH_CAPACITY] = c[GPU_GROUP_SAVED_CAPACITY];
    c[GPU_GROUP_VALID] |= 1u16 << group;
    c[GPU_GROUP_REFERENCED] |= 1u16 << group;
    c[GPU_GROUP_RECORDING] = 0;
    gpu_group_boundary(c);
    true
}
fn gpu_group_native_equal(c: Array<u16>, data: Array<u16>) -> bool {
    c[GPU_RECORD_VALID] & 3 == 3 && gpu_bind_equal(c, GPU_CB_GLOBAL, data, 0, 48)
}
fn gpu_group_part_append(slot: u16, group: u16) -> bool {
    if slot >= 2 || group >= GPU_GROUP_COUNT {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS
        || !gpu_group_part_ready(slot, group)
        || c[GPU_GROUP_RECORDING] != 0
    {
        return false;
    }
    let header: Buf<u16, 8> = Buf::new([0; 8]);
    let scratch = gpu_native_scratch();
    if !gpu_group_transfer(c, group, 0, header.as_array(), 8, true) {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    let words = header[0u16];
    let count = header[1u16];
    let native = header[2u16] != 0;
    if words > GPU_GROUP_CODE_WORDS
        || words & 15 != 0
        || count > GPU_RECORD_FACT_LIMIT
        || header[2u16] > 1
        || (native && words < 16)
    {
        return false;
    }
    if words == 0 {
        c[GPU_GROUP_REFERENCED] |= 1u16 << group;
        gpu_group_boundary(c);
        return true;
    }
    let mut skip: u16 = 0;
    if native {
        if !gpu_group_transfer(c, group, 16, scratch, 48, true) {
            c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
            return false;
        }
        if gpu_group_native_equal(c, scratch) {
            skip = 16;
        }
    }
    let pad = (16 - (c[GPU_RECORD_COMMAND_USED] & 15)) & 15;
    if !gpu_record_preflight(slot, pad + words - skip, c[GPU_RECORD_PUSH_USED]) {
        return false;
    }
    if !gpu_group_transfer(c, group, 128, scratch, count * 4, true) {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    if !gpu_record_capture_room(slot, scratch, count) {
        return false;
    }
    if !gpu_group_pad(slot)
        || !gpu_window_copy_lines(
            c[GPU_GROUP_TEMPLATE],
            group * GPU_GROUP_CODE_WORDS + skip,
            c[GPU_RECORD_COMMAND_BASE],
            c[GPU_RECORD_COMMAND_USED],
            words - skip,
        )
    {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    c[GPU_RECORD_COMMAND_USED] += words - skip;
    gpu_record_capture_commit(slot, scratch, count);
    if native {
        if !gpu_group_transfer(c, group, 64, scratch, 48, true) {
            c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
            return false;
        }
        gpu_bind_copy(c, GPU_CB_GLOBAL, scratch, 0, 48);
        c[GPU_RECORD_VALID] |= 3;
    }
    c[GPU_GROUP_WORDS] = words;
    c[GPU_GROUP_FACT_COUNT] = count;
    c[GPU_GROUP_REFERENCED] |= 1u16 << group;
    gpu_group_boundary(c);
    true
}
fn gpu_group_append(slot: u16) -> bool {
    gpu_group_part_append(slot, 0)
}
