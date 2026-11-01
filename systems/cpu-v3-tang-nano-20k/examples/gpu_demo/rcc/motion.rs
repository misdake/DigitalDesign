//! Classic first-person motion; gamepad owns acquisition and stick conversion.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gamepad;
use gamepad::*;

pub struct DemoMotion {
    pub eye: Vec3,
    pub yaw: fix32,
    pub pitch: fix32,
    pub previous_ms: u16,
    pub changed: bool,
}
pub fn demo_motion_init(now: u16) -> DemoMotion {
    DemoMotion {
        eye: Vec3 {
            x: fix32::zero(),
            y: fix32::from_words(14418, 0),
            z: fix32::from_words(52429, 2),
        },
        yaw: fix32::zero(),
        pitch: fix32::from_words(59932, 65535),
        previous_ms: now,
        changed: false,
    }
}
pub fn demo_motion_angles(motion: DemoMotion) -> Vec3 {
    Vec3 {
        x: motion.pitch,
        y: motion.yaw,
        z: fix32::zero(),
    }
}
fn bounded_position(value: fix32) -> fix32 {
    if value < -fix32::from_int(16) {
        return -fix32::from_int(16);
    }
    if value > fix32::from_int(16) {
        return fix32::from_int(16);
    }
    value
}
pub fn demo_motion_update(mut motion: DemoMotion, input: GamepadFrame, now: u16) -> DemoMotion {
    motion.changed = false;
    let mut elapsed = now - motion.previous_ms;
    motion.previous_ms = now;
    if elapsed > 100 {
        elapsed = 100;
    }
    if input.connected == 0 || elapsed == 0 {
        return motion;
    }
    // Preserve the original Q16 arithmetic, not a rounded 1/1000 seed.
    let distance = fix32::from_words(elapsed, 0) * fix32::from_words(4719, 131);
    let angle = fix32::from_words(elapsed, 0) * fix32::from_words(39846, 196);
    let old_yaw = motion.yaw;
    let old_pitch = motion.pitch;
    let mut yaw = old_yaw + input.right_x * angle;
    let mut pitch = old_pitch - input.right_y * angle;
    let pi = fix32::from_words(9279, 3);
    let turn = fix32::from_words(18559, 6);
    if yaw > pi {
        yaw = yaw - turn;
    }
    if yaw < -pi {
        yaw = yaw + turn;
    }
    let limit = fix32::from_words(32768, 1);
    if pitch > limit {
        pitch = limit;
    }
    if pitch < -limit {
        pitch = -limit;
    }
    let shoulders = input.buttons & 3072;
    let strafe = input.left_x * distance;
    let forward = -input.left_y * distance;
    let mut vertical = fix32::zero();
    if shoulders == 1024 {
        vertical = -distance;
    }
    if shoulders == 2048 {
        vertical = distance;
    }
    if yaw == old_yaw
        && pitch == old_pitch
        && strafe == fix32::zero()
        && forward == fix32::zero()
        && vertical == fix32::zero()
    {
        return motion;
    }
    motion.yaw = yaw;
    motion.pitch = pitch;
    let y = fsincos(yaw);
    motion.eye = Vec3 {
        x: bounded_position(motion.eye.x - y.y() * strafe - y.x() * forward),
        y: bounded_position(motion.eye.y + vertical),
        z: bounded_position(motion.eye.z + y.x() * strafe - y.y() * forward),
    };
    motion.changed = true;
    motion
}
