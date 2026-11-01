//! CPU-owned command records. This module neither submits nor retires GPU work.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_storage;
use gpu_storage::*;

static GPU_CB_RECORDS: Buf<u16, 416> = Buf::new([0; 416]);
// The logical 256-word span stays line-aligned independently of static layout.
static GPU_NATIVE_SCRATCH: Buf<u16, 271> = Buf::new([0; 271]);
// Allocation facts survive descriptor Drop and execution until this CB resets.
static GPU_CB_CAPTURE_FACTS: Buf<u16, 512> = Buf::new([0; 512]);

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
const GPU_RECORD_COLOR: u16 = 9;
const GPU_RECORD_DEPTH: u16 = 10;
const GPU_RECORD_INSTANCE_WORD: u16 = 11;
const GPU_RECORD_UI_CLEAN: u16 = 12;
const GPU_RECORD_UI_WORD: u16 = 13;
const GPU_RECORD_TERMINAL: u16 = 14;
const GPU_RECORD_FACT_COUNT: u16 = 15;
const GPU_RECORD_FACT_LIMIT: u16 = 64;
const GPU_FACT_MESH: u16 = 1;
const GPU_FACT_TEXTURE: u16 = 2;
const GPU_FACT_ATLAS: u16 = 3;
const GPU_FACT_GRID: u16 = 4;
const GPU_FACT_COLOR: u16 = 5;
const GPU_FACT_DEPTH: u16 = 6;
const GPU_FACT_BUFFER: u16 = 7;
const GPU_FACT_READ: u16 = 1;
const GPU_FACT_WRITE: u16 = 2;
const GPU_RECORD_READY: u16 = 1;
const GPU_RECORD_PASS: u16 = 2;
const GPU_RECORD_SEALED: u16 = 3;
const GPU_RECORD_POISON: u16 = 4;

fn gpu_record_data(slot: u16) -> Array<u16> {
    unsafe {
        GPU_CB_RECORDS
            .as_ptr()
            .add((slot * GPU_API_CB_WORDS) as i16)
            .as_u16_array()
    }
}
// Caller has validated slot; C reads exactly record.fact_count * 4 words.
fn gpu_record_capture_facts(slot: u16) -> Array<u16> {
    unsafe {
        GPU_CB_CAPTURE_FACTS
            .as_ptr()
            .add((slot * 256) as i16)
            .as_u16_array()
    }
}
fn gpu_record_fact_equal(a: Array<u16>, ai: u16, b: Array<u16>, bi: u16) -> bool {
    a[ai] == b[bi] && a[ai + 1] == b[bi + 1] && a[ai + 2] == b[bi + 2]
}
// Whole-group preflight; no record/fact/payload mutation, including rejection.
fn gpu_record_capture_room(slot: u16, input: Array<u16>, count: u16) -> bool {
    if slot >= 2 || count > GPU_RECORD_FACT_LIMIT {
        return false;
    }
    let c = gpu_record_data(slot);
    if (c[GPU_RECORD_PHASE] != GPU_RECORD_READY && c[GPU_RECORD_PHASE] != GPU_RECORD_PASS)
        || c[GPU_RECORD_FACT_COUNT] > GPU_RECORD_FACT_LIMIT
    {
        return false;
    }
    let facts = gpu_record_capture_facts(slot);
    let mut extra: u16 = 0;
    let mut i: u16 = 0;
    while i < count {
        let at = i * 4;
        if !gpu_memory_extent_valid(input[at], input[at + 1])
            || input[at + 2] == 0
            || input[at + 2] > GPU_FACT_BUFFER
            || input[at + 3] == 0
            || input[at + 3] > 3
        {
            return false;
        }
        let mut found = false;
        let mut j: u16 = 0;
        while j < c[GPU_RECORD_FACT_COUNT] {
            if gpu_record_fact_equal(input, at, facts, j * 4) {
                found = true;
            }
            j += 1;
        }
        j = 0;
        while j < i {
            if gpu_record_fact_equal(input, at, input, j * 4) {
                found = true;
            }
            j += 1;
        }
        if !found {
            extra += 1;
        }
        i += 1;
    }
    extra <= GPU_RECORD_FACT_LIMIT - c[GPU_RECORD_FACT_COUNT]
}
// Only after successful group preflight/native append, without an intervening
// fact mutation. This commit is CPU-only and cannot fail or perform external I/O.
fn gpu_record_capture_commit(slot: u16, input: Array<u16>, count: u16) {
    let mut c = gpu_record_data(slot);
    let mut facts = gpu_record_capture_facts(slot);
    let mut i: u16 = 0;
    while i < count {
        let at = i * 4;
        let mut found: u16 = 65535;
        let mut j: u16 = 0;
        while j < c[GPU_RECORD_FACT_COUNT] {
            if gpu_record_fact_equal(input, at, facts, j * 4) {
                found = j;
            }
            j += 1;
        }
        if found == 65535 {
            found = c[GPU_RECORD_FACT_COUNT];
            j = 0;
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
    while i < 416 {
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
    if slot >= 2
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
    c[GPU_RECORD_PHASE] = GPU_RECORD_READY;
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
    if count > GPU_API_STORAGE_CHUNK_WORDS || used > capacity || capacity > 32768 {
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
    if slot >= 2 {
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
    if slot >= 2 {
        return false;
    }
    let c = gpu_record_data(slot);
    let phase = c[GPU_RECORD_PHASE];
    if phase != GPU_RECORD_READY && phase != GPU_RECORD_PASS {
        return false;
    }
    let mut tail: u16 = 2;
    if phase == GPU_RECORD_PASS || c[GPU_RECORD_TERMINAL] == 0 {
        tail = 6;
    }
    gpu_record_command_room(c, command_words, tail)
        && push_end >= c[GPU_RECORD_PUSH_USED]
        && push_end <= c[GPU_RECORD_PUSH_CAPACITY]
}
fn gpu_record_room(slot: u16, command_words: u16, push_words: u16, alignment: u16) -> bool {
    let start = gpu_record_push_offset(slot, push_words, alignment);
    if start == 65535 {
        return false;
    }
    gpu_record_preflight(slot, command_words, start + push_words)
}
fn gpu_record_begin_pass(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_READY || !gpu_record_command_room(c, 0, 6) {
        return false;
    }
    c[GPU_RECORD_PHASE] = GPU_RECORD_PASS;
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
    if command_words > GPU_API_STORAGE_CHUNK_WORDS
        || !gpu_record_room(slot, command_words, push_words, alignment)
    {
        return false;
    }
    let mut c = gpu_record_data(slot);
    let start = gpu_record_push_offset(slot, push_words, alignment);
    if !gpu_window_write_words(c[GPU_RECORD_PUSH_BASE], start, payload, push_words)
        || !gpu_window_write_words(
            c[GPU_RECORD_COMMAND_BASE],
            c[GPU_RECORD_COMMAND_USED],
            commands,
            command_words,
        )
    {
        // Partial transport is not a transaction rollback and cannot be sealed.
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
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
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS || !gpu_record_command_room(c, 4, 2) {
        return false;
    }
    let code: Buf<u16, 4> = Buf::new([3, 0x4000, 0, 0x7000]);
    if !gpu_window_write_words(
        c[GPU_RECORD_COMMAND_BASE],
        c[GPU_RECORD_COMMAND_USED],
        code.as_array(),
        4,
    ) {
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
        return false;
    }
    c[GPU_RECORD_COMMAND_USED] += 4;
    c[GPU_RECORD_PHASE] = GPU_RECORD_READY;
    c[GPU_RECORD_TERMINAL] = 1;
    true
}
fn gpu_record_seal(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] == GPU_RECORD_SEALED {
        return true;
    }
    let used = c[GPU_RECORD_COMMAND_USED];
    let mut terminal: u16 = 0;
    if c[GPU_RECORD_TERMINAL] == 0 {
        terminal = 4;
    }
    if c[GPU_RECORD_PHASE] != GPU_RECORD_READY
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
        c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
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
            c[GPU_RECORD_PHASE] = GPU_RECORD_POISON;
            return false;
        }
    }
    c[GPU_RECORD_COMMAND_USED] = valid;
    c[GPU_RECORD_PADDED] = padded;
    c[GPU_RECORD_PHASE] = GPU_RECORD_SEALED;
    c[GPU_RECORD_TERMINAL] = 1;
    true
}
fn gpu_record_reset(slot: u16) -> bool {
    if slot >= 2 {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_READY && c[GPU_RECORD_PHASE] != GPU_RECORD_SEALED {
        return false;
    }
    c[GPU_RECORD_PHASE] = GPU_RECORD_READY;
    let mut i: u16 = GPU_RECORD_COMMAND_USED;
    while i < GPU_API_CB_WORDS {
        c[i] = 0;
        i += 1;
    }
    true
}
