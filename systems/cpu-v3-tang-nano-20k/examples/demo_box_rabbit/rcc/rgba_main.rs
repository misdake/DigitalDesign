//! RGBA4444/mip panels over the classic opaque box, rabbit and RGB triangle.
mod boot_resources;
mod gamepad;
mod gpu_api;
mod gpu_board;
mod gpu_demo_api;
mod motion;
mod rgba_scene;
mod time;

use crate::dsl_rt::*;
use crate::gamepad::{gamepad_capture, gamepad_init, gamepad_take_frame};
use crate::gpu_api::{Camera, ColorBufferPtr, ColorLoad, DepthLoad, Display, Gpu, Rgb565};
use crate::gpu_board::{board_color_a, board_color_b, board_depth, board_gpu_config};
use crate::motion::DemoMotion;
use crate::rgba_scene::RgbaScene;
use crate::time::time_milliseconds;

fn run(limit: u16) {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut buffer = gpu.command_buffer();
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open_with_format(DisplayFormat::LinearRgb565);
    let scene = RgbaScene::new();
    let mut camera = Camera::new(CameraDesc::new(
        Vec3::zero(),
        Vec3::zero(),
        Projection::from_focal_pixels(fix!(200), fix!(199)),
    ));
    let mut motion = DemoMotion::new(time_milliseconds());
    camera.set_pose(motion.eye, motion.angles());
    gamepad_init();
    let mut frame: u16 = 0;
    loop {
        gamepad_capture();
        let input = gamepad_take_frame();
        if motion.update(&input, time_milliseconds()) {
            camera.set_pose(motion.eye, motion.angles());
        }
        buffer.reset();
        if frame == 0 {
            buffer.invalidate_textures();
        }
        let back: ColorBufferPtr = if frame & 1 != 0 {
            color_a.ptr()
        } else {
            color_b.ptr()
        };
        {
            let mut pass = buffer.pass(
                back,
                depth.ptr(),
                ColorLoad::clear(Rgb565::from_raw(0)),
                DepthLoad::clear_far(),
            );
            scene.draw(&mut pass, &camera);
        }
        gpu.submit(buffer);
        buffer = gpu.wait();
        display.set_next_swap(back);
        display.wait_vblank();
        frame += 1;
        if limit != 0 && frame == limit {
            break;
        }
    }
    halt(0x600d);
}

fn main() {
    run(0);
}

pub fn rgba_three_frames() {
    run(3);
}
