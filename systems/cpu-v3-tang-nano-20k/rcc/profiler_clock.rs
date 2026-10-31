//! Optional application diagnostics. The physical wall clock and input are unchanged.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
// Total paused milliseconds modulo16 bits; resume sequence modulo16 bits.
static PROFILER_CLOCK: Buf<u16, 2> = Buf::new([0; 2]);

pub fn profiler_resume_epoch() -> u16 {
    PROFILER_CLOCK[1u16]
}
pub fn profiler_active_milliseconds() -> u16 {
    dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MILLISECONDS) - PROFILER_CLOCK[0u16]
}
// The shared diagnostic consumer is the sole ordinary writer. Updating the
// epoch last makes one completed resume the application observation boundary.
pub fn profiler_clock_record_resume(begin: u16, end: u16) {
    PROFILER_CLOCK[0u16] += end - begin;
    PROFILER_CLOCK[1u16] += 1;
}
