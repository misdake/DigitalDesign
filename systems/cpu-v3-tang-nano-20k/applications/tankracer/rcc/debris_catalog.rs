//! Constructed debris in the level's existing 32-KiB auxiliary working bank.
use crate::dsl_rt::*;
use crate::static_assets::AssetBytes;
use crate::{mesh_catalog, shatter};

fn at(data: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset as i16).as_u16_array() }
}
fn word(value: u16) -> fix32 {
    fix32::from_words(value, 0)
}
// Directory and immutable parameters stay in the caller's fixed mapped page.
// Asset I/O borrows DSEG3, so hot columns cross one small CPU scratch buffer.
pub fn load(
    asset: AssetBytes,
    page: u16,
    start: u16,
    directory: Array<u16>,
    info: Array<u16>,
    states: Array<shatter::State>,
) -> u16 {
    let header: Buf<u16, 16> = Buf::new([0; 16]);
    let h = header.as_array();
    mesh_catalog::read(asset, word(0), h, 16);
    if page >= 256
        || start > 16384
        || h[0u16] != 0x4453
        || h[1u16] != 1
        || h[2u16] != 6
        || h[3u16] > 16384 - start
        || h[4u16] != 186
        || h[5u16] != 16
        || h[6u16] != 76
        || h[7u16] != 262
        || h[8u16] != h[7u16] + h[3u16]
        || asset.byte_len_hi != 0
        || asset.byte_len_lo != h[8u16] * 2
    {
        halt(0x7f48);
    }
    mesh_catalog::read(asset, word(16), directory, 60);
    mesh_catalog::read(asset, word(h[6u16]), info, h[4u16]);
    let scratch: Buf<u16, 64> = Buf::new([0; 64]);
    let saved = read_dseg_page(3);
    let mut cursor = 0u16;
    let mut data = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    while cursor < h[3u16] {
        let mut count = h[3u16] - cursor;
        if count > 64 {
            count = 64;
        }
        mesh_catalog::read(asset, word(h[7u16] + cursor), scratch.as_array(), count);
        unsafe {
            write_dseg_page(3, page);
        }
        let mut i = 0u16;
        while i < count {
            data[start + cursor + i] = scratch.as_array()[i];
            i += 1;
        }
        unsafe {
            write_dseg_page(3, saved);
        }
        cursor += count;
    }
    let mut i = 0u16;
    let mut hot = 0u16;
    let mut cold = 0u16;
    while i < 6 {
        let row = at(directory, i * 10);
        let animation = row[4u16];
        if hot > h[3u16]
            || h[3u16] - hot < 18
            || cold > h[4u16]
            || row[1u16] != hot
            || row[2u16] != hot + 18
            || row[3u16] > (h[3u16] - hot - 18) / 42
            || row[5u16] != cold
            || row[3u16] > (h[4u16] - cold) / 3
        {
            halt(0x7f49);
        }
        hot += 18 + row[3u16] * 42;
        cold += row[3u16] * 3;
        if animation != 65535 {
            if animation != hot || h[3u16] - hot < 34 {
                halt(0x7f49);
            }
            hot += 34;
        }
        let s = &mut states[i];
        s.header = at(data, start + row[1u16]);
        s.pieces = at(data, start + row[2u16]);
        s.info = at(info, row[5u16]);
        s.count = row[3u16];
        s.animation = data;
        if animation != 65535 {
            s.animation = at(data, start + animation);
        }
        s.has_animation = animation != 65535;
        s.has_sprite = row[6u16] != 0;
        s.sprite_radius = fix32::from_words(row[7u16], row[8u16]);
        i += 1;
    }
    if hot != h[3u16] || cold != h[4u16] {
        halt(0x7f49);
    }
    start + hot
}
