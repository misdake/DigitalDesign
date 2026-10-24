//! Generate the numeric overlay directly in its final native UI object.
use crate::dsl_rt::*;
mod gpu;
use gpu::*;
mod frame_stats;
use frame_stats::*;

pub fn demo_ui_reset(state: Array<u16>) {
    frame_stats_reset(state);
}
pub fn demo_ui_frame_begin(state: Array<u16>, now: u16, completed: bool) {
    frame_stats_begin(state, now, completed);
}
pub fn demo_ui_record(pass: u16, atlas: Array<GpuUiAtlas>, state: Array<u16>) -> bool {
    if state[3u16] == 0 {
        return true;
    }
    let mut digits: Buf<u16, 5> = Buf::new([0; 5]);
    frame_stats_digits(state, digits.as_array());
    let mut first: u16 = 0;
    while first < 4 && digits[first] == 48 {
        first += 1;
    }
    let count = 5 - first;
    let ui = gpu_pass_ui_begin_range(pass, atlas, 255, 52, 52 + count);
    if ui == GPU_CB_NONE {
        return false;
    }
    let ids = unsafe { digits.as_ptr().add(first as i16).as_u16_array() };
    if !gpu_pass_ui_write(ui, 52, ids, count) {
        return false;
    }
    gpu_pass_ui_set(ui) && gpu_pass_draw_ui_range(pass, 52, 52 + count)
}
