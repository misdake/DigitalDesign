//! Write directly into the final native UI object; no full-screen CPU snapshot.
use crate::dsl_rt::*;
mod gpu;
use gpu::*;
mod boot_resources;
use boot_resources::*;

fn dui_write(ui: u16, first: u16, data: Array<u16>, count: u16) -> bool {
    gpu_pass_ui_write(ui, first, data, count)
}
fn dui_hex(ui: u16, first: u16, value: u16, count: u16) -> bool {
    let mut ids: Buf<u16, 4> = Buf::new([0; 4]);
    let mut i: u16 = 0;
    while i < count {
        let shift = (count - 1 - i) * 4;
        let mut digit = (value >> shift) & 15;
        if digit < 10 {
            digit += 48;
        } else {
            digit += 55;
        }
        ids[i] = digit;
        i += 1;
    }
    dui_write(ui, first, ids.as_array(), count)
}
fn dui_axis(ui: u16, first: u16, value: u16) -> bool {
    let mut ids: Buf<u16, 3> = Buf::new([48; 3]);
    let mut rest = value;
    while rest >= 100 {
        ids[0u16] += 1;
        rest -= 100;
    }
    while rest >= 10 {
        ids[1u16] += 1;
        rest -= 10;
    }
    ids[2u16] += rest;
    dui_write(ui, first, ids.as_array(), 3)
}
fn dui_panel(ui: u16, x: u16) -> bool {
    let mut row: Buf<u16, 8> = Buf::new([0; 8]);
    let mut y: u16 = 0;
    while y < 4 {
        let mut col: u16 = 0;
        while col < 8 {
            let mut flags: u16 = 0;
            if col == 0 {
                flags |= 1;
            }
            if col == 7 {
                flags |= 2;
            }
            if y == 0 {
                flags |= 4;
            }
            if y == 3 {
                flags |= 8;
            }
            if col == 4 {
                flags |= 16;
            }
            if y == 2 {
                flags |= 32;
            }
            row[col] = 160 + flags;
            col += 1;
        }
        if !dui_write(ui, (5 + y) * 50 + x, row.as_array(), 8) {
            return false;
        }
        y += 1;
    }
    return true;
}
pub fn dui_stick_cell(x: u16, raw_x: u16, raw_y: u16) -> u16 {
    return (5 + (raw_y >> 6)) * 50 + x + (raw_x >> 5);
}
pub fn dui_stick_id(raw_x: u16, raw_y: u16) -> u16 {
    return 128 + (((raw_y >> 3) & 7) << 2) + ((raw_x >> 3) & 3);
}
pub fn dui_record(s: Array<u16>, pressed: u16, released: u16, samples: u16) -> u16 {
    let cb = gpu_cb_begin();
    if cb == GPU_CB_NONE {
        return GPU_CB_NONE;
    }
    let pass = gpu_pass_begin(cb, 2113, 65535);
    if pass == GPU_CB_NONE {
        return GPU_CB_NONE;
    }
    let mut atlas: GpuUiAtlas = GpuUiAtlas {
        sector: DS2_UI_ATLAS,
        sectors: 512,
    };
    let ui = gpu_pass_ui_begin(pass, view_of(&mut atlas), 255);
    if ui == GPU_CB_NONE || !ds2_ui_labels(ui) {
        return GPU_CB_NONE;
    }
    let mut link: Buf<u16, 12> = Buf::new([68, 73, 83, 67, 79, 78, 78, 69, 67, 84, 69, 68]);
    let mut count: u16 = 12;
    if s[0u16] != 0 {
        link[0u16] = 67;
        link[1u16] = 79;
        link[2u16] = 78;
        link[3u16] = 78;
        link[4u16] = 69;
        link[5u16] = 67;
        link[6u16] = 84;
        link[7u16] = 69;
        link[8u16] = 68;
        count = 9;
    }
    if !dui_write(ui, 57, link.as_array(), count)
        || !dui_hex(ui, 76, s[1u16], 2)
        || !dui_hex(ui, 90, samples, 4)
        || !dui_hex(ui, 110, s[2u16], 4)
        || !dui_hex(ui, 124, pressed, 4)
        || !dui_hex(ui, 142, released, 4)
        || !dui_axis(ui, 158, s[5u16])
        || !dui_axis(ui, 167, s[6u16])
        || !dui_axis(ui, 184, s[3u16])
        || !dui_axis(ui, 193, s[4u16])
        || !dui_panel(ui, 6)
        || !dui_panel(ui, 32)
    {
        return GPU_CB_NONE;
    }
    let mut icon: Buf<u16, 1> = Buf::new([0]);
    let mut i: u16 = 0;
    while i < 16 {
        icon[0u16] = 1;
        if (s[2u16] & (1 << i)) != 0 {
            icon[0u16] = 2;
        }
        if !dui_write(
            ui,
            (12 + (i >> 3) * 2) * 50 + 2 + (i & 7) * 6,
            icon.as_array(),
            1,
        ) {
            return GPU_CB_NONE;
        }
        i += 1;
    }
    if !gpu_pass_ui_end(ui) {
        return GPU_CB_NONE;
    }
    // Separate transparent overlay preserves the panel background/crosshair.
    let left = dui_stick_cell(6, s[5u16], s[6u16]);
    let right = dui_stick_cell(32, s[3u16], s[4u16]);
    let mut first = left;
    let mut end = right + 1;
    if right < left {
        first = right;
        end = left + 1;
    }
    let dot = gpu_pass_ui_begin_range(pass, view_of(&mut atlas), 255, first, end);
    if dot == GPU_CB_NONE {
        return GPU_CB_NONE;
    }
    icon[0u16] = dui_stick_id(s[5u16], s[6u16]);
    if !dui_write(dot, left, icon.as_array(), 1) {
        return GPU_CB_NONE;
    }
    icon[0u16] = dui_stick_id(s[3u16], s[4u16]);
    if !dui_write(dot, right, icon.as_array(), 1)
        || !gpu_pass_ui_set(dot)
        || !gpu_pass_draw_ui_range(pass, left, left + 1)
        || !gpu_pass_draw_ui_range(pass, right, right + 1)
        || !gpu_pass_end(pass)
        || !gpu_cb_finish(cb)
    {
        return GPU_CB_NONE;
    }
    return cb;
}
