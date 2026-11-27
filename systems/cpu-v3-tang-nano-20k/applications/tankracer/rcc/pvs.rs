//! Shared PVS blocks; adjacent views update only changed native 16-byte chunks.
use crate::dsl_rt::*;
mod boot_resources;
mod gpu_window;
use boot_resources::*;
use gpu_window::gpu_window_asset_read;

// The allocation contains mask, heading row, incoming/cached block IDs, then
// cached cell/root. World retains its original one-pointer representation.
pub fn new() -> Ptr {
    if MAP_PVS_COARSE != 0 {
        let storage = malloc(14);
        let mut words = unsafe { storage.as_u16_array() };
        words[12u16] = 65535;
        words[13u16] = 65535;
        return storage;
    }
    let needed = MAP_PVS_MASK_BLOCKS * 10 + MAP_PVS_YAW_SECTORS + 2;
    // Include the boundary tags when sizing full 32-byte cache lines. Later
    // Prepared-offset allocations retain their original cache-line phase.
    let allocation = ((needed + 2 + 15) >> 4) << 4;
    let storage = malloc(allocation - 2);
    let mut words = unsafe { storage.as_u16_array() };
    let first = MAP_PVS_MASK_BLOCKS * 9 + MAP_PVS_YAW_SECTORS;
    let end = MAP_PVS_MASK_BLOCKS * 10 + MAP_PVS_YAW_SECTORS + 2;
    let mut i = first;
    while i < end {
        words[i] = 65535;
        i += 1;
    }
    storage
}

pub fn load(storage: Ptr, key: u16) -> bool {
    if MAP_PVS_COARSE != 0 {
        return load_coarse(storage, key);
    }
    let mut words = unsafe { storage.as_u16_array() };
    let control = MAP_PVS_MASK_BLOCKS * 10 + MAP_PVS_YAW_SECTORS;
    let cell = key / MAP_PVS_YAW_SECTORS;
    let headings = unsafe { storage.add((MAP_PVS_MASK_BLOCKS * 8) as i16).as_u16_array() };
    if cell != words[control] {
        let entry: Buf<u16, 1> = Buf::new([0]);
        if !gpu_window_asset_read(entry.as_array(), MAP_VIEW_PVS.first_sector, cell, 1) {
            return false;
        }
        let row = entry[0u16];
        if !gpu_window_asset_read(
            headings,
            MAP_VIEW_PVS.first_sector + MAP_PVS_HEADING_FIRST_SECTOR + (row >> 1),
            (row & 1) << 5,
            MAP_PVS_YAW_SECTORS,
        ) {
            return false;
        }
        words[control] = cell;
    }
    let root = headings[key % MAP_PVS_YAW_SECTORS];
    if root == words[control + 1] {
        return true;
    }
    let refs = unsafe {
        storage
            .add((MAP_PVS_MASK_BLOCKS * 8 + MAP_PVS_YAW_SECTORS) as i16)
            .as_u16_array()
    };
    if !gpu_window_asset_read(
        refs,
        MAP_VIEW_PVS.first_sector + MAP_PVS_ROOT_FIRST_SECTOR + (root >> MAP_PVS_ROOT_SHIFT),
        (root & ((1u16 << MAP_PVS_ROOT_SHIFT) - 1)) << (6 - MAP_PVS_ROOT_SHIFT),
        MAP_PVS_MASK_BLOCKS,
    ) {
        return false;
    }
    let mut blocks = unsafe {
        storage
            .add((MAP_PVS_MASK_BLOCKS * 9 + MAP_PVS_YAW_SECTORS) as i16)
            .as_u16_array()
    };
    let mut i: u16 = 0;
    while i < MAP_PVS_MASK_BLOCKS {
        let block = refs[i];
        if block != blocks[i] {
            let destination = storage.add((i * 8) as i16);
            if block < 2 {
                let mut value = fix32::zero();
                if block == 1 {
                    value = fix32::from_words(65535, 65535);
                }
                vec4::export(vec4::new(value, value, value, value), destination);
            } else {
                let output = unsafe { destination.as_u16_array() };
                if !gpu_window_asset_read(
                    output,
                    MAP_VIEW_PVS.first_sector + MAP_PVS_BLOCK_FIRST_SECTOR + (block >> 3),
                    (block & 7) << 3,
                    8,
                ) {
                    return false;
                }
            }
            blocks[i] = block;
        }
        i += 1;
    }
    words[control + 1] = root;
    true
}

// The small direct format needs no block references or decompression.
fn load_coarse(storage: Ptr, key: u16) -> bool {
    let mut words = unsafe { storage.as_u16_array() };
    let cell = key >> 3;
    let headings = unsafe { storage.add(4).as_u16_array() };
    if words[12u16] != cell {
        let entry: Buf<u16, 1> = Buf::new([0]);
        if !gpu_window_asset_read(entry.as_array(), MAP_VIEW_PVS.first_sector, cell, 1) {
            return false;
        }
        let row = entry[0u16];
        if !gpu_window_asset_read(
            headings,
            MAP_VIEW_PVS.first_sector + MAP_PVS_HEADING_FIRST_SECTOR + (row >> 3),
            (row & 7) << 3,
            8,
        ) {
            return false;
        }
        words[12u16] = cell;
    }
    let root = headings[key & 7];
    if root == words[13u16] {
        return true;
    }
    if !gpu_window_asset_read(
        words,
        MAP_VIEW_PVS.first_sector + MAP_PVS_ROOT_FIRST_SECTOR + (root >> 4),
        (root & 15) << 2,
        4,
    ) {
        return false;
    }
    words[13u16] = root;
    true
}
