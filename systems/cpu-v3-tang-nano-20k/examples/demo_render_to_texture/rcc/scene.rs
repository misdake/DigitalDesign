//! Explicit two-stage RTT; GPU completion precedes texture consumption and presentation.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_board;
mod gpu_demo_api;
use gpu_board::*;
use gpu_demo_api::*;

// Sector = 128 bytes; both 256 KiB reservations are excluded by tools/assets.py.
const OFFSCREEN_COLOR_SECTOR: u16 = 0xc000;
const OFFSCREEN_DEPTH_SECTOR: u16 = 0xc800;
const OFFSCREEN_SECTORS: u16 = 2048;

fn rtt_run(first: u16, end: u16, ordinary: bool) {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut buffer = gpu.command_buffer();
    // These reservations belong to this application's asset placement.
    let offscreen = ColorBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(OFFSCREEN_COLOR_SECTOR, OFFSCREEN_SECTORS)
    });
    let offscreen_depth = DepthBuffer::from_memory(unsafe {
        GpuMemory::from_sectors(OFFSCREEN_DEPTH_SECTOR, OFFSCREEN_SECTORS)
    });
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let camera = Camera::new(CameraDesc::new(
        Vec3::zero(),
        Vec3::zero(),
        Projection::from_focal_pixels(fix!(200), fix!(199)),
    ));
    let mesh = demo_mesh();
    let parts = mesh.ptr();
    let pattern = parts.slice(0, 1);
    let screen = parts.slice(1, 1);
    let producer = Model::new(Transform::translation(Vec3::new(
        fix!(0),
        fix!(0),
        fix!(-3),
    )));
    let mut pose = Transform::new(
        Vec3::new(fix!(0), fix!(0), fix!(-1)),
        Vec3::zero(),
        Vec3::new(fix!(1), fix!(0.6), fix!(1)),
    );
    if ordinary {
        pose = Transform::new(
            Vec3::new(fix!(0.125), fix!(0), fix!(-1.25)),
            Vec3::new(fix!(0), fix!(0.125), fix!(0)),
            Vec3::new(fix!(0.75), fix!(0.5), fix!(1)),
        );
    }
    let consumer = Model::new(pose);
    let material = Material::new(
        MaterialDesc::lit(Rgba8::white(), Rgb8::black(), Shininess::S16)
            .with_response(fix!(0.5), fix!(0.5))
            .with_unlit(true),
    );
    let texture = demo_texture();
    let ordinary_view = texture.view();
    let offscreen_view = offscreen.texture_view();
    let mut state = RenderState::new(
        RenderStateDesc::opaque(Lighting::directional(Vec3::unit_z(), fix!(1), fix!(1)))
            .with_depth(DepthFunc::LessEqual, true)
            .with_filter(TextureFilter::Nearest),
    );
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
                DepthLoad::clear_far(),
            );
            pass.set_camera(camera.ptr());
            pass.set_state(state.ptr());
            pass.draw(pattern, producer.ptr(), material.ptr(), ordinary_view.ptr());
        }

        gpu.submit(buffer);
        buffer = gpu.wait();
        buffer.reset();
        buffer.invalidate_textures();
        if update != 0 {
            state.set_filter(TextureFilter::Bilinear);
        }
        let back: ColorBufferPtr = if (update - first) & 1 != 0 {
            color_a.ptr()
        } else {
            color_b.ptr()
        };
        {
            let mut pass = buffer.pass(
                back,
                depth.ptr(),
                ColorLoad::clear(Rgb565::from_raw(0)),
                DepthLoad::clear_far(),
            );
            pass.set_camera(camera.ptr());
            pass.set_state(state.ptr());
            // The sampling view is persistent; only its backing pixels change.
            pass.draw(screen, consumer.ptr(), material.ptr(), offscreen_view.ptr());
        }

        gpu.submit(buffer);
        buffer = gpu.wait();
        display.set_next_swap(back);
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
