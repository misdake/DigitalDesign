//! Main-clock wall time. Include device_abi alongside this module.
use crate::dsl_rt::*;

pub fn time_milliseconds() -> u16 {
    dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MILLISECONDS)
}

// Saturates at 65535 us. The application owns clear; nested users only read.
pub fn time_microseconds() -> u16 {
    dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MICROSECONDS)
}

pub fn time_clear_microseconds() {
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_MICROSECONDS, 1);
}

// Valid only when the true interval is less than 65536 ms.
pub fn time_elapsed_ms(start: u16, end: u16) -> u16 {
    end - start
}
