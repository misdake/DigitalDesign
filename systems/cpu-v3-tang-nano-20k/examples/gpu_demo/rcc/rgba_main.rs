//! RGBA4444/mip panels over the classic opaque box, rabbit and RGB triangle.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
mod gpu_board;
use gpu_board::*;
mod boot_resources;
use boot_resources::*;
mod motion;
use motion::*;
mod gamepad;
use gamepad::*;
mod time;
use time::*;

fn panel_model(x: fix32) -> Model {
    Model::new(demo_pose(
        demo_point(x, fix32::from_words(26214, 65535), fix32::from_int(1)),
        demo_zero(),
        demo_scale(),
    ))
}

fn render_scene(pass: &mut Pass, camera: &Camera) {
    let room = demo_mesh_part(0, 6);
    let rgb = demo_mesh_part(6, 1);
    let rabbit = demo_mesh_part(7, 17);
    let panel = demo_mesh_part(24, 1);
    let model = Model::new(demo_pose(demo_zero(), demo_zero(), demo_scale()));
    let full_panel = panel_model(fix32::from_words(62259, 65534));
    let half_panel = panel_model(fix32::from_words(3277, 0));
    let unlit = demo_material(demo_white(), true);
    let mut lit = demo_material(demo_white(), false);
    lit.set_response(fix32::from_words(16384, 0), fix32::from_words(49152, 0));
    lit.set_specular(
        Rgb8 {
            r: 160,
            g: 160,
            b: 160,
        },
        Shininess::S32,
    );
    let half = demo_material(
        Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 128,
        },
        true,
    );
    let brick = demo_texture();
    let brick_view = brick.view();
    let alpha = Texture::from_native(TextureDesc {
        memory: unsafe { GpuMemory::from_sectors(DEMO_ALPHA_SECTOR, DEMO_ALPHA_SECTORS) },
        size_log2: 7,
        full_mip: true,
        format: TextureFormat::Rgba4444,
    });
    let alpha_view = alpha.view();
    let mut state = demo_state();
    state.set_depth(DepthFunc::Less, true);
    state.set_filter(TextureFilter::Trilinear);
    state.set_lighting(demo_lighting(demo_point(
        fix32::from_words(36045, 65535),
        fix32::from_words(52429, 0),
        fix32::from_words(6554, 1),
    )));
    pass.set_camera(camera);
    pass.set_state(&state);
    pass.draw(&room, &model, &unlit, &brick_view);
    pass.draw_colored(&rgb, &model, &unlit);
    pass.draw(&rabbit, &model, &lit, &brick_view);
    // Transparent geometry follows all opaque draws and never writes depth.
    state.set_depth(DepthFunc::Less, false);
    state.set_blend(BlendMode::SrcOver);
    pass.set_state(&state);
    pass.draw(&panel, &full_panel, &unlit, &alpha_view);
    pass.draw(&panel, &half_panel, &half, &alpha_view);
}

fn run(limit: u16) {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut buffer = gpu.command_buffer();
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open_with_format(DisplayFormat::LinearRgb565);
    let mut camera = demo_camera();
    let mut motion = demo_motion_init(time_milliseconds());
    camera.set_pose(motion.eye, demo_motion_angles(motion));
    gamepad_init();
    let mut frame: u16 = 0;
    loop {
        gamepad_capture();
        motion = demo_motion_update(motion, gamepad_take_frame(), time_milliseconds());
        if motion.changed {
            camera.set_pose(motion.eye, demo_motion_angles(motion));
        }
        buffer.reset();
        if frame == 0 {
            buffer.invalidate_textures();
        }
        if frame & 1 == 0 {
            let mut pass = buffer.pass(
                &color_b,
                &depth,
                ColorLoad::clear(Rgb565::from_raw(0)),
                DepthLoad::clear(65535),
            );
            render_scene(&mut pass, &camera);
        } else {
            let mut pass = buffer.pass(
                &color_a,
                &depth,
                ColorLoad::clear(Rgb565::from_raw(0)),
                DepthLoad::clear(65535),
            );
            render_scene(&mut pass, &camera);
        }
        gpu.submit(buffer);
        buffer = gpu.wait();
        if frame & 1 == 0 {
            display.set_next_swap(&color_b);
        } else {
            display.set_next_swap(&color_a);
        }
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
