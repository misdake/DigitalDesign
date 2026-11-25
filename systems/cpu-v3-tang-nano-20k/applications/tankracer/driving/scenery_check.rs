//! Live scenery image views and complete-world comparisons for driving frames.
#![link_cseg = "checks"]
use super::*;
#[inline(never)]
pub fn check_map_residency(map: &mut ground::World) {
    let cached = read_dseg_page(3);
    let stage = (DRIVING_WORLD.first_sector() >> 8) + 1;
    let outside = fix32::from_words(0, 32768);
    let expected = map.terrain.height(outside, outside);
    let seed = map.seed;
    unsafe {
        write_dseg_page(3, stage);
    }
    map.bind(outside, outside);
    if read_dseg_page(3) != cached {
        halt(0x7f40);
    }
    unsafe {
        write_dseg_page(3, stage);
    }
    let actual = map.height(outside, outside);
    if actual.status != expected.status
        || actual.height != expected.height
        || map.seed != seed
        || read_dseg_page(3) != stage
    {
        halt(0x7f41);
    }
}
// Scratch in the unused upper half of the world's metadata page. Every public
// mutation must return to the deliberately different incoming DSEG3 page.
#[inline(never)]
pub fn check_grid_residency(page: u16) {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, page);
    }
    let scratch = unsafe { Ptr::from_addr(0xc000 + 8200).as_u16_array() };
    let mut words = scratch;
    let mut i = 0u16;
    while i < 452 {
        words[i] = 0;
        i += 1;
    }
    let mut grid = spatial::Grid {
        page: page,
        origin: scratch,
        shift_x: 10,
        shift_z: 10,
        ranges: at(scratch, 4),
        items: at(scratch, 196),
    };
    let mut ranges = grid.ranges;
    i = 0;
    while i < 64 {
        ranges[i * 3] = i * 4;
        ranges[i * 3 + 2] = 2;
        i += 1;
    }
    let position: Buf<u16, 6> = Buf::new([0; 6]);
    unsafe {
        write_dseg_page(3, 0);
    }
    let id = fix32::zero();
    let limit = fix32::from_words(1000, 0);
    let first = spatial::insert(&mut grid, id, position.as_array(), true, 0);
    if first.status != 0 || read_dseg_page(3) != 0 {
        halt(0x7f3b);
    }
    let second = spatial::insert(&mut grid, id, position.as_array(), true, 0);
    if second.status != 0 || read_dseg_page(3) != 0 {
        halt(0x7f3b);
    }
    let full = spatial::insert(&mut grid, id, position.as_array(), true, 0);
    if full.status != 4 || read_dseg_page(3) != 0 {
        halt(0x7f3c);
    }
    if spatial::remove(&mut grid, id, &first.cell, fix32::zero()) != 2 || read_dseg_page(3) != 0 {
        halt(0x7f3d);
    }
    if spatial::remove(
        &mut grid,
        fix32::from_words(1, 0),
        &first.cell,
        fix32::from_words(1, 0),
    ) != 3
        || read_dseg_page(3) != 0
    {
        halt(0x7f3d);
    }
    if spatial::remove(&mut grid, id, &first.cell, limit) != 0 || read_dseg_page(3) != 0 {
        halt(0x7f3e);
    }
    if spatial::remove(&mut grid, id, &first.cell, limit) != 0 || read_dseg_page(3) != 0 {
        halt(0x7f3e);
    }
    spatial::select(&grid);
    i = 0;
    while i < 64 {
        if grid.ranges[i * 3 + 1] != 0 {
            halt(0x7f3f);
        }
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
pub fn live_section(image: Array<u16>, section: u16) -> Array<u16> {
    at(image, image[4 + section * 2] as i16)
}
pub fn valid_live(image: Array<u16>) -> bool {
    if image[0u16] != 0x534d
        || image[1u16] != 1
        || image[2u16] != 240
        || image[3u16] > image[13u16]
        || image[13u16] != image[15u16]
        || image[38u16] != DRIVING_SCENERY_WORDS
    {
        return false;
    }
    let mut cursor = 48u16;
    let mut section = 0u16;
    while section < 11 {
        if image[4 + section * 2] != cursor {
            return false;
        }
        let count = image[5 + section * 2];
        let mut capacity = count;
        let mut words = count;
        if section == 3 {
            words = count * 2;
        }
        if section >= 6 {
            capacity = image[13u16];
            if section >= 9 {
                capacity = image[2u16];
            }
            words = capacity * 2;
        }
        if count > capacity
            || cursor > DRIVING_SCENERY_WORDS
            || words > DRIVING_SCENERY_WORDS - cursor
        {
            return false;
        }
        cursor += words;
        section += 1;
    }
    if cursor != DRIVING_SCENERY_WORDS || image[5u16] != image[2u16] * 8 {
        return false;
    }
    true
}
fn live_list(image: Array<u16>, section: u16) -> ordered::List {
    let mut capacity = image[13u16];
    if section >= 9 {
        capacity = image[2u16];
    }
    ordered::List {
        items: live_section(image, section),
        count: image[5 + section * 2],
        capacity: capacity,
    }
}
pub fn live_runtime(image: Array<u16>, slot: Array<u16>, seed: Array<u16>) -> scene_world::Runtime {
    scene_world::Runtime {
        ticks: lifecycle::Ticks {
            flags: live_section(image, 3),
            flag_count: image[3u16],
            order: live_list(image, 6),
            next: attitude::get(at(image, 28), 0),
        },
        slot: slot,
        seed: seed,
        queues: lifecycle::Queues {
            notifications: live_list(image, 7),
            destruction: live_list(image, 8),
        },
        ordinary: live_list(image, 9),
        special: live_list(image, 10),
    }
}
fn loaded_list(data: Array<u16>, offset: u16, capacity: u16, source: Array<u16>) -> ordered::List {
    let count = source[0u16];
    if count > capacity {
        halt(0x7f22);
    }
    copy(at(data, offset as i16), at(source, 1), count * 2);
    ordered::List {
        items: at(data, offset as i16),
        count: count,
        capacity: capacity,
    }
}
pub fn load_life(
    image: Array<u16>,
    world: &scene_world::World,
    identity: Array<u16>,
    lists: Array<u16>,
) -> scene_lifecycle::Store {
    let temporary: Buf<u16, 256> = Buf::new([0; 256]);
    if DRIVING_LIFETIME_WORDS > 256 {
        halt(0x7f36);
    }
    let scratch = temporary.as_array();
    read(
        identity,
        DRIVING_IDENTITY,
        fix32::zero(),
        DRIVING_IDENTITY_WORDS,
    );
    read(
        scratch,
        DRIVING_LIFETIME,
        fix32::zero(),
        DRIVING_LIFETIME_WORDS,
    );
    let free = loaded_list(lists, 0, 16, at(scratch, 2));
    let mut cursor = 3 + free.count * 2;
    let lap = loaded_list(lists, 32, 256, at(scratch, cursor as i16));
    cursor += 1 + lap.count * 2;
    let signs = loaded_list(lists, 544, 256, at(scratch, cursor as i16));
    cursor += 1 + signs.count * 2;
    let enter = loaded_list(lists, 1056, 256, at(scratch, cursor as i16));
    cursor += 1 + enter.count * 2;
    let leave = loaded_list(lists, 1568, 256, at(scratch, cursor as i16));
    cursor += 1 + leave.count * 2;
    if cursor != DRIVING_LIFETIME_WORDS {
        halt(0x7f23);
    }
    scene_lifecycle::Store {
        identities: entities::State {
            header: identity,
            rows: at(identity, 12),
            storage_capacity: (DRIVING_IDENTITY_WORDS - 12) / 6,
        },
        records: live_section(image, 0),
        object_count: world.count,
        colliders: world.colliders,
        collider_stride: 60,
        generic_class: scratch[0u16],
        obstacle_class: scratch[1u16],
        limit: fix32::from_words(34464, 1),
        free_objects: free,
        lap_objects: lap,
        signs: signs,
        enter: enter,
        leave: leave,
    }
}
pub fn live_peer(
    components: &scenery_contact::Components,
    object: u16,
    actors: Array<driving::Car>,
    ids: &entities::State,
    world: &scene_world::World,
) -> Array<u16> {
    let row = at(components.records, (object * 8) as i16);
    let properties = at(world.properties, (object * 16) as i16);
    if (row[0u16] == 1 || row[0u16] == 3)
        && attitude::get(properties, 3) != fix32::zero()
        && attitude::get(properties, 4).lo_bits() & 2 != 0
    {
        let peer = attitude::get(at(components.state, row[1u16] as i16), 4);
        let kind = entities::handle_kind(ids, peer);
        let index = entities::handle(ids, peer);
        if index.hi_bits() == 0 {
            if kind == 2 && index.lo_bits() < 2 {
                return actors[index.lo_bits()].attitude;
            }
            if kind == 1 && index.lo_bits() < world.count {
                return at(world.colliders, (index.lo_bits() * 60 + 8) as i16);
            }
        }
        halt(0x7f19);
    }
    actors[0u16].attitude
}
fn compare_block(
    actual: Array<u16>,
    asset: AssetBytes,
    offset: fix32,
    count: u16,
    scratch: Array<u16>,
) {
    let mut position = 0u16;
    while position < count {
        let mut chunk = count - position;
        if chunk > 64 {
            chunk = 64;
        }
        read(
            scratch,
            asset,
            offset + fix32::from_words(position, 0),
            chunk,
        );
        check(at(actual, position as i16), scratch, chunk, position);
        position += chunk;
    }
}
fn selected_list(
    runtime: &scene_world::Runtime,
    store: &scene_lifecycle::Store,
    index: u16,
) -> ordered::List {
    let mut items = runtime.ticks.order.items;
    let mut count = runtime.ticks.order.count;
    if index == 1 {
        items = runtime.queues.notifications.items;
        count = runtime.queues.notifications.count;
    }
    if index == 2 {
        items = runtime.queues.destruction.items;
        count = runtime.queues.destruction.count;
    }
    if index == 3 {
        items = runtime.ordinary.items;
        count = runtime.ordinary.count;
    }
    if index == 4 {
        items = runtime.special.items;
        count = runtime.special.count;
    }
    if index == 5 {
        items = store.free_objects.items;
        count = store.free_objects.count;
    }
    if index == 6 {
        items = store.lap_objects.items;
        count = store.lap_objects.count;
    }
    if index == 7 {
        items = store.signs.items;
        count = store.signs.count;
    }
    if index == 8 {
        items = store.enter.items;
        count = store.enter.count;
    }
    if index == 9 {
        items = store.leave.items;
        count = store.leave.count;
    }
    ordered::List {
        items: items,
        count: count,
        capacity: count,
    }
}
pub struct ExpectedScene {
    pub asset: AssetBytes,
    pub offset: fix32,
}
#[inline(never)]
pub fn check_live(
    image: Array<u16>,
    store: &scene_lifecycle::Store,
    runtime: &scene_world::Runtime,
    world: &scene_world::World,
    scene: &scene_query::Scene,
    expected: &ExpectedScene,
) -> u16 {
    spatial::select(&world.grid);
    let asset = expected.asset;
    let base = expected.offset;
    let comparison: Buf<u16, 64> = Buf::new([0; 64]);
    let capture: Buf<u16, 2576> = Buf::new([0; 2576]);
    let scratch = comparison.as_array();
    let actual = capture.as_array();
    let header: Buf<u16, 24> = Buf::new([0; 24]);
    let h = header.as_array();
    read(h, asset, base, 24);
    if h[0u16] != 0x5346
        || h[1u16] != 1
        || h[3u16] != world.count
        || h[8u16] != runtime.ticks.flag_count
        || h[19u16] != store.generic_class
        || h[20u16] != store.obstacle_class
    {
        halt(0x7f24);
    }
    check_value(attitude::get(runtime.slot, 0), at(h, 13), 0);
    check_value(attitude::get(runtime.seed, 0), at(h, 15), 0);
    check_value(runtime.ticks.next, at(h, 17), 0);
    mark(10);
    let id_words = identity_capture::write(&store.identities, actual);
    if id_words != h[6u16] {
        halt(0x7f25);
    }
    compare_block(
        actual,
        asset,
        base + fix32::from_words(h[5u16], 0),
        id_words,
        scratch,
    );
    compare_block(
        runtime.ticks.flags,
        asset,
        base + fix32::from_words(h[7u16], 0),
        h[8u16] * 2,
        scratch,
    );
    // Only a live Rust Option has component state/configuration to compare.
    // Resource offsets describe each encoding and are not game state.
    read(
        actual,
        asset,
        base + fix32::from_words(h[4u16], 0),
        world.count * 8,
    );
    let mut i = 0u16;
    while i < world.count {
        mark(11);
        let r = at(store.records, (i * 8) as i16);
        let e = at(actual, (i * 8) as i16);
        if r[0u16] != e[0u16] || r[7u16] != e[7u16] {
            halt(0x7f26);
        }
        let mut flags = 0u16;
        if r[0u16] == 2 || r[0u16] == 4 {
            flags = r[3u16] & 1;
        }
        if r[7u16] & 1 != 0 {
            flags |= r[3u16] & 2;
            check(at(r, 4), at(e, 4), 3, i * 8 + 4);
        } else if e[4u16] != 0 || e[5u16] != 0 || e[6u16] != 0 {
            halt(0x7f27);
        }
        if flags != e[3u16] {
            halt(0x7f28);
        }
        let mut state_words = 0u16;
        let mut config_words = 0u16;
        if r[0u16] == 1 || r[0u16] == 3 {
            state_words = 10;
            config_words = 8;
        }
        if r[0u16] == 2 {
            state_words = 22;
            config_words = 22;
        }
        if r[0u16] == 4 {
            state_words = 6;
        }
        if state_words != 0 {
            compare_block(
                at(live_section(image, 1), r[1u16] as i16),
                asset,
                base + fix32::from_words(e[1u16], 0),
                state_words,
                scratch,
            );
            compare_block(
                at(live_section(image, 2), r[2u16] as i16),
                asset,
                base + fix32::from_words(e[2u16], 0),
                config_words,
                scratch,
            );
        }
        i += 1;
    }
    mark(12);
    let mut cursor = h[9u16];
    i = 0;
    while i < 10 {
        let list = selected_list(runtime, store, i);
        read(scratch, asset, base + fix32::from_words(cursor, 0), 1);
        if list.count != scratch[0u16] {
            halt(0x7f29);
        }
        cursor += 1;
        compare_block(
            list.items,
            asset,
            base + fix32::from_words(cursor, 0),
            list.count * 2,
            scratch,
        );
        cursor += list.count * 2;
        i += 1;
    }
    if cursor != h[10u16] {
        halt(0x7f2a);
    }
    mark(13);
    let counts: Buf<u16, 64> = Buf::new([0; 64]);
    read(
        counts.as_array(),
        asset,
        base + fix32::from_words(cursor, 0),
        64,
    );
    cursor += 64;
    i = 0;
    while i < 64 {
        let list = spatial::list(&world.grid, i);
        if list.count != counts[i] {
            halt(0x7f2b);
        }
        compare_block(
            list.items,
            asset,
            base + fix32::from_words(cursor, 0),
            list.count * 2,
            scratch,
        );
        cursor += list.count * 2;
        i += 1;
    }
    if cursor != h[11u16] {
        halt(0x7f2c);
    }
    mark(14);
    let mut reference = scene_query::record_zero();
    let mut changed = 0u16;
    let mut next = 65535u16;
    if changed < h[12u16] {
        read(scratch, asset, base + fix32::from_words(cursor, 0), 1);
        next = scratch[0u16];
    }
    i = 0;
    while i < world.count {
        if next == i {
            read(scratch, asset, base + fix32::from_words(cursor + 1, 0), 60);
            check(at(world.colliders, (i * 60) as i16), scratch, 60, 0);
            read(scratch, asset, base + fix32::from_words(cursor + 61, 0), 16);
            check(at(world.properties, (i * 16) as i16), scratch, 16, 60);
            cursor += 77;
            changed += 1;
            next = 65535;
            if changed < h[12u16] {
                read(scratch, asset, base + fix32::from_words(cursor, 0), 1);
                next = scratch[0u16];
            }
        } else {
            if !scene.load(i, &mut reference) {
                halt(0x7f2d);
            }
            let words = unsafe { view_of(&mut reference).as_ptr().as_u16_array() };
            check(at(world.colliders, (i * 60) as i16), words, 60, 0);
            check(at(world.properties, (i * 16) as i16), at(words, 60), 16, 60);
        }
        i += 1;
    }
    if changed != h[12u16] || cursor != h[2u16] {
        halt(0x7f2e);
    }
    h[2u16]
}

#[inline(never)]
pub fn check_resource_versions(
    components: Array<deform_contact::Component>,
    asset: AssetBytes,
    frame: u16,
) {
    let expected: Buf<u16, 28> = Buf::new([0; 28]);
    read(
        expected.as_array(),
        asset,
        fix32::from_words(frame * 28, 0),
        28,
    );
    let mut i = 0u16;
    while i < 7 {
        let c = &components[i];
        let view = scene_meshes::view(&c.resources[0u16], c.resource, false);
        if !view.valid
            || view.page != 65535
            || c.mesh.vertices.as_ptr().addr() != view.mesh.vertices.as_ptr().addr()
            || c.sampling.vertices.as_ptr().addr() != view.mesh.vertices.as_ptr().addr()
        {
            halt(0x7f44);
        }
        check(
            view.revision,
            at(expected.as_array(), (i * 4) as i16),
            4,
            i * 4,
        );
        i += 1;
    }
}
