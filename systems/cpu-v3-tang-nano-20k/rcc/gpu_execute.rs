//! Single-CB ownership, publication and Device4 execution. Capture owns its hook.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_record;
use gpu_record::*;
mod gpu_capture_abi;
use gpu_capture_abi::*;

// phase/owner/base/count/page/error; configured/taken; sealed capacities,
// padding, push bounds and fact count. Words14/15 remain spare.
static GPU_EXECUTION: Buf<u16, 16> = Buf::new([0; 16]);
const GPU_EXEC_CONFIGURED: u16 = 6;
const GPU_EXEC_TAKEN: u16 = 7;
const GPU_EXEC_IDLE: u16 = 1;
const GPU_EXEC_HELD: u16 = 2;
const GPU_EXEC_PREPARED: u16 = 3;
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
fn gpu_execute_init() -> bool {
    let status = dev_recv(GPU_DEVICE, GPU_STATUS);
    if status & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(1);
    }
    let mut i: u16 = 0;
    while i < 16 {
        GPU_EXECUTION[i] = 0;
        i += 1;
    }
    GPU_EXECUTION[4u16] = read_dseg_page(3);
    GPU_EXECUTION[0u16] = GPU_EXEC_IDLE;
    true
}
fn gpu_execute_prepare(slot: u16) -> bool {
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
    GPU_EXECUTION[8u16] = c[GPU_RECORD_COMMAND_CAPACITY];
    GPU_EXECUTION[9u16] = c[GPU_RECORD_PADDED];
    GPU_EXECUTION[10u16] = c[GPU_RECORD_PUSH_BASE];
    GPU_EXECUTION[11u16] = c[GPU_RECORD_PUSH_CAPACITY];
    GPU_EXECUTION[12u16] = c[GPU_RECORD_PUSH_USED];
    GPU_EXECUTION[13u16] = c[GPU_RECORD_FACT_COUNT];
    if read_dseg_page(3) != GPU_EXECUTION[4u16] {
        return gpu_execute_fail(5);
    }
    if dcache_invalidate_all() != CACHE_MAINTENANCE_STATUS_SUCCESS {
        return gpu_execute_fail(6);
    }
    if dev_recv(GPU_DEVICE, GPU_STATUS) & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(3);
    }
    GPU_EXECUTION[0u16] = GPU_EXEC_PREPARED;
    // C's pre-submit hook belongs here, after this function and before start.
    true
}
fn gpu_execute_prepared_matches(slot: u16) -> bool {
    if slot >= 2 || GPU_EXECUTION[0u16] != GPU_EXEC_PREPARED || GPU_EXECUTION[1u16] != slot + 1 {
        return false;
    }
    let c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_SEALED
        || c[GPU_RECORD_COMMAND_BASE] != GPU_EXECUTION[2u16]
        || (c[GPU_RECORD_COMMAND_USED] >> 1) - 1 != GPU_EXECUTION[3u16]
        || c[GPU_RECORD_COMMAND_CAPACITY] != GPU_EXECUTION[8u16]
        || c[GPU_RECORD_PADDED] != GPU_EXECUTION[9u16]
        || c[GPU_RECORD_PUSH_BASE] != GPU_EXECUTION[10u16]
        || c[GPU_RECORD_PUSH_CAPACITY] != GPU_EXECUTION[11u16]
        || c[GPU_RECORD_PUSH_USED] != GPU_EXECUTION[12u16]
        || c[GPU_RECORD_FACT_COUNT] != GPU_EXECUTION[13u16]
        || c[GPU_RECORD_FACT_COUNT] > GPU_RECORD_FACT_LIMIT
        || read_dseg_page(3) != GPU_EXECUTION[4u16]
    {
        return false;
    }
    true
}
// Read-only pre-start lease health, never retirement or fault-state mutation.
fn gpu_execute_capture_health(slot: u16) -> u16 {
    if !gpu_execute_prepared_matches(slot) {
        return 1;
    }
    if dev_recv(GPU_DEVICE, GPU_STATUS) & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return 2;
    }
    0
}
// Caller supplies twelve stable low-memory words; failure leaves them untouched.
fn gpu_execute_capture_source(slot: u16, mut out: Array<u16>) -> bool {
    if gpu_execute_capture_health(slot) != 0 {
        return false;
    }
    let c = gpu_record_data(slot);
    out[0u16] = 1;
    out[1u16] = slot;
    out[2u16] = c[GPU_RECORD_COMMAND_BASE];
    out[3u16] = c[GPU_RECORD_COMMAND_CAPACITY];
    out[4u16] = c[GPU_RECORD_COMMAND_USED];
    out[5u16] = c[GPU_RECORD_PADDED];
    out[6u16] = c[GPU_RECORD_PUSH_BASE];
    out[7u16] = c[GPU_RECORD_PUSH_CAPACITY];
    out[8u16] = c[GPU_RECORD_PUSH_USED];
    out[9u16] = c[GPU_RECORD_FACT_COUNT];
    out[10u16] = GPU_CAPTURE_RTT400_COMMAND_ABI;
    out[11u16] = 2;
    true
}
fn gpu_execute_start(slot: u16, republish: bool) -> bool {
    if !gpu_execute_prepared_matches(slot) {
        return gpu_execute_fail(7);
    }
    if republish && dcache_invalidate_all() != CACHE_MAINTENANCE_STATUS_SUCCESS {
        return gpu_execute_fail(6);
    }
    if dev_recv(GPU_DEVICE, GPU_STATUS) & (GPU_STATUS_BUSY | GPU_STATUS_TRANSPORT_ERROR) != 0 {
        return gpu_execute_fail(3);
    }
    GPU_EXECUTION[0u16] = GPU_EXEC_RUNNING;
    dev_send(GPU_DEVICE, GPU_COMMAND_COUNT, GPU_EXECUTION[3u16]);
    dev_send(GPU_DEVICE, GPU_COMMAND_START, GPU_EXECUTION[2u16]);
    true
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
