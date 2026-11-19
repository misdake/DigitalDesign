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

// CPU-only metadata: three header words followed by the two canonical bindings
// and just the used facts. A null resident pointer selects external backing.
const GPU_GROUP_META_ENTRY: u16 = 3;
const GPU_GROUP_META_EXIT: u16 = 51;
const GPU_GROUP_META_FACTS: u16 = 99;
static GPU_GROUP_RESIDENT: Buf<u16, 32> = Buf::new([0; 32]);
static GPU_GROUP_META_SCRATCH: Buf<u16, 355> = Buf::new([0; 355]);

fn gpu_group_resident_index(slot: u16, group: u16) -> u16 {
    slot * GPU_GROUP_COUNT + group
}
fn gpu_group_release_metadata(slot: u16, group: u16) {
    let mut resident = GPU_GROUP_RESIDENT.as_array();
    let index = gpu_group_resident_index(slot, group);
    let address = resident[index];
    if address != 0 {
        resident[index] = 0;
        free(Ptr::from_addr(address));
    }
}
fn gpu_group_release_all_metadata(slot: u16) {
    let mut group: u16 = 0;
    while group < GPU_GROUP_COUNT {
        gpu_group_release_metadata(slot, group);
        group += 1;
    }
}
// External fallback belongs to this CB's original, GPU-unfetched reservation.
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

fn gpu_group_store_metadata(slot: u16, group: u16, meta: Array<u16>, words: u16) -> bool {
    let pointer = try_malloc(words);
    if pointer.addr() == 0 {
        return gpu_group_transfer(gpu_record_data(slot), group, 0, meta, words, false);
    }
    let storage = unsafe { pointer.as_u16_array() };
    gpu_bind_copy(storage, 0, meta, 0, words);
    let mut resident = GPU_GROUP_RESIDENT.as_array();
    resident[gpu_group_resident_index(slot, group)] = pointer.addr();
    true
}

fn gpu_group_load_metadata(c: Array<u16>, slot: u16, group: u16) -> Ptr {
    let address = GPU_GROUP_RESIDENT[gpu_group_resident_index(slot, group)];
    if address != 0 {
        return Ptr::from_addr(address);
    }
    let meta = GPU_GROUP_META_SCRATCH.as_array();
    if !gpu_group_transfer(c, group, 0, meta, GPU_GROUP_META_ENTRY, true) {
        return Ptr::from_addr(0);
    }
    let count = meta[1u16];
    if count > GPU_RECORD_FACT_LIMIT || meta[2u16] > 1 {
        return Ptr::from_addr(0);
    }
    if meta[2u16] != 0 {
        let contents = unsafe { meta.as_ptr().add(GPU_GROUP_META_ENTRY as i16).as_u16_array() };
        if !gpu_group_transfer(c, group, GPU_GROUP_META_ENTRY, contents, 96 + count * 4, true) {
            return Ptr::from_addr(0);
        }
    }
    meta.as_ptr()
}
fn gpu_group_push_begin(group: u16, layout: u16) -> u16 {
    if layout == 1 && group == 15 {
        return 4096;
    }
    if group == 0 {
        0
    } else {
        8192 + (group - 1) * 1024
    }
}
fn gpu_group_push_end(group: u16, layout: u16) -> u16 {
    if layout == 1 {
        if group == 0 {
            return 4096;
        }
        if group == 15 {
            return 8192;
        }
    }
    if group == 0 {
        8192
    } else {
        8192 + group * 1024
    }
}
fn gpu_group_enable(slot: u16) -> bool {
    gpu_group_enable_layout(slot, 0)
}
fn gpu_group_enable_layout(slot: u16, layout: u16) -> bool {
    if slot >= 2 || layout > 1 {
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
    c[GPU_GROUP_LAYOUT] = layout;
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
    gpu_group_release_all_metadata(slot);
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
    gpu_group_release_metadata(slot, group);
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
    gpu_group_release_metadata(slot, group);
    c[GPU_GROUP_START] = c[GPU_RECORD_COMMAND_USED];
    c[GPU_GROUP_SAVED_PUSH] = c[GPU_RECORD_PUSH_USED];
    c[GPU_GROUP_SAVED_CAPACITY] = c[GPU_RECORD_PUSH_CAPACITY];
    c[GPU_RECORD_PUSH_USED] = gpu_group_push_begin(group, c[GPU_GROUP_LAYOUT]);
    c[GPU_RECORD_PUSH_CAPACITY] = gpu_group_push_end(group, c[GPU_GROUP_LAYOUT]);
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
    let mut meta = GPU_GROUP_META_SCRATCH.as_array();
    let mut count: u16 = 0;
    let native = c[GPU_GROUP_FIRST_GLOBAL] != 0;
    if native {
        let entry = unsafe { meta.as_ptr().add(GPU_GROUP_META_ENTRY as i16).as_u16_array() };
        let render = unsafe { entry.as_ptr().add(32).as_u16_array() };
        if !gpu_window_read_words(
            c[GPU_RECORD_PUSH_BASE],
            c[GPU_GROUP_FIRST_GLOBAL] - 1,
            entry,
            32,
        ) || !gpu_window_read_words(
            c[GPU_RECORD_PUSH_BASE],
            c[GPU_GROUP_FIRST_RENDER] - 1,
            render,
            16,
        ) {
            c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
            return false;
        }
        gpu_bind_copy(meta, GPU_GROUP_META_EXIT, c, GPU_CB_GLOBAL, 48);
        let facts = gpu_record_resource_facts(slot);
        let mut i: u16 = 0;
        while i < c[GPU_RECORD_FACT_COUNT] {
            let at = i * 4;
            if facts[at + 2] == GPU_FACT_MESH || facts[at + 2] == GPU_FACT_TEXTURE {
                gpu_bind_copy(meta, GPU_GROUP_META_FACTS + count * 4, facts, at, 4);
                count += 1;
            }
            i += 1;
        }
    }
    meta[0u16] = words;
    meta[1u16] = count;
    meta[2u16] = if native { 1u16 } else { 0u16 };
    let metadata_words = if native {
        GPU_GROUP_META_FACTS + count * 4
    } else {
        GPU_GROUP_META_ENTRY
    };
    if !gpu_group_store_metadata(slot, group, meta, metadata_words) {
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
    c[GPU_RECORD_VALID] & 3 == 3 && gpu_bind_equal(c, GPU_CB_GLOBAL, data, GPU_GROUP_META_ENTRY, 48)
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
    let pointer = gpu_group_load_metadata(c, slot, group);
    if pointer.addr() == 0 {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    let meta = unsafe { pointer.as_u16_array() };
    let words = meta[0u16];
    let count = meta[1u16];
    let native = meta[2u16] != 0;
    if words > GPU_GROUP_CODE_WORDS
        || words & 15 != 0
        || count > GPU_RECORD_FACT_LIMIT
        || meta[2u16] > 1
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
    if native && gpu_group_native_equal(c, meta) {
        skip = 16;
    }
    let pad = (16 - (c[GPU_RECORD_COMMAND_USED] & 15)) & 15;
    if !gpu_record_preflight(slot, pad + words - skip, c[GPU_RECORD_PUSH_USED]) {
        return false;
    }
    let facts = unsafe { pointer.add(GPU_GROUP_META_FACTS as i16).as_u16_array() };
    // The reservation writes every consumed plan word before commit.
    let fact_plan = gpu_native_scratch();
    if !gpu_record_resource_room(slot, facts, count, fact_plan) {
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
    gpu_record_resource_commit(slot, facts, count, fact_plan);
    if native {
        gpu_bind_copy(c, GPU_CB_GLOBAL, meta, GPU_GROUP_META_EXIT, 48);
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
