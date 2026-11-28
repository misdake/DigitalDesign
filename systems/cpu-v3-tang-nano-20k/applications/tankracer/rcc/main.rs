//! Static-map playback through the ordinary application and Display path.
use crate::dsl_rt::*;
mod boot_resources;
mod gpu_api;
mod gpu_board;
mod playback;
mod prepared_map;
mod range_map;
mod unlit_world;
mod world;
use boot_resources::*;
use gpu_api::*;
use gpu_board::*;
use playback::Playback;
use unlit_world::UnlitWorld;

fn main() {
    if TANKRACER_PREPARED != 0 {
        if RANGE_LIMIT != 0 {
            range_map::run();
            return;
        }
        prepared_map::run();
        return;
    }
    let mut gpu = Gpu::init(board_gpu_config());
    let mut commands = gpu.command_buffer();
    let mut recording = gpu.command_buffer();
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let mut last_present = display.frame_index();
    let mut world = UnlitWorld::new();
    let mut playback = Playback::new();
    world.record(
        &mut commands,
        playback.next(),
        0,
        color_b.ptr(),
        depth.ptr(),
    );
    gpu.submit(commands);
    let mut completed_color = color_b.ptr();
    let mut odd = true;
    loop {
        let mut back = color_b.ptr();
        if odd {
            back = color_a.ptr();
        }
        // CPU records the next view while the GPU owns the other CB's snapshots.
        // Recording declares its targets; submission waits for their release.
        let mut bank: u16 = 0;
        if odd {
            bank = 1;
        }
        world.record(&mut recording, playback.next(), bank, back, depth.ptr());
        let completed = gpu.wait();
        // Keep one scanout between swaps. A frame that already ran past that
        // scanout can queue immediately for the next edge.
        if display.frame_index() == last_present {
            display.wait_vblank();
        }
        display.set_next_swap(completed_color);
        display.wait_vblank();
        last_present = display.frame_index();
        // The next colour plane is now released by Display; depth is GPU-idle.
        gpu.submit(recording);
        recording = completed;
        completed_color = back;
        odd = !odd;
    }
}
