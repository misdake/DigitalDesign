//! Identical visible session events for the old full ring and new scratch snapshot.
use crate::dsl_rt::*;
mod session;
use session::*;
pub fn session_sequence() {
    session_init();
    let mut tick: u16 = 0;
    while tick < 3600 {
        if SESSION[3u16] != 0 {
            session_captured();
        }
        let mut buttons: u16 = 0;
        // More than 256 move attempts, alternating within the first fixture.
        if tick >= 20 && tick % 8 == 0 {
            buttons = if (tick / 8) & 1 == 0 { 32u16 } else { 128u16 };
        }
        if tick == 3000 {
            buttons = 4096;
        }
        if tick == 3300 {
            buttons = 8;
        }
        session_tick(buttons, 40);
        if SESSION[3u16] == 0 {
            session_presented();
        }
        signal(15, tick);
        tick += 1;
    }
    halt(0x600d);
}
