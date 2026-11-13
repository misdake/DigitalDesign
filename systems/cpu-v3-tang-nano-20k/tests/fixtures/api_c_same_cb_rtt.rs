//! One actual public CB produces an offscreen image and samples it before presentation.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_board;
mod gpu_demo_api;
use gpu_board::*;
use gpu_demo_api::*;
pub fn same_cb_rtt_once() {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut commands = gpu.command_buffer();
    let source = ColorBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc000, 2048) });
    let source_depth = DepthBuffer::from_memory(unsafe { GpuMemory::from_sectors(0xc800, 2048) });
    let color = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let camera = demo_camera();
    let state = demo_state();
    let material = demo_material(demo_white(), true);
    let pattern = demo_mesh_part(0, 1);
    let screen = demo_mesh_part(1, 1);
    let one = fix32::from_int(1);
    let producer = Model::new(demo_pose(
        demo_point(fix32::zero(), fix32::zero(), fix32::from_int(-3)),
        demo_zero(),
        demo_scale(),
    ));
    let consumer = Model::new(demo_pose(
        demo_point(fix32::zero(), fix32::zero(), -one),
        demo_zero(),
        // Keep the full physical targets and texture, but sample into a small
        // viewport so this functional dependency check has a bounded cost.
        demo_point(fix32::from_words(8192, 0), fix32::from_words(4915, 0), one),
    ));
    let texture = demo_texture();
    let ordinary = texture.view();
    commands.reset();
    commands.invalidate_textures();
    {
        let mut pass = commands.pass(
            source.ptr(),
            source_depth.ptr(),
            ColorLoad::clear(Rgb565::from_raw(0xf800)),
            DepthLoad::clear(65535),
        );
        pass.set_camera(camera.ptr());
        pass.set_state(state.ptr());
        pass.draw(
            pattern.ptr(),
            producer.ptr(),
            material.ptr(),
            ordinary.ptr(),
        );
    }
    commands.invalidate_textures();
    {
        let view = source.texture_view();
        let mut pass = commands.pass(
            color.ptr(),
            depth.ptr(),
            ColorLoad::clear(Rgb565::from_raw(0)),
            DepthLoad::clear(65535),
        );
        pass.set_camera(camera.ptr());
        pass.set_state(state.ptr());
        pass.draw(screen.ptr(), consumer.ptr(), material.ptr(), view.ptr());
    }
    gpu.submit(commands);
    commands = gpu.wait();
    display.set_next_swap(color.ptr());
    display.wait_vblank();
    halt(0x600d);
}
