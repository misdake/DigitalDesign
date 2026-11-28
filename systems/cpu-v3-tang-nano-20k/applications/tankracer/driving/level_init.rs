//! First-map configuration and bank binding. Storage stays with the caller.
use super::*;

pub fn config(
    storage: Array<u16>,
    asset: AssetBytes,
    sine: Array<i16>,
    angle: Array<i16>,
) -> driving::Config {
    read(storage, asset, fix32::zero(), DRIVING_CONFIG_WORDS);
    driving::Config {
        spring: storage,
        drive: at(storage, 52),
        attitude: at(storage, 572),
        pose: at(storage, 577),
        sampling: at(storage, 595),
        environment: at(storage, 625),
        auxiliary: at(storage, 661),
        body: at(storage, 673),
        sine,
        angle,
    }
}

// DSEG2/3 remain on the world's fixed rows and metadata after binding.
// The caller restores its previous mappings when the level is released.
pub fn world(
    deformables: Array<u16>,
    page: u16,
    dynamic_page: u16,
    deformable_capacity: u16,
) -> scene_world::World {
    if page >= 255 || page & 1 != 0 {
        halt(0x7f37);
    }
    unsafe {
        write_dseg_page(2, page);
        write_dseg_page(3, page + 1);
    }
    let colliders = unsafe { Ptr::from_addr(0x8000).as_u16_array() };
    let properties = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    let memberships = at(properties, 3840);
    let mut ranges = at(properties, 7936);
    let origin = at(properties, 8128);
    let mut i = 0u16;
    while i < 64 {
        ranges[i * 3] = i * 64;
        ranges[i * 3 + 2] = 32;
        i += 1;
    }
    scene_world::World {
        colliders,
        properties,
        count: 240,
        fixed_count: 240,
        dynamic_page,
        dynamic_capacity: scene_world::DYNAMIC_ROWS,
        grid: spatial::Grid {
            page: page + 1,
            origin,
            shift_x: 0,
            shift_z: 0,
            items: memberships,
            ranges,
        },
        deformables: ordered::List {
            items: deformables,
            count: 0,
            capacity: deformable_capacity,
        },
    }
}
