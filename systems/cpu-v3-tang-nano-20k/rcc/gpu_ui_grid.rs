//! Persistent external UI grids. Only one 25-word row lives on the CPU stack.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_storage;
use gpu_storage::*;

#[derive(Copy, Clone)]
pub struct CellRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

pub fn gpu_ui_atlas_init(mut out: Array<u16>, memory: GpuMemory, glyph_count: u16) -> bool {
    if memory.first_sector() & 511 != 0
        || glyph_count == 0
        || glyph_count > 255
        || memory.sector_count() < glyph_count * 2
    {
        return false;
    }
    let mut at: u16 = 0;
    while at < GPU_API_SMALL_WORDS {
        out[at] = 0;
        at += 1;
    }
    out[GPU_UI_ATLAS_BASE] = memory.first_sector();
    out[GPU_UI_ATLAS_COUNT] = memory.sector_count();
    out[GPU_UI_ATLAS_GLYPH_COUNT] = glyph_count;
    true
}

pub fn gpu_ui_grid_init(mut out: Array<u16>, memory: GpuMemory, atlas: Array<u16>) -> bool {
    if memory.sector_count() < 6 {
        return false;
    }
    let mut at: u16 = 0;
    while at < GPU_API_SMALL_WORDS {
        out[at] = 0;
        at += 1;
    }
    out[GPU_UI_GRID_BASE] = memory.first_sector();
    out[GPU_UI_GRID_COUNT] = memory.sector_count();
    out[GPU_UI_GRID_ATLAS_BASE] = atlas[GPU_UI_ATLAS_BASE];
    out[GPU_UI_GRID_ATLAS_COUNT] = atlas[GPU_UI_ATLAS_COUNT];
    out[GPU_UI_GRID_GLYPH_COUNT] = atlas[GPU_UI_ATLAS_GLYPH_COUNT];
    true
}

pub fn gpu_ui_descriptor_prepare(grid: Array<u16>, mut out: Array<u16>) -> bool {
    let mut at: u16 = 0;
    while at < 16 {
        out[at] = 0;
        at += 1;
    }
    out[0u16] = grid[GPU_UI_GRID_BASE];
    out[1u16] = grid[GPU_UI_GRID_ATLAS_BASE] >> 9;
    true
}

#[cfg(gpu_debug)]
pub fn gpu_ui_rect_valid(rect: CellRect) -> bool {
    rect.x <= 50 && rect.y <= 15 && rect.width <= 50 - rect.x && rect.height <= 15 - rect.y
}

#[cfg(gpu_debug)]
fn gpu_ui_glyph_valid(grid: Array<u16>, glyph: u8) -> bool {
    glyph == 255 || (glyph as u16) < grid[GPU_UI_GRID_GLYPH_COUNT]
}

pub fn gpu_ui_grid_fill(grid: Array<u16>, glyph: u8) -> bool {
    #[cfg(gpu_debug)]
    {
        if !gpu_ui_glyph_valid(grid, glyph) {
            return false;
        }
    }
    let mut row: Buf<u16, 25> = Buf::new([0; 25]);
    let packed = (glyph as u16) | ((glyph as u16) << 8);
    let mut at: u16 = 0;
    while at < 25 {
        row[at] = packed;
        at += 1;
    }
    let mut y: u16 = 0;
    while y < 15 {
        if !gpu_window_write_words(grid[GPU_UI_GRID_BASE], y * 25, row.as_array(), 25) {
            return false;
        }
        y += 1;
    }
    true
}

pub fn gpu_ui_grid_write(grid: Array<u16>, rect: CellRect, glyphs: &[u8]) -> bool {
    if rect.width == 0 || rect.height == 0 {
        return true;
    }
    #[cfg(gpu_debug)]
    {
        if !gpu_ui_rect_valid(rect) {
            return false;
        }
        // Check used glyphs before any write, so a later bad glyph cannot leave
        // an earlier row changed. Unused input after the rectangle is ignored.
        let mut n: u16 = 0;
        let cells = rect.width * rect.height;
        while n < cells && n < glyphs.len() as u16 {
            if !gpu_ui_glyph_valid(grid, glyphs[n as usize]) {
                return false;
            }
            n += 1;
        }
    }
    let mut row: Buf<u16, 25> = Buf::new([0; 25]);
    let parity = rect.x & 1;
    let words = (parity + rect.width + 1) >> 1;
    let sector = grid[GPU_UI_GRID_BASE];
    let mut cursor: u16 = 0;
    let mut y: u16 = 0;
    while y < rect.height {
        let first_word = (rect.y + y) * 25 + (rect.x >> 1);
        // Fully replaced words need no read. Preserve only the untouched byte
        // at either edge; A splits any row write crossing its DSEG3 window.
        if parity != 0 {
            if !gpu_window_read_words(sector, first_word, row.as_array(), 1) {
                return false;
            }
        }
        if (parity + rect.width) & 1 != 0 {
            let tail = unsafe { row.as_ptr().add((words - 1) as i16).as_u16_array() };
            if !gpu_window_read_words(sector, first_word + words - 1, tail, 1) {
                return false;
            }
        }
        let mut x: u16 = 0;
        while x < rect.width {
            let mut glyph: u16 = 255;
            if cursor < glyphs.len() as u16 {
                glyph = glyphs[cursor as usize] as u16;
            }
            let word = (parity + x) >> 1;
            if (parity + x) & 1 == 0 {
                row[word] = (row[word] & 0xff00) | glyph;
            } else {
                row[word] = (row[word] & 0x00ff) | (glyph << 8);
            }
            cursor += 1;
            x += 1;
        }
        if !gpu_window_write_words(sector, first_word, row.as_array(), words) {
            return false;
        }
        y += 1;
    }
    true
}
