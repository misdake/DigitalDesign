//! Single-CB ownership, publication and Device4 execution.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_record;
use gpu_record::*;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExecutionPhase {
    Uninitialized = 0,
    Idle = 1,
    Held = 2,
    Running = 4,
    Fault = 5,
}

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExecutionError {
    None = 0,
    DeviceNotIdleAtInit = 1,
    InvalidSubmission = 2,
    UnexpectedDeviceStatus = 3,
    InvalidCommandStream = 4,
    DsegMappingMismatch = 5,
    CachePublicationFailed = 6,
    NotInitialized = 8,
}

// phase/owner/base/count/page/error; configured/taken; live public controller.
#[repr(C)]
struct GpuExecutionState {
    phase: ExecutionPhase,
    owner: u16,
    base_sector: u16,
    count_minus_one: u16,
    dseg3: u16,
    error: ExecutionError,
    configured_slots: u16,
    taken_slots: u16,
    controller: u16,
}
static GPU_EXECUTION: GpuExecutionState = GpuExecutionState {
    phase: ExecutionPhase::Uninitialized,
    owner: 0,
    base_sector: 0,
    count_minus_one: 0,
    dseg3: 0,
    error: ExecutionError::None,
    configured_slots: 0,
    taken_slots: 0,
    controller: 0,
};

fn gpu_execute_fail(code: ExecutionError) -> bool {
    if GPU_EXECUTION.error == ExecutionError::None {
        GPU_EXECUTION.error = code;
    }
    GPU_EXECUTION.phase = ExecutionPhase::Fault;
    false
}
// Re-init is a caller-owned context boundary, not a wait/recovery operation.
fn gpu_execute_can_init() -> bool {
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if status & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(ExecutionError::DeviceNotIdleAtInit);
    }
    true
}
fn gpu_execute_init() {
    // Bulk reset of the repr(C) word layout; ordinary access uses named fields.
    let mut i: u16 = 0;
    while i < 9 {
        unsafe {
            addr_of(&GPU_EXECUTION).write(i as i16, 0);
        }
        i += 1;
    }
    GPU_EXECUTION.dseg3 = read_dseg_page(3);
    GPU_EXECUTION.phase = ExecutionPhase::Idle;
}
fn gpu_execute_publish(slot: u16) -> bool {
    if GPU_EXECUTION.phase != ExecutionPhase::Idle || slot >= 2 {
        return gpu_execute_fail(ExecutionError::InvalidSubmission);
    }
    // Ownership is retained even if seal/publication/status validation fails.
    GPU_EXECUTION.owner = slot + 1;
    GPU_EXECUTION.phase = ExecutionPhase::Held;
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if status & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(ExecutionError::UnexpectedDeviceStatus);
    }
    if !gpu_record_seal(slot) {
        return gpu_execute_fail(ExecutionError::InvalidCommandStream);
    }
    let c = gpu_record_data(slot);
    let words = c[GPU_RECORD_COMMAND_USED];
    if words < 2 || words > 32768 || words & 1 != 0 {
        return gpu_execute_fail(ExecutionError::InvalidCommandStream);
    }
    GPU_EXECUTION.base_sector = c[GPU_RECORD_COMMAND_BASE];
    GPU_EXECUTION.count_minus_one = (words >> 1) - 1;
    if read_dseg_page(3) != GPU_EXECUTION.dseg3 {
        return gpu_execute_fail(ExecutionError::DsegMappingMismatch);
    }
    if dcache_invalidate_all() != CACHE_MAINTENANCE_STATUS_SUCCESS {
        return gpu_execute_fail(ExecutionError::CachePublicationFailed);
    }
    if dev_recv(GPU_DEVICE, GPU_STATUS) & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(ExecutionError::UnexpectedDeviceStatus);
    }
    true
}
// Publication owns every check before the sole COUNT/START pair.
fn gpu_execute_submit(slot: u16) -> bool {
    if !gpu_execute_publish(slot) {
        return false;
    }
    gpu_execute_launch();
    true
}
fn gpu_execute_launch() {
    GPU_EXECUTION.phase = ExecutionPhase::Running;
    dev_send(GPU_DEVICE, GPU_COMMAND_COUNT, GPU_EXECUTION.count_minus_one);
    dev_send(GPU_DEVICE, GPU_COMMAND_START, GPU_EXECUTION.base_sector);
}
// Internal result: 0=no completion, slot+1=retired, 65535=fault with owner retained.
fn gpu_execute_poll() -> u16 {
    if GPU_EXECUTION.phase == ExecutionPhase::Fault {
        return 65535;
    }
    if GPU_EXECUTION.phase == ExecutionPhase::Uninitialized {
        gpu_execute_fail(ExecutionError::NotInitialized);
        return 65535;
    }
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if status & GPU_STATUS_TRANSPORT_ERROR != 0
        || (status & GPU_STATUS_BUSY != 0 && GPU_EXECUTION.phase != ExecutionPhase::Running)
    {
        gpu_execute_fail(ExecutionError::UnexpectedDeviceStatus);
        return 65535;
    }
    if GPU_EXECUTION.phase != ExecutionPhase::Running || status & GPU_STATUS_BUSY != 0 {
        return 0;
    }
    let completed = GPU_EXECUTION.owner;
    GPU_EXECUTION.owner = 0;
    GPU_EXECUTION.phase = ExecutionPhase::Idle;
    completed
}
