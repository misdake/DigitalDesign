use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_ui_grid;
use gpu_ui_grid::*;

static ATLAS: Buf<u16, 16> = Buf::new([0; 16]);
static GRID: Buf<u16, 16> = Buf::new([0; 16]);
static DESCRIPTOR: Buf<u16, 16> = Buf::new([0xaced; 16]);

fn main() {
    let atlas_memory: GpuMemory = GpuMemory {
        first_sector: 0xa000,
        sector_count: 6,
    };
    let grid_memory: GpuMemory = GpuMemory {
        first_sector: 0x98ff,
        sector_count: 6,
    };
    if !gpu_ui_atlas_init(ATLAS.as_array(), atlas_memory, 3) {
        halt(1);
    }
    if !gpu_ui_grid_init(GRID.as_array(), grid_memory, ATLAS.as_array()) {
        halt(2);
    }
    // Releasing/reusing the atlas description must not change the grid's copy.
    ATLAS[0u16] = 0;
    if !gpu_ui_descriptor_prepare(GRID.as_array(), DESCRIPTOR.as_array())
        || DESCRIPTOR[0u16] != 0x98ff
        || DESCRIPTOR[1u16] != 0x50
    {
        halt(3);
    }
    let mut n: u16 = 2;
    while n < 16 {
        if DESCRIPTOR[n] != 0 {
            halt(4);
        }
        n += 1;
    }
    unsafe {
        write_dseg_page(3, 7);
    }
    if !gpu_ui_grid_fill(GRID.as_array(), 0) || read_dseg_page(3) != 7 {
        halt(5);
    }
    let rect: CellRect = CellRect {
        x: 1,
        y: 2,
        width: 49,
        height: 3,
    };
    if !gpu_ui_grid_write(GRID.as_array(), rect, b"\x01\x02\x01") || read_dseg_page(3) != 7 {
        halt(6);
    }
    let last: CellRect = CellRect {
        x: 48,
        y: 14,
        width: 1,
        height: 1,
    };
    if !gpu_ui_grid_write(GRID.as_array(), last, b"\x02") {
        halt(7);
    }
    let odd: CellRect = CellRect {
        x: 49,
        y: 13,
        width: 1,
        height: 1,
    };
    if !gpu_ui_grid_write(GRID.as_array(), odd, b"\x01") {
        halt(8);
    }
    #[cfg(gpu_debug)]
    {
        let invalid: CellRect = CellRect {
            x: 49,
            y: 14,
            width: 2,
            height: 1,
        };
        if gpu_ui_grid_write(GRID.as_array(), invalid, b"\x01") {
            halt(9);
        }
        if gpu_ui_grid_write(GRID.as_array(), last, b"\x03") {
            halt(10);
        }
        if gpu_ui_grid_fill(GRID.as_array(), 3) {
            halt(11);
        }
    }
    let empty: CellRect = CellRect {
        x: 0,
        y: 0,
        width: 0,
        height: 1,
    };
    let absent = unsafe { Ptr::from_addr(0xffff).as_u16_array() };
    if !gpu_ui_grid_write(absent, empty, b"") || read_dseg_page(3) != 7 {
        halt(12);
    }
    halt(0x600d);
}
