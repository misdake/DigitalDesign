//! Complete canonical box, RGB triangle and 948-face rabbit through public APIs.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;

pub fn profiler_run_full(limit: u16) {
    let mut renderer = renderer_open();
    let zero = vec3::zero();
    let one = fix32::from_int(1);
    let mut camera = renderer_camera(
        &mut renderer,
        vec3::new(fix32::zero(), fix32::from_words(14418, 0), fix32::from_words(52429, 2)),
        vec3::new(fix32::from_words(59932, 65535), fix32::zero(), fix32::zero()),
        fix32::from_int(200), fix32::from_int(199),
    );
    camera_light(&mut camera, vec3::new(
        fix32::from_words(36045, 65535), fix32::from_words(52429, 0), fix32::from_words(6554, 1),
    ));
    let mut room = renderer_mesh_batch(&mut renderer, 0, 6);
    let mut rgb = renderer_mesh_batch(&mut renderer, 6, 1);
    let mut rabbit = renderer_mesh_batch(&mut renderer, 7, 17);
    let mut model = renderer_model(&mut renderer, zero, zero, vec3::new(one, one, one));
    let mut unlit = renderer_material(&mut renderer, 255, 255, 255, false);
    let mut lit = renderer_material(&mut renderer, 255, 255, 255, true);
    material_lighting(&mut lit, 64, 192, 12);
    material_specular(&mut lit, 160, 160, 160);
    let mut label = renderer_text(&mut renderer, 0, 0, 8);
    let mut count: u16 = 0;
    loop {
        let mut frame = renderer_frame(&mut renderer, 0x1111);
        let mut pass = frame_pass(&mut frame, &mut camera);
        pass_draw(&mut pass, &mut room, &mut model, &mut unlit);
        pass_draw(&mut pass, &mut rgb, &mut model, &mut unlit);
        pass_draw(&mut pass, &mut rabbit, &mut model, &mut lit);
        pass_observed_time(&mut pass, &mut label);
        pass_complete(pass);
        frame_complete(frame);
        count += 1;
        if limit != 0 && count == limit {
            break;
        }
    }
    renderer_close(renderer);
}
