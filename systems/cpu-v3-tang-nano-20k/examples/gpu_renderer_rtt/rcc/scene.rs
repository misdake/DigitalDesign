//! Two GPU stages; no CPU pixel reads, copies, conversion or staging texture.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;

fn rtt_run(first: u16, end: u16, ordinary: bool) {
    let mut renderer = renderer_open();
    // Caller-owned static reservation. The driver borrows it and never frees it.
    let mut memory: RenderBufferMemory = RenderBufferMemory {
        color_base_low: 0,
        color_base_high: 0x60,
        color_capacity_low: 0,
        color_capacity_high: 4,
        depth_base_low: 0,
        depth_base_high: 0x64,
        depth_capacity_low: 0,
        depth_capacity_high: 4,
    };
    let mut buffer = renderer.render_buffer(view_of(&mut memory));
    let zero = vec3::zero();
    let one = fix32::from_int(1);
    let mut camera = renderer_camera(
        &mut renderer,
        zero,
        zero,
        fix32::from_int(200),
        fix32::from_int(199),
    );
    let mut pattern = renderer_mesh_batch(&mut renderer, 0, 1);
    let mut screen = renderer_mesh_batch(&mut renderer, 1, 1);
    let mut producer = renderer_model(
        &mut renderer,
        vec3::new(fix32::zero(), fix32::zero(), fix32::from_int(-3)),
        zero,
        vec3::new(one, one, one),
    );
    let mut consumer = renderer_model(
        &mut renderer,
        vec3::new(fix32::zero(), fix32::zero(), fix32::from_int(-1)),
        zero,
        vec3::new(one, fix32::from_words(39322, 0), one),
    );
    let mut material = renderer_material(&mut renderer, 255, 255, 255, false);
    if ordinary {
        model_pose(
            &mut consumer,
            vec3::new(
                fix32::from_words(8192, 0),
                fix32::zero(),
                -fix32::from_int(1) - fix32::from_words(16384, 0),
            ),
            vec3::new(fix32::zero(), fix32::from_words(8192, 0), fix32::zero()),
            vec3::new(
                fix32::from_words(49152, 0),
                fix32::from_words(32768, 0),
                one,
            ),
        );
    }
    let mut update = first;
    while end == 0 || update < end {
        let clear: u16 = if update == 0 { 0xf800 } else { 0x07e0 };
        {
            let mut frame = renderer.frame_to(&mut buffer, clear);
            let mut pass = frame.pass(&mut camera);
            pass.draw(&mut pattern, &mut producer, &mut material);
            pass.finish();
            frame.finish();
        }
        if renderer.submit() != RENDERER_PENDING || renderer.wait(WAIT_GPU) != RENDERER_COMPLETED {
            halt(0xe070);
        }
        {
            let mut texture = buffer.texture(update != 0);
            let mut frame = renderer.frame(0);
            let mut pass = frame.pass(&mut camera);
            pass.draw_texture(&mut screen, &mut consumer, &mut material, &mut texture);
            pass.finish();
            frame.finish();
            if renderer.submit() != RENDERER_PENDING {
                halt(0xe071);
            }
            texture.close();
        }
        // Dropping the CPU view did not release the submission's source pin.
        if renderer.wait(WAIT_ALL) != RENDERER_COMPLETED {
            halt(0xe071);
        }
        update += 1;
    }
    if !buffer.close() {
        halt(0xe072);
    }
    renderer.close();
}

pub fn renderer_rtt_two_updates() {
    rtt_run(0, 2, false);
    halt(0x600d);
}

pub fn renderer_rtt_profiler() {
    rtt_run(0, 0, false);
}
pub fn renderer_rtt_nearest() {
    rtt_run(0, 1, false);
    halt(0x600d);
}
pub fn renderer_rtt_bilinear() {
    rtt_run(1, 2, true);
    halt(0x600d);
}
