//! Single-CB ownership, publication and Device4 execution.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_record;
use gpu_record::*;

// phase/owner/base/count/page/error; configured/taken; live public controller.
static GPU_EXECUTION: Buf<u16, 9> = Buf::new([0; 9]);
const GPU_EXEC_CONFIGURED: u16 = 6;
const GPU_EXEC_TAKEN: u16 = 7;
const GPU_EXEC_CONTROLLER: u16 = 8;
const GPU_EXEC_IDLE: u16 = 1;
const GPU_EXEC_HELD: u16 = 2;
const GPU_EXEC_RUNNING: u16 = 4;
const GPU_EXEC_FAULT: u16 = 5;

fn gpu_execute_fail(code: u16) -> bool {
    if GPU_EXECUTION[5u16] == 0 {
        GPU_EXECUTION[5u16] = code;
    }
    GPU_EXECUTION[0u16] = GPU_EXEC_FAULT;
    false
}
// Re-init is a caller-owned context boundary, not a wait/recovery operation.
fn gpu_execute_can_init() -> bool {
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if status & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(1);
    }
    true
}
fn gpu_execute_init() {
    let mut i: u16 = 0;
    while i < 9 {
        GPU_EXECUTION[i] = 0;
        i += 1;
    }
    GPU_EXECUTION[4u16] = read_dseg_page(3);
    GPU_EXECUTION[0u16] = GPU_EXEC_IDLE;
}
fn gpu_execute_publish(slot: u16) -> bool {
    if GPU_EXECUTION[0u16] != GPU_EXEC_IDLE || slot >= 2 {
        return gpu_execute_fail(2);
    }
    // Ownership is retained even if seal/publication/status validation fails.
    GPU_EXECUTION[1u16] = slot + 1;
    GPU_EXECUTION[0u16] = GPU_EXEC_HELD;
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if status & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(3);
    }
    if !gpu_record_seal(slot) {
        return gpu_execute_fail(4);
    }
    let c = gpu_record_data(slot);
    let words = c[GPU_RECORD_COMMAND_USED];
    if words < 2 || words > 32768 || words & 1 != 0 {
        return gpu_execute_fail(4);
    }
    GPU_EXECUTION[2u16] = c[GPU_RECORD_COMMAND_BASE];
    GPU_EXECUTION[3u16] = (words >> 1) - 1;
    if read_dseg_page(3) != GPU_EXECUTION[4u16] {
        return gpu_execute_fail(5);
    }
    if dcache_invalidate_all() != CACHE_MAINTENANCE_STATUS_SUCCESS {
        return gpu_execute_fail(6);
    }
    if dev_recv(GPU_DEVICE, GPU_STATUS) & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(3);
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
    GPU_EXECUTION[0u16] = GPU_EXEC_RUNNING;
    dev_send(GPU_DEVICE, GPU_COMMAND_COUNT, GPU_EXECUTION[3u16]);
    dev_send(GPU_DEVICE, GPU_COMMAND_START, GPU_EXECUTION[2u16]);
}
// Internal result: 0=no completion, slot+1=retired, 65535=fault with owner retained.
fn gpu_execute_poll() -> u16 {
    if GPU_EXECUTION[0u16] == GPU_EXEC_FAULT {
        return 65535;
    }
    if GPU_EXECUTION[0u16] == 0 {
        gpu_execute_fail(8);
        return 65535;
    }
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if status & GPU_STATUS_TRANSPORT_ERROR != 0
        || (status & GPU_STATUS_BUSY != 0 && GPU_EXECUTION[0u16] != GPU_EXEC_RUNNING)
    {
        gpu_execute_fail(3);
        return 65535;
    }
    if GPU_EXECUTION[0u16] != GPU_EXEC_RUNNING || status & GPU_STATUS_BUSY != 0 {
        return 0;
    }
    let completed = GPU_EXECUTION[1u16];
    GPU_EXECUTION[1u16] = 0;
    GPU_EXECUTION[0u16] = GPU_EXEC_IDLE;
    completed
}
