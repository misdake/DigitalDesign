//! Dynamic Display requests; GPU producer ordering is the application's contract.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_schema;
use gpu_schema::*;

// ready, owned pending, requested sector, first error, chosen format; three spare words.
static GPU_PRESENT: Buf<u16, 8> = Buf::new([0; 8]);
fn gpu_present_fail(code: u16) -> bool {
    if GPU_PRESENT[3u16] == 0 {
        GPU_PRESENT[3u16] = code;
    }
    false
}
// 0=stable, 1=retry, 2=invalid. No output from a torn snapshot is consumed.
fn gpu_present_snapshot(mut out: Array<u16>) -> u16 {
    let before = dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND);
    let low = dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW);
    let high = dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH);
    let format = dev_recv(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT);
    let after = dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND);
    if (before | after) & 65534 != 0 || high > 63 || format > DISPLAY_LINEAR_RGB565 {
        return 2;
    }
    if before != after {
        return 1;
    }
    out[0u16] = before;
    out[1u16] = low;
    out[2u16] = high;
    out[3u16] = format;
    0
}
// Capture observes only. Absence does not query/open Device3; errors stay held.
// 0=not open, 1=retry, 2=stable out4, 65535=driver/hardware fault.
fn gpu_present_capture_state(out: Array<u16>) -> u16 {
    if GPU_PRESENT[3u16] != 0 {
        return 65535;
    }
    if GPU_PRESENT[0u16] == 0 {
        return 0;
    }
    let status = gpu_present_snapshot(out);
    if status == 2 {
        return 65535;
    }
    if status == 1 {
        return 1;
    }
    2
}
fn gpu_present_open() -> bool {
    gpu_present_open_with_format(DISPLAY_RGB565)
}
fn gpu_present_open_with_format(format: u16) -> bool {
    if format > DISPLAY_LINEAR_RGB565 {
        return gpu_present_fail(1);
    }
    let s: Buf<u16, 4> = Buf::new([0; 4]);
    let mut status = gpu_present_snapshot(s.as_array());
    while status == 1 {
        status = gpu_present_snapshot(s.as_array());
    }
    if status == 2 {
        return gpu_present_fail(1);
    }
    let mut i: u16 = 0;
    while i < 8 {
        GPU_PRESENT[i] = 0;
        i += 1;
    }
    GPU_PRESENT[0u16] = 1;
    GPU_PRESENT[4u16] = format;
    true
}
fn gpu_present_set_next(sector: u16) -> bool {
    if GPU_PRESENT[0u16] == 0
        || GPU_PRESENT[3u16] != 0
        || GPU_PRESENT[1u16] != 0
        || sector & 511 != 0
        || GPU_API_PLANE_SECTORS - 1 > 65535 - sector
    {
        return gpu_present_fail(2);
    }
    let s: Buf<u16, 4> = Buf::new([0; 4]);
    let mut status = gpu_present_snapshot(s.as_array());
    while status == 1 {
        status = gpu_present_snapshot(s.as_array());
    }
    if status == 2 || s[0u16] != 0 {
        return gpu_present_fail(2);
    }
    GPU_PRESENT[1u16] = 1;
    GPU_PRESENT[2u16] = sector;
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW, sector << 6);
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH, sector >> 10);
    dev_send(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT, GPU_PRESENT[4u16]);
    dev_send(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND, DISPLAY_NEXT_SWAP);
    true
}
// 0=stable/no owned request, 1=pending/retry, 2=applied and retired, 65535=fault.
fn gpu_present_poll() -> u16 {
    if GPU_PRESENT[0u16] == 0 || GPU_PRESENT[3u16] != 0 {
        gpu_present_fail(3);
        return 65535;
    }
    let s: Buf<u16, 4> = Buf::new([0; 4]);
    let status = gpu_present_snapshot(s.as_array());
    if status == 2 {
        gpu_present_fail(1);
        return 65535;
    }
    if status == 1 || s[0u16] != 0 {
        return 1;
    }
    if GPU_PRESENT[1u16] != 0 {
        let sector = GPU_PRESENT[2u16];
        if s[1u16] != sector << 6 || s[2u16] != sector >> 10 || s[3u16] != GPU_PRESENT[4u16] {
            gpu_present_fail(4);
            return 65535;
        }
        // Hardware Pending clears only after the old external reads retire.
        GPU_PRESENT[1u16] = 0;
        return 2;
    }
    0
}
fn gpu_present_wait_vblank() -> bool {
    if GPU_PRESENT[0u16] == 0 || GPU_PRESENT[3u16] != 0 {
        return gpu_present_fail(3);
    }
    if GPU_PRESENT[1u16] != 0 {
        let mut status = gpu_present_poll();
        while status == 1 {
            status = gpu_present_poll();
        }
        return status == 2;
    }
    let before = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
    while true {
        let after = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
        if after != before {
            let status = gpu_present_poll();
            if status == 65535 {
                return false;
            }
            if status == 0 {
                return true;
            }
        }
    }
    false
}
