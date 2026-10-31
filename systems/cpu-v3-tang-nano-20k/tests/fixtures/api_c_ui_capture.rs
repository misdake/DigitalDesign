//! One small public UI recording; full physical Clear and real capture remain enabled.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;

pub fn ui_capture_once() {
    let mut gpu = Gpu::init(demo_config());
    let mut commands = gpu.command_buffer();
    let color = demo_color_b();
    let depth = demo_depth();
    let mut display = Display::open();
    let mut grid = demo_grid(false);
    demo_number(&mut grid, 0, 0, 1, 7);
    {
        let mut pass = commands.pass(
            &color,
            &depth,
            ColorLoad::clear(Rgb565::from_raw(0x1111)),
            DepthLoad::clear(65535),
        );
        pass.draw_ui(
            &grid,
            CellRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
    }
    gpu.submit(commands);
    commands = gpu.wait();
    display.set_next_swap(&color);
    display.wait_vblank();
    halt(0x600d);
}
