//! Full-screen nearest RTT demo.
mod boot_resources;
mod gpu_api;
mod gpu_board;
mod gpu_demo_api;
mod scene;

// The checked entry selector calls these functions by name.
use crate::scene::{
    renderer_rtt_bilinear, renderer_rtt_nearest, renderer_rtt_profiler, renderer_rtt_two_updates,
};

fn main() {
    renderer_rtt_nearest();
}
