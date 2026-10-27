//! Actual caller round2: semantic edits and sequential 3D/UI Pass snapshots.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;

fn run(limit: u16) {
    let mut renderer = renderer_open();
    let z = fix32::zero();
    let one = fix32::from_int(1);
    let zero = vec3::zero();
    let scale = vec3::new(one, one, one);
    let mut camera = renderer_camera(
        &mut renderer,
        zero,
        zero,
        fix32::from_int(200),
        fix32::from_int(199),
    );
    let mut mesh = renderer_mesh(&mut renderer);
    let mut left = renderer_model(
        &mut renderer,
        vec3::new(-one, z, fix32::from_int(-3)),
        zero,
        scale,
    );
    let mut right = renderer_model(
        &mut renderer,
        vec3::new(one, z, fix32::from_int(-4)),
        zero,
        scale,
    );
    let mut opaque = renderer_material(&mut renderer, 255, 255, 255, true);
    let mut glass = renderer_material(&mut renderer, 255, 128, 64, true);
    material_color(&mut glass, 255, 128, 64, 128);
    let mut number = renderer_text(&mut renderer, 0, 0, 8);
    let mut stats = renderer_text(&mut renderer, 10, 0, 8);
    let mut tick: u16 = 0;
    loop {
        let phase = tick % 11;
        let mut frame = renderer_frame(&mut renderer, 0x1111 + tick);
        // Model-only SIGNAL markers are NOPs in hardware; no driver control.
        signal(1, tick);
        if phase == 0 {
            camera_pose(&mut camera, zero, zero);
            camera_projection(&mut camera, fix32::from_int(200), fix32::from_int(199));
            camera_light(&mut camera, vec3::new(z, z, one));
            model_pose(
                &mut left,
                vec3::new(-one, z, fix32::from_int(-3)),
                zero,
                scale,
            );
            material_color(&mut opaque, 255, 255, 255, 255);
            material_lighting(&mut opaque, 64, 192, 12);
            material_specular(&mut opaque, 160, 160, 160);
            text_visible(&mut number, true);
        }
        if phase == 2 {
            camera_pose(
                &mut camera,
                vec3::new(fix32::from_words(16384, 0), z, z),
                zero,
            );
        }
        if phase == 4 {
            camera_pose(
                &mut camera,
                vec3::new(fix32::from_words(16384, 0), z, z),
                vec3::new(z, fix32::from_words(8192, 0), z),
            );
        }
        if phase == 5 {
            camera_light(&mut camera, vec3::new(one, one, one));
            text_visible(&mut number, false);
        }
        if phase == 6 {
            text_visible(&mut number, true);
        }
        if phase < 4 {
            frame_number(&mut frame, &mut number, 12345);
        } else {
            if phase == 4 {
                frame_number(&mut frame, &mut number, 7);
            } else {
                frame_number(&mut frame, &mut number, 42);
            }
        }
        {
            let mut pass = frame_pass(&mut frame, &mut camera);
            pass_draw(&mut pass, &mut mesh, &mut left, &mut opaque);
            pass_draw(&mut pass, &mut mesh, &mut right, &mut glass);
            if phase == 4 {
                // Existing Pass keeps its view; a later Pass observes the new Camera.
                camera_pose(
                    &mut camera,
                    vec3::new(fix32::from_words(16384, 0), z, z),
                    vec3::new(z, fix32::from_words(16384, 0), z),
                );
                camera_projection(&mut camera, fix32::from_int(144), fix32::from_int(180));
                model_pose(
                    &mut left,
                    vec3::new(fix32::from_words(32768, 0), z, fix32::from_int(-3)),
                    vec3::new(z, z, fix32::from_words(8192, 0)),
                    vec3::new(fix32::from_words(16384, 1), one, one),
                );
                material_color(&mut opaque, 32, 160, 255, 255);
                material_lighting(&mut opaque, 64, 192, 8);
                material_specular(&mut opaque, 32, 64, 128);
                pass_draw(&mut pass, &mut mesh, &mut left, &mut opaque);
            }
            pass_complete(pass);
        }
        if phase == 4 {
            let mut next_view = frame_pass(&mut frame, &mut camera);
            pass_draw(&mut next_view, &mut mesh, &mut right, &mut glass);
            pass_complete(next_view);
        }
        {
            let mut ui = frame_ui_pass(&mut frame);
            pass_text(&mut ui, &mut number);
            pass_observed_time(&mut ui, &mut stats);
            pass_complete(ui);
        }
        signal(2, tick);
        frame_complete(frame);
        signal(3, tick);
        tick += 1;
        if limit != 0 && tick == limit {
            break;
        }
    }
    renderer_close(renderer);
}
pub fn renderer_changed_frames() {
    run(11);
    halt(0x600d);
}
fn main() {
    run(0);
}
