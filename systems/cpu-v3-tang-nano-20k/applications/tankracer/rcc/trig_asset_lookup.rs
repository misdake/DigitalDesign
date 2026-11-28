//! Immutable AssetBytes backend for the same raw signed-integer lookup contract.
//! The argument points to an ordinary low-memory AssetBytes root, never a window.
use crate::dsl_rt::*;
use crate::integer;
mod gpu_window;
use gpu_window::*;
pub fn index(table: Array<i16>, index: u16) -> fix32 {
    let root = unsafe { table.as_ptr().as_u16_array() };
    // These exact tables are below 64KiB; preserve the root's exact byte length.
    if root[3u16] != 0 || index >= root[2u16] >> 1 {
        halt(0x7d01);
    }
    let first = root[0u16];
    if gpu_memory_faulted() || (index >> 6) > 65535 - first {
        halt(0x7d02);
    }
    // One immutable word: physical_word = first_sector * 64 + index.
    // The page/offset split is exact and leaves the incoming window intact.
    let sector = first + (index >> 6);
    let page = sector >> 8;
    let offset = ((sector & 255) << 6) + (index & 63);
    let saved = read_dseg_page(3);
    if saved != page {
        unsafe {
            write_dseg_page(3, page);
        }
    }
    let value = unsafe { Ptr::from_addr(0xc000 + offset).read(0) };
    if saved != page {
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    // Raw i32 carrier: explicit i16 sign extension, never ordinary I16TOF scaling.
    integer::from_i16(value as i16)
}
