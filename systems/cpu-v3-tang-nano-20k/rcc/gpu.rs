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
mod gpu_ui;
use gpu_ui::*;
mod gpu_display;
use gpu_display::*;

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
    gpu_init_with_format(DISPLAY_RGB565)
}
// Linear GPU colors need Display's existing linear-to-screen encoding.
pub fn gpu_init_with_format(format: u16) -> u16 {
    if format > DISPLAY_LINEAR_RGB565 {
        return GPU_REJECTED;
    }
    if GPU_DRIVER.as_array()[0u16] != 0 {
        return GPU_REJECTED;
    }
    let display = gpu_display_init_format(format);
    if display != GPU_IDLE && display != GPU_PENDING {
        return display;
    }
    let result = gpu_driver_init();
    if result == GPU_IDLE || result == GPU_PENDING {
        if !gpu_high_slot_init(view_of(&mut GPU_CB), 0) {
            return GPU_REJECTED;
        }
    }
    if result == GPU_IDLE && display == GPU_PENDING {
        return GPU_PENDING;
    }
    result
}
pub fn gpu_cb_begin() -> u16 {
    if GPU_RECORDING != 0 || !gpu_driver_can_build() || gpu_display_writable() == 0 {
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
    let target = gpu_display_reserve();
    if target == 0 {
        return GPU_CB_NONE;
    }
    if !gpu_builder_begin(view_of(&mut GPU_CB), target, GPU_DEPTH_SECTOR, color, depth) {
        if gpu_memory_faulted() {
            gpu_display_fault();
        } else {
            gpu_display_unreserve();
        }
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
pub fn gpu_pass_draw_ui_grid(pass: u16, atlas: Array<GpuUiAtlas>, cells: Array<u16>) -> bool {
    if pass != GPU_CB_HANDLE || GPU_RECORDING != 2 {
        return false;
    }
    gpu_builder_ui(view_of(&mut GPU_CB), atlas, cells)
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
    if gpu_memory_faulted() || GPU_DISPLAY.as_array()[0u16] == 3 || GPU_DRIVER.as_array()[0u16] == 3
    {
        return GPU_FAULT;
    }
    if GPU_CB.state != GPU_CB_READY {
        return GPU_REJECTED;
    }
    if !gpu_display_render_start(GPU_CB.color) {
        return GPU_REJECTED;
    }
    let result = gpu_driver_submit_idle(view_of(&mut GPU_CB));
    if result == GPU_REJECTED {
        gpu_display_render_rejected();
    }
    if result == GPU_FAULT || result == GPU_CACHE_ERROR {
        gpu_display_fault();
    }
    result
}
pub fn gpu_poll(cb: u16) -> u16 {
    if cb != GPU_CB_HANDLE {
        return GPU_REJECTED;
    }
    if gpu_display_refresh() == GPU_FAULT {
        return GPU_FAULT;
    }
    let result = gpu_driver_poll(view_of(&mut GPU_CB));
    if result == GPU_COMPLETED && !gpu_display_render_done(GPU_CB.color) {
        return GPU_FAULT;
    }
    if result == GPU_FAULT {
        gpu_display_fault();
    }
    result
}
pub fn gpu_wait(cb: u16, limit: u16) -> u16 {
    if cb != GPU_CB_HANDLE {
        return GPU_REJECTED;
    }
    let mut left = limit;
    while left != 0 {
        let result = gpu_poll(cb);
        if result != GPU_PENDING {
            return result;
        }
        left -= 1;
    }
    GPU_TIMEOUT
}
pub fn gpu_present(cb: u16) -> u16 {
    if gpu_memory_faulted() || GPU_DRIVER.as_array()[0u16] == 3 || GPU_DISPLAY.as_array()[0u16] == 3
    {
        return GPU_FAULT;
    }
    if cb != GPU_CB_HANDLE || GPU_CB.state != GPU_CB_COMPLETED || !gpu_driver_can_build() {
        return GPU_REJECTED;
    }
    gpu_display_present(GPU_CB.color)
}
pub fn gpu_present_poll() -> u16 {
    gpu_display_refresh()
}
pub fn gpu_present_wait(limit: u16) -> u16 {
    let mut left = limit;
    while left != 0 {
        let result = gpu_present_poll();
        if result != GPU_PENDING {
            return result;
        }
        left -= 1;
    }
    GPU_TIMEOUT
}
fn gpu_recording_reset() {
    GPU_CB.command_words = 0;
    GPU_CB.object_words = 0;
    GPU_CB.state = GPU_CB_BUILDING;
    GPU_BUILD.as_array()[0u16] = 0;
    GPU_BUILD.as_array()[1u16] = 0;
    GPU_BUILD.as_array()[2u16] = 0;
    unsafe {
        addr_of(&GPU_RECORDING).write(0, 0);
    }
}
pub fn gpu_cb_cancel(cb: u16) -> bool {
    if cb != GPU_CB_HANDLE
        || GPU_RECORDING == 0
        || GPU_CB.state != GPU_CB_BUILDING
        || !gpu_driver_can_build()
    {
        return false;
    }
    let status = gpu_display_refresh();
    if status != GPU_IDLE && status != GPU_COMPLETED {
        return false;
    }
    if GPU_DISPLAY.as_array()[4u16] != 0 && !gpu_display_unreserve() {
        return false;
    }
    gpu_recording_reset();
    true
}
pub fn gpu_discard_ready(cb: u16) -> bool {
    if cb != GPU_CB_HANDLE || GPU_RECORDING != 0 || !gpu_driver_can_build() {
        return false;
    }
    if GPU_CB.state == GPU_CB_READY {
        if gpu_display_refresh() != GPU_IDLE || !gpu_display_unreserve() {
            return false;
        }
    } else {
        if GPU_CB.state != GPU_CB_COMPLETED || !gpu_display_discard(GPU_CB.color) {
            return false;
        }
    }
    gpu_recording_reset();
    true
}
