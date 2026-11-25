//! Ordinary transformed mesh with bilinear RTT sampling.
mod boot_resources;
mod gpu_api;
mod gpu_board;
mod gpu_demo_api;
mod scene;

use crate::scene::renderer_rtt_bilinear;

fn main() {
    renderer_rtt_bilinear();
}
