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
    let sample: Buf<u16, 1> = Buf::new([0; 1]);
    let values = sample.as_array();
    if !gpu_window_asset_read(values, root[0u16], index, 1) {
        halt(0x7d02);
    }
    // Raw i32 carrier: explicit i16 sign extension, never ordinary I16TOF scaling.
    integer::from_i16(values[0u16] as i16)
}
