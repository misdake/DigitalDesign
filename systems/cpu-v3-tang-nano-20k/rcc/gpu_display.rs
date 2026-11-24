//! Fixed planes, one Pending and one trusted Display writer. No local recovery.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_layout;
use gpu_layout::*;
// phase(0=new,1=unknown pending,2=owned,3=fault), Front, Pending,
// Ready, Reserved, Rendering, Front format, desired format.
// Color identities are A=1/B=2, never addresses.
#[repr(C)]
struct GpuDisplayState {
    phase: u16,
    front: u16,
    pending: u16,
    ready: u16,
    reserved: u16,
    rendering: u16,
    front_format: u16,
    desired_format: u16,
}
static GPU_DISPLAY: GpuDisplayState = GpuDisplayState {
    phase: 0,
    front: 0,
    pending: 0,
    ready: 0,
    reserved: 0,
    rendering: 0,
    front_format: 0,
    desired_format: 0,
};
fn gpu_display_fault() -> u16 {
    GPU_DISPLAY.phase = 3;
    GPU_FAULT
}
fn gpu_display_sector(color: u16) -> u16 {
    if color == 1 {
        return GPU_COLOR_A_SECTOR;
    }
    if color == 2 {
        return GPU_COLOR_B_SECTOR;
    }
    0
}
fn gpu_display_snapshot(mut out: Array<u16>) -> u16 {
    let before = dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND);
    let low = dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW);
    let high = dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH);
    let format = dev_recv(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT);
    let after = dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND);
    if (before | after) & 65534 != 0 {
        return GPU_FAULT;
    }
    if before != after {
        return GPU_PENDING;
    }
    out[0u16] = before;
    out[1u16] = low;
    out[2u16] = high;
    out[3u16] = format;
    GPU_IDLE
}
fn gpu_display_refresh() -> u16 {
    if GPU_DISPLAY.phase == 0 {
        return GPU_NOT_INITIALIZED;
    }
    if GPU_DISPLAY.phase == 3 || gpu_memory_faulted() {
        return gpu_display_fault();
    }
    let gpu_status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if gpu_status & GPU_STATUS_TRANSPORT_ERROR != 0
        || GPU_DRIVER.phase == 3
        || (gpu_status & GPU_STATUS_BUSY != 0 && GPU_DRIVER.phase == 1)
    {
        return gpu_display_fault();
    }
    let mut s: Buf<u16, 4> = Buf::new([0; 4]);
    let status = gpu_display_snapshot(s.as_array());
    if status == GPU_FAULT {
        return gpu_display_fault();
    }
    if status == GPU_PENDING {
        return GPU_PENDING;
    }
    let mut active: u16 = 0;
    if s[1u16] == GPU_COLOR_A_SECTOR << 6 && s[2u16] == GPU_COLOR_A_SECTOR >> 10 {
        active = 1;
    }
    if s[1u16] == GPU_COLOR_B_SECTOR << 6 && s[2u16] == GPU_COLOR_B_SECTOR >> 10 {
        active = 2;
    }
    if active == 0 || s[3u16] > DISPLAY_LINEAR_RGB565 {
        return gpu_display_fault();
    }
    if GPU_DISPLAY.phase == 1 {
        if GPU_DISPLAY.desired_format == DISPLAY_RGB565 && s[3u16] != DISPLAY_RGB565 {
            return gpu_display_fault();
        }
        if s[0u16] != 0 {
            return GPU_PENDING;
        }
        GPU_DISPLAY.front = active;
        GPU_DISPLAY.front_format = s[3u16];
        GPU_DISPLAY.phase = 2;
        return GPU_IDLE;
    }
    let mut expected_format = GPU_DISPLAY.front_format;
    if GPU_DISPLAY.pending != 0 && s[0u16] == 0 {
        expected_format = GPU_DISPLAY.desired_format;
    }
    if s[3u16] != expected_format {
        return gpu_display_fault();
    }
    if GPU_DISPLAY.pending != 0 {
        if s[0u16] != 0 {
            if active != GPU_DISPLAY.front {
                return gpu_display_fault();
            }
            return GPU_PENDING;
        }
        if active != GPU_DISPLAY.pending {
            return gpu_display_fault();
        }
        // The RTL clears Pending only at frame event + frame_complete,
        // after the final old external read response, not after NEXT_SWAP.
        GPU_DISPLAY.front = active;
        GPU_DISPLAY.front_format = s[3u16];
        GPU_DISPLAY.pending = 0;
        return GPU_COMPLETED;
    }
    if s[0u16] != 0 || active != GPU_DISPLAY.front {
        return gpu_display_fault();
    }
    GPU_IDLE
}
fn gpu_display_init() -> u16 {
    gpu_display_init_format(DISPLAY_RGB565)
}
fn gpu_display_init_format(format: u16) -> u16 {
    if GPU_DISPLAY.phase != 0 {
        return GPU_REJECTED;
    }
    GPU_DISPLAY.desired_format = format;
    GPU_DISPLAY.phase = 1;
    gpu_display_refresh()
}
fn gpu_display_writable() -> u16 {
    let status = gpu_display_refresh();
    if status != GPU_IDLE && status != GPU_COMPLETED {
        return 0;
    }
    let mut color: u16 = 1;
    while color <= 2 {
        if color != GPU_DISPLAY.front
            && color != GPU_DISPLAY.pending
            && color != GPU_DISPLAY.ready
            && color != GPU_DISPLAY.reserved
            && color != GPU_DISPLAY.rendering
        {
            return color;
        }
        color += 1;
    }
    0
}
fn gpu_display_reserve() -> u16 {
    let color = gpu_display_writable();
    if color == 0 {
        return 0;
    }
    GPU_DISPLAY.reserved = color;
    gpu_display_sector(color)
}
fn gpu_display_unreserve() -> bool {
    if GPU_DISPLAY.phase != 2
        || GPU_DISPLAY.reserved == 0
        || GPU_DISPLAY.reserved == GPU_DISPLAY.front
        || GPU_DISPLAY.reserved == GPU_DISPLAY.pending
    {
        return false;
    }
    GPU_DISPLAY.reserved = 0;
    true
}
fn gpu_display_render_start(sector: u16) -> bool {
    let status = gpu_display_refresh();
    if status != GPU_IDLE && status != GPU_COMPLETED {
        return false;
    }
    let color = GPU_DISPLAY.reserved;
    if color == 0
        || gpu_display_sector(color) != sector
        || color == GPU_DISPLAY.front
        || color == GPU_DISPLAY.pending
        || GPU_DISPLAY.ready != 0
        || GPU_DISPLAY.rendering != 0
    {
        return false;
    }
    GPU_DISPLAY.reserved = 0;
    GPU_DISPLAY.rendering = color;
    true
}
fn gpu_display_render_rejected() {
    GPU_DISPLAY.reserved = GPU_DISPLAY.rendering;
    GPU_DISPLAY.rendering = 0;
}
fn gpu_display_render_done(sector: u16) -> bool {
    let color = GPU_DISPLAY.rendering;
    if GPU_DISPLAY.phase != 2
        || color == 0
        || gpu_display_sector(color) != sector
        || GPU_DISPLAY.ready != 0
    {
        gpu_display_fault();
        return false;
    }
    GPU_DISPLAY.rendering = 0;
    GPU_DISPLAY.ready = color;
    true
}
fn gpu_display_discard(sector: u16) -> bool {
    let status = gpu_display_refresh();
    if status != GPU_IDLE && status != GPU_COMPLETED {
        return false;
    }
    let color = GPU_DISPLAY.ready;
    if color == 0
        || gpu_display_sector(color) != sector
        || color == GPU_DISPLAY.front
        || color == GPU_DISPLAY.pending
    {
        return false;
    }
    GPU_DISPLAY.ready = 0;
    true
}
fn gpu_display_present(sector: u16) -> u16 {
    let status = gpu_display_refresh();
    if status == GPU_FAULT {
        return GPU_FAULT;
    }
    if status != GPU_IDLE && status != GPU_COMPLETED {
        return GPU_REJECTED;
    }
    let color = GPU_DISPLAY.ready;
    if color == 0
        || gpu_display_sector(color) != sector
        || color == GPU_DISPLAY.front
        || GPU_DISPLAY.pending != 0
        || GPU_DISPLAY.reserved != 0
        || GPU_DISPLAY.rendering != 0
    {
        return GPU_REJECTED;
    }
    // Establish ownership before the four serialized writes. Present needs no
    // post-render CPU cache clean: the GPU's actual FLUSH already made it visible.
    GPU_DISPLAY.pending = color;
    GPU_DISPLAY.ready = 0;
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW, sector << 6);
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH, sector >> 10);
    dev_send(
        DISPLAY_DEVICE,
        DISPLAY_COLOR_FORMAT,
        GPU_DISPLAY.desired_format,
    );
    dev_send(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND, DISPLAY_NEXT_SWAP);
    GPU_PENDING
}
