//! Minimal fixed-storage CB: object snapshots and a complete Clear pass.
use crate::dsl_rt::*;
mod gpu_encode;
use gpu_encode::*;
mod gpu_memory;
use gpu_memory::*;
mod gpu_window;
use gpu_window::*;
mod gpu_layout;
use gpu_layout::*;
mod gpu_builder;
use gpu_builder::*;
static GPU_SLOT_OWNERS: Buf<u16, 2> = Buf::new([0; 2]);
// Target object word + 1. Zero means no relocation was recorded by the builder.
static GPU_SLOT_TARGETS: Buf<u16, 2> = Buf::new([0; 2]);

pub const GPU_CB_EMPTY: u16 = 0;
pub const GPU_CB_BUILDING: u16 = 1;
pub const GPU_CB_READY: u16 = 2;
pub const GPU_CB_INFLIGHT: u16 = 3;
pub const GPU_CB_COMPLETED: u16 = 4;
// Published input held by the driver, before either Device4 SEND.
pub const GPU_CB_PREPARED: u16 = 5;
pub struct GpuBuffer {
    pub high_slot: u16,
    pub commands: GpuStorage,
    pub objects: GpuStorage,
    pub command_words: u16,
    pub object_words: u16,
    pub state: u16,
    pub color: u16,
    pub depth: u16,
}
pub fn gpu_buffer_empty() -> GpuBuffer {
    GpuBuffer {
        commands: GpuStorage {
            cpu: Ptr::from_addr(0),
            sector: 0,
            sectors: 0,
        },
        objects: GpuStorage {
            cpu: Ptr::from_addr(0),
            sector: 0,
            sectors: 0,
        },
        command_words: 0,
        object_words: 0,
        state: 0,
        color: 0,
        depth: 0,
        high_slot: 0,
    }
}
pub fn gpu_buffer_init(
    mut cb: Array<GpuBuffer>,
    commands: Array<GpuStorage>,
    objects: Array<GpuStorage>,
) -> bool {
    if cb[0u16].state != GPU_CB_EMPTY
        || !gpu_storage_valid(commands)
        || !gpu_storage_valid(objects)
        || gpu_extents_overlap(
            commands[0u16].sector,
            commands[0u16].sectors,
            objects[0u16].sector,
            objects[0u16].sectors,
        )
        || gpu_extents_overlap(
            commands[0u16].cpu.addr() >> 6,
            commands[0u16].sectors,
            objects[0u16].cpu.addr() >> 6,
            objects[0u16].sectors,
        )
    {
        return false;
    }
    cb[0u16].high_slot = 0;
    cb[0u16].commands = commands[0u16];
    cb[0u16].objects = objects[0u16];
    cb[0u16].command_words = 0;
    cb[0u16].object_words = 0;
    cb[0u16].color = 0;
    cb[0u16].depth = 0;
    cb[0u16].state = GPU_CB_BUILDING;
    true
}
pub fn gpu_high_slot_init(mut cb: Array<GpuBuffer>, slot: u16) -> bool {
    if slot >= GPU_INPUT_SLOTS
        || cb[0u16].state != GPU_CB_EMPTY
        || cb.as_ptr().addr() == 0
        || gpu_memory_faulted()
    {
        return false;
    }
    let mut owners = GPU_SLOT_OWNERS.as_array();
    if owners[slot] != 0 {
        return false;
    }
    let c = gpu_slot_commands(slot);
    let o = gpu_slot_objects(slot);
    cb[0u16].commands = GpuStorage {
        cpu: Ptr::from_addr(0),
        sector: c,
        sectors: GPU_COMMAND_CAPACITY_SECTORS,
    };
    cb[0u16].objects = GpuStorage {
        cpu: Ptr::from_addr(0),
        sector: o,
        sectors: GPU_OBJECT_CAPACITY_SECTORS,
    };
    cb[0u16].command_words = 0;
    cb[0u16].object_words = 0;
    cb[0u16].color = 0;
    cb[0u16].depth = 0;
    cb[0u16].state = GPU_CB_BUILDING;
    owners[slot] = cb.as_ptr().addr();
    GPU_SLOT_TARGETS.as_array()[slot] = 0;
    cb[0u16].high_slot = slot + 1;
    true
}
fn gpu_high_slot_valid(cb: Array<GpuBuffer>) -> bool {
    if cb[0u16].high_slot == 0 || cb[0u16].high_slot > GPU_INPUT_SLOTS {
        return false;
    }
    if cb[0u16].commands.cpu.addr() != 0 || cb[0u16].objects.cpu.addr() != 0 {
        return false;
    }
    let slot = cb[0u16].high_slot - 1;

    let c = gpu_slot_commands(slot);
    let o = gpu_slot_objects(slot);
    GPU_SLOT_OWNERS.as_array()[slot] == cb.as_ptr().addr()
        && cb[0u16].high_slot == slot + 1
        && cb[0u16].commands.sector == c
        && cb[0u16].objects.sector == o
        && cb[0u16].commands.sectors == GPU_COMMAND_CAPACITY_SECTORS
        && cb[0u16].objects.sectors == GPU_OBJECT_CAPACITY_SECTORS
}
fn gpu_slot_commands(slot: u16) -> u16 {
    if slot == 0 {
        return GPU_COMMAND_FIRST_SECTOR;
    }
    GPU_SLOT1_COMMAND_FIRST_SECTOR
}
fn gpu_slot_objects(slot: u16) -> u16 {
    if slot == 0 {
        return GPU_OBJECT_FIRST_SECTOR;
    }
    GPU_SLOT1_OBJECT_FIRST_SECTOR
}
// Internal relocation only: Ready inputs are still CPU-owned/unpublished.
// The scheduler reserves the real Writable plane before calling this, and the
// driver performs the final cache handoff after this write, before either SEND.
fn gpu_slot_bind_target(mut cb: Array<GpuBuffer>, color: u16) -> bool {
    if cb[0u16].state != GPU_CB_READY
        || !gpu_high_slot_valid(cb)
        || (color != GPU_COLOR_A_SECTOR && color != GPU_COLOR_B_SECTOR)
        || gpu_memory_faulted()
    {
        return false;
    }
    let patch = GPU_SLOT_TARGETS.as_array()[cb[0u16].high_slot - 1];
    if patch == 0 || patch - 1 > cb[0u16].object_words || cb[0u16].object_words - (patch - 1) < 16 {
        return false;
    }
    let mut storage = gpu_encoding_scratch(GPU_ENCODING.as_array());
    if !gpu_encode_target(view_of(&mut storage), 0, color, GPU_DEPTH_SECTOR)
        || !gpu_window_record_pair(
            unsafe { storage.cpu.as_u16_array() },
            16,
            unsafe { storage.cpu.as_u16_array() },
            0,
            cb[0u16].objects.sector,
            patch - 1,
        )
    {
        return false;
    }
    cb[0u16].color = color;
    true
}
pub fn gpu_slot_upload(
    cb: Array<GpuBuffer>,
    objects: bool,
    word: u16,
    source: Array<u16>,
    count: u16,
) -> bool {
    if cb[0u16].state != GPU_CB_BUILDING
        || !gpu_high_slot_valid(cb)
        || word > 32768
        || count > 32768 - word
    {
        return false;
    }
    let base = if objects {
        cb[0u16].objects.sector
    } else {
        cb[0u16].commands.sector
    };
    gpu_window_copy(source, base, word, count, false)
}
pub fn gpu_slot_download(
    cb: Array<GpuBuffer>,
    objects: bool,
    word: u16,
    destination: Array<u16>,
    count: u16,
) -> bool {
    if (cb[0u16].state != GPU_CB_BUILDING && cb[0u16].state != GPU_CB_COMPLETED)
        || !gpu_high_slot_valid(cb)
        || word > 32768
        || count > 32768 - word
    {
        return false;
    }
    let base = if objects {
        cb[0u16].objects.sector
    } else {
        cb[0u16].commands.sector
    };
    gpu_window_copy(destination, base, word, count, true)
}
pub fn gpu_buffer_reset(mut cb: Array<GpuBuffer>) -> bool {
    if cb[0u16].state != GPU_CB_COMPLETED {
        return false;
    }
    cb[0u16].command_words = 0;
    cb[0u16].object_words = 0;
    cb[0u16].state = GPU_CB_BUILDING;
    true
}
pub fn gpu_record_global(mut cb: Array<GpuBuffer>, g: Array<GpuGlobal>) -> bool {
    if cb[0u16].high_slot != 0 {
        return gpu_record_high_global(cb, g);
    }
    if cb[0u16].state != GPU_CB_BUILDING {
        return false;
    }
    let mut s: GpuStorage = cb[0u16].objects;
    let cursor = cb[0u16].object_words;
    if cursor > 32736 {
        return false;
    }
    let aligned = (cursor + 31) & 65504;
    if !gpu_encode_global(view_of(&mut s), aligned, g) {
        return false;
    }
    cb[0u16].object_words = aligned + 32;
    true
}
pub fn gpu_record_render(mut cb: Array<GpuBuffer>, r: Array<GpuRender>) -> bool {
    if cb[0u16].high_slot != 0 {
        return gpu_record_high_render(cb, r);
    }
    if cb[0u16].state != GPU_CB_BUILDING {
        return false;
    }
    let mut s: GpuStorage = cb[0u16].objects;
    let cursor = cb[0u16].object_words;
    if cursor > 32752 {
        return false;
    }
    let aligned = (cursor + 15) & 65520;
    if !gpu_encode_render(view_of(&mut s), aligned, r) {
        return false;
    }
    cb[0u16].object_words = aligned + 16;
    true
}
pub fn gpu_record_instance(
    mut cb: Array<GpuBuffer>,
    matrix: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
) -> bool {
    if cb[0u16].high_slot != 0 {
        return gpu_record_high_instance(cb, matrix, rgba_low, rgba_high);
    }
    if cb[0u16].state != GPU_CB_BUILDING {
        return false;
    }
    let mut s: GpuStorage = cb[0u16].objects;
    let cursor = cb[0u16].object_words;
    if cursor > 32704 {
        return false;
    }
    let aligned = (cursor + 63) & 65472;
    if !gpu_encode_instance(view_of(&mut s), aligned, matrix, rgba_low, rgba_high) {
        return false;
    }
    cb[0u16].object_words = aligned + 64;
    true
}
pub fn gpu_buffer_valid(cb: Array<GpuBuffer>) -> bool {
    if cb[0u16].high_slot != 0 {
        return gpu_high_slot_valid(cb)
            && cb[0u16].command_words >= 20
            && cb[0u16].command_words <= 32768
            && cb[0u16].command_words & 1 == 0
            && cb[0u16].object_words >= 16
            && cb[0u16].object_words <= 32768
            && (cb[0u16].color == GPU_COLOR_A_SECTOR || cb[0u16].color == GPU_COLOR_B_SECTOR)
            && cb[0u16].depth == GPU_DEPTH_SECTOR;
    }
    let mut c: GpuStorage = cb[0u16].commands;
    let mut o: GpuStorage = cb[0u16].objects;
    if !gpu_storage_valid(view_of(&mut c))
        || !gpu_storage_valid(view_of(&mut o))
        || cb[0u16].command_words == 0
        || cb[0u16].command_words > 32768
        || cb[0u16].command_words & 1 != 0
        || cb[0u16].command_words > c.sectors << 6
        || cb[0u16].object_words > o.sectors << 6
        || !gpu_target_valid(cb[0u16].color, cb[0u16].depth)
    {
        return false;
    }
    // Entire allocations, including padding, are disjoint from both planes.
    if gpu_extents_overlap(c.sector, c.sectors, o.sector, o.sectors)
        || gpu_extents_overlap(c.sector, c.sectors, cb[0u16].color, GPU_PLANE_SECTORS)
        || gpu_extents_overlap(c.sector, c.sectors, cb[0u16].depth, GPU_PLANE_SECTORS)
        || gpu_extents_overlap(o.sector, o.sectors, cb[0u16].color, GPU_PLANE_SECTORS)
        || gpu_extents_overlap(o.sector, o.sectors, cb[0u16].depth, GPU_PLANE_SECTORS)
    {
        return false;
    }
    true
}
pub fn gpu_build_clear(cb: Array<GpuBuffer>, color: u16, depth: u16, cv: u16, dv: u16) -> bool {
    gpu_builder_begin(cb, color, depth, cv, dv) && gpu_builder_end(cb) && gpu_builder_finish(cb)
}

fn gpu_encoding_scratch(raw: Array<u16>) -> GpuStorage {
    let aligned = (raw.as_ptr().addr() + 63) & 65472;
    GpuStorage {
        cpu: Ptr::from_addr(aligned),
        sector: aligned >> 6,
        sectors: 1,
    }
}
fn gpu_record_high_global(mut cb: Array<GpuBuffer>, g: Array<GpuGlobal>) -> bool {
    if cb[0u16].state != GPU_CB_BUILDING
        || !gpu_high_slot_valid(cb)
        || cb[0u16].object_words > 32736
    {
        return false;
    }
    let word = (cb[0u16].object_words + 31) & 65504;
    let mut raw: Buf<u16, 127> = Buf::new([0; 127]);
    let mut s: GpuStorage = gpu_encoding_scratch(raw.as_array());
    if !gpu_encode_global(view_of(&mut s), 0, g) {
        return false;
    }
    if !gpu_slot_upload(cb, true, word, unsafe { s.cpu.as_u16_array() }, 32) {
        return false;
    }
    cb[0u16].object_words = word + 32;
    true
}
fn gpu_record_high_render(mut cb: Array<GpuBuffer>, r: Array<GpuRender>) -> bool {
    if cb[0u16].state != GPU_CB_BUILDING
        || !gpu_high_slot_valid(cb)
        || cb[0u16].object_words > 32752
    {
        return false;
    }
    let word = (cb[0u16].object_words + 15) & 65520;
    let mut raw: Buf<u16, 127> = Buf::new([0; 127]);
    let mut s: GpuStorage = gpu_encoding_scratch(raw.as_array());
    if !gpu_encode_render(view_of(&mut s), 0, r) {
        return false;
    }
    if !gpu_slot_upload(cb, true, word, unsafe { s.cpu.as_u16_array() }, 16) {
        return false;
    }
    cb[0u16].object_words = word + 16;
    true
}
fn gpu_record_high_instance(
    mut cb: Array<GpuBuffer>,
    m: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
) -> bool {
    if cb[0u16].state != GPU_CB_BUILDING
        || !gpu_high_slot_valid(cb)
        || cb[0u16].object_words > 32704
    {
        return false;
    }
    let word = (cb[0u16].object_words + 63) & 65472;
    let mut raw: Buf<u16, 127> = Buf::new([0; 127]);
    let mut s: GpuStorage = gpu_encoding_scratch(raw.as_array());
    if !gpu_encode_instance(view_of(&mut s), 0, m, rgba_low, rgba_high) {
        return false;
    }
    if !gpu_slot_upload(cb, true, word, unsafe { s.cpu.as_u16_array() }, 64) {
        return false;
    }
    cb[0u16].object_words = word + 64;
    true
}
