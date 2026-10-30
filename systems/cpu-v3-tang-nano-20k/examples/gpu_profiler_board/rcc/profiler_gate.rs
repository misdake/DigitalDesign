//! Diagnostic stimulus uses the same driver/UART implementation as ordinary builds.
use crate::dsl_rt::*;
mod boot_resources;
use boot_resources::*;
mod profiler_runtime;
use profiler_runtime::*;
pub fn profiler_gate(descriptor: Array<u16>) -> u16 {
    profiler_gate_run(descriptor, GPU_PROFILER_AUTOMATIC_TEST != 0)
}
