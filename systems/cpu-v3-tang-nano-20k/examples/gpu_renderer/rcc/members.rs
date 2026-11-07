//! One mutable Model, two recorded snapshots, explicit GPU and Display retirement.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
mod gpu_board;use gpu_board::*;
use gpu_demo_api::*;
mod device_abi;
use device_abi::*;

fn report_byte(value: u16) {
    if dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 == 0 {
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, value);
    }
}
fn draw_pair(
    pass: &mut Pass,
    camera: &Camera,
    mesh: &Mesh,
    model: &mut Model,
    material: &Material,
    texture: &TextureView,
) {
    pass.set_camera(camera);
    let z = fix32::zero();
    let half = fix32::from_words(32768, 0);
    model.set_pose(demo_pose(
        demo_point(-half, z, fix32::from_int(-3)),
        demo_zero(),
        demo_scale(),
    ));
    pass.draw(mesh, model, material, texture);
    model.set_pose(demo_pose(
        demo_point(half, z, fix32::from_int(-3)),
        demo_zero(),
        demo_scale(),
    ));
    pass.draw(mesh, model, material, texture);
}
fn run(limit: u16) {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut buffer = gpu.command_buffer();
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let camera = demo_camera();
    let mesh = demo_mesh();
    let mut model = Model::new(demo_pose(demo_zero(), demo_zero(), demo_scale()));
    let material = demo_material(demo_white(), true);
    let texture = demo_texture();
    let view = texture.view();
    let state = demo_state();
    let mut count: u16 = 0;
    loop {
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
            pass.set_state(&state);
            draw_pair(&mut pass, &camera, &mesh, &mut model, &material, &view);
        } else {
            let mut pass = buffer.pass(
                &color_a,
                &depth,
                ColorLoad::clear(Rgb565::from_raw(0x1111)),
                DepthLoad::clear(65535),
            );
            pass.set_state(&state);
            draw_pair(&mut pass, &camera, &mesh, &mut model, &material, &view);
        }
        gpu.submit(buffer);
        buffer = gpu.wait();
        if count & 1 == 0 {
            display.set_next_swap(&color_b);
        } else {
            display.set_next_swap(&color_a);
        }
        display.wait_vblank();
        count += 1;
        if count & 63 == 0 {
            report_byte(77);
        }
        if limit != 0 && count == limit {
            break;
        }
    }
}
pub fn renderer_member_three_frames() {
    run(3);
    halt(0x600d);
}
fn main() {
    run(0);
}
