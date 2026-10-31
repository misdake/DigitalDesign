//! Semantic edits and sequential Pass snapshots through the explicit public API.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
use gpu_demo_api::*;
mod profiler_clock;
use profiler_clock::*;
mod time;
use time::*;

fn reset_scene(
    camera: &mut Camera,
    left: &mut Model,
    opaque: &mut Material,
    state: &mut RenderState,
    alpha: &mut RenderState,
) {
    let z = fix32::zero();
    let one = fix32::from_int(1);
    let zero = demo_zero();
    let scale = demo_scale();

    camera.set_pose(zero, zero);
    camera.set_projection(Projection::from_focal_pixels(
        fix32::from_int(200),
        fix32::from_int(199),
    ));
    state.set_lighting(demo_lighting(demo_point(z, z, one)));
    alpha.set_lighting(demo_lighting(demo_point(z, z, one)));
    left.set_pose(demo_pose(
        demo_point(-one, z, fix32::from_int(-3)),
        zero,
        scale,
    ));
    opaque.set_uniform_color(demo_white());
    opaque.set_response(fix32::from_words(16384, 0), fix32::from_words(49152, 0));
    opaque.set_specular(
        Rgb8 {
            r: 160,
            g: 160,
            b: 160,
        },
        Shininess::S12,
    );
}
fn edit_scene(camera: &mut Camera, left: &mut Model, opaque: &mut Material) {
    let z = fix32::zero();
    let one = fix32::from_int(1);
    camera.set_pose(
        demo_point(fix32::from_words(16384, 0), z, z),
        demo_point(z, fix32::from_words(16384, 0), z),
    );
    camera.set_projection(Projection::from_focal_pixels(
        fix32::from_int(144),
        fix32::from_int(180),
    ));
    left.set_pose(demo_pose(
        demo_point(fix32::from_words(32768, 0), z, fix32::from_int(-3)),
        demo_point(z, z, fix32::from_words(8192, 0)),
        demo_point(fix32::from_words(16384, 1), one, one),
    ));
    opaque.set_uniform_color(Rgba8 {
        r: 32,
        g: 160,
        b: 255,
        a: 255,
    });
    opaque.set_response(fix32::from_words(16384, 0), fix32::from_words(49152, 0));
    opaque.set_specular(
        Rgb8 {
            r: 32,
            g: 64,
            b: 128,
        },
        Shininess::S8,
    );
}

fn run(limit: u16) {
    let mut gpu = Gpu::init(demo_config());
    let mut buffer = gpu.command_buffer();
    let color_a = demo_color_a();
    let color_b = demo_color_b();
    let depth = demo_depth();
    let mut display = Display::open();
    let z = fix32::zero();
    let one = fix32::from_int(1);
    let zero = demo_zero();
    let scale = demo_scale();
    let mut camera = demo_camera();
    let mesh = demo_mesh();
    let mut left = Model::new(demo_pose(
        demo_point(-one, z, fix32::from_int(-3)),
        zero,
        scale,
    ));
    let right = Model::new(demo_pose(
        demo_point(one, z, fix32::from_int(-4)),
        zero,
        scale,
    ));
    let mut opaque = demo_material(demo_white(), false);
    let glass = demo_material(
        Rgba8 {
            r: 255,
            g: 128,
            b: 64,
            a: 128,
        },
        false,
    );
    let mut state = demo_state();
    let mut alpha = demo_state();
    alpha.set_blend(BlendMode::SrcOver);
    let texture = demo_texture();
    let view = texture.view();
    let mut grid = demo_grid(false);
    let mut tick: u16 = 0;
    let mut observed: u16 = 65535;
    loop {
        let phase = tick % 11;
        if phase == 0 {
            reset_scene(&mut camera, &mut left, &mut opaque, &mut state, &mut alpha);
        }
        if phase == 2 {
            camera.set_pose(demo_point(fix32::from_words(16384, 0), z, z), zero);
        }
        if phase == 4 {
            camera.set_pose(
                demo_point(fix32::from_words(16384, 0), z, z),
                demo_point(z, fix32::from_words(8192, 0), z),
            );
        }
        if phase == 5 {
            state.set_lighting(demo_lighting(demo_point(one, one, one)));
            alpha.set_lighting(demo_lighting(demo_point(one, one, one)));
        }
        let mut number: u16 = 42;
        if phase < 4 {
            number = 12345;
        } else if phase == 4 {
            number = 7;
        }
        if phase == 5 {
            let mut i: u16 = 0;
            while i < 8 {
                demo_glyph(&mut grid, i, 0, 255);
                i += 1;
            }
        } else {
            demo_number(&mut grid, 0, 0, 8, number);
        }
        demo_number(&mut grid, 10, 0, 8, observed);
        signal(1, tick);
        buffer.reset();
        if tick == 0 {
            buffer.invalidate_textures();
        }
        if tick & 1 == 0 {
            {
                let mut pass = buffer.pass(
                    &color_b,
                    &depth,
                    ColorLoad::clear(Rgb565::from_raw(0x1111 + tick)),
                    DepthLoad::clear(65535),
                );
                pass.set_camera(&camera);
                pass.set_state(&state);
                pass.draw(&mesh, &left, &opaque, &view);
                pass.set_state(&alpha);
                pass.draw(&mesh, &right, &glass, &view);
                if phase == 4 {
                    // This Pass keeps its installed Camera while its successor sees the edit.
                    edit_scene(&mut camera, &mut left, &mut opaque);
                    pass.set_state(&state);
                    pass.draw(&mesh, &left, &opaque, &view);
                }
            }
            if phase == 4 {
                let mut next = buffer.pass(&color_b, &depth, ColorLoad::load(), DepthLoad::load());
                next.set_camera(&camera);
                next.set_state(&alpha);
                next.draw(&mesh, &right, &glass, &view);
            }
            {
                let mut ui = buffer.pass(&color_b, &depth, ColorLoad::load(), DepthLoad::load());
                ui.draw_ui(
                    &grid,
                    CellRect {
                        x: 0,
                        y: 0,
                        width: 18,
                        height: 1,
                    },
                );
            }
        } else {
            {
                let mut pass = buffer.pass(
                    &color_a,
                    &depth,
                    ColorLoad::clear(Rgb565::from_raw(0x1111 + tick)),
                    DepthLoad::clear(65535),
                );
                pass.set_camera(&camera);
                pass.set_state(&state);
                pass.draw(&mesh, &left, &opaque, &view);
                pass.set_state(&alpha);
                pass.draw(&mesh, &right, &glass, &view);
                if phase == 4 {
                    // This Pass keeps its installed Camera while its successor sees the edit.
                    edit_scene(&mut camera, &mut left, &mut opaque);
                    pass.set_state(&state);
                    pass.draw(&mesh, &left, &opaque, &view);
                }
            }
            if phase == 4 {
                let mut next = buffer.pass(&color_a, &depth, ColorLoad::load(), DepthLoad::load());
                next.set_camera(&camera);
                next.set_state(&alpha);
                next.draw(&mesh, &right, &glass, &view);
            }
            {
                let mut ui = buffer.pass(&color_a, &depth, ColorLoad::load(), DepthLoad::load());
                ui.draw_ui(
                    &grid,
                    CellRect {
                        x: 0,
                        y: 0,
                        width: 18,
                        height: 1,
                    },
                );
            }
        }
        signal(2, tick);
        let resume_before = profiler_resume_epoch();
        time_clear_microseconds();
        gpu.submit(buffer);
        buffer = gpu.wait();
        observed = time_microseconds();
        if profiler_resume_epoch() != resume_before {
            observed = 65535;
        }
        if tick & 1 == 0 {
            display.set_next_swap(&color_b);
        } else {
            display.set_next_swap(&color_a);
        }
        display.wait_vblank();
        signal(3, tick);
        tick += 1;
        if limit != 0 && tick == limit {
            break;
        }
    }
}
pub fn renderer_changed_frames() {
    run(11);
    halt(0x600d);
}
fn main() {
    run(0);
}
