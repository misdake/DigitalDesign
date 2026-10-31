//! Live external grid drawing; only a canonical 32-byte descriptor is recorded.
use crate::dsl_rt::*;
mod gpu_bind;
use gpu_bind::*;

fn gpu_ui_packet_range(mut code: Array<u16>, at: u16, first: u16, end: u16) {
    let mut low: u16 = 69;
    let mut high: u16 = 0xb000;
    if first != 0 || end != 750 {
        low |= first << 7;
        high |= 0x0800 | (first >> 9) | ((end - 1) << 1);
    }
    code[at] = low;
    code[at + 1] = high;
}
// The public adapter passes B's CellRect fields; no second public rect type.
fn gpu_ui_packet_draw(
    slot: u16,
    descriptor: Array<u16>,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) -> bool {
    if slot >= 2 || x > 50 || y > 15 || width > 50 - x || height > 15 - y {
        return false;
    }
    let mut c = gpu_record_data(slot);
    if c[GPU_RECORD_PHASE] != GPU_RECORD_PASS {
        return false;
    }
    if width == 0 || height == 0 {
        return true;
    }
    let same = c[GPU_RECORD_UI_WORD] != 0 && gpu_bind_equal(c, GPU_CB_UI, descriptor, 0, 16);
    let install = !same || c[GPU_RECORD_VALID] & 32 == 0;
    let fence = c[GPU_RECORD_UI_CLEAN] == 0;
    let mut ranges = height;
    if width == 50 {
        ranges = 1;
    }
    let mut words = ranges * 2;
    if fence {
        words += 4;
    }
    if install {
        words += 4;
    }
    let mut push = c[GPU_RECORD_PUSH_USED];
    let mut count: u16 = 0;
    let mut alignment: u16 = 1;
    if !same {
        push = gpu_record_push_offset(slot, 16, 16);
        if push == 65535 {
            return false;
        }
        count = 16;
        alignment = 16;
    }
    let mut source = push;
    if same {
        source = c[GPU_RECORD_UI_WORD] - 1;
    }
    if !gpu_record_preflight(slot, words, push + count) {
        return false;
    }
    let mut code: Buf<u16, 40> = Buf::new([0; 40]);
    let mut at: u16 = 0;
    if fence {
        code[0u16] = 3;
        code[1u16] = 0x4000;
        code[2u16] = 0;
        code[3u16] = 0x7000;
        at = 4;
    }
    if install {
        gpu_packet_dma(code.as_array(), at, c, source, 69, 0);
        code[at + 2] = 1;
        code[at + 3] = 0x4000;
        at += 4;
    }
    if width == 50 {
        gpu_ui_packet_range(code.as_array(), at, y * 50, (y + height) * 50);
    } else {
        let mut row: u16 = 0;
        while row < height {
            let first = (y + row) * 50 + x;
            gpu_ui_packet_range(code.as_array(), at + row * 2, first, first + width);
            row += 1;
        }
    }
    if !gpu_record_append(slot, code.as_array(), words, descriptor, count, alignment) {
        return false;
    }
    if !same {
        gpu_bind_copy(c, GPU_CB_UI, descriptor, 0, 16);
        c[GPU_RECORD_UI_WORD] = source + 1;
    }
    c[GPU_RECORD_VALID] |= 32;
    c[GPU_RECORD_UI_CLEAN] = 1;
    true
}
