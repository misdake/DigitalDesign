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
#[repr(C)]
struct GpuArenaControl {
    base: u16,
    capacity: u16,
    small_used: u16,
    large_used: u16,
    free_heads: Buf<u16, 2>,
    live: u16,
    storage: u16,
    generations: u16,
}
static GPU_OBJECT_CONTROL: GpuArenaControl = GpuArenaControl {
    base: 0,
    capacity: 0,
    small_used: 0,
    large_used: 0,
    free_heads: Buf::new([0; 2]),
    live: 0,
    storage: 0,
    generations: 0,
};
// Zero marks a free slot. The serial is never reset, even across Gpu::init.
static GPU_OBJECT_SERIAL: GpuObjectGeneration = GpuObjectGeneration { low: 0, high: 0 };

#[repr(C)]
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
    GPU_OBJECT_CONTROL.live == 0
}
fn gpu_arena_init(bytes: u16) -> bool {
    if bytes == 0 || bytes > GPU_API_ARENA_BYTES || bytes & 31 != 0 {
        return false;
    }
    let words = bytes >> 1;
    let slots = words >> 4;
    let mut metadata_words = slots * 2;
    #[cfg(gpu_debug)]
    {
        metadata_words += slots;
    }
    let previous = GPU_OBJECT_CONTROL.storage;
    let mut storage = Ptr::from_addr(previous);
    if previous == 0 || words != GPU_OBJECT_CONTROL.capacity {
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
    GPU_OBJECT_CONTROL.base = base;
    GPU_OBJECT_CONTROL.capacity = words;
    GPU_OBJECT_CONTROL.small_used = 0;
    GPU_OBJECT_CONTROL.large_used = 0;
    GPU_OBJECT_CONTROL.free_heads[0u16] = 0;
    GPU_OBJECT_CONTROL.free_heads[1u16] = 0;
    GPU_OBJECT_CONTROL.live = 0;
    GPU_OBJECT_CONTROL.storage = storage.addr();
    GPU_OBJECT_CONTROL.generations = generations;
    if previous != 0 && previous != storage.addr() {
        free(Ptr::from_addr(previous));
    }
    true
}
fn gpu_arena_generations() -> Array<u16> {
    unsafe { Ptr::from_addr(GPU_OBJECT_CONTROL.generations).as_u16_array() }
}
#[cfg(gpu_debug)]
fn gpu_arena_tags() -> Array<u16> {
    let base = GPU_OBJECT_CONTROL.generations + (GPU_OBJECT_CONTROL.capacity >> 3);
    unsafe { Ptr::from_addr(base).as_u16_array() }
}
fn gpu_arena_data(slot: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(GPU_OBJECT_CONTROL.base + slot - 1).as_u16_array() }
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
fn gpu_arena_resolve(handle: GpuObjectHandle, kind: GpuObjectKind) -> u16 {
    let slot = handle.slot;
    if slot & 15 != 1 || slot > GPU_OBJECT_CONTROL.capacity {
        gpu_api_handle_fault(
            slot,
            kind as u16,
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
        valid = valid && gpu_arena_tags()[at >> 1] & 255 == kind as u16;
    }
    if !valid {
        gpu_api_handle_fault(
            slot,
            kind as u16,
            handle.generation.low,
            handle.generation.high,
            low,
            high,
        );
    }
    slot
}
fn gpu_arena_alloc(large: bool, kind: GpuObjectKind) -> u16 {
    if GPU_OBJECT_SERIAL.low == 65535 && GPU_OBJECT_SERIAL.high == 65535 {
        return 0;
    }
    let mut pool: u16 = 0;
    let mut words = GPU_API_SMALL_WORDS;
    if large {
        pool = 1;
        words = GPU_API_LARGE_WORDS;
    }
    let mut handle = GPU_OBJECT_CONTROL.free_heads[pool];
    if handle != 0 {
        GPU_OBJECT_CONTROL.free_heads[pool] = gpu_arena_data(handle)[0u16];
    } else {
        let used = GPU_OBJECT_CONTROL.small_used * GPU_API_SMALL_WORDS
            + GPU_OBJECT_CONTROL.large_used * GPU_API_LARGE_WORDS;
        if used > GPU_OBJECT_CONTROL.capacity || words > GPU_OBJECT_CONTROL.capacity - used {
            return 0;
        }
        if large {
            GPU_OBJECT_CONTROL.large_used += 1;
            handle = GPU_OBJECT_CONTROL.capacity
                - GPU_OBJECT_CONTROL.large_used * GPU_API_LARGE_WORDS
                + 1;
        } else {
            handle = GPU_OBJECT_CONTROL.small_used * GPU_API_SMALL_WORDS + 1;
            GPU_OBJECT_CONTROL.small_used += 1;
        }
    }
    let low = GPU_OBJECT_SERIAL.low + 1;
    if low == 0 {
        GPU_OBJECT_SERIAL.high += 1;
    }
    GPU_OBJECT_SERIAL.low = low;
    let at = (handle - 1) >> 3;
    let mut generations = gpu_arena_generations();
    generations[at] = low;
    generations[at + 1] = GPU_OBJECT_SERIAL.high;
    GPU_OBJECT_CONTROL.live += 1;
    #[cfg(gpu_debug)]
    {
        let mut tag = kind as u16;
        if large {
            tag |= 256;
        }
        let mut tags = gpu_arena_tags();
        tags[(handle - 1) >> 4] = tag;
    }
    handle
}
fn gpu_arena_free(handle: u16, large: bool, kind: GpuObjectKind) -> bool {
    if handle == 0 || handle - 1 >= GPU_OBJECT_CONTROL.capacity || (handle - 1) & 15 != 0 {
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
        let mut tag = kind as u16;
        if large {
            tag |= 256;
        }
        if tags[index] != tag {
            return false;
        }
        tags[index] = 0;
    }
    let mut pool: u16 = 0;
    if large {
        pool = 1;
    }
    generations[at] = 0;
    generations[at + 1] = 0;
    GPU_OBJECT_CONTROL.live -= 1;
    gpu_arena_data(handle)[0u16] = GPU_OBJECT_CONTROL.free_heads[pool];
    GPU_OBJECT_CONTROL.free_heads[pool] = handle;
    true
}
