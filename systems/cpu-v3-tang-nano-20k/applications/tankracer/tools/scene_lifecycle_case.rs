fn life_at(data: Array<u16>, words: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(words as i16).as_u16_array() }
}
fn life_list(data: Array<u16>, index: u16) -> ordered::List {
    let offsets: Buf<u16, 10> = Buf::new([0, 64, 96, 128, 640, 1152, 1184, 1696, 2208, 2720]);
    let caps: Buf<u16, 10> = Buf::new([32, 16, 16, 256, 256, 16, 256, 256, 256, 256]);
    ordered::List {
        items: life_at(data, offsets[index]),
        count: 0,
        capacity: caps[index],
    }
}
fn life_write_list(out: Array<u16>, position: u16, list: &ordered::List) -> u16 {
    attitude::put(out, position, fix32::from_words(list.count, 0));
    let mut i = 0u16;
    while i < list.count {
        attitude::put(out, position + 1 + i, attitude::get(list.items, i));
        i += 1;
    }
    position + 1 + list.count
}
pub struct SceneCase {
    pub store: scene_lifecycle::Store,
    pub runtime: scene_world::Runtime,
    pub grid: spatial::Grid,
    pub object: u16,
    pub tail: u16,
}
// The caller owns and zero-initializes 11,988 words for all borrowed views.
pub fn load_scene_lifecycle(input: Array<u16>, storage: Array<u16>) -> SceneCase {
    let headers = life_at(storage, 0);
    let identity_rows = life_at(storage, 12);
    let ids = entities::State {
        row_words: 6,
        header: headers,
        rows: identity_rows,
        storage_capacity: 256,
    };
    let mut i = 0u16;
    let mut h = ids.header;
    while i < 12 {
        h[i] = input[24 + i];
        i += 1;
    }
    let capacity = attitude::get(h, 0).lo_bits();
    if capacity != 256 {
        halt(0x7f22);
    }
    let mut ir = ids.rows;
    i = 0;
    while i < capacity * 6 {
        ir[i] = input[36 + i];
        i += 1;
    }
    let mut cursor = 18 + capacity * 3;
    let flag_storage = life_at(storage, 1548);
    let flags = flag_storage;
    let flag_count = attitude::get(input, 11).lo_bits();
    i = 0;
    while i < flag_count {
        attitude::put(flags, i, attitude::get(input, cursor + i));
        i += 1;
    }
    cursor += flag_count;
    let list_storage = life_at(storage, 2060);
    let lists: Buf<ordered::List, 10> = Buf::new([
        life_list(list_storage, 0),
        life_list(list_storage, 1),
        life_list(list_storage, 2),
        life_list(list_storage, 3),
        life_list(list_storage, 4),
        life_list(list_storage, 5),
        life_list(list_storage, 6),
        life_list(list_storage, 7),
        life_list(list_storage, 8),
        life_list(list_storage, 9),
    ]);
    let mut ls = lists.as_array();
    i = 0;
    while i < 10 {
        ls[i].count = attitude::get(input, cursor).lo_bits();
        cursor += 1;
        if ls[i].count > ls[i].capacity {
            halt(0x7f23);
        }
        let mut j = 0u16;
        while j < ls[i].count {
            attitude::put(ls[i].items, j, attitude::get(input, cursor + j));
            j += 1;
        }
        cursor += ls[i].count;
        i += 1;
    }
    let ranges = life_at(storage, 5292);
    let grid_storage = life_at(storage, 5484);
    let origin = life_at(storage, 9580);
    let mut rs = ranges;
    i = 0;
    while i < 64 {
        rs[i * 3] = i * 64;
        rs[i * 3 + 1] = attitude::get(input, cursor + i).lo_bits();
        rs[i * 3 + 2] = 32;
        if rs[i * 3 + 1] > 32 {
            halt(0x7f24);
        }
        i += 1;
    }
    cursor += 64;
    let grid = spatial::Grid {
        page: 65535,
        origin: origin,
        shift_x: 0,
        shift_z: 0,
        items: grid_storage,
        ranges: rs,
    };
    i = 0;
    while i < 64 {
        let list = spatial::list(&grid, i);
        let mut j = 0u16;
        while j < list.count {
            attitude::put(list.items, j, attitude::get(input, cursor + j));
            j += 1;
        }
        cursor += list.count;
        i += 1;
    }
    let metadata = life_at(storage, 9584);
    let object = attitude::get(input, 2).lo_bits();
    let object_count = attitude::get(input, 3).lo_bits();
    if object_count > 240 || object >= object_count {
        halt(0x7f25);
    }
    let mut row = life_at(metadata, object * 8);
    i = 0;
    while i < 8 {
        row[i] = input[cursor * 2 + i];
        i += 1;
    }
    let collider_ids = life_at(storage, 11504);
    attitude::put(collider_ids, object, attitude::get(input, 6));
    let store = scene_lifecycle::Store {
        identities: ids,
        records: metadata,
        object_count: object_count,
        object_capacity: object_count,
        colliders: collider_ids,
        collider_stride: 2,
        fixed_count: object_count,
        dynamic_page: 0,
        generic_class: attitude::get(input, 4).lo_bits(),
        obstacle_class: attitude::get(input, 5).lo_bits(),
        limit: attitude::get(input, 1),
        free_objects: ordered::List {
            items: ls[5u16].items,
            count: ls[5u16].count,
            capacity: ls[5u16].capacity,
        },
        lap_objects: ordered::List {
            items: ls[6u16].items,
            count: ls[6u16].count,
            capacity: ls[6u16].capacity,
        },
        signs: ordered::List {
            items: ls[7u16].items,
            count: ls[7u16].count,
            capacity: ls[7u16].capacity,
        },
        enter: ordered::List {
            items: ls[8u16].items,
            count: ls[8u16].count,
            capacity: ls[8u16].capacity,
        },
        leave: ordered::List {
            items: ls[9u16].items,
            count: ls[9u16].count,
            capacity: ls[9u16].capacity,
        },
    };
    let dispatch_words = life_at(storage, 11984);
    attitude::put(dispatch_words, 0, attitude::get(input, 8));
    attitude::put(dispatch_words, 1, attitude::get(input, 9));
    let runtime = scene_world::Runtime {
        ticks: lifecycle::Ticks {
            flag_words: 2,
            flags: flags,
            flag_count: flag_count,
            order: ordered::List {
                items: ls[0u16].items,
                count: ls[0u16].count,
                capacity: ls[0u16].capacity,
            },
            next: attitude::get(input, 10),
        },
        slot: dispatch_words,
        seed: life_at(dispatch_words, 2),
        queues: lifecycle::Queues {
            notifications: ordered::List {
                items: ls[1u16].items,
                count: ls[1u16].count,
                capacity: ls[1u16].capacity,
            },
            destruction: ordered::List {
                items: ls[2u16].items,
                count: ls[2u16].count,
                capacity: ls[2u16].capacity,
            },
        },
        ordinary: ordered::List {
            items: ls[3u16].items,
            count: ls[3u16].count,
            capacity: ls[3u16].capacity,
        },
        special: ordered::List {
            items: ls[4u16].items,
            count: ls[4u16].count,
            capacity: ls[4u16].capacity,
        },
    };
    SceneCase {
        store: store,
        runtime: runtime,
        grid: grid,
        object: object,
        tail: cursor + 4,
    }
}
pub fn capture_scene_lifecycle(out: Array<u16>, status: fix32, state: &SceneCase) -> u16 {
    capture_lifetime_parts(
        out,
        status,
        &state.store,
        &state.runtime,
        &state.grid,
        state.object,
    )
}
pub fn capture_lifetime_parts(
    out: Array<u16>,
    status: fix32,
    store: &scene_lifecycle::Store,
    runtime: &scene_world::Runtime,
    grid: &spatial::Grid,
    object: u16,
) -> u16 {
    let mut i = 0u16;
    let row = life_at(store.records, object * 8);
    attitude::put(out, 0, status);
    attitude::put(out, 1, attitude::get(runtime.slot, 0));
    attitude::put(out, 2, attitude::get(runtime.seed, 0));
    attitude::put(out, 3, runtime.ticks.next);
    attitude::put(out, 4, fix32::from_words(runtime.ticks.flag_count, 0));
    let mut position = 5 + identity_capture::write(&store.identities, life_at(out, 10)) / 2;
    i = 0;
    while i < runtime.ticks.flag_count {
        attitude::put(out, position + i, attitude::get(runtime.ticks.flags, i));
        i += 1;
    }
    position += runtime.ticks.flag_count;
    position = life_write_list(out, position, &runtime.ticks.order);
    position = life_write_list(out, position, &runtime.queues.notifications);
    position = life_write_list(out, position, &runtime.queues.destruction);
    position = life_write_list(out, position, &runtime.ordinary);
    position = life_write_list(out, position, &runtime.special);
    position = life_write_list(out, position, &store.free_objects);
    position = life_write_list(out, position, &store.lap_objects);
    position = life_write_list(out, position, &store.signs);
    position = life_write_list(out, position, &store.enter);
    position = life_write_list(out, position, &store.leave);
    i = 0;
    while i < 64 {
        let list = spatial::list(grid, i);
        attitude::put(out, position + i, fix32::from_words(list.count, 0));
        i += 1;
    }
    position += 64;
    i = 0;
    while i < 64 {
        let list = spatial::list(grid, i);
        let mut j = 0u16;
        while j < list.count {
            attitude::put(out, position + j, attitude::get(list.items, j));
            j += 1;
        }
        position += list.count;
        i += 1;
    }
    let canonical: Buf<u16, 8> = Buf::new([0; 8]);
    let mut r = canonical.as_array();
    r[0u16] = row[0u16];
    r[7u16] = row[7u16];
    if row[0u16] == 2 {
        r[3u16] = row[3u16] & 1;
    }
    if row[7u16] & 1 != 0 {
        r[3u16] |= row[3u16] & 2;
        r[4u16] = row[4u16];
        r[5u16] = row[5u16];
        r[6u16] = row[6u16];
    }
    let mut dst = life_at(out, position * 2);
    i = 0;
    while i < 8 {
        dst[i] = r[i];
        i += 1;
    }
    position * 2 + 8
}
pub fn run_scene_lifecycle(input: Array<u16>, out: Array<u16>, asset: static_assets::AssetBytes) {
    let scene = scene_query::Scene::begin(asset);
    if !scene.valid {
        halt(0x7f21);
    }
    let storage: Buf<u16, 11988> = Buf::new([0; 11988]);
    let mut state = load_scene_lifecycle(input, storage.as_array());
    let action = attitude::get(input, 0).lo_bits();
    let entity = attitude::get(input, 6);
    let mut status = fix32::zero();
    if action == 0 {
        status = scene_lifecycle::drain(
            &scene,
            &mut state.store,
            &mut state.runtime,
            &mut state.grid,
        );
    } else if action == 1 {
        status = scene_lifecycle::destroy(
            &scene,
            &mut state.store,
            &mut state.runtime,
            &mut state.grid,
            entity,
        );
    } else if action == 2 {
        status = scene_lifecycle::stop(
            &scene,
            &mut state.store,
            &mut state.runtime,
            &mut state.grid,
            entity,
        );
    } else {
        let mut slot = 8u16;
        if action == 4 {
            slot = 27;
        }
        status = scene_lifecycle::dispatch(
            &scene,
            &mut state.store,
            &mut state.runtime,
            &mut state.grid,
            entity,
            slot,
        );
    }
    capture_scene_lifecycle(out, status, &state);
}
