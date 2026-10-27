use crate::dsl_rt::*;
mod gpu;
use gpu::*;
mod dualshock2;
mod panel;
mod stick_controls;
mod time;
use panel::*;
use stick_controls::*;

static INPUT: Buf<u16, 10> = Buf::new([0; 10]);
static PACKET: Buf<u16, 9> = Buf::new([0; 9]);
static EVENTS: Buf<u16, 3> = Buf::new([0; 3]);

fn dui_require(ok: bool, code: u16) {
    if !ok {
        dev_send(0, 2, 3);
        halt(0xd500 | code);
    }
}
fn dui_service() {
    if ds2_service(INPUT.as_array(), PACKET.as_array()) {
        dui_controls(INPUT.as_array());
        EVENTS[0u16] |= INPUT[8u16];
        EVENTS[1u16] |= INPUT[9u16];
        EVENTS[2u16] += 1;
    }
    if INPUT[0u16] == 0 {
        EVENTS[0u16] = 0;
        EVENTS[1u16] = 0;
    }
    dui_probe_wire();
    dev_send(0, 2, 1 | (INPUT[0u16] << 1));
}
fn dui_wait(cb: u16, present: bool) -> u16 {
    let begin = time_milliseconds();
    while true {
        let mut status = GPU_PENDING;
        if present {
            status = gpu_present_poll();
        } else {
            status = gpu_poll(cb);
        }
        if status != GPU_PENDING {
            return status;
        }
        if time_elapsed_ms(begin, time_milliseconds()) >= 2000 {
            return GPU_TIMEOUT;
        }
        // Read the latest complete snapshot; acquisition never waits for this loop.
        dui_service();
    }
    return GPU_TIMEOUT;
}
pub fn dui_run(limit: u16) {
    dui_controls_reset();
    ds2_init(INPUT.as_array());
    dui_require(gpu_init() == GPU_IDLE, 1);
    let mut frame: u16 = 0;
    loop {
        dui_service();
        let cb = dui_record(INPUT.as_array(), EVENTS[0u16], EVENTS[1u16], EVENTS[2u16]);
        dui_require(cb != GPU_CB_NONE, 2);
        EVENTS[0u16] = 0;
        EVENTS[1u16] = 0;
        dui_require(gpu_submit_idle(cb) == GPU_PENDING, 3);
        dui_require(dui_wait(cb, false) == GPU_COMPLETED, 4);
        dui_require(gpu_present(cb) == GPU_PENDING, 5);
        dui_require(dui_wait(cb, true) == GPU_COMPLETED, 6);
        frame += 1;
        if limit != 0 && frame >= limit {
            halt(0x600d);
        }
    }
}
fn main() {
    dui_run(0);
}
pub fn dui_one_frame_main() {
    dui_run(1);
}
