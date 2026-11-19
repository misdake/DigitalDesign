//! Fixed low-memory first fault. No allocation, owner release or recovery.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;

// Magic, site, code, detail0, detail1, DSEG3, then site-specific words.
static GPU_API_FAULT: Buf<u16, 16> = Buf::new([0; 16]);
fn gpu_api_handle_fault(
    slot: u16,
    kind: u16,
    expected_low: u16,
    expected_high: u16,
    current_low: u16,
    current_high: u16,
) {
    if GPU_API_FAULT[0u16] == 0 {
        GPU_API_FAULT[6u16] = expected_low;
        GPU_API_FAULT[7u16] = expected_high;
        GPU_API_FAULT[8u16] = current_low;
        GPU_API_FAULT[9u16] = current_high;
    }
    gpu_api_fail(16, 1, slot, kind);
}
fn gpu_api_uart_byte(value: u16) {
    while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, value & 255);
}
fn gpu_api_fail(site: u16, code: u16, detail0: u16, detail1: u16) {
    if GPU_API_FAULT[0u16] == 0 {
        GPU_API_FAULT[1u16] = site;
        GPU_API_FAULT[2u16] = code;
        GPU_API_FAULT[3u16] = detail0;
        GPU_API_FAULT[4u16] = detail1;
        GPU_API_FAULT[5u16] = read_dseg_page(3);
        GPU_API_FAULT[0u16] = 0xfa17;
    }
    let mut i: u16 = 0;
    while i < 16 {
        let word = GPU_API_FAULT[i];
        gpu_api_uart_byte(word & 255);
        gpu_api_uart_byte(word >> 8);
        i += 1;
    }
    // Ready means the last stop bit has completed, not merely queued data.
    while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
    halt(0xe001);
}
