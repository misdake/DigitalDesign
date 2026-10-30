//! Default driver capture hook; normal rendering continues until a pause edge.
use crate::dsl_rt::*;
mod profiler_runtime;
use profiler_runtime::*;
pub fn profiler_gate(descriptor: Array<u16>) -> u16 {
    profiler_gate_run(descriptor, false)
}
