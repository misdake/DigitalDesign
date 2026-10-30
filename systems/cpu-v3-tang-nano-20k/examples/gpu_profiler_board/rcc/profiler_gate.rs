//! Diagnostic stimulus uses the same driver/UART implementation as ordinary builds.
use crate::dsl_rt::*;
mod boot_resources;
use boot_resources::*;
mod profiler_runtime;
use profiler_runtime::*;
mod gpu_capture_abi;
use gpu_capture_abi::*;
pub fn profiler_gate(descriptor: Array<u16>) -> u16 {
    // This diagnostic reserves 0x600000 for its producer's offscreen color.
    // Let it finish; the consumer lease then freezes the actual texture input.
    if GPU_PROFILER_SHORT_CASE == 1
        && descriptor[GPU_CAPTURE_H_COLOR_BASE] == 0
        && descriptor[GPU_CAPTURE_H_COLOR_BASE + 1] == 0x60
    {
        return GPU_CAPTURE_CONTINUE;
    }
    profiler_gate_run(descriptor, GPU_PROFILER_AUTOMATIC_TEST != 0)
}
