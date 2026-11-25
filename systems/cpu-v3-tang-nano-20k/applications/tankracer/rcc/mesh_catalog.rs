//! Constructed resources: hot slot metadata, shared indices and cold attributes.
use crate::dsl_rt::*;
use crate::static_assets::AssetBytes;
use crate::{attitude, scene_meshes};
mod gpu_window;

pub struct Catalog {
    pub store: scene_meshes::Store,
    pub header: Buf<u16, 16>,
    pub models: Sources,
}
// Immutable model lookup stays usable after the mutable slot owner is moved
// into its final level state. All backing metadata remains caller-owned.
pub struct Sources {
    pub asset: AssetBytes,
    pub count: u16,
    pub maximum_words: u16,
    // Six words per source: counts, cold packet offset, index offset, geometry words.
    pub records: Array<u16>,
    pub indices: Array<u16>,
}
fn at(data: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset as i16).as_u16_array() }
}
pub fn read(asset: AssetBytes, offset: fix32, destination: Array<u16>, count: u16) {
    let words = fix32::from_words(
        (asset.byte_len_lo >> 1) | (asset.byte_len_hi << 15),
        asset.byte_len_hi >> 1,
    );
    if offset < fix32::zero() || offset > words || word(count) > words - offset {
        halt(0x7f60);
    }
    if count == 0 {
        return;
    }
    if !gpu_window::gpu_window_asset_read(
        destination,
        asset.first_sector + (offset.hi_bits() << 10) + (offset.lo_bits() >> 6),
        offset.lo_bits() & 63,
        count,
    ) {
        halt(0x7f61);
    }
}
fn word(offset: u16) -> fix32 {
    fix32::from_words(offset, 0)
}
pub fn model(catalog: &Sources, id: u16, scratch: Array<u16>) -> scene_meshes::Model {
    if id >= catalog.count {
        halt(0x7f62);
    }
    let source = at(catalog.records, id * 6);
    let offset = attitude::get(at(source, 2), 0);
    read(catalog.asset, offset, scratch, 8);
    let vertices = source[0u16] * 3;
    let triangles = source[1u16];
    let span = vertices + triangles + scratch[4u16] * 3;
    if span > catalog.maximum_words || span > 1528 || scratch[0u16] != source[5u16] {
        halt(0x7f63);
    }
    scene_meshes::Model {
        geometry: source,
        geometry_words: source[5u16],
        cold: true,
        page: 65535,
        vertices: unsafe { Ptr::from_addr(0).as_i16_array() },
        indices: at(catalog.indices, source[4u16]),
        flags: unsafe { Ptr::from_addr(0).as_u16_array() },
        centers: unsafe { Ptr::from_addr(0).as_u16_array() },
        lengths: at(scratch, 5),
        center_stride: scratch[4u16],
        hint: scratch[1u16],
        radius: scratch[2u16],
        materials: scratch[3u16],
    }
}
// The caller reserves the declared metadata capacity and eight scratch words.
// Payload allocations stay stable until release; scratch is needed only on load.
pub fn load(asset: AssetBytes, memory: Array<u16>, capacity: u16, scratch: Array<u16>) -> Catalog {
    if capacity < 2560 {
        halt(0x7f68);
    }
    let mut catalog = Catalog {
        store: scene_meshes::Store {
            records: memory,
            free: at(memory, 1664),
            sources: at(memory, 1792),
            data: at(memory, 1920),
            pages: at(memory, 2176),
            arena_page: 65535,
            arena_start: 0,
            arena_words: 0,
            indices: at(memory, 2048),
            cold: asset,
            names: at(memory, 2304),
            slot_words: 0,
            count: 0,
            capacity: 128,
            free_count: 0,
            name_count: 0,
            name_capacity: 128,
        },
        header: Buf::new([0; 16]),
        models: Sources {
            asset: asset,
            count: 0,
            maximum_words: 0,
            records: at(memory, 2560),
            indices: memory,
        },
    };
    let h = catalog.header.as_array();
    read(asset, word(0), h, 16);
    if h[0u16] != 0x4d52
        || h[1u16] != 1
        || h[2u16] > 128
        || h[3u16] > 128
        || h[4u16] > 128
        || h[15u16] > 128
        || h[6u16] > 1528
        || h[7u16] != 16
        || h[8u16] != h[7u16] + h[3u16] * 8
        || h[9u16] != h[8u16] + h[5u16]
        || h[10u16] != h[9u16] + h[2u16] * 14
        || 2560 + h[3u16] * 6 > capacity
        || h[5u16] > capacity - 2560 - h[3u16] * 6
    {
        halt(0x7f64);
    }
    if h[13u16] != ((asset.byte_len_lo >> 1) | (asset.byte_len_hi << 15))
        || h[14u16] != asset.byte_len_hi >> 1
    {
        halt(0x7f64);
    }
    catalog.store.slot_words = h[6u16];
    catalog.models.count = h[3u16];
    catalog.models.maximum_words = h[6u16];
    catalog.models.indices = at(catalog.models.records, h[3u16] * 6);
    read(asset, word(h[8u16]), catalog.models.indices, h[5u16]);
    let row: Buf<u16, 14> = Buf::new([0; 14]);
    let source_row = row.as_array();
    let mut i = 0u16;
    while i < h[3u16] {
        read(asset, word(h[7u16] + i * 8), source_row, 8);
        if source_row[0u16] > 509
            || source_row[1u16] > 1528
            || source_row[1u16] * 3 > h[5u16]
            || source_row[2u16] > h[5u16] - source_row[1u16] * 3
        {
            halt(0x7f65);
        }
        let mut destination = at(catalog.models.records, i * 6);
        destination[0u16] = source_row[0u16];
        destination[1u16] = source_row[1u16];
        destination[2u16] = source_row[4u16];
        destination[3u16] = source_row[5u16];
        destination[4u16] = source_row[2u16];
        destination[5u16] = source_row[3u16];
        i += 1;
    }
    i = 0;
    while i < h[2u16] {
        read(asset, word(h[9u16] + i * 14), source_row, 14);
        let m = model(&catalog.models, source_row[0u16], scratch);
        let id = scene_meshes::create(&mut catalog.store, &m, source_row[1u16] & 2 != 0);
        if id != word(i) {
            halt(0x7f66);
        }
        let mut destination = at(catalog.store.records, i * 13);
        let mut j = 0u16;
        while j < 13 {
            destination[j] = source_row[1 + j];
            j += 1;
        }
        i += 1;
    }
    read(asset, word(h[10u16]), catalog.store.names, h[4u16] * 2);
    i = 0;
    while i < h[4u16] {
        if catalog.store.names[i * 2] >= h[15u16]
            || catalog.store.names[i * 2 + 1] >= catalog.store.count
        {
            halt(0x7f67);
        }
        i += 1;
    }
    catalog.store.name_count = h[4u16];
    catalog
}
