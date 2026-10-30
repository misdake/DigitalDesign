//! Repeated producer/consumer stages; only consumer submissions enter the gate.
use crate::dsl_rt::*;
mod scene;
use scene::*;
fn main() {
    renderer_rtt_profiler();
}
