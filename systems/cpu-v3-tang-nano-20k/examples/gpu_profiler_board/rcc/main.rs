//! Static diagnostic scene through the explicit public API and shared capture gate.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_demo_api;
mod gpu_board;use gpu_board::*;
use gpu_demo_api::*;
mod device_abi;
use device_abi::*;
mod boot_resources;
use boot_resources::*;
mod full_scene;
use full_scene::*;

fn profiler_marker(stage: u16) {
    if GPU_PROFILER_AUTOMATIC_TEST == 0 {
        return;
    }
    let mut i: u16 = 0;
    while i < 3 {
        let mut polls: u16 = 0;
        while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {
            polls += 1;
            if polls == 65535 {
                return;
            }
        }
        let mut byte: u16 = 10;
        if i == 0 {
            byte = 71;
        } else if i == 1 {
            byte = 48 + stage;
        }
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, byte);
        i += 1;
    }
}
fn run(limit: u16) {
    let mut gpu = Gpu::init(board_gpu_config());
    let mut buffer = gpu.command_buffer();
    profiler_marker(1);
    let color_a = board_color_a();
    let color_b = board_color_b();
    let depth = board_depth();
    let mut display = Display::open();
    let camera = demo_camera();
    let mesh = demo_mesh();
    let model = Model::new(demo_pose(
        demo_point(fix32::zero(), fix32::zero(), fix32::from_int(-3)),
        demo_zero(),
        demo_scale(),
    ));
    let material = demo_material(
        Rgba8 {
            r: 240,
            g: 180,
            b: 90,
            a: 255,
        },
        false,
    );
    let state = demo_state();
    let texture = demo_texture();
    let view = texture.view();
    let grid = demo_grid(false);
    let mut count: u16 = 0;
    profiler_marker(2);
    loop {
        buffer.reset();
        if count == 0 {
            buffer.invalidate_textures();
        }
        if count < 4 {
            profiler_marker(3);
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
            pass.draw(&mesh, &model, &material, &view);
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
            pass.draw(&mesh, &model, &material, &view);
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
        if count < 4 {
            profiler_marker(4);
        }
        if GPU_PROFILER_AUTOMATIC_TEST != 0 {
            gpu.request_snapshot();
        }
        gpu.submit_capturable(buffer);
        buffer = gpu.wait();
        if count & 1 == 0 {
            display.set_next_swap(&color_b);
        } else {
            display.set_next_swap(&color_a);
        }
        display.wait_vblank();
        if count < 4 {
            profiler_marker(5);
        }
        count += 1;
        if limit != 0 && count == limit {
            break;
        }
    }
}
pub fn profiler_minimal_three_frames() {
    run(3);
    halt(0x600d);
}
fn main() {
    profiler_marker(0);
    if GPU_PROFILER_FULL_SCENE != 0 {
        profiler_run_full(0, GPU_PROFILER_AUTOMATIC_TEST != 0);
    } else {
        run(0);
    }
}
