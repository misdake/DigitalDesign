//! Packed row-major cells; snapshots and descriptors remain owned by the CB.
use crate::dsl_rt::*;
mod gpu_assets;
use gpu_assets::*;

pub const GPU_UI_CELL_WORDS: u16 = 375;
fn gpu_ui_cells_valid(cells: Array<u16>) -> bool {
    let address = cells.as_ptr().addr();
    address < 0xc000 && GPU_UI_CELL_WORDS <= 0xc000 - address
}
// Array is a pointer view: the caller supplies at least 375 allocated words.
pub fn gpu_ui_fill(mut cells: Array<u16>, id: u16) -> bool {
    if id > 255 || !gpu_ui_cells_valid(cells) {
        return false;
    }
    let pair = id | (id << 8);
    let mut i: u16 = 0;
    while i < GPU_UI_CELL_WORDS {
        cells[i] = pair;
        i += 1;
    }
    true
}
pub fn gpu_ui_set_cell(mut cells: Array<u16>, x: u16, y: u16, id: u16) -> bool {
    if x >= 50 || y >= 15 || id > 255 || !gpu_ui_cells_valid(cells) {
        return false;
    }
    let word = y * 25 + (x >> 1);
    if x & 1 == 0 {
        cells[word] = (cells[word] & 65280) | id;
    } else {
        cells[word] = (cells[word] & 255) | (id << 8);
    }
    true
}
fn gpu_ui_atlas_valid(atlas: Array<GpuUiAtlas>) -> bool {
    atlas[0u16].sector & 511 == 0
        && atlas[0u16].sectors >= 512
        && gpu_asset_extent(atlas[0u16].sector, atlas[0u16].sectors)
}
