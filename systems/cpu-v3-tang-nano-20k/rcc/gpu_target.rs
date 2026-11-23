//! Target-only relocation of sealed frame streams. No START or publication.
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
mod gpu_arena;
use gpu_arena::*;
mod gpu_memcpy;
use gpu_memcpy::*;
mod gpu_present;
use gpu_present::*;

fn gpu_target_span(c: Array<u16>, at: u16, words: u16) -> bool {
    at != 0
        && at - 1 <= c[GPU_RECORD_PUSH_USED]
        && words <= c[GPU_RECORD_PUSH_USED] - (at - 1)
        && c[GPU_RECORD_PUSH_USED] <= c[GPU_RECORD_PUSH_CAPACITY]
}
fn gpu_target_owned(slot: u16) -> bool {
    if slot >= 2 || GPU_EXECUTION.owner == slot + 1 {
        return false;
    }
    let c = gpu_record_data(slot);
    c[GPU_RECORD_PHASE] == RecordPhase::Ready as u16
        || c[GPU_RECORD_PHASE] == RecordPhase::Sealed as u16
}
fn gpu_target_targets(slot: u16, color: Array<u16>, depth: Array<u16>) -> bool {
    let c = gpu_record_data(slot);
    let facts = gpu_record_resource_facts(slot);
    let ci = c[GPU_RECORD_COLOR_FACT];
    let di = c[GPU_RECORD_DEPTH_FACT];
    #[cfg(gpu_debug)]
    {
        if !gpu_target_facts_valid(slot) {
            return false;
        }
    }
    // Public surface handles already establish alignment and valid extents.
    // Allocation sizes are part of the sealed resource contract; addresses may vary.
    if facts[(ci - 1) * 4 + 1] != color[1u16]
        || facts[(di - 1) * 4 + 1] != depth[1u16]
        || gpu_record_overlap(color[0u16], color[1u16], depth[0u16], depth[1u16])
    {
        return false;
    }
    let mut i: u16 = 0;
    while i < GPU_EXECUTION.configured_slots {
        let other = gpu_record_data(i);
        if gpu_record_overlap(
            color[0u16],
            color[1u16],
            other[GPU_RECORD_COMMAND_BASE],
            other[GPU_RECORD_COMMAND_CAPACITY] >> 6,
        ) || gpu_record_overlap(
            color[0u16],
            color[1u16],
            other[GPU_RECORD_PUSH_BASE],
            other[GPU_RECORD_PUSH_CAPACITY] >> 6,
        ) || gpu_record_overlap(
            depth[0u16],
            depth[1u16],
            other[GPU_RECORD_COMMAND_BASE],
            other[GPU_RECORD_COMMAND_CAPACITY] >> 6,
        ) || gpu_record_overlap(
            depth[0u16],
            depth[1u16],
            other[GPU_RECORD_PUSH_BASE],
            other[GPU_RECORD_PUSH_CAPACITY] >> 6,
        ) {
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
fn gpu_target_shape(c: Array<u16>) -> bool {
    if c[GPU_RECORD_TARGET_BLOCKS] != 1
        || c[GPU_RECORD_UI_WORD] != 0
        || (c[GPU_RECORD_GENERATION_LOW] | c[GPU_RECORD_GENERATION_HIGH]) == 0
        || !gpu_target_span(c, c[GPU_RECORD_TARGET_WORD], 16)
        || c[GPU_RECORD_COPY_COUNT] > 1
    {
        return false;
    }
    if c[GPU_RECORD_COPY_COUNT] == 0 {
        return c[GPU_RECORD_VALID] & GPU_RECORD_HAS_COPY == 0;
    }
    c[GPU_RECORD_VALID] & GPU_RECORD_HAS_COPY != 0
        && gpu_target_span(c, c[GPU_RECORD_COPY_WORD], 16)
        && c[GPU_RECORD_COPY_WORD] < c[GPU_RECORD_TARGET_WORD]
        && c[GPU_RECORD_TARGET_WORD] - c[GPU_RECORD_COPY_WORD] >= 16
        && c[GPU_RECORD_COPY_DESTINATION] == c[GPU_RECORD_COLOR]
        && gpu_memcpy_native_valid(
            c[GPU_RECORD_COPY_SOURCE],
            c[GPU_RECORD_COLOR],
            GPU_API_PLANE_SECTORS,
        )
}
// Record-time plan indices are stable: resource commit only appends or merges.
// The only post-seal mutation replaces the two target addresses at these indices.
#[cfg(gpu_debug)]
fn gpu_target_facts_valid(slot: u16) -> bool {
    let c = gpu_record_data(slot);
    let facts = gpu_record_resource_facts(slot);
    let ci = c[GPU_RECORD_COLOR_FACT];
    let di = c[GPU_RECORD_DEPTH_FACT];
    let count = c[GPU_RECORD_FACT_COUNT];
    if count > GPU_RECORD_FACT_LIMIT || ci == 0 || di == 0 || ci > count || di > count {
        return false;
    }
    let color = (ci - 1) * 4;
    let depth = (di - 1) * 4;
    if facts[color] != c[GPU_RECORD_COLOR]
        || facts[color + 2] != ResourceFactKind::Color as u16
        || facts[color + 3] != 3
        || facts[depth] != c[GPU_RECORD_DEPTH]
        || facts[depth + 2] != ResourceFactKind::Depth as u16
        || facts[depth + 3] != 3
    {
        return false;
    }
    if c[GPU_RECORD_COPY_COUNT] != 0 {
        let source = c[GPU_RECORD_COPY_FACT];
        if source == 0 || source > count {
            return false;
        }
        let at = (source - 1) * 4;
        if facts[at] != c[GPU_RECORD_COPY_SOURCE]
            || facts[at + 1] < GPU_API_PLANE_SECTORS
            || facts[at + 2] != ResourceFactKind::Color as u16
            || facts[at + 3] != GPU_FACT_READ
        {
            return false;
        }
    }
    true
}
fn gpu_target_proven(c: Array<u16>) -> bool {
    c[GPU_RECORD_PHASE] == RecordPhase::Sealed as u16
        && c[GPU_RECORD_VALID] & GPU_RECORD_TARGET_PROVEN != 0
}
fn gpu_target_binding(slot: u16) -> bool {
    if !gpu_target_owned(slot) {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if !gpu_target_proven(c) {
        // Eligibility is API behavior, not a debug assertion. A later copy to
        // another plane or a second Pass is not a relocatable target stream.
        if c[GPU_RECORD_TARGET_BLOCKS] != 1
            || c[GPU_RECORD_UI_WORD] != 0
            || c[GPU_RECORD_COPY_COUNT] > 1
        {
            return false;
        }
        if c[GPU_RECORD_COPY_COUNT] != 0
            && (c[GPU_RECORD_COPY_FACT] == 0
                || c[GPU_RECORD_COPY_DESTINATION] != c[GPU_RECORD_COLOR]
                || c[GPU_RECORD_COPY_WORD] >= c[GPU_RECORD_TARGET_WORD])
        {
            return false;
        }
        #[cfg(gpu_debug)]
        {
            if !gpu_target_shape(c) || !gpu_target_facts_valid(slot) {
                return false;
            }
        }
        if !gpu_record_seal(slot) {
            return false;
        }
        c[GPU_RECORD_VALID] |= GPU_RECORD_TARGET_PROVEN;
    }
    true
}
// Observation only: no poll/retire, no waiting and no sticky Display mutation.
fn gpu_target_front_safe(color: Array<u16>, depth: Array<u16>) -> bool {
    let snapshot: Buf<u16, 4> = Buf::new([0; 4]);
    let status = gpu_present_observe(snapshot.as_array());
    if status == PresentObservation::Fault || status == PresentObservation::Retry {
        return false;
    }
    if status == PresentObservation::Stable {
        if GPU_PRESENT.pending != 0 || snapshot[0u16] != 0 {
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
fn gpu_target_retarget(
    slot: u16,
    low: u16,
    high: u16,
    color: Array<u16>,
    depth: Array<u16>,
) -> bool {
    if !gpu_target_owned(slot) {
        return false;
    }
    let c = gpu_record_data(slot);
    if !gpu_target_proven(c)
        || c[GPU_RECORD_GENERATION_LOW] != low
        || c[GPU_RECORD_GENERATION_HIGH] != high
    {
        return false;
    }
    #[cfg(gpu_debug)]
    {
        if !gpu_target_shape(c) {
            return false;
        }
    }
    if !gpu_target_targets(slot, color, depth) || !gpu_target_front_safe(color, depth) {
        return false;
    }
    gpu_target_write(slot, color, depth)
}

// Call only after validating the sealed recording and replacement extents.
fn gpu_target_write(slot: u16, color: Array<u16>, depth: Array<u16>) -> bool {
    let mut c = gpu_record_data(slot);
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
        c[GPU_RECORD_PHASE] = RecordPhase::Poisoned as u16;
        return false;
    }
    let mut facts = gpu_record_resource_facts(slot);
    facts[(c[GPU_RECORD_COLOR_FACT] - 1) * 4] = color[0u16];
    facts[(c[GPU_RECORD_DEPTH_FACT] - 1) * 4] = depth[0u16];
    c[GPU_RECORD_COLOR] = color[0u16];
    c[GPU_RECORD_DEPTH] = depth[0u16];
    if c[GPU_RECORD_COPY_COUNT] != 0 {
        c[GPU_RECORD_COPY_DESTINATION] = color[0u16];
    }
    true
}
