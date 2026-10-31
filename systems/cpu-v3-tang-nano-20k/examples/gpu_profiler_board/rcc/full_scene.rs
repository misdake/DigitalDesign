//! Canonical box, RGB triangle and rabbit; explicit recording and completion.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
mod profiler_clock;
use profiler_clock::*;
mod time;
use time::*;

pub fn profiler_run_full(limit: u16) {
    let mut gpu = Gpu::init(demo_config());
    let mut buffer = gpu.command_buffer();
    let color_a = demo_color_a();
    let color_b = demo_color_b();
    let depth = demo_depth();
    let mut display = Display::open();
    let mut camera = demo_camera();
    camera.set_pose(
        demo_point(
            fix32::zero(),
            fix32::from_words(14418, 0),
            fix32::from_words(52429, 2),
        ),
        demo_point(
            fix32::from_words(59932, 65535),
            fix32::zero(),
            fix32::zero(),
        ),
    );
    let mut state = demo_state();
    state.set_lighting(demo_lighting(demo_point(
        fix32::from_words(36045, 65535),
        fix32::from_words(52429, 0),
        fix32::from_words(6554, 1),
    )));
    let room = demo_mesh_part(0, 6);
    let rgb = demo_mesh_part(6, 1);
    let rabbit = demo_mesh_part(7, 17);
    let model = Model::new(demo_pose(demo_zero(), demo_zero(), demo_scale()));
    let unlit = demo_material(demo_white(), true);
    let mut lit = demo_material(demo_white(), false);
    lit.set_response(fix32::from_words(16384, 0), fix32::from_words(49152, 0));
    lit.set_specular(
        Rgb8 {
            r: 160,
            g: 160,
            b: 160,
        },
        Shininess::S12,
    );
    let texture = demo_texture();
    let view = texture.view();
    let mut grid = demo_grid(false);
    let mut count: u16 = 0;
    let mut observed: u16 = 65535;
    loop {
        demo_number(&mut grid, 0, 0, 8, observed);
        buffer.reset();
        if count == 0 {
            buffer.invalidate_textures();
        }
        if count & 1 == 0 {
            let mut pass = buffer.pass(
                &color_b,
                &depth,
                ColorLoad::clear(Rgb565::from_raw(0x1111)),
                DepthLoad::clear(65535),
            );
            pass.set_camera(&camera);
            pass.set_state(&state);
            pass.draw(&room, &model, &unlit, &view);
            pass.draw_colored(&rgb, &model, &unlit);
            pass.draw(&rabbit, &model, &lit, &view);
            pass.draw_ui(
                &grid,
                CellRect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 1,
                },
            );
        } else {
            let mut pass = buffer.pass(
                &color_a,
                &depth,
                ColorLoad::clear(Rgb565::from_raw(0x1111)),
                DepthLoad::clear(65535),
            );
            pass.set_camera(&camera);
            pass.set_state(&state);
            pass.draw(&room, &model, &unlit, &view);
            pass.draw_colored(&rgb, &model, &unlit);
            pass.draw(&rabbit, &model, &lit, &view);
            pass.draw_ui(
                &grid,
                CellRect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 1,
                },
            );
        }
        let resume_before = profiler_resume_epoch();
        time_clear_microseconds();
        gpu.submit(buffer);
        buffer = gpu.wait();
        observed = time_microseconds();
        if profiler_resume_epoch() != resume_before {
            observed = 65535;
        }
        if count & 1 == 0 {
            display.set_next_swap(&color_b);
        } else {
            display.set_next_swap(&color_a);
        }
        display.wait_vblank();
        count += 1;
        if limit != 0 && count == limit {
            break;
        }
    }
}
pub fn profiler_full_three_frames() {
    profiler_run_full(3);
    halt(0x600d);
}
