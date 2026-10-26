//! Scope/UI caller model, before camera/mesh/statistics and trusted-domain integration.
use crate::dsl_rt::*;
mod renderer_model;
use renderer_model::*;

fn main() {
    let mut renderer = renderer_open();
    let mut tick: u16 = 0;
    while tick < 4 {
        let mut color: u16 = 0x1111;
        if tick != 0 {
            color += tick - 1;
        }
        let mut frame = renderer_frame(&mut renderer, color);
        frame_number(&mut frame, 10 + tick);
        let mut pass = frame_pass(&mut frame);
        pass_text(&mut pass);
        if tick == 0 {
            tick += 1;
            // Leaving an unfinished Pass/Frame cancels recording only.
            continue;
        }
        pass_complete(pass);
        // An edit after Pass closes still belongs to this unsealed Frame.
        frame_number(&mut frame, tick - 1);
        frame_complete(frame);
        tick += 1;
    }
    renderer_close(renderer);
    halt(0x600d);
}
