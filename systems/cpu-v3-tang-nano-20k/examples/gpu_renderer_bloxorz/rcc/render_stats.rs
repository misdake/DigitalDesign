//! Application DRAW counters and the last completed vblank-to-render interval.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
pub static DRAW_CALLS: Buf<u16, 1> = Buf::new([0]);
pub fn counted_draw(
    pass: &mut Pass,
    mesh: &Mesh,
    model: &Model,
    material: &Material,
    view: &TextureView,
) {
    pass.draw(mesh.ptr(), model.ptr(), material.ptr(), view.ptr());
    DRAW_CALLS[0u16] += mesh.meshlet_count();
}
pub fn counted_ui(pass: &mut Pass, grid: &UiGrid, rect: CellRect) {
    pass.draw_ui(grid.ptr(), rect);
    if rect.width != 0 && rect.height != 0 {
        DRAW_CALLS[0u16] += if rect.width == 50 { 1u16 } else { rect.height };
    }
}

// Value and validity are separate from DRAW: cold F1 has no preceding vblank.
pub static FRAME_RENDER_US: Buf<u16, 2> = Buf::new([0; 2]);
pub fn render_time_complete(elapsed: u16, started: bool) {
    FRAME_RENDER_US[0u16] = elapsed;
    FRAME_RENDER_US[1u16] = if started && elapsed != 65535 {
        1u16
    } else {
        0u16
    };
}

pub fn draw_render_time(grid: &mut UiGrid, x: u16, y: u16) {
    if FRAME_RENDER_US[1u16] == 0 {
        grid.write(
            CellRect {
                x,
                y,
                width: 5,
                height: 1,
            },
            b"-----",
        );
        return;
    }
    let elapsed = FRAME_RENDER_US[0u16];
    // The last four valid readings already round down. Avoid overflowing u16.
    let rounded = if elapsed <= 65530 {
        elapsed + 5
    } else {
        elapsed
    };
    let hundredths = rounded / 10;
    let milliseconds = hundredths / 100;
    let leading = if milliseconds < 10 {
        255u16
    } else {
        48 + milliseconds / 10
    };
    grid.write_glyph(x, y, leading as u8);
    grid.write_glyph(x + 1, y, (48 + milliseconds % 10) as u8);
    grid.write_glyph(x + 2, y, 46);
    grid.write_glyph(x + 3, y, (48 + (hundredths / 10) % 10) as u8);
    grid.write_glyph(x + 4, y, (48 + hundredths % 10) as u8);
}
