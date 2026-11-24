//! Fixed low-memory first fault. No allocation, owner release or recovery.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;

// Stable wire code: high byte is the site, low byte its error code.
#[repr(u16)]
#[derive(Copy, Clone)]
enum GpuFault {
    MemoryExtent = 0x0101,
    MemoryRange = 0x0102,
    MemoryTransfer = 0x0103,
    Initialization = 0x0201,
    CommandBufferUnavailable = 0x0202,
    SurfaceAllocation = 0x0301,
    SurfaceDescription = 0x0302,
    SurfaceRelease = 0x0303,
    RecorderReset = 0x0401,
    RecorderOperation = 0x0402,
    PassPhase = 0x0403,
    CameraBinding = 0x0404,
    OpenPassDrop = 0x0405,
    ResourceConflict = 0x0406,
    DisplayOpen = 0x0501,
    DisplaySwap = 0x0502,
    DisplayWait = 0x0503,
    SubmissionRejected = 0x0602,
    ExecutionFault = 0x0603,
    WaitWithoutSubmission = 0x0604,
    ObjectAllocation = 0x0701,
    ObjectValidation = 0x0702,
    ObjectRelease = 0x0703,
    MeshAllocation = 0x0801,
    MeshDescription = 0x0802,
    MeshRange = 0x0803,
    MeshRelease = 0x0804,
    DrawBindings = 0x0901,
    StaleHandle = 0x1001,
}

// Magic, site, code, detail0, detail1, DSEG3, then site-specific words.
#[repr(C)]
struct GpuFaultRecord {
    magic: u16,
    site: u16,
    code: u16,
    detail0: u16,
    detail1: u16,
    dseg3: u16,
    expected_low: u16,
    expected_high: u16,
    current_low: u16,
    current_high: u16,
    reserved: Buf<u16, 6>,
}
static GPU_API_FAULT: GpuFaultRecord = GpuFaultRecord {
    magic: 0,
    site: 0,
    code: 0,
    detail0: 0,
    detail1: 0,
    dseg3: 0,
    expected_low: 0,
    expected_high: 0,
    current_low: 0,
    current_high: 0,
    reserved: Buf::new([0; 6]),
};
fn gpu_api_handle_fault(
    slot: u16,
    kind: u16,
    expected_low: u16,
    expected_high: u16,
    current_low: u16,
    current_high: u16,
) {
    if GPU_API_FAULT.magic == 0 {
        GPU_API_FAULT.expected_low = expected_low;
        GPU_API_FAULT.expected_high = expected_high;
        GPU_API_FAULT.current_low = current_low;
        GPU_API_FAULT.current_high = current_high;
    }
    gpu_api_fail(GpuFault::StaleHandle, slot, kind);
}
fn gpu_api_uart_byte(value: u16) {
    while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, value & 255);
}
fn gpu_api_fail(fault: GpuFault, detail0: u16, detail1: u16) {
    let wire = fault as u16;
    if GPU_API_FAULT.magic == 0 {
        GPU_API_FAULT.site = wire >> 8;
        GPU_API_FAULT.code = wire & 255;
        GPU_API_FAULT.detail0 = detail0;
        GPU_API_FAULT.detail1 = detail1;
        GPU_API_FAULT.dseg3 = read_dseg_page(3);
        GPU_API_FAULT.magic = 0xfa17;
    }
    let mut i: u16 = 0;
    while i < 16 {
        let word = (unsafe { addr_of(&GPU_API_FAULT).as_u16_array() })[i];
        gpu_api_uart_byte(word & 255);
        gpu_api_uart_byte(word >> 8);
        i += 1;
    }
    // Ready means the last stop bit has completed, not merely queued data.
    while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
    halt(0xe001);
}
