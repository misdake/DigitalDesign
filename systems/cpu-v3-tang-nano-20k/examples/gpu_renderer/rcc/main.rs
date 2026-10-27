//! Minimal semantic renderer caller; asset layout and GPU work stay in the library.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;

fn clamp_eye(v: fix32) -> fix32 {
    if v > fix32::from_int(20) {
        return fix32::from_int(20);
    }
    if v < fix32::from_int(-20) {
        return fix32::from_int(-20);
    }
    v
}
fn run(limit: u16) {
    let mut renderer = renderer_open();
    let zero = vec3::zero();
    let one = fix32::from_int(1);
    let mut camera = renderer_camera(
        &mut renderer,
        zero,
        zero,
        fix32::from_int(200),
        fix32::from_int(199),
    );
    let mut mesh = renderer_mesh(&mut renderer);
    let mut model = renderer_model(
        &mut renderer,
        vec3::new(fix32::zero(), fix32::zero(), fix32::from_int(-3)),
        zero,
        vec3::new(one, one, one),
    );
    let mut material = renderer_material(&mut renderer, 255, 255, 255, true);
    let mut stats = renderer_text(&mut renderer, 0, 0, 8);
    let mut eye_x = fix32::zero();
    let mut eye_z = fix32::zero();
    let mut count: u16 = 0;
    loop {
        let mut frame = renderer_frame(&mut renderer, 0x1111 + count);
        let input = frame_input(&mut frame);
        let mut elapsed = frame_elapsed_seconds(&mut frame);
        if elapsed > fix32::from_words(6554, 0) {
            elapsed = fix32::from_words(6554, 0);
        }
        // Held input integrates on each tick, even without a new controller packet.
        eye_x += input.left_x * elapsed;
        eye_z += input.left_y * elapsed;
        eye_x = clamp_eye(eye_x);
        eye_z = clamp_eye(eye_z);
        camera_pose(&mut camera, vec3::new(eye_x, fix32::zero(), eye_z), zero);
        let mut pass = frame_pass(&mut frame, &mut camera);
        pass_draw(&mut pass, &mut mesh, &mut model, &mut material);
        pass_observed_time(&mut pass, &mut stats);
        pass_complete(pass);
        frame_complete(frame);
        count += 1;
        if limit != 0 && count == limit {
            break;
        }
    }
    renderer_close(renderer);
}
pub fn renderer_three_frames() {
    run(3);
    halt(0x600d);
}
fn main() {
    run(0);
}
