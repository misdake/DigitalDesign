//! One driver, one in-flight CB. No ACK, local recovery or Display ownership.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_command_buffer;
use gpu_command_buffer::*;
mod gpu_encode;
use gpu_encode::*;
mod gpu_window;
use gpu_window::*;

pub const GPU_IDLE: u16 = 0;
pub const GPU_PENDING: u16 = 1;
pub const GPU_COMPLETED: u16 = 2;
pub const GPU_FAULT: u16 = 3;
pub const GPU_TIMEOUT: u16 = 4;
pub const GPU_REJECTED: u16 = 5;
pub const GPU_CACHE_ERROR: u16 = 6;
pub const GPU_NOT_INITIALIZED: u16 = 7;
// Phase: 0 uninitialized, 1 idle, 2 owned, 3 sticky fault, 4 unknown busy.
// Only this module writes the driver record; target code must not bypass it.
static GPU_DRIVER: Buf<u16, 2> = Buf::new([0; 2]);
pub fn gpu_driver_can_build() -> bool {
    GPU_DRIVER.as_array()[0u16] == 1 && !gpu_memory_faulted()
}

pub fn gpu_driver_init() -> u16 {
    let mut d = GPU_DRIVER.as_array();
    if d[0u16] != 0 {
        return GPU_REJECTED;
    }
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if gpu_memory_faulted() || status & GPU_STATUS_TRANSPORT_ERROR != 0 {
        d[0u16] = 3;
        return GPU_FAULT;
    }
    if status & GPU_STATUS_BUSY != 0 {
        d[0u16] = 4;
        return GPU_PENDING;
    }
    d[0u16] = 1;
    GPU_IDLE
}
pub fn gpu_driver_owned_buffer() -> Ptr {
    Ptr::from_addr(GPU_DRIVER.as_array()[1u16])
}

pub fn gpu_driver_poll(mut cb: Array<GpuBuffer>) -> u16 {
    let mut d = GPU_DRIVER.as_array();
    if d[0u16] == 0 {
        return GPU_NOT_INITIALIZED;
    }
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    // Error wins even with busy=0; neither fault nor timeout returns ownership.
    if gpu_memory_faulted() || d[0u16] == 3 || status & GPU_STATUS_TRANSPORT_ERROR != 0 {
        d[0u16] = 3;
        return GPU_FAULT;
    }
    if d[0u16] == 2 && d[1u16] != cb.as_ptr().addr() {
        return GPU_REJECTED;
    }
    if status & GPU_STATUS_BUSY != 0 {
        return GPU_PENDING;
    }
    if d[0u16] == 2 {
        if cb[0u16].state != GPU_CB_INFLIGHT {
            d[0u16] = 3;
            return GPU_FAULT;
        }
        cb[0u16].state = GPU_CB_COMPLETED;
        d[1u16] = 0;
        d[0u16] = 1;
        return GPU_COMPLETED;
    }
    d[0u16] = 1;
    GPU_IDLE
}
pub fn gpu_driver_wait(cb: Array<GpuBuffer>, limit: u16) -> u16 {
    let mut left = limit;
    while left != 0 {
        let result = gpu_driver_poll(cb);
        if result != GPU_PENDING {
            return result;
        }
        left -= 1;
    }
    GPU_TIMEOUT
}
pub fn gpu_driver_submit_idle(mut cb: Array<GpuBuffer>) -> u16 {
    let mut d = GPU_DRIVER.as_array();
    if d[0u16] == 0 {
        return GPU_NOT_INITIALIZED;
    }
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if gpu_memory_faulted() || d[0u16] == 3 || status & GPU_STATUS_TRANSPORT_ERROR != 0 {
        d[0u16] = 3;
        return GPU_FAULT;
    }
    if d[0u16] != 1 || status & GPU_STATUS_BUSY != 0 {
        return GPU_REJECTED;
    }
    if cb[0u16].high_slot == 0
        || cb[0u16].state != GPU_CB_READY
        || cb.as_ptr().addr() == 0
        || !gpu_buffer_valid(cb)
    {
        return GPU_REJECTED;
    }
    let mut submission: GpuSubmission = GpuSubmission {
        count_minus_one: 0,
        base_sector: 0,
    };
    if !gpu_encode_submission(
        view_of(&mut submission),
        cb[0u16].commands.sector << 7,
        cb[0u16].commands.sector >> 9,
        cb[0u16].command_words << 1,
        cb[0u16].command_words >> 15,
    ) {
        return GPU_REJECTED;
    }
    // This intrinsic is the compiler memory/control barrier and blocks until
    // actual cache completion. A failure leaves the CB Ready and sends nothing.
    if dcache_invalidate_all() != CACHE_MAINTENANCE_STATUS_SUCCESS {
        d[0u16] = 3;
        return GPU_CACHE_ERROR;
    }
    let final_status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if final_status & GPU_STATUS_TRANSPORT_ERROR != 0 {
        d[0u16] = 3;
        return GPU_FAULT;
    }
    if final_status & GPU_STATUS_BUSY != 0 {
        return GPU_REJECTED;
    }
    // Serialize both SENDs and establish software ownership before either one.
    cb[0u16].state = GPU_CB_INFLIGHT;
    d[1u16] = cb.as_ptr().addr();
    d[0u16] = 2;
    dev_send(GPU_DEVICE, GPU_COMMAND_COUNT, submission.count_minus_one);
    dev_send(GPU_DEVICE, GPU_COMMAND_START, submission.base_sector);
    GPU_PENDING
}
