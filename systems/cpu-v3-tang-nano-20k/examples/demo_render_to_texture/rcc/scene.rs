//! Explicit two-stage RTT; GPU completion precedes texture consumption and presentation.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_board;
mod gpu_demo_api;
use gpu_board::*;
use gpu_demo_api::*;

fn consume(
    pass: &mut Pass,
    camera: &Camera,
    mesh: &Mesh,
    model: &Model,
    material: &Material,
    texture: &TextureView,
) {
    pass.set_camera(camera.ptr());
    pass.draw(mesh.ptr(), model.ptr(), material.ptr(), texture.ptr());
}
fn rtt_run(first: u16, end: u16, ordinary: bool) {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut buffer = gpu.command_buffer();
    // These reservations belong to this application's asset placement.
    let offscreen = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc000, 2048) });
    let offscreen_depth =
        DepthBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc800, 2048) });
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let camera = demo_camera();
    let pattern = demo_mesh_part(0, 1);
    let screen = demo_mesh_part(1, 1);
    let producer = Model::new(demo_pose(
        demo_point(fix32::zero(), fix32::zero(), fix32::from_int(-3)),
        demo_zero(),
        demo_scale(),
    ));
    let one = fix32::from_int(1);
    let mut consumer = Model::new(demo_pose(
        demo_point(fix32::zero(), fix32::zero(), -one),
        demo_zero(),
        demo_point(one, fix32::from_words(39322, 0), one),
    ));
    if ordinary {
        consumer.set_pose(demo_pose(
            demo_point(
                fix32::from_words(8192, 0),
                fix32::zero(),
                -one - fix32::from_words(16384, 0),
            ),
            demo_point(fix32::zero(), fix32::from_words(8192, 0), fix32::zero()),
            demo_point(
                fix32::from_words(49152, 0),
                fix32::from_words(32768, 0),
                one,
            ),
        ));
    }
    let material = demo_material(demo_white(), true);
    let texture = demo_texture();
    let ordinary_view = texture.view();
    let offscreen_view = offscreen.texture_view();
    let mut state = demo_state();
    let mut update = first;
    while end == 0 || update < end {
        let clear: u16 = if update == 0 { 0xf800 } else { 0x07e0 };
        buffer.reset();
        buffer.invalidate_textures();
        {
            let mut pass = buffer.pass(
                offscreen.ptr(),
                offscreen_depth.ptr(),
                ColorLoad::clear(Rgb565::from_raw(clear)),
                DepthLoad::clear(65535),
            );
            pass.set_camera(camera.ptr());
            pass.set_state(state.ptr());
            pass.draw(
                pattern.ptr(),
                producer.ptr(),
                material.ptr(),
                ordinary_view.ptr(),
            );
        }

        gpu.submit(buffer);
        buffer = gpu.wait();
        buffer.reset();
        buffer.invalidate_textures();
        if update != 0 {
            state.set_filter(TextureFilter::Bilinear);
        }
        {
            if (update - first) & 1 == 0 {
                let mut pass = buffer.pass(
                    color_b.ptr(),
                    depth.ptr(),
                    ColorLoad::clear(Rgb565::from_raw(0)),
                    DepthLoad::clear(65535),
                );
                pass.set_state(state.ptr());
                consume(
                    &mut pass,
                    &camera,
                    &screen,
                    &consumer,
                    &material,
                    &offscreen_view,
                );
            } else {
                let mut pass = buffer.pass(
                    color_a.ptr(),
                    depth.ptr(),
                    ColorLoad::clear(Rgb565::from_raw(0)),
                    DepthLoad::clear(65535),
                );
                pass.set_state(state.ptr());
                consume(
                    &mut pass,
                    &camera,
                    &screen,
                    &consumer,
                    &material,
                    &offscreen_view,
                );
            }
            // The sampling view is persistent; only its backing pixels change.
        }

        gpu.submit(buffer);
        buffer = gpu.wait();
        if (update - first) & 1 == 0 {
            display.set_next_swap(color_b.ptr());
        } else {
            display.set_next_swap(color_a.ptr());
        }
        display.wait_vblank();
        update += 1;
    }
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
