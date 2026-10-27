//! Cancellation and transfer caller through the production resource facade.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;

fn main() {
    let mut renderer = renderer_open();
    let mut field = renderer_text(&mut renderer, 0, 0, 8);
    let mut tick: u16 = 0;
    while tick < 4 {
        let mut color: u16 = 0x1111;
        if tick != 0 {
            color += tick - 1;
        }
        let mut frame = renderer_frame(&mut renderer, color);
        frame_number(&mut frame, &mut field, 10 + tick);
        let mut pass = frame_ui_pass(&mut frame);
        pass_text(&mut pass, &mut field);
        if tick == 0 {
            tick += 1;
            // Leaving an unfinished Pass/Frame cancels recording only.
            continue;
        }
        pass_complete(pass);
        // An edit after Pass closes still belongs to this unsealed Frame.
        frame_number(&mut frame, &mut field, tick - 1);
        frame_complete(frame);
        tick += 1;
    }
    renderer_close(renderer);
    halt(0x600d);
}
