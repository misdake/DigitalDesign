// Shared qualification: source packets and slot metadata stay in caller storage.
pub struct MeshStoreCase {
    pub store: scene_meshes::Store,
    pub input: Array<u16>,
    pub operations: Array<u16>,
    pub count: u16,
}
fn store_at(data: Array<u16>, words: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(words as i16).as_u16_array() }
}
pub fn load_mesh_store_case(input: Array<u16>, storage: Array<u16>) -> MeshStoreCase {
    if input[2u16] != 4 || input[1u16] > (11988 - 64) / 4 {
        halt(0x7f45);
    }
    let store = scene_meshes::Store {
        records: storage,
        sources: store_at(storage, 60),
        data: store_at(storage, 64),
        pages: store_at(storage, 72),
        arena_page: 65535,
        arena_start: 0,
        arena_words: 0,
        indices: store_at(storage, 68),
        cold: static_assets::AssetBytes {
            first_sector: 0,
            sector_count: 0,
            byte_len_lo: 0,
            byte_len_hi: 0,
        },
        slot_words: input[1u16],
        count: 0,
        capacity: 4,
        free: store_at(storage, 52),
        free_count: 0,
        names: store_at(storage, 56),
        name_count: 0,
        name_capacity: 2,
    };
    MeshStoreCase {
        store: store,
        input: input,
        operations: store_at(input, input[6u16]),
        count: input[3u16],
    }
}
fn store_model(input: Array<u16>, index: u16) -> scene_meshes::Model {
    let packet = store_at(input, input[4 + index]);
    load_mesh_model(packet)
}
pub fn load_mesh_model(packet: Array<u16>) -> scene_meshes::Model {
    scene_meshes::Model {
        geometry: store_at(packet, 8),
        geometry_words: packet[0u16],
        cold: false,
        page: 65535,
        vertices: unsafe { store_at(packet, 10).as_ptr().as_i16_array() },
        indices: store_at(packet, 10 + packet[8u16] * 3),
        flags: store_at(packet, 10 + packet[8u16] * 3 + packet[9u16] * 3),
        centers: store_at(packet, 8 + packet[0u16]),
        lengths: store_at(packet, 5),
        center_stride: packet[4u16],
        hint: packet[1u16],
        radius: packet[2u16],
        materials: packet[3u16],
    }
}
fn store_copy(mut out: Array<u16>, source: Array<u16>, position: u16, count: u16) -> u16 {
    let mut i = 0u16;
    while i < count {
        out[position + i] = source[i];
        i += 1;
    }
    position + count
}
pub fn capture_mesh_store(out: Array<u16>, result: fix32, store: &scene_meshes::Store) -> u16 {
    let mut output = out;
    output[0u16] = result.lo_bits();
    output[1u16] = result.hi_bits();
    output[2u16] = store.count;
    output[3u16] = store.free_count;
    let mut position = store_copy(out, store.free, 4, store.free_count);
    output[position] = store.name_count;
    position += 1;
    let mut key = 0u16;
    while key < 2 {
        let mut id = 65535u16;
        let mut i = 0u16;
        while i < store.name_count {
            if store.names[i * 2] == key {
                id = store.names[i * 2 + 1];
            }
            i += 1;
        }
        output[position] = id;
        position += 1;
        key += 1;
    }
    let mut i = 0u16;
    while i < store.count {
        let saved = scene_meshes::select(store, i);
        let row = store_at(store.records, i * 13);
        let data = scene_meshes::payload(store, i);
        position = store_copy(out, row, position, 13);
        let mut mutable_words = 0u16;
        if row[8u16] != 0 {
            let shape = scene_meshes::source(store, i);
            let vertices = shape[0u16] * 3;
            let triangles = shape[1u16];
            position = store_copy(out, shape, position, 2);
            position = store_copy(out, data, position, vertices);
            position = store_copy(out, store_at(shape, 2 + vertices), position, triangles * 3);
            position = store_copy(out, store_at(data, vertices), position, triangles);
            let attributes = 2 + vertices + triangles * 4;
            position = store_copy(
                out,
                store_at(shape, attributes),
                position,
                row[8u16] - attributes,
            );
            mutable_words = vertices + triangles;
        }
        let mut column = 0u16;
        while column < 3 {
            let length = row[9 + column];
            if length != 65535 {
                position = store_copy(
                    out,
                    store_at(data, mutable_words + column * row[12u16]),
                    position,
                    length,
                );
            }
            column += 1;
        }
        i += 1;
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    position
}
pub fn run_mesh_store_step(case: &mut MeshStoreCase, frame: u16, out: Array<u16>) -> u16 {
    let op = store_at(case.operations, frame * 4);
    let kind = op[0u16];
    let a = op[1u16];
    let b = op[2u16];
    let c = op[3u16];
    let mut result = integer::from_i16(-1);
    if kind == 0 {
        let model = store_model(case.input, a);
        let mut key = b;
        if b == 2 {
            key = 0;
        }
        result = scene_meshes::bind(&mut case.store, key, &model);
    } else if kind == 1 {
        result = scene_meshes::private_copy(&mut case.store, a);
    } else if kind == 2 {
        if scene_meshes::release_private(&mut case.store, a) {
            result = integer::from_i16(1);
        }
    } else if kind == 3 || kind == 4 || kind == 8 {
        let view = scene_meshes::view(&case.store, a, kind != 8);
        let saved = scene_meshes::select(&case.store, a);
        if view.valid {
            result = integer::from_i16(1);
            if kind == 3 {
                if view.mesh.vertex_count != 0 {
                    let mut vertices = view.mesh.vertices;
                    vertices[0u16] = b as i16;
                }
            } else if kind == 4 {
                let mut radius = view.mesh.radius;
                radius[0u16] = b;
                if view.mesh.triangle_count != 0 {
                    let mut flags = view.mesh.flags;
                    flags[0u16] ^= c;
                }
                if view.mesh.center_lengths[0u16] != 65535 && view.mesh.center_lengths[0u16] != 0 {
                    let mut centers = view.mesh.centers;
                    centers[0u16] -= c as i16;
                }
            } else {
                result = fix32::from_words(view.mesh.radius[0u16], 0);
            }
        }
        unsafe {
            write_dseg_page(3, saved);
        }
    } else if kind == 5 {
        if scene_meshes::set_hint(&case.store, a, b) {
            result = integer::from_i16(1);
        }
    } else if kind == 6 {
        let model = store_model(case.input, a);
        result = scene_meshes::insert(&mut case.store, model.geometry, model.geometry_words);
    } else if kind == 7 {
        let mut row = store_at(case.store.records, a * 13);
        row[4u16] = 65535;
        row[5u16] = 65535;
        row[6u16] = 0;
        row[7u16] = 0;
        if b == 0 {
            row[6u16] = 65535;
            row[7u16] = 65535;
        }
        result = integer::from_i16(1);
    }
    capture_mesh_store(out, result, &case.store)
}
