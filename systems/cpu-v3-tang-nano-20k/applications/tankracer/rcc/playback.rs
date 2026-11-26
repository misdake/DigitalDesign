//! Wall-clock camera playback; ordinary sample lists retain benchmark stepping.
use crate::dsl_rt::*;
mod boot_resources;
mod device_abi;
mod time;
use boot_resources::*;
use device_abi::*;
use time::*;

pub struct Playback {
    start: u16,
    frame: u16,
}

impl Playback {
    pub fn new() -> Self {
        let mut start: u16 = 0;
        if CAMERA_FPS != 0 {
            start = time_milliseconds();
        }
        Self { start, frame: 0 }
    }

    pub fn next(&mut self) -> u16 {
        if CAMERA_FPS == 0 {
            let frame = self.frame;
            self.frame += 1;
            if self.frame == CAMERA_COUNT {
                self.frame = 0;
            }
            return frame;
        }
        // Rebase every lap before the independent 16-bit millisecond wrap.
        // Rendering must revisit this clock within one timer period.
        let mut elapsed = time_elapsed_ms(self.start, time_milliseconds());
        while elapsed >= CAMERA_LOOP_MS {
            self.start += CAMERA_LOOP_MS;
            elapsed -= CAMERA_LOOP_MS;
        }
        // At most 60 samples/s: both products fit u16, without a u32 divide.
        (elapsed / 1000) * CAMERA_FPS + ((elapsed % 1000) * CAMERA_FPS) / 1000
    }
}
