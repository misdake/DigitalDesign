//! Application API: one opaque command buffer and one ordered Clear/Store Pass.
//! Physical CB placement, capacity, DMA and DSEG3 are library-owned details.
use crate::dsl_rt::*;
mod gpu_device;
use gpu_device::*;
mod gpu_command_buffer;
use gpu_command_buffer::*;
mod gpu_memory;
use gpu_memory::*;
mod gpu_layout;
use gpu_layout::*;
mod gpu_assets;
use gpu_assets::*;
mod gpu_builder;
use gpu_builder::*;

static GPU_CB: GpuBuffer = GpuBuffer {
    high_slot: 0,
    commands: GpuStorage {
        cpu: 0 as Ptr,
        sector: 0,
        sectors: 0,
    },
    objects: GpuStorage {
        cpu: 0 as Ptr,
        sector: 0,
        sectors: 0,
    },
    command_words: 0,
    object_words: 0,
    state: 0,
    color: 0,
    depth: 0,
};
static GPU_RECORDING: u16 = 0;
pub const GPU_CB_NONE: u16 = 0;
pub const GPU_CB_HANDLE: u16 = 1;

pub fn gpu_init() -> u16 {
    let result = gpu_driver_init();
    if result == GPU_IDLE || result == GPU_PENDING {
        if !gpu_high_slot_init(view_of(&mut GPU_CB), 0) {
            return GPU_REJECTED;
        }
    }
    result
}
pub fn gpu_cb_begin() -> u16 {
    if GPU_RECORDING != 0 || !gpu_driver_can_build() {
        return GPU_CB_NONE;
    }
    if GPU_CB.state == GPU_CB_COMPLETED {
        if !gpu_buffer_reset(view_of(&mut GPU_CB)) {
            return GPU_CB_NONE;
        }
    }
    if GPU_CB.state != GPU_CB_BUILDING {
        return GPU_CB_NONE;
    }
    unsafe {
        addr_of(&GPU_RECORDING).write(0, 1);
    }
    GPU_CB_HANDLE
}
pub fn gpu_pass_clear(cb: u16, color: u16, depth: u16) -> bool {
    let pass = gpu_pass_begin(cb, color, depth);
    pass != GPU_CB_NONE && gpu_pass_end(pass) && gpu_cb_finish(cb)
}
pub fn gpu_pass_begin(cb: u16, color: u16, depth: u16) -> u16 {
    if cb != GPU_CB_HANDLE || GPU_RECORDING != 1 {
        return GPU_CB_NONE;
    }
    if !gpu_builder_begin(
        view_of(&mut GPU_CB),
        GPU_COLOR_B_SECTOR,
        GPU_DEPTH_SECTOR,
        color,
        depth,
    ) {
        return GPU_CB_NONE;
    }
    unsafe {
        addr_of(&GPU_RECORDING).write(0, 2);
    }
    GPU_CB_HANDLE
}
pub fn gpu_pass_set_global(pass: u16, global: Array<GpuGlobal>) -> bool {
    if pass != GPU_CB_HANDLE || GPU_RECORDING != 2 {
        return false;
    }
    gpu_builder_global(view_of(&mut GPU_CB), global)
}
pub fn gpu_pass_set_render(
    pass: u16,
    texture: Array<GpuTexture>,
    render: Array<GpuRender>,
) -> bool {
    if pass != GPU_CB_HANDLE || GPU_RECORDING != 2 {
        return false;
    }
    gpu_builder_render(view_of(&mut GPU_CB), texture, render)
}
pub fn gpu_pass_draw_mesh(
    pass: u16,
    mesh: Array<GpuMesh>,
    matrix: Array<u16>,
    rgba_low: u16,
    rgba_high: u16,
) -> bool {
    if pass != GPU_CB_HANDLE || GPU_RECORDING != 2 {
        return false;
    }
    gpu_builder_draw(view_of(&mut GPU_CB), mesh, matrix, rgba_low, rgba_high)
}
pub fn gpu_pass_end(pass: u16) -> bool {
    if pass != GPU_CB_HANDLE || GPU_RECORDING != 2 {
        return false;
    }
    if !gpu_builder_end(view_of(&mut GPU_CB)) {
        return false;
    }
    unsafe {
        addr_of(&GPU_RECORDING).write(0, 3);
    }
    true
}
pub fn gpu_cb_finish(cb: u16) -> bool {
    if cb != GPU_CB_HANDLE || GPU_RECORDING != 3 {
        return false;
    }
    if !gpu_builder_finish(view_of(&mut GPU_CB)) {
        return false;
    }
    unsafe {
        addr_of(&GPU_RECORDING).write(0, 0);
    }
    true
}
pub fn gpu_submit_idle(cb: u16) -> u16 {
    if cb != GPU_CB_HANDLE {
        return GPU_REJECTED;
    }
    gpu_driver_submit_idle(view_of(&mut GPU_CB))
}
pub fn gpu_poll(cb: u16) -> u16 {
    if cb != GPU_CB_HANDLE {
        return GPU_REJECTED;
    }
    gpu_driver_poll(view_of(&mut GPU_CB))
}
pub fn gpu_wait(cb: u16, limit: u16) -> u16 {
    if cb != GPU_CB_HANDLE {
        return GPU_REJECTED;
    }
    gpu_driver_wait(view_of(&mut GPU_CB), limit)
}
