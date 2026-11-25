//! Application-owned prepared allocations and immutable command concatenation.
use crate::dsl_rt::*;
mod gpu_bind;
use gpu_bind::*;
mod gpu_record;
use gpu_record::*;
mod gpu_execute;
use gpu_execute::*;
mod gpu_storage;
use gpu_storage::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_arena;
use gpu_arena::*;
mod gpu_api_values;
use gpu_api_values::*;

// Plans must survive scene preparation and command encoding in native scratch.
static GPU_PREPARED_FACT_PLAN: Buf<u16, 64> = Buf::new([0; 64]);
// Only standalone parts and pools claim ranges. Pool children use first-fit.
static GPU_GROUP_BACKINGS: u16 = 0;

fn gpu_group_backing_available(memory: GpuMemory) -> bool {
    let first = memory.first_sector();
    let count = memory.sector_count();
    if !gpu_memory_extent_valid(first, count) {
        return false;
    }
    let mut slot: u16 = 0;
    while slot < GPU_EXECUTION.configured_slots {
        let record = gpu_record_data(slot);
        if gpu_record_overlap(
            first,
            count,
            record[GPU_RECORD_COMMAND_BASE],
            record[GPU_RECORD_COMMAND_CAPACITY] >> 6,
        ) || gpu_record_overlap(
            first,
            count,
            record[GPU_RECORD_PUSH_BASE],
            record[GPU_RECORD_PUSH_CAPACITY] >> 6,
        ) {
            return false;
        }
        slot += 1;
    }
    slot = GPU_GROUP_BACKINGS;
    while slot != 0 {
        let data = gpu_arena_data(slot);
        let overlaps = if data[GPU_GROUP_BACKING_KIND] == GpuObjectKind::PreparedPool as u16 {
            gpu_record_overlap(first, count, data[GPU_POOL_BASE], data[GPU_POOL_CAPACITY])
        } else {
            gpu_record_overlap(
                first,
                count,
                data[GPU_PREPARED_COMMAND],
                data[GPU_PREPARED_COMMAND_CAPACITY] >> 6,
            ) || gpu_record_overlap(
                first,
                count,
                data[GPU_PREPARED_PUSH],
                data[GPU_PREPARED_PUSH_CAPACITY] >> 6,
            )
        };
        if overlaps {
            return false;
        }
        slot = data[GPU_GROUP_BACKING_NEXT];
    }
    true
}
fn gpu_group_link_backing(slot: u16, kind: GpuObjectKind) {
    let mut data = gpu_arena_data(slot);
    data[GPU_GROUP_BACKING_KIND] = kind as u16;
    data[GPU_GROUP_BACKING_NEXT] = GPU_GROUP_BACKINGS;
    unsafe {
        addr_of(&GPU_GROUP_BACKINGS).write(0, slot);
    }
}
fn gpu_group_unlink_backing(slot: u16) -> bool {
    let mut link = addr_of(&GPU_GROUP_BACKINGS);
    let mut current = unsafe { link.read(0) };
    while current != slot {
        if current == 0 {
            return false;
        }
        link = gpu_arena_data(current)
            .as_ptr()
            .add(GPU_GROUP_BACKING_NEXT as i16);
        current = unsafe { link.read(0) };
    }
    unsafe {
        link.write(0, gpu_arena_data(slot)[GPU_GROUP_BACKING_NEXT]);
    }
    true
}
fn gpu_group_new(slot: u16, memory: PreparedMemory) -> bool {
    if !gpu_group_backing_available(memory.commands)
        || !gpu_group_backing_available(memory.push)
        || !gpu_group_init(slot, memory)
    {
        return false;
    }
    gpu_group_link_backing(slot, GpuObjectKind::Prepared);
    true
}
fn gpu_group_init(slot: u16, memory: PreparedMemory) -> bool {
    let code = memory.commands;
    let push = memory.push;
    if !gpu_record_extent(code.first_sector(), code.sector_count())
        || !gpu_record_extent(push.first_sector(), push.sector_count())
        || gpu_record_overlap(
            code.first_sector(),
            code.sector_count(),
            push.first_sector(),
            push.sector_count(),
        )
    {
        return false;
    }
    let mut data = gpu_arena_data(slot);
    gpu_scene_zero(data, GPU_API_SMALL_WORDS);
    data[GPU_PREPARED_COMMAND] = code.first_sector();
    data[GPU_PREPARED_COMMAND_CAPACITY] = code.sector_count() << 6;
    data[GPU_PREPARED_PUSH] = push.first_sector();
    data[GPU_PREPARED_PUSH_CAPACITY] = push.sector_count() << 6;
    true
}
fn gpu_group_free(slot: u16) -> bool {
    gpu_group_invalidate(slot)
        && (gpu_arena_data(slot)[GPU_PREPARED_POOL] != 0 || gpu_group_unlink_backing(slot))
        && gpu_arena_free(slot, false, GpuObjectKind::Prepared)
}
fn gpu_group_mutable(data: Array<u16>) -> bool {
    data[GPU_PREPARED_STATE] != PreparedState::Recording as u16 && data[GPU_PREPARED_LIST_REFS] == 0
}
fn gpu_group_clear_slice_marks(mut data: Array<u16>) {
    let marks = data[GPU_PREPARED_SLICE_MARKS];
    if marks != 0 {
        free(Ptr::from_addr(marks));
        data[GPU_PREPARED_SLICE_MARKS] = 0;
    }
    #[cfg(gpu_debug)]
    {
        let mut head = data[GPU_PREPARED_SLICE_RESPONSES];
        while head != 0 {
            let pointer = Ptr::from_addr(head);
            head = unsafe { pointer.read(0) };
            free(pointer);
        }
        data[GPU_PREPARED_SLICE_RESPONSES] = 0;
    }
}
fn gpu_group_invalidate(slot: u16) -> bool {
    let mut data = gpu_arena_data(slot);
    if !gpu_group_mutable(data) {
        return false;
    }
    if data[GPU_PREPARED_META] != 0 {
        free(Ptr::from_addr(data[GPU_PREPARED_META]));
    }
    gpu_group_clear_slice_marks(data);
    data[GPU_PREPARED_META] = 0;
    data[GPU_PREPARED_STATE] = PreparedState::Empty as u16;
    true
}
fn gpu_group_part_ready(owner: u16) -> bool {
    gpu_arena_data(owner)[GPU_PREPARED_STATE] == PreparedState::Ready as u16
}
fn gpu_group_mark_slice(owner: u16, offset: u16) -> bool {
    let mut data = gpu_arena_data(owner);
    let capacity = data[GPU_PREPARED_COMMAND_CAPACITY];
    if offset > capacity >> 1 {
        return false;
    }
    let mut marks = data[GPU_PREPARED_SLICE_MARKS];
    if marks == 0 {
        // One bit per 32-bit command and one for the endpoint of a full buffer.
        let words = (capacity >> 5) + 1;
        let pointer = try_malloc(words);
        marks = pointer.addr();
        if marks == 0 {
            return false;
        }
        gpu_scene_zero(unsafe { pointer.as_u16_array() }, words);
        data[GPU_PREPARED_SLICE_MARKS] = marks;
    }
    let mut bits = unsafe { Ptr::from_addr(marks).as_u16_array() };
    bits[offset >> 4] |= 1u16 << (offset & 15);
    true
}
fn gpu_group_slice_marked(bits: Array<u16>, offset: u16) -> bool {
    bits[offset >> 4] & (1u16 << (offset & 15)) != 0
}
fn gpu_group_slice_valid(owner: u16, first: u16, end: u16, scene: u16) -> bool {
    let data = gpu_arena_data(owner);
    let marks = data[GPU_PREPARED_SLICE_MARKS];
    if scene == 0
        || !gpu_group_part_ready(owner)
        || marks == 0
        || data[GPU_PREPARED_LIST_REFS] == 65535
    {
        return false;
    }
    let meta = unsafe { Ptr::from_addr(data[GPU_PREPARED_META]).as_u16_array() };
    if first >= end || end > meta[GPU_GROUP_META_COMMAND_COUNT] {
        return false;
    }
    let bits = unsafe { Ptr::from_addr(marks).as_u16_array() };
    if !gpu_group_slice_marked(bits, first) || !gpu_group_slice_marked(bits, end) {
        return false;
    }
    #[cfg(gpu_debug)]
    {
        let c = gpu_record_data(scene - 1);
        let state = unsafe { c.as_ptr().add(GPU_CB_STATE as i16).as_u16_array() };
        // Newest group first. Each node stores next, first, and four response words.
        let mut head = data[GPU_PREPARED_SLICE_RESPONSES];
        let maxima: Buf<u16, 4> = Buf::new([0; 4]);
        while head != 0 {
            let entry = unsafe { Ptr::from_addr(head).as_u16_array() };
            let start = entry[1u16];
            if start < first {
                break;
            }
            if start < end {
                gpu_group_merge_responses(maxima.as_array(), 0, entry, 2);
            }
            head = entry[0u16];
        }
        if !gpu_scene_responses_valid(
            state,
            gpu_scene_fix_read(maxima.as_array(), 0),
            gpu_scene_fix_read(maxima.as_array(), 2),
        ) {
            return false;
        }
    }
    true
}
fn gpu_group_pool_init(slot: u16, memory: GpuMemory) -> bool {
    if !gpu_group_backing_available(memory) {
        return false;
    }
    let mut data = gpu_arena_data(slot);
    gpu_scene_zero(data, GPU_API_SMALL_WORDS);
    data[GPU_POOL_BASE] = memory.first_sector();
    data[GPU_POOL_CAPACITY] = memory.sector_count();
    gpu_group_link_backing(slot, GpuObjectKind::PreparedPool);
    true
}
// First-fit through address-sorted live allocations. No external free-list
// traffic, fixed part count, fixed partitions or heap compaction is required.
fn gpu_group_pool_allocate(pool: u16, command_bytes: u16, push_bytes: u16) -> u16 {
    if command_bytes == 0 || push_bytes == 0 {
        return 0;
    }
    let commands = (command_bytes >> 7) + ((command_bytes & 127 != 0) as u16);
    let push = (push_bytes >> 7) + ((push_bytes & 127 != 0) as u16);
    let total = commands + push;
    let mut p = gpu_arena_data(pool);
    let mut at = p[GPU_POOL_BASE];
    let mut remaining = p[GPU_POOL_CAPACITY];
    let mut previous: u16 = 0;
    let mut next = p[GPU_POOL_HEAD];
    while next != 0 {
        let data = gpu_arena_data(next);
        let base = data[GPU_PREPARED_COMMAND];
        if total <= base - at {
            break;
        }
        let end = data[GPU_PREPARED_PUSH] + (data[GPU_PREPARED_PUSH_CAPACITY] >> 6);
        // Handle the physical endpoint 65536 without losing the remaining span.
        let consumed = (base - at)
            + (data[GPU_PREPARED_COMMAND_CAPACITY] >> 6)
            + (data[GPU_PREPARED_PUSH_CAPACITY] >> 6);
        remaining -= consumed;
        at = end;
        previous = next;
        next = data[GPU_PREPARED_NEXT];
    }
    if total > remaining {
        return 0;
    }
    let slot = gpu_arena_alloc(false, GpuObjectKind::Prepared);
    if slot == 0 {
        return 0;
    }
    let memory = PreparedMemory::new(
        GpuMemory {
            first_sector: at,
            sector_count: commands,
        },
        GpuMemory {
            first_sector: at + commands,
            sector_count: push,
        },
    );
    if !gpu_group_init(slot, memory) {
        gpu_arena_free(slot, false, GpuObjectKind::Prepared);
        return 0;
    }
    let mut data = gpu_arena_data(slot);
    data[GPU_PREPARED_POOL] = pool;
    data[GPU_PREPARED_NEXT] = next;
    if previous == 0 {
        p[GPU_POOL_HEAD] = slot;
    } else {
        let mut before = gpu_arena_data(previous);
        before[GPU_PREPARED_NEXT] = slot;
    }
    slot
}
fn gpu_group_pool_free(pool: u16, owner: u16) -> bool {
    let data = gpu_arena_data(owner);
    if data[GPU_PREPARED_POOL] != pool || !gpu_group_mutable(data) {
        return false;
    }
    let next = data[GPU_PREPARED_NEXT];
    let mut p = gpu_arena_data(pool);
    let mut previous: u16 = 0;
    let mut current = p[GPU_POOL_HEAD];
    while current != owner {
        if current == 0 {
            return false;
        }
        previous = current;
        current = gpu_arena_data(current)[GPU_PREPARED_NEXT];
    }
    if previous == 0 {
        p[GPU_POOL_HEAD] = next;
    } else {
        let mut before = gpu_arena_data(previous);
        before[GPU_PREPARED_NEXT] = next;
    }
    gpu_group_free(owner)
}
fn gpu_group_pool_drop(pool: u16) -> bool {
    let mut current = gpu_arena_data(pool)[GPU_POOL_HEAD];
    while current != 0 {
        let data = gpu_arena_data(current);
        if !gpu_group_mutable(data) {
            return false;
        }
        current = data[GPU_PREPARED_NEXT];
    }
    current = gpu_arena_data(pool)[GPU_POOL_HEAD];
    while current != 0 {
        let next = gpu_arena_data(current)[GPU_PREPARED_NEXT];
        if !gpu_group_free(current) {
            return false;
        }
        current = next;
    }
    gpu_group_unlink_backing(pool) && gpu_arena_free(pool, false, GpuObjectKind::PreparedPool)
}

fn gpu_group_pad(slot: u16) -> bool {
    let c = gpu_record_data(slot);
    let words = (16 - (c[GPU_RECORD_COMMAND_USED] & 15)) & 15;
    let zero: Buf<u16, 16> = Buf::new([0; 16]);
    gpu_record_append(slot, zero.as_array(), words, zero.as_array(), 0, 1)
}
#[cfg(gpu_debug)]
fn gpu_group_merge_responses(out: Array<u16>, at: u16, input: Array<u16>, start: u16) {
    let mut i: u16 = 0;
    while i < 4 {
        let value = gpu_scene_fix_read(input, start + i);
        if value > gpu_scene_fix_read(out, at + i) {
            gpu_scene_fix_write(out, at + i, value);
        }
        i += 2;
    }
}
#[cfg(gpu_debug)]
fn gpu_group_note_responses(slot: u16, material: Array<u16>) -> bool {
    let c = gpu_record_data(slot);
    gpu_group_merge_responses(c, GPU_GROUP_RESPONSE_MAX, material, GPU_MATERIAL_AMBIENT);
    let mut data = gpu_arena_data(c[GPU_GROUP_RECORDING]);
    if data[GPU_PREPARED_SLICE_MARKS] == 0 {
        return true;
    }
    let start = c[GPU_GROUP_SLICE_START];
    let mut head = data[GPU_PREPARED_SLICE_RESPONSES];
    if head == 0 || unsafe { Ptr::from_addr(head).read(1) } != start {
        let pointer = try_malloc(6);
        if pointer.addr() == 0 {
            return false;
        }
        let mut entry = unsafe { pointer.as_u16_array() };
        gpu_scene_zero(entry, 6);
        entry[0u16] = head;
        entry[1u16] = start;
        head = pointer.addr();
        data[GPU_PREPARED_SLICE_RESPONSES] = head;
    }
    let entry = unsafe { Ptr::from_addr(head).as_u16_array() };
    gpu_group_merge_responses(entry, 2, material, GPU_MATERIAL_AMBIENT);
    true
}
fn gpu_group_part_begin(owner: u16) -> bool {
    if GPU_PREPARED_CONTEXT != 0 {
        return false;
    }
    let mut data = gpu_arena_data(owner);
    if !gpu_group_mutable(data) {
        return false;
    }
    let context = try_malloc(GPU_API_CB_WORDS + 256);
    if context.addr() == 0 {
        return false;
    }
    unsafe {
        addr_of(&GPU_PREPARED_CONTEXT).write(0, context.addr());
    }
    if !gpu_record_configure(
        GPU_PREPARED_RECORD,
        data[GPU_PREPARED_COMMAND],
        data[GPU_PREPARED_COMMAND_CAPACITY] >> 6,
        data[GPU_PREPARED_PUSH],
        data[GPU_PREPARED_PUSH_CAPACITY] >> 6,
    ) {
        return false;
    }
    if !gpu_group_invalidate(owner) {
        return false;
    }
    data[GPU_PREPARED_STATE] = PreparedState::Recording as u16;
    let mut c = gpu_record_data(GPU_PREPARED_RECORD);
    c[GPU_GROUP_RECORDING] = owner;
    c[GPU_RECORD_PHASE] = RecordPhase::PassOpen as u16;
    true
}
fn gpu_group_abandon(slot: u16) {
    if slot == GPU_PREPARED_RECORD && GPU_PREPARED_CONTEXT != 0 {
        let owner = gpu_record_data(slot)[GPU_GROUP_RECORDING];
        let mut data = gpu_arena_data(owner);
        gpu_group_clear_slice_marks(data);
        data[GPU_PREPARED_STATE] = PreparedState::Empty as u16;
        free(Ptr::from_addr(GPU_PREPARED_CONTEXT));
        unsafe {
            addr_of(&GPU_PREPARED_CONTEXT).write(0, 0);
        }
    }
}
fn gpu_group_end(slot: u16) -> bool {
    if slot != GPU_PREPARED_RECORD || GPU_PREPARED_CONTEXT == 0 {
        return false;
    }
    let c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16 {
        return false;
    }
    let command_count = c[GPU_RECORD_COMMAND_USED] >> 1;
    if !gpu_group_pad(slot) {
        return false;
    }
    let words = c[GPU_RECORD_COMMAND_USED];
    let count = c[GPU_RECORD_FACT_COUNT];
    // Target relocation must see the immutable push span as well as asset reads.
    // The caller preserves both; resource facts do not retain allocations.
    if count >= GPU_RECORD_FACT_LIMIT {
        return false;
    }
    let pointer = try_malloc(GPU_GROUP_META_FACTS + (count + 1) * 4);
    if pointer.addr() == 0 {
        return false;
    }
    let mut meta = unsafe { pointer.as_u16_array() };
    meta[0u16] = words;
    meta[1u16] = count + 1;
    meta[GPU_GROUP_META_COMMAND_COUNT] = command_count;
    meta[3u16] = 0;
    #[cfg(gpu_debug)]
    {
        gpu_bind_copy(meta, 4, c, GPU_GROUP_RESPONSE_MAX, 4);
    }
    gpu_bind_copy(meta, GPU_GROUP_META_EXIT, c, GPU_CB_RENDER, 12);
    gpu_bind_copy(
        meta,
        GPU_GROUP_META_FACTS,
        gpu_record_resource_facts(slot),
        0,
        count * 4,
    );
    let at = GPU_GROUP_META_FACTS + count * 4;
    meta[at] = c[GPU_RECORD_PUSH_BASE];
    meta[at + 1] = c[GPU_RECORD_PUSH_CAPACITY] >> 6;
    meta[at + 2] = ResourceFactKind::Buffer as u16;
    meta[at + 3] = GPU_FACT_READ;
    let mut data = gpu_arena_data(c[GPU_GROUP_RECORDING]);
    data[GPU_PREPARED_META] = pointer.addr();
    data[GPU_PREPARED_STATE] = PreparedState::Ready as u16;
    free(Ptr::from_addr(GPU_PREPARED_CONTEXT));
    unsafe {
        addr_of(&GPU_PREPARED_CONTEXT).write(0, 0);
    }
    true
}

// Prepared commands retain World/material only. Install the caller's scene once,
// then copy the entire immutable command span without per-material segmentation.
fn gpu_group_part_append(slot: u16, owner: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16
        || !gpu_group_part_ready(owner)
        || c[GPU_GROUP_RECORDING] != 0
    {
        return false;
    }
    let data = gpu_arena_data(owner);
    let pointer = Ptr::from_addr(data[GPU_PREPARED_META]);
    let meta = unsafe { pointer.as_u16_array() };
    let words = meta[0u16];
    let count = meta[1u16];
    if words == 0 {
        return true;
    }
    if c[GPU_RECORD_VALID] & 24 != 24 {
        return false;
    }
    #[cfg(gpu_debug)]
    {
        let state = unsafe { c.as_ptr().add(GPU_CB_STATE as i16).as_u16_array() };
        if !gpu_scene_responses_valid(
            state,
            gpu_scene_fix_read(meta, 4),
            gpu_scene_fix_read(meta, 6),
        ) {
            return false;
        }
    }
    let native = gpu_native_scratch();
    let desired = unsafe { c.as_ptr().add(GPU_CB_CAMERA as i16).as_u16_array() };
    let mut plan: Buf<u16, 7> = Buf::new([0; 7]);
    plan[GPU_BIND_PLAN_PUSH_END] = c[GPU_RECORD_PUSH_USED];
    if !gpu_bind_scene_current(slot) {
        if !gpu_bind_prepare_scene(slot, native) {
            return false;
        }
        let mut native = native;
        native[GPU_SCRATCH_WORLD_MODE] = 1;
        if !gpu_bind_plan_fields(slot, native, native, plan.as_array(), true) {
            return false;
        }
    }
    let pad = (16 - ((c[GPU_RECORD_COMMAND_USED] + plan[GPU_BIND_PLAN_CODE_WORDS]) & 15)) & 15;
    if !gpu_record_preflight(
        slot,
        plan[GPU_BIND_PLAN_CODE_WORDS] + pad + words,
        plan[GPU_BIND_PLAN_PUSH_END],
    ) {
        return false;
    }
    let facts = unsafe { pointer.add(GPU_GROUP_META_FACTS as i16).as_u16_array() };
    let fact_plan = GPU_PREPARED_FACT_PLAN.as_array();
    if !gpu_record_resource_room(slot, facts, count, fact_plan) {
        return false;
    }
    if plan[GPU_BIND_PLAN_CHANGES] & GPU_BIND_PLAN_VIEW != 0 {
        gpu_scene_view_prepare(desired, native);
    }
    if plan[GPU_BIND_PLAN_CODE_WORDS] != 0 && !gpu_bind_emit(slot, native, plan.as_array()) {
        return false;
    }
    if !gpu_group_pad(slot) {
        return false;
    }
    // Bindings are already published; native scratch is dead until the next
    // operation. Facts and the material exit snapshot live elsewhere.
    if !gpu_window_copy_span(
        data[GPU_PREPARED_COMMAND],
        0,
        c[GPU_RECORD_COMMAND_BASE],
        c[GPU_RECORD_COMMAND_USED],
        words,
        native,
    ) {
        c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
        return false;
    }
    c[GPU_RECORD_COMMAND_USED] += words;
    gpu_record_resource_commit(slot, facts, count, fact_plan);
    gpu_bind_copy(c, GPU_CB_RENDER, meta, GPU_GROUP_META_EXIT, 12);
    // Only the last material is known; the last World packet is not CPU-cached.
    c[GPU_RECORD_VALID] = (c[GPU_RECORD_VALID] | 2) & (65535 ^ (4 | GPU_BIND_RESPONSE_VALID));
    c[GPU_RECORD_UI_CLEAN] = 0;
    true
}
