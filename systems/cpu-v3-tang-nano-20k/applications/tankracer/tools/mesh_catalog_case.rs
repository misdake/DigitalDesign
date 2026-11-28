// Whole-owner comparisons stream one resource at a time into caller scratch.
pub fn capture_catalog_header(out: Array<u16>, catalog: &mesh_catalog::Catalog) -> u16 {
    let store = &catalog.store;
    let mut output = out;
    output[0u16] = store.count;
    output[1u16] = store.free_count;
    let mut position = store_copy(out, store.free, 2, store.free_count);
    output[position] = store.name_count;
    position += 1;
    let mut key = 0u16;
    while key < catalog.header.as_array()[15u16] {
        let mut i = 0u16;
        while i < store.name_count {
            if store.names[i * 2] == key {
                output[position] = key;
                output[position + 1] = store.names[i * 2 + 1];
                position += 2;
            }
            i += 1;
        }
        key += 1;
    }
    position
}
pub fn capture_catalog_resource(out: Array<u16>, catalog: &mesh_catalog::Catalog, id: u16) -> u16 {
    let store = &catalog.store;
    let saved = scene_meshes::select(store, id);
    let row = store_at(store.records, id * 13);
    let mut position = store_copy(out, row, 0, 13);
    let mut output = out;
    output[0u16] = row[0u16] & 3;
    let data = scene_meshes::payload(store, id);
    let mut mutable_words = 0u16;
    if row[8u16] != 0 {
        let source = scene_meshes::source(store, id);
        let vertices = source[0u16] * 3;
        let triangles = source[1u16];
        position = store_copy(out, source, position, 2);
        let packet = attitude::get(store_at(source, 2), 0);
        if store.data[id] == 0 {
            mesh_catalog::read(
                catalog.models.asset,
                packet + fix32::from_words(10, 0),
                store_at(out, position),
                vertices,
            );
            position += vertices;
        } else {
            position = store_copy(out, data, position, vertices);
        }
        // Capture may itself live in DSEG2. The asset reader preserves the
        // destination window while streaming the immutable index column.
        mesh_catalog::read(
            catalog.models.asset,
            fix32::from_words(catalog.header.as_array()[8u16] + source[4u16], 0),
            store_at(out, position),
            triangles * 3,
        );
        position += triangles * 3;
        if store.data[id] == 0 {
            mesh_catalog::read(
                catalog.models.asset,
                packet + fix32::from_words(10 + vertices + triangles * 3, 0),
                store_at(out, position),
                triangles,
            );
            position += triangles;
        } else {
            position = store_copy(out, store_at(data, vertices), position, triangles);
        }
        let offset = attitude::get(store_at(source, 2), 0)
            + fix32::from_words(10 + vertices + triangles * 4, 0);
        mesh_catalog::read(
            catalog.models.asset,
            offset,
            store_at(out, position),
            triangles * 19,
        );
        position += triangles * 19;
        mutable_words = vertices + triangles;
    }
    let mut column = 0u16;
    while column < 3 {
        let length = row[9 + column];
        if length != 65535 {
            if store.data[id] == 0 {
                let source = scene_meshes::source(store, id);
                let offset = attitude::get(store_at(source, 2), 0)
                    + fix32::from_words(8 + row[8u16] + column * row[12u16], 0);
                mesh_catalog::read(
                    catalog.models.asset,
                    offset,
                    store_at(out, position),
                    length,
                );
                position += length;
            } else {
                position = store_copy(
                    out,
                    store_at(data, mutable_words + column * row[12u16]),
                    position,
                    length,
                );
            }
        }
        column += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    position
}
pub fn run_catalog_controls(
    catalog: &mut mesh_catalog::Catalog,
    controls: static_assets::AssetBytes,
    count: u16,
    scratch: Array<u16>,
) {
    let operation: Buf<u16, 6> = Buf::new([0; 6]);
    let op = operation.as_array();
    let mut i = 0u16;
    while i < count {
        let saved = read_dseg_page(3);
        mesh_catalog::read(controls, fix32::from_words(i * 6, 0), op, 6);
        if op[0u16] == 0 {
            let model = mesh_catalog::model(&catalog.models, op[1u16], scratch);
            let shared = scene_meshes::bind(&mut catalog.store, op[2u16], &model);
            if shared != fix32::from_words(op[3u16], 0) {
                halt(0x7f69);
            }
            let private = scene_meshes::private_copy(&mut catalog.store, shared.lo_bits());
            if private != fix32::from_words(op[4u16], 0) {
                halt(0x7f6a);
            }
        } else if op[0u16] == 1 {
            let view = scene_meshes::view(&catalog.store, op[1u16], true);
            scene_meshes::select(&catalog.store, op[1u16]);
            let mut vertices = view.mesh.vertices;
            vertices[0u16] = op[2u16] as i16;
            let view = scene_meshes::view(&catalog.store, op[1u16], true);
            let mut radius = view.mesh.radius;
            let mut flags = view.mesh.flags;
            let mut centers = view.mesh.centers;
            radius[0u16] = op[3u16];
            flags[0u16] ^= op[4u16];
            centers[0u16] -= op[4u16] as i16;
        } else if op[0u16] == 2 {
            if scene_meshes::private_copy(&mut catalog.store, op[1u16])
                != fix32::from_words(op[2u16], 0)
            {
                halt(0x7f6b);
            }
        } else if op[0u16] == 4 {
            let view = scene_meshes::view(&catalog.store, op[1u16], true);
            scene_meshes::select(&catalog.store, op[1u16]);
            if !view.valid {
                halt(0x7f6b);
            }
            let mut vertices = view.mesh.vertices;
            vertices[0u16] = op[2u16] as i16;
        } else if op[0u16] == 3 {
            if !scene_meshes::release_private(&mut catalog.store, op[1u16]) {
                halt(0x7f6c);
            }
        } else {
            halt(0x7f6d);
        }
        unsafe {
            write_dseg_page(3, saved);
        }
        i += 1;
    }
}

pub fn catalog_payload_words(catalog: &mesh_catalog::Catalog) -> u16 {
    let mut total = 0u16;
    let mut i = 0u16;
    while i < catalog.store.count {
        if catalog.store.data[i] != 0 {
            let root = scene_meshes::source(&catalog.store, i);
            let row = store_at(catalog.store.records, i * 13);
            total += root[0u16] * 3 + root[1u16] + row[12u16] * 3;
        }
        i += 1;
    }
    total
}
pub fn catalog_cpu_words(catalog: &mesh_catalog::Catalog) -> u16 {
    let mut total = 0u16;
    let mut i = 0u16;
    while i < catalog.store.count {
        if catalog.store.data[i] != 0 && catalog.store.pages[i] == 65535 {
            let root = scene_meshes::source(&catalog.store, i);
            let row = store_at(catalog.store.records, i * 13);
            total += root[0u16] * 3 + root[1u16] + row[12u16] * 3;
        }
        i += 1;
    }
    total
}
