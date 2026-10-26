//! Internal two-input scheduler. This is not the public resource/RAII API.
//! One recorder, one sealed successor, one GPU execution; no execution queue.
use crate::dsl_rt::*;
mod gpu_device;
use gpu_device::*;
mod gpu_command_buffer;
use gpu_command_buffer::*;
mod gpu_builder;
use gpu_builder::*;
mod gpu_display;
use gpu_display::*;
// Explicit internal helper set for the forthcoming resource-library boundary.
// No resource/library attribute is used until the compiler capability is accepted.
mod gpu_encode;
use gpu_encode::*;
mod gpu_memory;
use gpu_memory::*;
mod gpu_layout;
use gpu_layout::*;
mod gpu_assets;
use gpu_assets::*;
mod gpu_window;
use gpu_window::*;
mod gpu_ui;
use gpu_ui::*;

static GPU_PIPE_A: GpuBuffer = GpuBuffer {
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
static GPU_PIPE_B: GpuBuffer = GpuBuffer {
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
// phase(0=new,1=active,3=frozen), recorder, recorded, GPU, next slot, sequence.
// Stored slot identities are index+1; zero is none. They never identify colors.
static GPU_PIPE: Buf<u16, 6> = Buf::new([0; 6]);
// First failure: code, sequence, slot, valid command words, sampled GPU status.
static GPU_PIPE_FAULT: Buf<u16, 5> = Buf::new([0; 5]);

fn gpu_pipeline_buffer(slot: u16) -> Array<GpuBuffer> {
    if slot == 1 {
        return view_of(&mut GPU_PIPE_A);
    }
    view_of(&mut GPU_PIPE_B)
}
fn gpu_pipeline_fail(code: u16) -> u16 {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] != 3 {
        let mut f = GPU_PIPE_FAULT.as_array();
        f[0u16] = code;
        f[1u16] = p[5u16];
        let mut slot = p[3u16];
        if slot == 0 {
            slot = p[2u16];
        }
        if slot == 0 {
            slot = p[1u16];
        }
        f[2u16] = slot;
        if slot != 0 {
            f[3u16] = gpu_pipeline_buffer(slot)[0u16].command_words;
        }
        f[4u16] = dev_recv(GPU_DEVICE, GPU_STATUS);
        p[0u16] = 3;
        GPU_DRIVER.as_array()[0u16] = 3;
        gpu_display_fault();
    }
    GPU_FAULT
}
fn gpu_pipeline_init(format: u16) -> u16 {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] != 0 || GPU_DRIVER.as_array()[0u16] != 0 || format > DISPLAY_LINEAR_RGB565 {
        return GPU_REJECTED;
    }
    let display = gpu_display_init_format(format);
    if display != GPU_IDLE && display != GPU_PENDING {
        return gpu_pipeline_fail(1);
    }
    let driver = gpu_driver_init();
    if driver != GPU_IDLE && driver != GPU_PENDING {
        return gpu_pipeline_fail(2);
    }
    if !gpu_high_slot_init(view_of(&mut GPU_PIPE_A), 0)
        || !gpu_high_slot_init(view_of(&mut GPU_PIPE_B), 1)
    {
        return gpu_pipeline_fail(3);
    }
    p[4u16] = 1;
    p[0u16] = 1;
    if display == GPU_PENDING || driver == GPU_PENDING {
        return GPU_PENDING;
    }
    GPU_IDLE
}
// Polling never submits a queued successor. Successful GPU terminal releases
// its input slot; present is automatic, with independent Display retirement.
fn gpu_pipeline_poll() -> u16 {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] == 0 {
        return GPU_NOT_INITIALIZED;
    }
    if p[0u16] == 3 {
        return GPU_FAULT;
    }
    if gpu_memory_faulted() {
        return gpu_pipeline_fail(4);
    }
    let slot = p[3u16];
    let mut owned = gpu_pipeline_buffer(1);
    if slot != 0 {
        owned = gpu_pipeline_buffer(slot);
    }
    let result = gpu_driver_poll(owned);
    if result == GPU_FAULT || result == GPU_REJECTED {
        return gpu_pipeline_fail(5);
    }
    if slot != 0 && result == GPU_COMPLETED {
        if !gpu_display_render_done(owned[0u16].color)
            || gpu_display_present(owned[0u16].color) != GPU_PENDING
        {
            return gpu_pipeline_fail(6);
        }
        p[3u16] = 0;
    }
    let display = gpu_display_refresh();
    if display == GPU_FAULT {
        return gpu_pipeline_fail(7);
    }
    if p[3u16] != 0 || GPU_DRIVER.as_array()[0u16] == 4 || display == GPU_PENDING {
        return GPU_PENDING;
    }
    display
}
// Recording does not reserve a plane. The placeholder Target is private input;
// its builder-generated relocation is replaced before final cache publication.
fn gpu_pipeline_begin(color_value: u16, depth_value: u16) -> u16 {
    let observed = gpu_pipeline_poll();
    let mut p = GPU_PIPE.as_array();
    if observed == GPU_FAULT
        || observed == GPU_NOT_INITIALIZED
        || p[1u16] != 0
        || p[2u16] != 0
        || GPU_DRIVER.as_array()[0u16] == 4
    {
        return 0;
    }
    let slot = p[4u16];
    let mut cb = gpu_pipeline_buffer(slot);
    if cb[0u16].state == GPU_CB_COMPLETED && !gpu_buffer_reset(cb) {
        return 0;
    }
    if cb[0u16].state != GPU_CB_BUILDING || cb[0u16].command_words != 0 {
        return 0;
    }
    p[1u16] = slot;
    if !gpu_builder_begin(
        cb,
        GPU_COLOR_A_SECTOR,
        GPU_DEPTH_SECTOR,
        color_value,
        depth_value,
    ) {
        if gpu_memory_faulted() {
            gpu_pipeline_fail(8);
        } else {
            p[1u16] = 0;
        }
        return 0;
    }
    slot
}
fn gpu_pipeline_seal() -> bool {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] != 1 || p[1u16] == 0 || p[2u16] != 0 {
        return false;
    }
    let cb = gpu_pipeline_buffer(p[1u16]);
    if !gpu_builder_end(cb) || !gpu_builder_finish(cb) {
        if gpu_memory_faulted() {
            gpu_pipeline_fail(9);
        }
        return false;
    }
    p[2u16] = p[1u16];
    p[1u16] = 0;
    true
}
fn gpu_pipeline_cancel() -> bool {
    let mut p = GPU_PIPE.as_array();
    if p[0u16] != 1 || p[1u16] == 0 || gpu_memory_faulted() {
        return false;
    }
    let mut cb = gpu_pipeline_buffer(p[1u16]);
    if cb[0u16].state != GPU_CB_BUILDING {
        return false;
    }
    cb[0u16].command_words = 0;
    cb[0u16].object_words = 0;
    GPU_SLOT_TARGETS.as_array()[p[1u16] - 1] = 0;
    GPU_BUILD.as_array()[0u16] = 0;
    GPU_BUILD.as_array()[1u16] = 0;
    GPU_BUILD.as_array()[2u16] = 0;
    p[1u16] = 0;
    true
}
fn gpu_pipeline_advance() -> u16 {
    let result = gpu_pipeline_poll();
    let mut p = GPU_PIPE.as_array();
    if result == GPU_FAULT || result == GPU_NOT_INITIALIZED {
        return result;
    }
    if p[3u16] != 0 {
        return GPU_PENDING;
    }
    if p[2u16] == 0 {
        return result;
    }
    let target = gpu_display_reserve();
    if target == 0 {
        if GPU_DISPLAY.as_array()[0u16] == 3 {
            return gpu_pipeline_fail(10);
        }
        return GPU_PENDING;
    }
    let slot = p[2u16];
    let cb = gpu_pipeline_buffer(slot);
    if !gpu_slot_bind_target(cb, target) || !gpu_display_render_start(target) {
        return gpu_pipeline_fail(11);
    }
    // This final driver fence drains old scalar/FST stores and every relocation.
    // Neither SEND occurs on a reported maintenance failure.
    if gpu_driver_submit_idle(cb) != GPU_PENDING {
        return gpu_pipeline_fail(12);
    }
    p[2u16] = 0;
    p[3u16] = slot;
    p[4u16] = 3 - slot;
    p[5u16] += 1;
    GPU_PENDING
}
