//! CPU-owned command records. This module neither submits nor retires GPU work.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_storage;
use gpu_storage::*;
mod gpu_arena;
use gpu_arena::*;

// Two records of GPU_API_CB_WORDS; checked by the recording-layout fixtures.
static GPU_CB_RECORDS: Buf<u16, 320> = Buf::new([0; 320]);
// The logical 256-word span stays line-aligned independently of static layout.
static GPU_NATIVE_SCRATCH: Buf<u16, 271> = Buf::new([0; 271]);
// Allocation facts survive descriptor Drop and execution until this CB resets.
static GPU_CB_RESOURCE_FACTS: Buf<u16, 512> = Buf::new([0; 512]);
// Never reset with a context: copied binding locations cannot regain an epoch.
static GPU_RECORD_GENERATION: Buf<u16, 2> = Buf::new([0; 2]);
// Per-CB owned table and the first not-yet-linked dynamic command (32-bit units).
static GPU_CB_LISTS: Buf<u16, 4> = Buf::new([0; 4]);
// Transient record and facts are acquired only while recording a prepared part.
static GPU_PREPARED_CONTEXT: u16 = 0;

fn gpu_list_table_address(allocation: u16) -> u16 {
    (allocation + GPU_LIST_METADATA_WORDS + GPU_LIST_TABLE_WORDS - 1) & 0xffc0
}
fn gpu_api_list_metadata(allocation: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(allocation).as_u16_array() }
}
fn gpu_api_list_parts(allocation: u16) -> Array<u16> {
    unsafe {
        Ptr::from_addr(gpu_list_table_address(allocation) + GPU_LIST_TABLE_WORDS).as_u16_array()
    }
}
fn gpu_record_release_list_parts(allocation: u16) {
    let count = gpu_api_list_metadata(allocation)[GPU_LIST_USED];
    let parts = gpu_api_list_parts(allocation);
    let mut index: u16 = 0;
    while index < count {
        let owner = parts[index];
        if owner != 0 {
            let mut data = gpu_arena_data(owner);
            data[GPU_PREPARED_LIST_REFS] -= 1;
        }
        index += 1;
    }
    free(Ptr::from_addr(allocation));
}
fn gpu_record_discard_list(slot: u16) {
    let allocation = GPU_CB_LISTS[slot * 2];
    if allocation != 0 {
        GPU_CB_LISTS[slot * 2] = 0;
        gpu_record_release_list_parts(allocation);
    }
}

fn gpu_native_scratch() -> Array<u16> {
    unsafe { Ptr::from_addr((GPU_NATIVE_SCRATCH.as_ptr().addr() + 15) & 65520).as_u16_array() }
}

const GPU_RECORD_PHASE: u16 = 0;
const GPU_RECORD_COMMAND_BASE: u16 = 1;
const GPU_RECORD_COMMAND_CAPACITY: u16 = 2;
const GPU_RECORD_PUSH_BASE: u16 = 3;
const GPU_RECORD_PUSH_CAPACITY: u16 = 4;
const GPU_RECORD_COMMAND_USED: u16 = 5;
const GPU_RECORD_PUSH_USED: u16 = 6;
const GPU_RECORD_PADDED: u16 = 7;
const GPU_RECORD_VALID: u16 = 8;
// Copy-bearing target streams retain this proof flag through sealing.
const GPU_RECORD_HAS_COPY: u16 = 32768;
const GPU_RECORD_COLOR: u16 = 9;
const GPU_RECORD_DEPTH: u16 = 10;
const GPU_RECORD_INSTANCE_WORD: u16 = 11;
const GPU_RECORD_UI_CLEAN: u16 = 12;
const GPU_RECORD_UI_WORD: u16 = 13;
const GPU_RECORD_TERMINAL: u16 = 14;
const GPU_RECORD_FACT_COUNT: u16 = 15;
// Target-only streams forbid UI: reuse four words of their inactive UI cache.
// Camera revision owns control16..19; never overlay it.
const GPU_RECORD_COPY_WORD: u16 = 128;
const GPU_RECORD_COPY_SOURCE: u16 = 129;
const GPU_RECORD_COPY_DESTINATION: u16 = 130;
const GPU_RECORD_COPY_COUNT: u16 = 131;
const GPU_RECORD_GENERATION_LOW: u16 = 20;
const GPU_RECORD_GENERATION_HIGH: u16 = 21;
const GPU_RECORD_TARGET_WORD: u16 = 22;
const GPU_RECORD_GLOBAL_WORD: u16 = 23;
const GPU_RECORD_TARGET_BLOCKS: u16 = 24;
const GPU_RECORD_GLOBAL_BLOCKS: u16 = 25;
const GPU_RECORD_INSTANCE_BLOCKS: u16 = 26;
const GPU_RECORD_COLOR_FACT: u16 = 27;
const GPU_RECORD_DEPTH_FACT: u16 = 28;
// Recording captures resource-plan positions; sealing turns them into a proof.
const GPU_RECORD_TARGET_PROVEN: u16 = 1024;
const GPU_RECORD_COPY_FACT: u16 = 104;
const GPU_RECORD_FACT_LIMIT: u16 = 64;
#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
enum ResourceFactKind {
    Mesh = 1,
    Texture = 2,
    Atlas = 3,
    Grid = 4,
    Color = 5,
    Depth = 6,
    Buffer = 7,
}

const GPU_FACT_READ: u16 = 1;
const GPU_FACT_WRITE: u16 = 2;

// Discriminants stored in the CPU recording array.
#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
enum RecordPhase {
    Empty = 0,
    Ready = 1,
    PassOpen = 2,
    Sealed = 3,
    Poisoned = 4,
}

fn gpu_record_data(slot: u16) -> Array<u16> {
    if slot == GPU_PREPARED_RECORD {
        return unsafe { Ptr::from_addr(GPU_PREPARED_CONTEXT).as_u16_array() };
    }
    unsafe {
        GPU_CB_RECORDS
            .as_ptr()
            .add((slot * GPU_API_CB_WORDS) as i16)
            .as_u16_array()
    }
}
fn gpu_record_generation_available() -> bool {
    GPU_RECORD_GENERATION[0u16] != 65535 || GPU_RECORD_GENERATION[1u16] != 65535
}
fn gpu_record_new_generation(mut c: Array<u16>) -> bool {
    if !gpu_record_generation_available() {
        return false;
    }
    GPU_RECORD_GENERATION[0u16] += 1;
    if GPU_RECORD_GENERATION[0u16] == 0 {
        GPU_RECORD_GENERATION[1u16] += 1;
    }
    c[GPU_RECORD_GENERATION_LOW] = GPU_RECORD_GENERATION[0u16];
    c[GPU_RECORD_GENERATION_HIGH] = GPU_RECORD_GENERATION[1u16];
    true
}
// Caller has validated slot; C reads exactly record.fact_count * 4 words.
fn gpu_record_resource_facts(slot: u16) -> Array<u16> {
    if slot == GPU_PREPARED_RECORD {
        return unsafe { Ptr::from_addr(GPU_PREPARED_CONTEXT + GPU_API_CB_WORDS).as_u16_array() };
    }
    unsafe {
        GPU_CB_RESOURCE_FACTS
            .as_ptr()
            .add((slot * 256) as i16)
            .as_u16_array()
    }
}
// Reserve each input's final table position without changing recorded facts.
// The caller retains this local plan until its native operation succeeds.
fn gpu_record_resource_room(
    slot: u16,
    input: Array<u16>,
    count: u16,
    mut plan: Array<u16>,
) -> bool {
    if slot >= GPU_RECORD_COUNT || count > GPU_RECORD_FACT_LIMIT {
        return false;
    }
    let c = gpu_record_data(slot);
    if (c[GPU_RECORD_PHASE] != RecordPhase::Ready as u16
        && c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16)
        || c[GPU_RECORD_FACT_COUNT] > GPU_RECORD_FACT_LIMIT
    {
        return false;
    }
    let facts = gpu_record_resource_facts(slot);
    let existing = c[GPU_RECORD_FACT_COUNT];
    let existing_end = existing * 4;
    let mut next = existing;
    let mut i: u16 = 0;
    while i < count {
        let at = i * 4;
        let first = input[at];
        let sectors = input[at + 1];
        let kind = input[at + 2];
        if !gpu_memory_extent_valid(first, sectors)
            || kind == 0
            || kind > ResourceFactKind::Buffer as u16
            || input[at + 3] == 0
            || input[at + 3] > 3
        {
            return false;
        }
        let mut found: u16 = 65535;
        let mut index: u16 = 0;
        while index < existing_end {
            if first == facts[index] && sectors == facts[index + 1] && kind == facts[index + 2] {
                found = index >> 2;
                break;
            }
            index += 4;
        }
        index = 0;
        while index < at && found == 65535 {
            if first == input[index] && sectors == input[index + 1] && kind == input[index + 2] {
                found = plan[index >> 2];
                break;
            }
            index += 4;
        }
        if found == 65535 {
            if next == GPU_RECORD_FACT_LIMIT {
                return false;
            }
            found = next;
            next += 1;
        }
        plan[i] = found;
        i += 1;
    }
    true
}
// No intervening fact mutation is permitted between reservation and commit.
fn gpu_record_resource_commit(slot: u16, input: Array<u16>, count: u16, plan: Array<u16>) {
    let mut c = gpu_record_data(slot);
    let mut facts = gpu_record_resource_facts(slot);
    let mut i: u16 = 0;
    while i < count {
        let at = i * 4;
        let found = plan[i];
        if found == c[GPU_RECORD_FACT_COUNT] {
            let mut j: u16 = 0;
            while j < 4 {
                facts[found * 4 + j] = input[at + j];
                j += 1;
            }
            c[GPU_RECORD_FACT_COUNT] += 1;
        } else {
            facts[found * 4 + 3] |= input[at + 3];
        }
        i += 1;
    }
}
fn gpu_record_init() {
    let mut i: u16 = 0;
    while i < GPU_API_CB_WORDS * 2 {
        GPU_CB_RECORDS[i] = 0;
        i += 1;
    }
}
fn gpu_record_extent(first: u16, sectors: u16) -> bool {
    sectors != 0 && sectors <= 512 && sectors - 1 <= 65535 - first
}
fn gpu_record_overlap(a: u16, an: u16, b: u16, bn: u16) -> bool {
    a <= b + bn - 1 && b <= a + an - 1
}
// Init-time only. Cross-CB/configuration and asset lifetime are checked by Gpu.
fn gpu_record_configure(
    slot: u16,
    command: u16,
    command_sectors: u16,
    push: u16,
    push_sectors: u16,
) -> bool {
    if slot >= GPU_RECORD_COUNT
        || (slot == GPU_PREPARED_RECORD && GPU_PREPARED_CONTEXT == 0)
        || !gpu_record_extent(command, command_sectors)
        || !gpu_record_extent(push, push_sectors)
        || gpu_record_overlap(command, command_sectors, push, push_sectors)
    {
        return false;
    }
    let mut c = gpu_record_data(slot);
    let mut i: u16 = 0;
    while i < GPU_API_CB_WORDS {
        c[i] = 0;
        i += 1;
    }
    c[GPU_RECORD_PHASE] = RecordPhase::Ready as u16;
    c[GPU_RECORD_COMMAND_BASE] = command;
    c[GPU_RECORD_COMMAND_CAPACITY] = command_sectors << 6;
    c[GPU_RECORD_PUSH_BASE] = push;
    c[GPU_RECORD_PUSH_CAPACITY] = push_sectors << 6;
    true
}
fn gpu_record_command_room(c: Array<u16>, add: u16, tail: u16) -> bool {
    let used = c[GPU_RECORD_COMMAND_USED];
    let capacity = c[GPU_RECORD_COMMAND_CAPACITY];
    if add & 1 != 0 || used > capacity || tail > capacity - used || add > capacity - used - tail {
        return false;
    }
    let end = used + add + tail;
    ((end + 63) & 65472) <= capacity
}
// 65535 is an internal failure sentinel; valid offsets include 32768.
fn gpu_record_align_push(used: u16, count: u16, alignment: u16, capacity: u16) -> u16 {
    if count > GPU_API_PUSH_CHUNK_WORDS || used > capacity || capacity > 32768 {
        return 65535;
    }
    if count == 0 {
        return used;
    }
    if alignment != 16 && alignment != 32 && alignment != 64 {
        return 65535;
    }
    let start = (used + alignment - 1) & (65535 - alignment + 1);
    if start > capacity || count > capacity - start {
        return 65535;
    }
    start
}
fn gpu_record_push_offset(slot: u16, count: u16, alignment: u16) -> u16 {
    if slot >= GPU_RECORD_COUNT {
        return 65535;
    }
    let c = gpu_record_data(slot);
    gpu_record_align_push(
        c[GPU_RECORD_PUSH_USED],
        count,
        alignment,
        c[GPU_RECORD_PUSH_CAPACITY],
    )
}
// The caller computes its complete aligned payload layout before its first store.
// This accepts a whole multi-batch operation, not just a 64-word transfer.
fn gpu_record_preflight(slot: u16, command_words: u16, push_end: u16) -> bool {
    if slot >= GPU_RECORD_COUNT {
        return false;
    }
    let c = gpu_record_data(slot);
    let phase = c[GPU_RECORD_PHASE];
    if phase != RecordPhase::Ready as u16 && phase != RecordPhase::PassOpen as u16 {
        return false;
    }
    let mut tail: u16 = 2;
    if phase == RecordPhase::PassOpen as u16 || c[GPU_RECORD_TERMINAL] == 0 {
        tail = 6;
    }
    // Templates have their own word limit and no enclosing Pass/END tail.
    if c[GPU_GROUP_RECORDING] != 0 {
        tail = 0;
    }
    gpu_record_command_room(c, command_words, tail)
        && push_end >= c[GPU_RECORD_PUSH_USED]
        && push_end <= c[GPU_RECORD_PUSH_CAPACITY]
}
fn gpu_record_begin_pass(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::Ready as u16 || !gpu_record_command_room(c, 0, 6) {
        return false;
    }
    c[GPU_RECORD_PHASE] = RecordPhase::PassOpen as u16;
    c[GPU_RECORD_TERMINAL] = 0;
    true
}
// Arrays are stable low-memory spans. Native encoders have already filled them.
fn gpu_record_append(
    slot: u16,
    commands: Array<u16>,
    command_words: u16,
    payload: Array<u16>,
    push_words: u16,
    alignment: u16,
) -> bool {
    if command_words > GPU_API_STORAGE_CHUNK_WORDS {
        return false;
    }
    let start = gpu_record_push_offset(slot, push_words, alignment);
    if start == 65535 || !gpu_record_preflight(slot, command_words, start + push_words) {
        return false;
    }
    gpu_record_append_reserved(slot, commands, command_words, payload, push_words, start)
}
// Consume a private operation's preflighted layout without another reservation.
// Callers preserve the phase, span and exclusive CPU ownership until completion.
fn gpu_record_append_reserved(
    slot: u16,
    commands: Array<u16>,
    command_words: u16,
    payload: Array<u16>,
    push_words: u16,
    start: u16,
) -> bool {
    #[cfg(gpu_debug)]
    {
        if command_words > GPU_API_STORAGE_CHUNK_WORDS
            || push_words > GPU_API_PUSH_CHUNK_WORDS
            || start > 32768
            || push_words > 32768 - start
            || !gpu_record_preflight(slot, command_words, start + push_words)
        {
            return false;
        }
        if start < gpu_record_data(slot)[GPU_RECORD_PUSH_USED] {
            return false;
        }
    }
    let mut c = gpu_record_data(slot);
    if !gpu_window_write_words(c[GPU_RECORD_PUSH_BASE], start, payload, push_words)
        || !gpu_window_write_words(
            c[GPU_RECORD_COMMAND_BASE],
            c[GPU_RECORD_COMMAND_USED],
            commands,
            command_words,
        )
    {
        // Partial transport is not a transaction rollback and cannot be sealed.
        c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
        return false;
    }
    c[GPU_RECORD_COMMAND_USED] += command_words;
    c[GPU_RECORD_PUSH_USED] = start + push_words;
    true
}
fn gpu_record_close_pass(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::PassOpen as u16
        || c[GPU_GROUP_RECORDING] != 0
        || !gpu_record_command_room(c, 4, 2)
    {
        return false;
    }
    let code: Buf<u16, 4> = Buf::new([3, 0x4000, 0, 0x7000]);
    if !gpu_window_write_words(
        c[GPU_RECORD_COMMAND_BASE],
        c[GPU_RECORD_COMMAND_USED],
        code.as_array(),
        4,
    ) {
        c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
        return false;
    }
    c[GPU_RECORD_COMMAND_USED] += 4;
    c[GPU_RECORD_PHASE] = RecordPhase::Ready as u16;
    c[GPU_RECORD_TERMINAL] = 1;
    true
}
fn gpu_record_seal(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] == RecordPhase::Sealed as u16 {
        return true;
    }
    let used = c[GPU_RECORD_COMMAND_USED];
    let terminal: u16 = if c[GPU_RECORD_TERMINAL] == 0 { 4 } else { 0 };
    if c[GPU_RECORD_PHASE] != RecordPhase::Ready as u16
        || used == 0
        || !gpu_record_command_room(c, terminal, 2)
    {
        return false;
    }
    let valid = used + terminal + 2;
    let padded = (valid + 63) & 65472;
    // Native ABI4: END opcode is 15 with zero payload; padding alone is zero.
    // A maintenance-only stream also needs the missing terminal render fence.
    let mut zero: Buf<u16, 64> = Buf::new([0; 64]);
    if terminal != 0 {
        zero[0u16] = 3;
        zero[1u16] = 0x4000;
        zero[3u16] = 0x7000;
    }
    zero[terminal + 1] = 0xf000;
    let mut count = padded - used;
    if count > 64 {
        count = 64;
    }
    if !gpu_window_write_words(c[GPU_RECORD_COMMAND_BASE], used, zero.as_array(), count) {
        c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
        return false;
    }
    if padded - used > count {
        let padding = unsafe { zero.as_ptr().add(6).as_u16_array() };
        if !gpu_window_write_words(
            c[GPU_RECORD_COMMAND_BASE],
            used + count,
            padding,
            padded - used - count,
        ) {
            c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
            return false;
        }
    }
    c[GPU_RECORD_COMMAND_USED] = valid;
    c[GPU_RECORD_PADDED] = padded;
    c[GPU_RECORD_PHASE] = RecordPhase::Sealed as u16;
    c[GPU_RECORD_TERMINAL] = 1;
    true
}
fn gpu_record_reset(slot: u16) -> bool {
    if slot >= 2 || !gpu_record_generation_available() {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != RecordPhase::Ready as u16
        && c[GPU_RECORD_PHASE] != RecordPhase::Sealed as u16
    {
        return false;
    }
    gpu_record_discard_list(slot);
    c[GPU_RECORD_PHASE] = RecordPhase::Ready as u16;
    let mut i: u16 = GPU_RECORD_COMMAND_USED;
    while i < GPU_API_CB_WORDS {
        c[i] = 0;
        i += 1;
    }
    c[GPU_CB_VIEW_WORD] = 0;
    c[GPU_CB_VIEW_BLOCKS] = 0;
    gpu_record_new_generation(c)
}
