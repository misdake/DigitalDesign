//! Ordinary transformed mesh with bilinear RTT sampling.
mod scene;
use scene::renderer_rtt_bilinear;

fn main() {
    renderer_rtt_bilinear();
}
