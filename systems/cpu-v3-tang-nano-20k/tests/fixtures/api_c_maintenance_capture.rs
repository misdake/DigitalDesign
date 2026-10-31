//! Tiny maintenance capture/dump path through the actual public owner.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
pub fn maintenance_capture_three_submissions() {
    let mut gpu = Gpu::init(demo_config());
    let mut commands = gpu.command_buffer();
    let mut count: u16 = 0;
    while count < 3 {
        commands.invalidate_textures();
        gpu.submit(commands);
        commands = gpu.wait();
        commands.reset();
        count += 1;
    }
    halt(0x600d);
}
