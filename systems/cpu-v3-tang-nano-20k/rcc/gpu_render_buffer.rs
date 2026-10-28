//! Borrowed external planes. Metadata is owned here; physical storage is not.
use crate::dsl_rt::*;
mod gpu_memory;
use gpu_memory::*;
mod gpu_layout;
use gpu_layout::*;
mod gpu_assets;
use gpu_assets::*;
mod boot_resources;
use boot_resources::*;

// registered, owner, color/depth sector, color/depth capacity, defined,
// writer slot, reader slots, live views, generation lo/hi, previous defined.
static GPU_RTT_BUFFERS: Buf<u16, 64> = Buf::new([0; 64]);
static GPU_RTT_WRITERS: Buf<u16, 2> = Buf::new([0; 2]);
static GPU_RTT_READERS: Buf<u16, 2> = Buf::new([0; 2]);
// Content epochs do not restart when a registry slot or physical base is reused.
static GPU_RTT_GENERATION: Buf<u16, 2> = Buf::new([0; 2]);

fn gpu_rtt_buffer(id: u16) -> Array<u16> {
    unsafe {
        GPU_RTT_BUFFERS
            .as_ptr()
            .add(((id - 1) * 16) as i16)
            .as_u16_array()
    }
}
fn gpu_rtt_byte_sectors(low: u16, high: u16) -> u16 {
    if high >= 128 || low & 127 != 0 {
        return 65535;
    }
    (high << 9) | (low >> 7)
}
fn gpu_rtt_plane_valid(base: u16, count: u16) -> bool {
    // This interval also excludes fixed Display/depth, input and UI storage.
    if base & 511 != 0 || count < GPU_PLANE_SECTORS || !gpu_asset_extent(base, count) {
        return false;
    }
    if gpu_extents_overlap(
        base,
        count,
        GPU_RENDERER_MESH_FIRST,
        GPU_RENDERER_MESH_SECTORS,
    ) || gpu_extents_overlap(
        base,
        count,
        GPU_RENDERER_TEXTURE,
        GPU_RENDERER_TEXTURE_SECTORS,
    ) || gpu_extents_overlap(base, count, GPU_PUI_CANVAS[2u16], GPU_PUI_CANVAS[3u16])
    {
        return false;
    }
    let mut id: u16 = 1;
    while id <= 4 {
        let b = gpu_rtt_buffer(id);
        if b[0u16] != 0
            && (gpu_extents_overlap(base, count, b[2u16], b[4u16])
                || gpu_extents_overlap(base, count, b[3u16], b[5u16]))
        {
            return false;
        }
        id += 1;
    }
    true
}
fn gpu_rtt_register(memory: Array<RenderBufferMemory>) -> u16 {
    if memory.as_ptr().addr() > 0xc000 - 8 || gpu_memory_faulted() {
        return 0;
    }
    let color = gpu_rtt_byte_sectors(memory[0u16].color_base_low, memory[0u16].color_base_high);
    let depth = gpu_rtt_byte_sectors(memory[0u16].depth_base_low, memory[0u16].depth_base_high);
    let colors = gpu_rtt_byte_sectors(
        memory[0u16].color_capacity_low,
        memory[0u16].color_capacity_high,
    );
    let depths = gpu_rtt_byte_sectors(
        memory[0u16].depth_capacity_low,
        memory[0u16].depth_capacity_high,
    );
    if !gpu_rtt_plane_valid(color, colors)
        || !gpu_rtt_plane_valid(depth, depths)
        || gpu_extents_overlap(color, colors, depth, depths)
    {
        return 0;
    }
    let mut id: u16 = 1;
    while id <= 4 {
        let mut b = gpu_rtt_buffer(id);
        if b[0u16] == 0 {
            gpu_zero(b.as_ptr(), 16);
            b[0u16] = 1;
            b[1u16] = 1;
            b[2u16] = color;
            b[3u16] = depth;
            b[4u16] = colors;
            b[5u16] = depths;
            return id;
        }
        id += 1;
    }
    0
}
fn gpu_rtt_live(id: u16) -> bool {
    id != 0 && id <= 4 && gpu_rtt_buffer(id)[0u16] != 0 && gpu_rtt_buffer(id)[1u16] != 0
}
fn gpu_rtt_busy(id: u16) -> bool {
    let b = gpu_rtt_buffer(id);
    b[7u16] != 0 || b[8u16] != 0 || b[9u16] != 0
}
fn gpu_rtt_collect(id: u16) {
    let mut b = gpu_rtt_buffer(id);
    if b[1u16] == 0 && !gpu_rtt_busy(id) {
        b[0u16] = 0;
    }
}
fn gpu_rtt_close(id: u16) -> bool {
    if !gpu_rtt_live(id) {
        return false;
    }
    let healthy = !gpu_memory_faulted() && GPU_PIPE[0u16] != 3;
    let idle = !gpu_rtt_busy(id);
    gpu_rtt_buffer(id)[1u16] = 0;
    // On faults even an apparently idle range stays registered to forbid aliases.
    if healthy && idle {
        gpu_rtt_collect(id);
        return true;
    }
    false
}
fn gpu_rtt_write(id: u16, slot: u16) -> bool {
    if !gpu_rtt_live(id) || slot == 0 || slot > 2 || gpu_memory_faulted() {
        return false;
    }
    let mut b = gpu_rtt_buffer(id);
    if gpu_rtt_busy(id)
        || GPU_RTT_WRITERS[slot - 1] != 0
        || (GPU_RTT_GENERATION[0u16] == 65535 && GPU_RTT_GENERATION[1u16] == 65535)
    {
        return false;
    }
    b[12u16] = b[6u16];
    b[6u16] = 0;
    b[7u16] = slot;
    GPU_RTT_WRITERS[slot - 1] = id;
    true
}
fn gpu_rtt_view_begin(id: u16) -> bool {
    if !gpu_rtt_live(id) || gpu_memory_faulted() {
        return false;
    }
    let mut b = gpu_rtt_buffer(id);
    if b[6u16] == 0 || b[7u16] != 0 || b[9u16] != 0 {
        return false;
    }
    b[9u16] = 1;
    true
}
fn gpu_rtt_view_end(id: u16) {
    gpu_rtt_buffer(id)[9u16] = 0;
    if !gpu_memory_faulted() && GPU_PIPE[0u16] != 3 {
        gpu_rtt_collect(id);
    }
}
fn gpu_rtt_read(id: u16, slot: u16) -> bool {
    if !gpu_rtt_live(id) || slot == 0 || slot > 2 || gpu_memory_faulted() {
        return false;
    }
    let mut b = gpu_rtt_buffer(id);
    if b[6u16] == 0 || b[7u16] != 0 || b[9u16] == 0 || GPU_RTT_WRITERS[slot - 1] == id {
        return false;
    }
    b[8u16] |= 1 << (slot - 1);
    GPU_RTT_READERS[slot - 1] |= 1 << (id - 1);
    true
}
fn gpu_rtt_retire(slot: u16, completed: bool) -> bool {
    if slot == 0 || slot > 2 || gpu_memory_faulted() {
        return false;
    }
    let writer = GPU_RTT_WRITERS[slot - 1];
    if writer == 0 && GPU_RTT_READERS[slot - 1] == 0 {
        return true;
    }
    if writer != 0 {
        let mut b = gpu_rtt_buffer(writer);
        if b[0u16] == 0 || b[7u16] != slot {
            return false;
        }
        if completed {
            if GPU_RTT_GENERATION[0u16] == 65535 && GPU_RTT_GENERATION[1u16] == 65535 {
                return false;
            }
            GPU_RTT_GENERATION[0u16] += 1;
            if GPU_RTT_GENERATION[0u16] == 0 {
                GPU_RTT_GENERATION[1u16] += 1;
            }
            b[10u16] = GPU_RTT_GENERATION[0u16];
            b[11u16] = GPU_RTT_GENERATION[1u16];
            b[6u16] = 1;
        } else {
            b[6u16] = b[12u16];
        }
        b[7u16] = 0;
        GPU_RTT_WRITERS[slot - 1] = 0;
        gpu_rtt_collect(writer);
    }
    let mut id: u16 = 1;
    while id <= 4 {
        if GPU_RTT_READERS[slot - 1] & (1 << (id - 1)) != 0 {
            let mut b = gpu_rtt_buffer(id);
            b[8u16] &= 65535 - (1 << (slot - 1));
            gpu_rtt_collect(id);
        }
        id += 1;
    }
    GPU_RTT_READERS[slot - 1] = 0;
    true
}
