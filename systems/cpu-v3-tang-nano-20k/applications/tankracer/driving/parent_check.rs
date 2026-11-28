//! Stream original parent/resource answers through the existing bounded cache.
#![link_cseg = "frame"]
use super::*;
pub fn check_parents(
    parents: Array<scene_parent::Actor>,
    bindings: Array<u16>,
    state: &scene_parent::State,
    asset: AssetBytes,
    offset: fix32,
) -> u16 {
    let store = &state.resources;
    let header: Buf<u16, 1> = Buf::new([0; 1]);
    read(header.as_array(), asset, offset, 1);
    let count = header[0u16];
    let mut expected = reference(asset, offset, count);
    expect_word(store.count, &mut expected, 1);
    expect_word(store.free_count, &mut expected, 2);
    let mut cursor = 3u16;
    expect_block(store.free, &mut expected, cursor, store.free_count);
    cursor += store.free_count;
    expect_word(store.name_count, &mut expected, cursor);
    cursor += 1;
    let mut i = 0u16;
    while i < bindings[2u16] {
        let name = bindings[8 + i * 26 + 3];
        let mut value = 65535u16;
        let mut j = 0u16;
        while j < store.name_count {
            if store.names[j * 2] == name {
                value = store.names[j * 2 + 1];
                break;
            }
            j += 1;
        }
        expect_word(value, &mut expected, cursor);
        cursor += 1;
        i += 1;
    }
    let saved = read_dseg_page(3);
    i = 0;
    while i < bindings[2u16] {
        let actor = &parents[i];
        let object = &actor.fragments;
        expect_word(object.id, &mut expected, cursor);
        let present = at(state.lifetime.records, (object.id * 8) as i16)[7u16] & 1;
        expect_word(present, &mut expected, cursor + 1);
        cursor += 2;
        if present == 0 {
            if actor.has_state {
                halt(0x7f4b);
            }
            i += 1;
            continue;
        }
        expect_word(object.active[0u16], &mut expected, cursor);
        expect_block(object.current_mesh, &mut expected, cursor + 1, 2);
        let mesh = object.private_mesh;
        expect_word(mesh.lo_bits(), &mut expected, cursor + 3);
        expect_word(mesh.hi_bits(), &mut expected, cursor + 4);
        expect_word(object.motion.count, &mut expected, cursor + 5);
        cursor += 6;
        if object.motion.count != 0 {
            unsafe {
                write_dseg_page(3, object.motion_page);
            }
            expect_block(object.motion.header, &mut expected, cursor, 8);
            cursor += 8;
            expect_block(
                object.motion.faces,
                &mut expected,
                cursor,
                object.motion.count * 16,
            );
            cursor += object.motion.count * 16;
        }
        if mesh.hi_bits() == 0 {
            let id = mesh.lo_bits();
            if id >= store.count {
                halt(0x7f48);
            }
            let row = at(store.records, (id * 13) as i16);
            expect_word((row[0u16] >> 1) & 1, &mut expected, cursor);
            expect_word(row[1u16], &mut expected, cursor + 1);
            expect_word(row[2u16], &mut expected, cursor + 2);
            expect_block(at(row, 4), &mut expected, cursor + 3, 4);
            let prior = scene_meshes::select(store, id);
            let view = scene_meshes::view(store, id, false);
            if !view.valid {
                halt(0x7f49);
            }
            let geometry = &view.mesh;
            expect_word(geometry.vertex_count, &mut expected, cursor + 7);
            expect_word(geometry.triangle_count, &mut expected, cursor + 8);
            cursor += 9;
            expect_block(
                unsafe { geometry.vertices.as_ptr().as_u16_array() },
                &mut expected,
                cursor,
                geometry.vertex_count * 3,
            );
            cursor += geometry.vertex_count * 3;
            expect_block(
                geometry.flags,
                &mut expected,
                cursor,
                geometry.triangle_count,
            );
            cursor += geometry.triangle_count;
            let mut column = 0u16;
            while column < 3 {
                let length = geometry.center_lengths[column];
                expect_word(length, &mut expected, cursor);
                cursor += 1;
                if length != 65535 {
                    let data = unsafe {
                        geometry
                            .centers
                            .as_ptr()
                            .add((column * geometry.center_stride) as i16)
                            .as_u16_array()
                    };
                    expect_block(data, &mut expected, cursor, length);
                    cursor += length;
                }
                column += 1;
            }
            unsafe {
                write_dseg_page(3, prior);
            }
        }
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    if cursor != count {
        halt(0x7f4a);
    }
    count
}

// The complete physical columns and live blast recipients, with one bounded
// answer cache. Asset reads restore the selected working bank.
pub fn check_debris(state: &scene_parent::State, asset: AssetBytes, offset: fix32) -> u16 {
    let h: Buf<u16, 1> = Buf::new([0; 1]);
    read(h.as_array(), asset, offset, 1);
    let count = h[0u16];
    let mut expected = reference(asset, offset, count);
    let saved = read_dseg_page(3);
    let mut cursor = 1u16;
    let mut i = 0u16;
    while i < state.actor_count {
        let actor = &state.actors[i];
        let id = actor.fragments.id;
        if id != 169 && id != 170 && id != 171 && id != 179 && id != 180 && id != 181 {
            i += 1;
            continue;
        }
        expect_word(id, &mut expected, cursor);
        let present = at(state.lifetime.records, (id * 8) as i16)[7u16] & 8;
        expect_word(present >> 3, &mut expected, cursor + 1);
        cursor += 2;
        if present != 0 {
            let d = &actor.debris[0u16];
            unsafe {
                write_dseg_page(3, actor.fragments.motion_page);
            }
            expect_block(d.header, &mut expected, cursor, 18);
            cursor += 18;
            expect_block(d.pieces, &mut expected, cursor, d.count * 42);
            cursor += d.count * 42;
            if d.has_animation {
                expect_block(d.animation, &mut expected, cursor, 34);
                cursor += 34;
            }
            expect_block(d.info, &mut expected, cursor, d.count * 3);
            cursor += d.count * 3;
        }
        i += 1;
    }
    i = 0;
    while i < state.car_count {
        let car = &state.cars[i];
        expect_word(car.entity.lo_bits(), &mut expected, cursor);
        expect_word(car.entity.hi_bits(), &mut expected, cursor + 1);
        expect_word(car.slot, &mut expected, cursor + 2);
        let mut player = 0u16;
        let mut visible = 0u16;
        if car.player {
            player = 1;
        }
        if car.visible {
            visible = 1;
        }
        expect_word(player, &mut expected, cursor + 3);
        expect_word(visible, &mut expected, cursor + 4);
        expect_block(car.blast, &mut expected, cursor + 5, 14);
        cursor += 19;
        i += 1;
    }
    let symbols = state.symbols;
    expect_block(symbols, &mut expected, cursor, 1 + symbols[0u16] * 4);
    cursor += 1 + symbols[0u16] * 4;
    unsafe {
        write_dseg_page(3, saved);
    }
    if cursor != count {
        halt(0x7f4d);
    }
    count
}

// All 37 authored auxiliary payloads coexist with all 62 debris pieces.
// Exercise the exact remaining span, exhaustion and coalescing before gameplay.
#[inline(never)]
pub fn check_debris_bank(store: &scene_meshes::Store, prefix: u16) {
    let spans: Buf<u16, 37> = Buf::new([0; 37]);
    let allocations: Buf<u16, 37> = Buf::new([0; 37]);
    read(spans.as_array(), DRIVING_AUXILIARY_SPANS, fix32::zero(), 37);
    if prefix != 11106 || store.arena_start != prefix {
        halt(0x7f4e);
    }
    let page = store.arena_page;
    let count = store.arena_words;
    let mut slots = allocations.as_array();
    let saved = read_dseg_page(3);
    let mut i = 0u16;
    while i < 37 {
        let size = spans[i];
        let address = mesh_storage::allocate(page, prefix, count, size);
        if address == 0 {
            halt(0x7f4e);
        }
        slots[i] = address;
        unsafe {
            write_dseg_page(3, page);
        }
        let mut payload = unsafe { Ptr::from_addr(address).as_u16_array() };
        let mut j = 0u16;
        while j < size {
            payload[j] = (i * 257) ^ j;
            j += 1;
        }
        unsafe {
            write_dseg_page(3, saved);
        }
        i += 1;
    }
    if mesh_storage::used(page, prefix, count) != 5173
        || mesh_storage::allocate(page, prefix, count, 104) != 0
    {
        halt(0x7f4e);
    }
    let tail = mesh_storage::allocate(page, prefix, count, 103);
    if tail == 0 || mesh_storage::allocate(page, prefix, count, 1) != 0 {
        halt(0x7f4e);
    }
    mesh_storage::release(page, prefix, count, tail);
    i = 0;
    while i < 37 {
        unsafe {
            write_dseg_page(3, page);
        }
        let payload = unsafe { Ptr::from_addr(slots[i]).as_u16_array() };
        let mut j = 0u16;
        while j < spans[i] {
            if payload[j] != ((i * 257) ^ j) {
                halt(0x7f4e);
            }
            j += 1;
        }
        unsafe {
            write_dseg_page(3, saved);
        }
        i += 1;
    }
    // Alternating release order checks both preceding and following merges.
    i = 0;
    while i < 37 {
        mesh_storage::release(page, prefix, count, slots[i]);
        i += 2;
    }
    i = 1;
    while i < 37 {
        mesh_storage::release(page, prefix, count, slots[i]);
        i += 2;
    }
    if mesh_storage::used(page, prefix, count) != 0 || read_dseg_page(3) != saved {
        halt(0x7f4e);
    }
}
