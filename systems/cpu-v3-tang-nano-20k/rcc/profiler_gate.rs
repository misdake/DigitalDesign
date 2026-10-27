//! Ordinary renderer builds have no capture consumer or device side effects.
use crate::dsl_rt::*;
pub fn profiler_gate(descriptor: Array<u16>) -> u16 {
    0
}
