//! RGBA4444/mip panels over the classic opaque box, rabbit and RGB triangle.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_board;
use gpu_board::*;
mod motion;
use motion::*;
mod gamepad;
use gamepad::*;
mod time;
use time::*;

mod rgba_scene;
use rgba_scene::*;

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
        let mut back = color_b.ptr();
        if frame & 1 != 0 {
            back = color_a.ptr();
        }
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
