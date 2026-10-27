//! Static diagnostic inputs make adjacent frozen Front/output comparisons exact.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;
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
    let mut renderer = renderer_open();
    profiler_marker(1);
    let zero = vec3::zero();
    let one = fix32::from_int(1);
    let mut camera = renderer_camera(
        &mut renderer, zero, zero, fix32::from_int(200), fix32::from_int(199),
    );
    let mut mesh = renderer_mesh(&mut renderer);
    let mut model = renderer_model(
        &mut renderer,
        vec3::new(fix32::zero(), fix32::zero(), fix32::from_int(-3)),
        zero, vec3::new(one, one, one),
    );
    let mut material = renderer_material(&mut renderer, 240, 180, 90, true);
    let mut label = renderer_text(&mut renderer, 0, 0, 8);
    let mut count: u16 = 0;
    profiler_marker(2);
    loop {
        let mut frame = renderer_frame(&mut renderer, 0x1111);
        if count < 4 {
            profiler_marker(3);
        }
        let mut pass = frame_pass(&mut frame, &mut camera);
        pass_draw(&mut pass, &mut mesh, &mut model, &mut material);
        pass_text(&mut pass, &mut label);
        if count < 4 {
            profiler_marker(4);
        }
        pass_complete(pass);
        frame_complete(frame);
        if count < 4 {
            profiler_marker(5);
        }
        count += 1;
        if limit != 0 && count == limit {
            break;
        }
    }
    renderer_close(renderer);
}
fn main() {
    profiler_marker(0);
    if GPU_PROFILER_FULL_SCENE != 0 {
        profiler_run_full(0);
    } else {
        run(0);
    }
}
