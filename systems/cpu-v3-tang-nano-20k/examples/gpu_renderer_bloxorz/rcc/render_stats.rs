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
    pass.draw(mesh, model, material, view);
    DRAW_CALLS[0u16] += mesh.meshlet_count();
}
pub fn counted_ui(pass: &mut Pass, grid: &UiGrid, rect: CellRect) {
    pass.draw_ui(grid, rect);
    if rect.width != 0 && rect.height != 0 {
        DRAW_CALLS[0u16] += if rect.width == 50 { 1u16 } else { rect.height };
    }
}

// Value and validity are separate from DRAW: cold F1 has no preceding vblank.
pub static FRAME_RENDER_MS: Buf<u16, 2> = Buf::new([0; 2]);
pub fn render_time_complete(begin: u16, end: u16, started: bool) {
    FRAME_RENDER_MS[0u16] = end - begin;
    FRAME_RENDER_MS[1u16] = if started { 1u16 } else { 0u16 };
}
