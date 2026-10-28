//! Minimal member caller; explicit submission and GPU/Display retirement.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;
mod device_abi;
use device_abi::*;

fn report_byte(value: u16) {
    if dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 == 0 {
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, value);
    }
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
    let mut material = renderer_material(&mut renderer, 255, 255, 255, false);
    let mut count: u16 = 0;
    loop {
        let mut frame = renderer.frame(0x1111);
        let mut pass = frame.pass(&mut camera);
        model_pose(
            &mut model,
            vec3::new(
                -fix32::from_words(32768, 0),
                fix32::zero(),
                fix32::from_int(-3),
            ),
            zero,
            vec3::new(one, one, one),
        );
        pass.draw(&mut mesh, &mut model, &mut material);
        model_pose(
            &mut model,
            vec3::new(
                fix32::from_words(32768, 0),
                fix32::zero(),
                fix32::from_int(-3),
            ),
            zero,
            vec3::new(one, one, one),
        );
        pass.draw(&mut mesh, &mut model, &mut material);
        pass.finish();
        frame.finish();
        if renderer.submit() != RENDERER_PENDING {
            report_byte(69);
            halt(0xbad);
        }
        if renderer.wait(WAIT_ALL) != RENDERER_COMPLETED {
            report_byte(69);
            halt(0xbad);
        }
        count += 1;
        // One 'M' after each 64 fully retired frames; 'E' marks an API failure.
        if count & 63 == 0 {
            report_byte(77);
        }
        if limit != 0 && count == limit {
            break;
        }
    }
    renderer.close();
}
pub fn renderer_member_three_frames() {
    run(3);
    halt(0x600d);
}
fn main() {
    run(0);
}
