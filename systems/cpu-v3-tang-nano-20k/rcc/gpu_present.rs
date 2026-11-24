//! Dynamic Display requests; GPU producer ordering is the application's contract.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_schema;
use gpu_schema::*;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentSnapshot {
    Stable = 0,
    Retry = 1,
    Invalid = 2,
}

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentPoll {
    Idle = 0,
    Pending = 1,
    Retired = 2,
    Fault = 65535,
}

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentObservation {
    Closed = 0,
    Retry = 1,
    Stable = 2,
    Fault = 65535,
}

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentError {
    None = 0,
    InvalidSnapshot = 1,
    SwapRejected = 2,
    DisplayUnavailable = 3,
    SwapMismatch = 4,
}

// ready, owned pending, requested sector, first error, chosen format; three spare words.
#[repr(C)]
struct GpuPresentState {
    ready: u16,
    pending: u16,
    sector: u16,
    error: PresentError,
    format: DisplayFormat,
    reserved: Buf<u16, 3>,
}
static GPU_PRESENT: GpuPresentState = GpuPresentState {
    ready: 0,
    pending: 0,
    sector: 0,
    error: PresentError::None,
    format: DisplayFormat::Rgb565,
    reserved: Buf::new([0; 3]),
};
fn gpu_present_fail(code: PresentError) -> bool {
    if GPU_PRESENT.error == PresentError::None {
        GPU_PRESENT.error = code;
    }
    false
}
// 0=stable, 1=retry, 2=invalid. No output from a torn snapshot is consumed.
fn gpu_present_snapshot(mut out: Array<u16>) -> PresentSnapshot {
    let before = dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND);
    let low = dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW);
    let high = dev_recv(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH);
    let format = dev_recv(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT);
    let after = dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND);
    if (before | after) & 65534 != 0 || high > 63 || format > DISPLAY_LINEAR_RGB565 {
        return PresentSnapshot::Invalid;
    }
    if before != after {
        return PresentSnapshot::Retry;
    }
    out[0u16] = before;
    out[1u16] = low;
    out[2u16] = high;
    out[3u16] = format;
    PresentSnapshot::Stable
}
fn gpu_present_open() -> bool {
    gpu_present_open_with_format(DisplayFormat::Rgb565)
}
fn gpu_present_open_with_format(format: DisplayFormat) -> bool {
    // Reject duplicate controllers without resetting the first owner's state.
    if GPU_PRESENT.ready != 0 {
        return false;
    }
    let s: Buf<u16, 4> = Buf::new([0; 4]);
    let mut status = gpu_present_snapshot(s.as_array());
    while status == PresentSnapshot::Retry {
        status = gpu_present_snapshot(s.as_array());
    }
    if status == PresentSnapshot::Invalid {
        return gpu_present_fail(PresentError::InvalidSnapshot);
    }
    if s[0u16] != 0 {
        return false;
    }
    if GPU_PRESENT.pending != 0 {
        let sector = GPU_PRESENT.sector;
        if s[1u16] != sector << 6 || s[2u16] != sector >> 10 || s[3u16] != GPU_PRESENT.format as u16
        {
            return gpu_present_fail(PresentError::SwapMismatch);
        }
    }
    // Include reserved compatibility words in the bulk reset.
    let mut i: u16 = 0;
    while i < 8 {
        (unsafe { addr_of(&GPU_PRESENT).as_u16_array() })[i] = 0;
        i += 1;
    }
    GPU_PRESENT.ready = 1;
    GPU_PRESENT.format = format;
    true
}
fn gpu_present_set_next(sector: u16) -> bool {
    gpu_present_set_next_with_format(sector, GPU_PRESENT.format)
}
fn gpu_present_set_next_with_format(sector: u16, format: DisplayFormat) -> bool {
    if GPU_PRESENT.ready == 0
        || GPU_PRESENT.error != PresentError::None
        || GPU_PRESENT.pending != 0
        || sector & 511 != 0
        || GPU_API_PLANE_SECTORS - 1 > 65535 - sector
    {
        return gpu_present_fail(PresentError::SwapRejected);
    }
    let s: Buf<u16, 4> = Buf::new([0; 4]);
    let mut status = gpu_present_snapshot(s.as_array());
    while status == PresentSnapshot::Retry {
        status = gpu_present_snapshot(s.as_array());
    }
    if status == PresentSnapshot::Invalid || s[0u16] != 0 {
        return gpu_present_fail(PresentError::SwapRejected);
    }
    GPU_PRESENT.pending = 1;
    GPU_PRESENT.sector = sector;
    GPU_PRESENT.format = format;
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW, sector << 6);
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_HIGH, sector >> 10);
    dev_send(
        DISPLAY_DEVICE,
        DISPLAY_COLOR_FORMAT,
        GPU_PRESENT.format as u16,
    );
    dev_send(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND, DISPLAY_NEXT_SWAP);
    true
}
// 0=stable/no owned request, 1=pending/retry, 2=applied and retired, 65535=fault.
fn gpu_present_poll() -> PresentPoll {
    if GPU_PRESENT.ready == 0 || GPU_PRESENT.error != PresentError::None {
        gpu_present_fail(PresentError::DisplayUnavailable);
        return PresentPoll::Fault;
    }
    let s: Buf<u16, 4> = Buf::new([0; 4]);
    let status = gpu_present_snapshot(s.as_array());
    if status == PresentSnapshot::Invalid {
        gpu_present_fail(PresentError::InvalidSnapshot);
        return PresentPoll::Fault;
    }
    if status == PresentSnapshot::Retry || s[0u16] != 0 {
        return PresentPoll::Pending;
    }
    if GPU_PRESENT.pending != 0 {
        let sector = GPU_PRESENT.sector;
        if s[1u16] != sector << 6 || s[2u16] != sector >> 10 || s[3u16] != GPU_PRESENT.format as u16
        {
            gpu_present_fail(PresentError::SwapMismatch);
            return PresentPoll::Fault;
        }
        // Hardware Pending clears only after the old external reads retire.
        GPU_PRESENT.pending = 0;
        return PresentPoll::Retired;
    }
    PresentPoll::Idle
}
fn gpu_present_watch(channel: u16, expected: u16) {
    dev_send(
        SYSTEM_CONTROL_DEVICE,
        SYSCTL_WATCH_TARGET,
        sysctl_watch_target(DISPLAY_DEVICE, channel),
    );
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_WATCH_EXPECTED, expected);
}
fn gpu_present_wait_vblank() -> bool {
    if GPU_PRESENT.ready == 0 || GPU_PRESENT.error != PresentError::None {
        return gpu_present_fail(PresentError::DisplayUnavailable);
    }
    if GPU_PRESENT.pending != 0 {
        let mut status = gpu_present_poll();
        while status == PresentPoll::Pending {
            // A torn snapshot can also return retry. Sleep only on actual Pending;
            // the next poll still checks errors, target, format and read retirement.
            if dev_recv(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND) == 1 {
                gpu_present_watch(DISPLAY_SWAP_COMMAND, 1);
            }
            status = gpu_present_poll();
        }
        return status == PresentPoll::Retired;
    }
    let before = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
    while true {
        let after = dev_recv(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX);
        if after == before {
            // A change between this read and arming releases the watch immediately.
            gpu_present_watch(DISPLAY_FRAME_INDEX, before);
        } else {
            let status = gpu_present_poll();
            if status == PresentPoll::Fault {
                return false;
            }
            if status == PresentPoll::Idle {
                return true;
            }
        }
    }
    false
}

fn gpu_present_observe(mut out: Array<u16>) -> PresentObservation {
    if GPU_PRESENT.error != PresentError::None {
        return PresentObservation::Fault;
    }
    if GPU_PRESENT.ready == 0 {
        return PresentObservation::Closed;
    }
    let status = gpu_present_snapshot(out);
    if status == PresentSnapshot::Invalid {
        return PresentObservation::Fault;
    }
    if status == PresentSnapshot::Retry {
        return PresentObservation::Retry;
    }
    PresentObservation::Stable
}
