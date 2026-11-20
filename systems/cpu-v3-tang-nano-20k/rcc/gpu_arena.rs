//! Two-ended CPU arena with a fixed capacity per initialization.
//! Slots are stable word offsets plus one.
//! Allocation does not initialize payload; producers overwrite defined fields/padding.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_api_fault;
use gpu_api_fault::*;

// Aligned base, effective words, small/large high water, small/large free heads,
// live registration count, owning heap pointer and generation table base.
const GPU_OBJECT_LIVE: u16 = 6;
const GPU_OBJECT_STORAGE: u16 = 7;
const GPU_OBJECT_GENERATION_BASE: u16 = 8;
static GPU_OBJECT_CONTROL: Buf<u16, 9> = Buf::new([0; 9]);
// Zero marks a free slot. The serial is never reset, even across Gpu::init.
static GPU_OBJECT_SERIAL: Buf<u16, 2> = Buf::new([0; 2]);

#[derive(Copy, Clone)]
struct GpuObjectGeneration {
    low: u16,
    high: u16,
}
#[derive(Copy, Clone)]
struct GpuObjectHandle {
    slot: u16,
    generation: GpuObjectGeneration,
}

fn gpu_arena_empty() -> bool {
    GPU_OBJECT_CONTROL[GPU_OBJECT_LIVE] == 0
}
fn gpu_arena_init(bytes: u16) -> bool {
    if bytes == 0 || bytes > GPU_API_ARENA_BYTES || bytes & 31 != 0 {
        return false;
    }
    let mut c = GPU_OBJECT_CONTROL.as_array();
    let words = bytes >> 1;
    let slots = words >> 4;
    let mut metadata_words = slots * 2;
    #[cfg(gpu_debug)]
    {
        metadata_words += slots;
    }
    let previous = c[GPU_OBJECT_STORAGE];
    let mut storage = Ptr::from_addr(previous);
    if previous == 0 || words != c[1u16] {
        // Reserve alignment, payload and slot metadata in the same heap block.
        // Keep the old allocation until the replacement succeeds.
        storage = try_malloc(words + 15 + metadata_words);
        if storage.addr() == 0 {
            return false;
        }
    }
    let base = (storage.addr() + 15) & 65520;
    let generations = base + words;
    let mut metadata = unsafe { Ptr::from_addr(generations).as_u16_array() };
    let mut n: u16 = 0;
    while n < metadata_words {
        metadata[n] = 0;
        n += 1;
    }
    c[0u16] = base;
    c[1u16] = words;
    c[2u16] = 0;
    c[3u16] = 0;
    c[4u16] = 0;
    c[5u16] = 0;
    c[GPU_OBJECT_LIVE] = 0;
    c[GPU_OBJECT_STORAGE] = storage.addr();
    c[GPU_OBJECT_GENERATION_BASE] = generations;
    if previous != 0 && previous != storage.addr() {
        free(Ptr::from_addr(previous));
    }
    true
}
fn gpu_arena_generations() -> Array<u16> {
    unsafe { Ptr::from_addr(GPU_OBJECT_CONTROL[GPU_OBJECT_GENERATION_BASE]).as_u16_array() }
}
#[cfg(gpu_debug)]
fn gpu_arena_tags() -> Array<u16> {
    let base = GPU_OBJECT_CONTROL[GPU_OBJECT_GENERATION_BASE] + (GPU_OBJECT_CONTROL[1u16] >> 3);
    unsafe { Ptr::from_addr(base).as_u16_array() }
}
fn gpu_arena_data(slot: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(GPU_OBJECT_CONTROL[0u16] + slot - 1).as_u16_array() }
}
fn gpu_arena_handle(slot: u16) -> GpuObjectHandle {
    let at = (slot - 1) >> 3;
    let generations = gpu_arena_generations();
    GpuObjectHandle {
        slot: slot,
        generation: GpuObjectGeneration {
            low: generations[at],
            high: generations[at + 1],
        },
    }
}
fn gpu_arena_resolve(handle: GpuObjectHandle, kind: u16) -> u16 {
    let slot = handle.slot;
    if slot & 15 != 1 || slot > GPU_OBJECT_CONTROL[1u16] {
        gpu_api_handle_fault(
            slot,
            kind,
            handle.generation.low,
            handle.generation.high,
            0,
            0,
        );
        return 0;
    }
    let at = (slot - 1) >> 3;
    let generations = gpu_arena_generations();
    let low = generations[at];
    let high = generations[at + 1];
    let mut valid =
        (low | high) != 0 && low == handle.generation.low && high == handle.generation.high;
    #[cfg(gpu_debug)]
    {
        valid = valid && gpu_arena_tags()[at >> 1] & 255 == kind;
    }
    if !valid {
        gpu_api_handle_fault(
            slot,
            kind,
            handle.generation.low,
            handle.generation.high,
            low,
            high,
        );
    }
    slot
}
fn gpu_arena_alloc(large: bool, kind: u16) -> u16 {
    if GPU_OBJECT_SERIAL[0u16] == 65535 && GPU_OBJECT_SERIAL[1u16] == 65535 {
        return 0;
    }
    let mut c = GPU_OBJECT_CONTROL.as_array();
    let mut free_at: u16 = 4;
    let mut words = GPU_API_SMALL_WORDS;
    if large {
        free_at = 5;
        words = GPU_API_LARGE_WORDS;
    }
    let mut handle = c[free_at];
    if handle != 0 {
        c[free_at] = gpu_arena_data(handle)[0u16];
    } else {
        let used = c[2u16] * GPU_API_SMALL_WORDS + c[3u16] * GPU_API_LARGE_WORDS;
        if used > c[1u16] || words > c[1u16] - used {
            return 0;
        }
        if large {
            c[3u16] += 1;
            handle = c[1u16] - c[3u16] * GPU_API_LARGE_WORDS + 1;
        } else {
            handle = c[2u16] * GPU_API_SMALL_WORDS + 1;
            c[2u16] += 1;
        }
    }
    let low = GPU_OBJECT_SERIAL[0u16] + 1;
    if low == 0 {
        GPU_OBJECT_SERIAL[1u16] += 1;
    }
    GPU_OBJECT_SERIAL[0u16] = low;
    let at = (handle - 1) >> 3;
    let mut generations = gpu_arena_generations();
    generations[at] = low;
    generations[at + 1] = GPU_OBJECT_SERIAL[1u16];
    c[GPU_OBJECT_LIVE] += 1;
    #[cfg(gpu_debug)]
    {
        let mut tag = kind;
        if large {
            tag |= 256;
        }
        let mut tags = gpu_arena_tags();
        tags[(handle - 1) >> 4] = tag;
    }
    handle
}
fn gpu_arena_free(handle: u16, large: bool, kind: u16) -> bool {
    if handle == 0 || handle - 1 >= GPU_OBJECT_CONTROL[1u16] || (handle - 1) & 15 != 0 {
        return false;
    }
    let at = (handle - 1) >> 3;
    let mut generations = gpu_arena_generations();
    if (generations[at] | generations[at + 1]) == 0 {
        return false;
    }
    #[cfg(gpu_debug)]
    {
        let index = (handle - 1) >> 4;
        let mut tags = gpu_arena_tags();
        let mut tag = kind;
        if large {
            tag |= 256;
        }
        if tags[index] != tag {
            return false;
        }
        tags[index] = 0;
    }
    let mut free_at: u16 = 4;
    if large {
        free_at = 5;
    }
    generations[at] = 0;
    generations[at + 1] = 0;
    GPU_OBJECT_CONTROL[GPU_OBJECT_LIVE] -= 1;
    gpu_arena_data(handle)[0u16] = GPU_OBJECT_CONTROL[free_at];
    GPU_OBJECT_CONTROL[free_at] = handle;
    true
}
