//! Word reads into CPU storage or the selected level bank.
use super::*;
pub fn read(dst: Array<u16>, asset: AssetBytes, offset: fix32, count: u16) {
    if !gpu_window_asset_read(
        dst,
        asset.first_sector + (offset.hi_bits() << 10) + (offset.lo_bits() >> 6),
        offset.lo_bits() & 63,
        count,
    ) {
        halt(0x7f01);
    }
}
pub fn copy(mut to: Array<u16>, from: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        to[i] = from[i];
        i += 1;
    }
}
pub fn at(data: Array<u16>, offset: i16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset).as_u16_array() }
}
pub fn read_mapped(out: Array<u16>, asset: AssetBytes, offset: u16, count: u16) {
    let scratch: Buf<u16, 64> = Buf::new([0; 64]);
    let mut done = 0u16;
    while done < count {
        let mut length = count - done;
        if length > 64 {
            length = 64;
        }
        read(
            scratch.as_array(),
            asset,
            fix32::from_words(offset + done, 0),
            length,
        );
        copy(at(out, done as i16), scratch.as_array(), length);
        done += length;
    }
}
