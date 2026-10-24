//! Fixed planes, one Pending and one trusted Display writer. No local recovery.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_layout;
use gpu_layout::*;
// phase(0=new,1=unknown pending,2=owned,3=fault), Front, Pending,
// Ready, Reserved, Rendering, Front format, desired format.
// Color identities are A=1/B=2, never addresses.
static GPU_DISPLAY: Buf<u16, 8> = Buf::new([0; 8]);
fn gpu_display_fault() -> u16 {
    GPU_DISPLAY.as_array()[0u16] = 3;
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
    let mut d = GPU_DISPLAY.as_array();
    if d[0u16] == 0 {
        return GPU_NOT_INITIALIZED;
    }
    if d[0u16] == 3 || gpu_memory_faulted() {
        return gpu_display_fault();
    }
    let gpu_status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if gpu_status & GPU_STATUS_TRANSPORT_ERROR != 0
        || GPU_DRIVER.as_array()[0u16] == 3
        || (gpu_status & GPU_STATUS_BUSY != 0 && GPU_DRIVER.as_array()[0u16] == 1)
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
    if d[0u16] == 1 {
        if d[7u16] == DISPLAY_RGB565 && s[3u16] != DISPLAY_RGB565 {
            return gpu_display_fault();
        }
        if s[0u16] != 0 {
            return GPU_PENDING;
        }
        d[1u16] = active;
        d[6u16] = s[3u16];
        d[0u16] = 2;
        return GPU_IDLE;
    }
    let mut expected_format = d[6u16];
    if d[2u16] != 0 && s[0u16] == 0 {
        expected_format = d[7u16];
    }
    if s[3u16] != expected_format {
        return gpu_display_fault();
    }
    if d[2u16] != 0 {
        if s[0u16] != 0 {
            if active != d[1u16] {
                return gpu_display_fault();
            }
            return GPU_PENDING;
        }
        if active != d[2u16] {
            return gpu_display_fault();
        }
        // The RTL clears Pending only at frame event + frame_complete,
        // after the final old external read response, not after NEXT_SWAP.
        d[1u16] = active;
        d[6u16] = s[3u16];
        d[2u16] = 0;
        return GPU_COMPLETED;
    }
    if s[0u16] != 0 || active != d[1u16] {
        return gpu_display_fault();
    }
    GPU_IDLE
}
fn gpu_display_init() -> u16 {
    gpu_display_init_format(DISPLAY_RGB565)
}
fn gpu_display_init_format(format: u16) -> u16 {
    let mut d = GPU_DISPLAY.as_array();
    if d[0u16] != 0 {
        return GPU_REJECTED;
    }
    d[7u16] = format;
    d[0u16] = 1;
    gpu_display_refresh()
}
fn gpu_display_writable() -> u16 {
    let status = gpu_display_refresh();
    if status != GPU_IDLE && status != GPU_COMPLETED {
        return 0;
    }
    let d = GPU_DISPLAY.as_array();
    let mut color: u16 = 1;
    while color <= 2 {
        if color != d[1u16]
            && color != d[2u16]
            && color != d[3u16]
            && color != d[4u16]
            && color != d[5u16]
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
    GPU_DISPLAY.as_array()[4u16] = color;
    gpu_display_sector(color)
}
fn gpu_display_unreserve() -> bool {
    let mut d = GPU_DISPLAY.as_array();
    if d[0u16] != 2 || d[4u16] == 0 || d[4u16] == d[1u16] || d[4u16] == d[2u16] {
        return false;
    }
    d[4u16] = 0;
    true
}
fn gpu_display_render_start(sector: u16) -> bool {
    let status = gpu_display_refresh();
    if status != GPU_IDLE && status != GPU_COMPLETED {
        return false;
    }
    let mut d = GPU_DISPLAY.as_array();
    let color = d[4u16];
    if color == 0
        || gpu_display_sector(color) != sector
        || color == d[1u16]
        || color == d[2u16]
        || d[3u16] != 0
        || d[5u16] != 0
    {
        return false;
    }
    d[4u16] = 0;
    d[5u16] = color;
    true
}
fn gpu_display_render_rejected() {
    let mut d = GPU_DISPLAY.as_array();
    d[4u16] = d[5u16];
    d[5u16] = 0;
}
fn gpu_display_render_done(sector: u16) -> bool {
    let mut d = GPU_DISPLAY.as_array();
    let color = d[5u16];
    if d[0u16] != 2 || color == 0 || gpu_display_sector(color) != sector || d[3u16] != 0 {
        gpu_display_fault();
        return false;
    }
    d[5u16] = 0;
    d[3u16] = color;
    true
}
fn gpu_display_discard(sector: u16) -> bool {
    let status = gpu_display_refresh();
    if status != GPU_IDLE && status != GPU_COMPLETED {
        return false;
    }
    let mut d = GPU_DISPLAY.as_array();
    let color = d[3u16];
    if color == 0 || gpu_display_sector(color) != sector || color == d[1u16] || color == d[2u16] {
        return false;
    }
    d[3u16] = 0;
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
    let mut d = GPU_DISPLAY.as_array();
    let color = d[3u16];
    if color == 0
        || gpu_display_sector(color) != sector
        || color == d[1u16]
        || d[2u16] != 0
        || d[4u16] != 0
        || d[5u16] != 0
    {
        return GPU_REJECTED;
    }
    // Establish ownership before the four serialized writes. Present needs no
    // post-render CPU cache clean: the GPU's actual FLUSH already made it visible.
    d[2u16] = color;
    d[3u16] = 0;
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW, sector << 6);
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH, sector >> 10);
    dev_send(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT, d[7u16]);
    dev_send(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND, DISPLAY_NEXT_SWAP);
    GPU_PENDING
}
