//! Main owns submission, retirement and presentation; scene owns its resources.
mod boot_resources;
mod gpu_api;
mod scene;
mod surface;

use crate::boot_resources::{HELLO_ATLAS, HELLO_FRAMES};
use crate::dsl_rt::*;
use crate::gpu_api::{
    ColorBuffer, ColorBufferPtr, ColorLoad, DepthBuffer, DepthLoad, Display, Gpu, Rgb565, UiAtlas,
    UiGrid,
};
use crate::scene::Scene;
use crate::surface::{DepthSurface, Rgb565Surface, UiGridStorage};

#[static_buffer]
const COLOR_A: Rgb565Surface = Rgb565Surface::allocate();
#[static_buffer]
const COLOR_B: Rgb565Surface = Rgb565Surface::allocate();
#[static_buffer]
const DEPTH: DepthSurface = DepthSurface::allocate();
#[static_buffer]
const COMMANDS: CommandMemory = CommandMemory::allocate(4096, 4096);
#[static_buffer]
const UI: UiGridStorage = UiGridStorage::allocate();

fn main() {
    let mut gpu = Gpu::init(GpuConfig::single(COMMANDS));
    let mut cb = gpu.command_buffer();
    let color_a = ColorBuffer::from_surface(COLOR_A);
    let color_b = ColorBuffer::from_surface(COLOR_B);
    let depth = DepthBuffer::from_surface(DEPTH);
    let atlas = UiAtlas::from_desc(HELLO_ATLAS);
    let mut grid = UiGrid::from_storage(UI, &atlas);
    grid.fill(255);
    grid.write_text(2, 1, b"HELLO GPU");
    let scene = Scene::new();
    let mut display = Display::open();
    let mut frame: u16 = 0;
    loop {
        let back: ColorBufferPtr = if frame & 1 == 0 {
            color_b.ptr()
        } else {
            color_a.ptr()
        };
        cb.reset();
        {
            let mut pass = cb.pass(
                back,
                depth.ptr(),
                ColorLoad::clear(Rgb565::rgb(8, 8, 24)),
                DepthLoad::clear_far(),
            );
            scene.draw(&mut pass);
            pass.draw_ui(grid.ptr(), CellRect::full_grid());
        }
        gpu.submit(cb);
        cb = gpu.wait();
        display.set_next_swap(back);
        display.wait_vblank();
        frame += 1;
        if HELLO_FRAMES != 0 && frame == HELLO_FRAMES {
            break;
        }
    }
    halt(0x600d);
}
