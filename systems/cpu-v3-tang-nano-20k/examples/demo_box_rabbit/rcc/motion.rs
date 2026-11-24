//! Classic first-person motion; gamepad owns acquisition and stick conversion.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gamepad;
use gamepad::*;

const BUTTON_L1: u16 = 1 << 10;
const BUTTON_R1: u16 = 1 << 11;

pub struct DemoMotion {
    pub eye: Vec3,
    pub yaw: fix32,
    pub pitch: fix32,
    pub previous_ms: u16,
}
impl DemoMotion {
    pub fn new(now: u16) -> Self {
        Self {
            eye: Vec3::new(fix!(0), fix!(0.22), fix!(2.8)),
            yaw: fix!(0),
            // Pinned classic phase-zero pitch; preserve its exact Q16 code.
            pitch: fix32::from_words(59932, 65535),
            previous_ms: now,
        }
    }
    pub fn angles(&self) -> Vec3 {
        Vec3::new(self.pitch, self.yaw, fix!(0))
    }
    pub fn update(&mut self, input: &GamepadFrame, now: u16) -> bool {
        let mut elapsed = now - self.previous_ms;
        self.previous_ms = now;
        if elapsed > 100 {
            elapsed = 100;
        }
        if input.connected == 0 || elapsed == 0 {
            return false;
        }
        // elapsed is encoded as elapsed/65536. Gains are round(2 or 3 *
        // 65536^2 / 1000), preserving the original Q16 speed, not rounded 1/1000.
        let distance = fix32::from_words(elapsed, 0) * fix32::from_words(4719, 131);
        let angle = fix32::from_words(elapsed, 0) * fix32::from_words(39846, 196);
        let old_yaw = self.yaw;
        let old_pitch = self.pitch;
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
        let limit = fix!(1.5);
        if pitch > limit {
            pitch = limit;
        }
        if pitch < -limit {
            pitch = -limit;
        }
        let shoulders = input.buttons & (BUTTON_L1 | BUTTON_R1);
        let strafe = input.left_x * distance;
        let forward = -input.left_y * distance;
        let mut vertical = fix32::zero();
        if shoulders == BUTTON_L1 {
            vertical = -distance;
        }
        if shoulders == BUTTON_R1 {
            vertical = distance;
        }
        if yaw == old_yaw
            && pitch == old_pitch
            && strafe == fix32::zero()
            && forward == fix32::zero()
            && vertical == fix32::zero()
        {
            return false;
        }
        self.yaw = yaw;
        self.pitch = pitch;
        let y = fsincos(yaw);
        self.eye = Vec3 {
            x: bounded_position(self.eye.x - y.y() * strafe - y.x() * forward),
            y: bounded_position(self.eye.y + vertical),
            z: bounded_position(self.eye.z + y.x() * strafe - y.y() * forward),
        };
        true
    }
}

fn bounded_position(value: fix32) -> fix32 {
    if value < -fix!(16) {
        return -fix!(16);
    }
    if value > fix!(16) {
        return fix!(16);
    }
    value
}
