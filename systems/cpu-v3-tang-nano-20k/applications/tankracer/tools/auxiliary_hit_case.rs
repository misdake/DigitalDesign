// Persistent real contact inputs, private resource arena and complete grid.
pub struct AuxiliaryHitCase {
    pub resources: scene_meshes::Store,
    pub object: scene_fragments::Object,
    pub runtime: scene_world::Runtime,
    pub grid: spatial::Grid,
    pub row: Array<u16>,
    pub meta: Array<u16>,
    pub model: scene_meshes::Model,
    pub controls: Array<u16>,
    pub count: u16,
}
fn auxiliary_model(input: Array<u16>, index: u16) -> scene_meshes::Model {
    load_mesh_model(store_at(input, input[2 + index]))
}
pub fn auxiliary_resources(input: Array<u16>, storage: Array<u16>) -> scene_meshes::Store {
    let size = input[4u16];
    if size > (11988 - 260) / 4 || input[13u16] > (1536 - 8) / 16 {
        halt(0x7f46);
    }
    let mut resources = scene_meshes::Store {
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
        slot_words: size,
        count: 0,
        capacity: 4,
        free: store_at(storage, 52),
        free_count: 0,
        names: store_at(storage, 56),
        name_count: 0,
        name_capacity: 2,
    };
    let initial = store_at(input, input[10u16]);
    let mut i = 0u16;
    while i < input[11u16] {
        let op = store_at(initial, i * 4);
        let mut status = fix32::zero();
        if op[0u16] == 0 {
            let model = auxiliary_model(input, op[1u16]);
            status = scene_meshes::bind(&mut resources, op[2u16], &model);
        } else if op[0u16] == 1 {
            status = scene_meshes::private_copy(&mut resources, op[1u16]);
        } else if !scene_meshes::release_private(&mut resources, op[1u16]) {
            status = integer::from_i16(-1);
        }
        if status < fix32::zero() {
            halt(0x7f47);
        }
        i += 1;
    }
    resources
}
pub fn auxiliary_object(
    input: Array<u16>,
    scratch: Array<u16>,
    resources: &scene_meshes::Store,
) -> scene_fragments::Object {
    let row = store_at(input, input[5u16]);
    let meta = store_at(input, input[7u16]);
    let state = store_at(input, input[12u16]);
    let mut header = scratch;
    let mut faces = store_at(scratch, 8);
    let mut i = 0u16;
    while i < 1536 {
        header[i] = 0;
        i += 1;
    }
    if row[7u16] & 32 != 0 {
        i = 0;
        while i < 8 {
            header[i] = state[1 + i];
            i += 1;
        }
        i = 0;
        while i < state[0u16] * 16 {
            faces[i] = state[9 + i];
            i += 1;
        }
    }
    let private = attitude::get(store_at(meta, 3), 0);
    let view = scene_meshes::view(resources, private.lo_bits(), false);
    scene_fragments::Object {
        id: input[14u16],
        collider: store_at(input, input[6u16]),
        active: meta,
        current_mesh: store_at(meta, 1),
        private_mesh: private,
        motion: mesh_break::State {
            header: header,
            faces: faces,
            count: state[0u16],
        },
        motion_capacity: input[13u16],
        motion_page: 65535,
        mesh: view.mesh,
        config: mesh_break::Config {
            gravity: attitude::get(store_at(meta, 10), 0),
            gains: store_at(meta, 12),
        },
        version: view.revision,
        resource_present: private.hi_bits() == 0 && private.lo_bits() < resources.count,
        geometry_present: view.valid,
    }
}
pub fn load_auxiliary_hit(
    input: Array<u16>,
    storage: Array<u16>,
    scratch: Array<u16>,
) -> AuxiliaryHitCase {
    let resources = auxiliary_resources(input, storage);
    let origin = store_at(storage, 76);
    let mut ranges = store_at(origin, 4);
    let mut items = store_at(ranges, 192);
    let source = store_at(input, input[8u16]);
    let mut position = 0u16;
    let mut i = 0u16;
    while i < 64 {
        ranges[i * 3] = position;
        ranges[i * 3 + 1] = source[i];
        ranges[i * 3 + 2] = source[i];
        position += source[i] * 2;
        i += 1;
    }
    if 272 + position > 11988 {
        halt(0x7f48);
    }
    i = 0;
    while i < position {
        items[i] = source[64 + i];
        i += 1;
    }
    let grid = spatial::Grid {
        page: 65535,
        origin: origin,
        shift_x: 0,
        shift_z: 0,
        items: items,
        ranges: ranges,
    };
    let row = store_at(input, input[5u16]);
    let meta = store_at(input, input[7u16]);
    let object = auxiliary_object(input, scratch, &resources);
    let empty = ordered::List {
        items: storage,
        count: 0,
        capacity: 0,
    };
    let runtime = scene_world::Runtime {
        ticks: lifecycle::Ticks {
            flag_words: 2,
            flags: storage,
            flag_count: 0,
            order: ordered::List {
                items: storage,
                count: 0,
                capacity: 0,
            },
            next: integer::from_i16(-1),
        },
        queues: lifecycle::Queues {
            notifications: ordered::List {
                items: storage,
                count: 0,
                capacity: 0,
            },
            destruction: ordered::List {
                items: storage,
                count: 0,
                capacity: 0,
            },
        },
        seed: store_at(meta, 5),
        slot: store_at(meta, 7),
        ordinary: empty,
        special: ordered::List {
            items: storage,
            count: 0,
            capacity: 0,
        },
    };
    AuxiliaryHitCase {
        resources: resources,
        object: object,
        runtime: runtime,
        grid: grid,
        row: row,
        meta: meta,
        model: auxiliary_model(input, 0),
        controls: store_at(input, input[9u16]),
        count: input[1u16],
    }
}
pub fn auxiliary_map(cells: static_assets::AssetBytes, directory: Array<u16>) -> ground::World {
    let first = static_assets::AssetBytes {
        first_sector: cells.first_sector,
        sector_count: 256,
        byte_len_lo: 32768,
        byte_len_hi: 0,
    };
    ground::World {
        terrain: terrain::Terrain::begin(first),
        cells: cells,
        directory: directory,
        seed: fix32::zero(),
        noise: Buf::new([0; 16]),
        shield_page: 65535,
        shield: unsafe { Ptr::from_addr(0).as_u16_array() },
    }
}
pub fn run_auxiliary_hit(
    case: &mut AuxiliaryHitCase,
    frame: u16,
    out: Array<u16>,
    map: &mut ground::World,
) -> u16 {
    let control = store_at(case.controls, frame * 18);
    if attitude::get(control, 8) != fix32::zero() && case.row[7u16] & 1 != 0 {
        let mut active = case.object.active;
        active[0u16] = 0;
        let mut row = case.row;
        row[7u16] &= 65407;
    }
    let result = store_at(case.meta, 22);
    attitude::put(result, 0, fix32::from_words(0x7182, 0));
    let tail = attitude::get(control, 7);
    let hit = scene_fragments::Hit {
        row: case.row,
        limit: attitude::get(control, 0),
        powerup: case.meta[9u16] != 0,
        name: 0,
        model_status: attitude::get(control, 5),
        model: case.model,
        has_terrain: attitude::get(control, 6) != fix32::zero(),
        vehicle: attitude::get(control, 1),
        velocity: store_at(control, 4),
        paired_tail: tail.lo_bits(),
        has_paired_tail: tail >= fix32::zero(),
        result: result,
    };
    let status = scene_fragments::activate(
        &mut case.resources,
        &mut case.object,
        &mut case.runtime,
        &mut case.grid,
        map,
        &hit,
    );
    attitude::put(out, 0, status);
    attitude::put(out, 1, attitude::get(result, 0));
    attitude::put(out, 2, attitude::get(case.runtime.seed, 0));
    attitude::put(out, 3, attitude::get(case.runtime.slot, 0));
    let mut private = integer::from_i16(-1);
    if case.row[7u16] & 16 != 0 {
        private = case.object.private_mesh;
    }
    attitude::put(out, 4, private);
    attitude::put(out, 5, attitude::get(case.object.current_mesh, 0));
    let mut output = out;
    output[12u16] = case.object.active[0u16];
    let mut position = store_copy(out, case.row, 13, 8);
    position = store_copy(out, case.object.collider, position, 60);
    let saved = read_dseg_page(3);
    if case.object.motion_page != 65535 {
        unsafe {
            write_dseg_page(3, case.object.motion_page);
        }
    }
    output[position] = 0;
    position += 1;
    if case.row[7u16] & 32 != 0 {
        output[position - 1] = case.object.motion.count;
        position = store_copy(out, case.object.motion.header, position, 8);
        position = store_copy(
            out,
            case.object.motion.faces,
            position,
            case.object.motion.count * 16,
        );
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    position += capture_mesh_store(store_at(out, position), status, &case.resources);
    let mut i = 0u16;
    while i < 64 {
        output[position + i] = case.grid.ranges[i * 3 + 1];
        i += 1;
    }
    position += 64;
    i = 0;
    while i < 64 {
        let list = spatial::list(&case.grid, i);
        position = store_copy(out, list.items, position, list.count * 2);
        i += 1;
    }
    position
}
