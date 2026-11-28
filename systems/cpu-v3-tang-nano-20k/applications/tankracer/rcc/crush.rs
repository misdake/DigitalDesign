//! Deformable record animation and deferred retirement on the shared scheduler.
use crate::dsl_rt::*;
use crate::{deformation, integer, lifecycle};
pub struct Runtime {
    pub siren_timer: fix32,
    pub sound_channel: fix32,
    pub highlighted: bool,
    pub private_mesh: bool,
}
pub struct Frame {
    pub dt: fix32,
    pub entity: fix32,
    pub limit: fix32,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
// 0 success, 2 invalid limit, 3 work limit, 4 detached records,
// 6 missing entity and 7 caller storage capacity.
pub fn tick(
    surface: &mut deformation::Surface,
    runtime: &mut Runtime,
    frame: &Frame,
    ticks: &mut lifecycle::Ticks,
    queues: &mut lifecycle::Queues,
) -> u16 {
    if frame.limit == n(0) {
        return 2;
    }
    runtime.sound_channel = n(0);
    if surface.dirty != 0 && runtime.siren_timer != n(0) {
        if runtime.siren_timer < n(11026) {
            runtime.siren_timer = n(0);
        } else {
            runtime.siren_timer = runtime.siren_timer - fmul_lo32(frame.dt, n(8));
        }
    }
    if !surface.attached && surface.record_count != 0 {
        return 4;
    }
    let mut retired = 0u16;
    let mut i = 0u16;
    let mut records = surface.records;
    while i < surface.record_count {
        if frame.limit.hi_bits() == 0 && i >= frame.limit.lo_bits() {
            return 3;
        }
        let record = &mut records[i];
        if record.state == n(0) || record.state == n(1) {
            if record.timer > frame.dt {
                record.timer = record.timer - frame.dt;
            } else {
                if record.state == n(0) {
                    record.state = n(1);
                } else {
                    record.state = n(0);
                }
                record.timer = record.timer + n(500);
            }
        } else if record.state == n(2) {
            let red = record.red.lo_bits() & 255;
            let mut signed = fix32::from_words(red, 0);
            if red >= 128 {
                signed = signed - n(256);
            }
            if signed <= frame.dt {
                record.state = n(3);
                retired += 1;
            } else {
                let color = fix32::from_words((red - frame.dt.lo_bits()) & 255, 0);
                record.red = color;
                record.green = color;
                record.blue = color;
            }
        } else if record.state == n(3) {
            retired += 1;
        }
        i += 1;
    }
    if retired == surface.record_count && runtime.siren_timer == n(0) {
        surface.attached = false;
        let status = lifecycle::queue(queues, ticks, frame.entity, false, frame.limit);
        if status == 4 {
            return 7;
        }
        return status;
    }
    0
}
