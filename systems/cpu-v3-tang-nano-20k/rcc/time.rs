//! Main-clock wall time. Include device_abi alongside this module.
use crate::dsl_rt::*;

pub fn time_milliseconds() -> u16 {
    dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MILLISECONDS)
}

// Valid only when the true interval is less than 65536 ms.
pub fn time_elapsed_ms(start: u16, end: u16) -> u16 {
    end - start
}
