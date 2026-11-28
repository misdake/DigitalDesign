//! Live scenery image views and complete-world comparisons for driving frames.
#![link_cseg = "frame"]
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
fn valid_live(image: Array<u16>, compact: bool) -> bool {
    let mut total = DRIVING_SCENERY_WORDS;
    let mut ids = (DRIVING_IDENTITY_WORDS - 12) / 6;
    if compact {
        total = DRIVING_LIVE_WORDS;
        ids = DRIVING_ENTITY_CAPACITY;
    }
    if image[0u16] != 0x534d
        || image[1u16] != 1
        || image[2u16] != 240
        || image[3u16] > image[13u16]
        || image[11u16] != image[3u16]
        || image[13u16] != image[15u16]
        || image[13u16] != ids
        || image[38u16] != total
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
            if count > 16383 {
                return false;
            }
            words = count * 2;
            if compact {
                words = DRIVING_ENTITY_CAPACITY;
            }
        }
        if compact && (section == 0 || section == 4 || section == 5) {
            words = 0;
        }
        if section >= 6 {
            capacity = image[13u16];
            if section >= 9 {
                capacity = image[2u16];
                if compact {
                    capacity = DRIVING_OBJECT_CAPACITY;
                }
            }
            words = capacity * 2;
        }
        if count > capacity || cursor > total || words > total - cursor {
            return false;
        }
        cursor += words;
        section += 1;
    }
    if cursor != total || image[5u16] != image[2u16] * 8 {
        return false;
    }
    true
}
// Source sections retain their reference layout. Resident component rows have
// one owner, IDs and flags use checked narrow fields, and lists reserve growth.
// State/configuration offsets stay local to their own sections.
pub fn load_live(image: Array<u16>, records: Array<u16>, asset: AssetBytes) -> bool {
    let header: Buf<u16, 48> = Buf::new([0; 48]);
    let h = header.as_array();
    read(h, asset, fix32::zero(), 48);
    if !valid_live(h, false) {
        return false;
    }
    copy(image, h, 48);
    let mut rows = records;
    let mut i = 0u16;
    while i < DRIVING_OBJECT_CAPACITY * 8 {
        rows[i] = 0;
        i += 1;
    }
    read(records, asset, fix32::from_words(h[4u16], 0), h[5u16]);
    let mut destination = image;
    destination[13u16] = DRIVING_ENTITY_CAPACITY;
    destination[15u16] = DRIVING_ENTITY_CAPACITY;
    let flags: Buf<u16, 64> = Buf::new([0; 64]);
    let scratch = flags.as_array();
    let mut cursor = 48u16;
    let mut section = 0u16;
    while section < 11 {
        destination[4 + section * 2] = cursor;
        if section == 3 {
            let mut target = at(image, cursor as i16);
            i = 0;
            while i < DRIVING_ENTITY_CAPACITY {
                target[i] = 0;
                i += 1;
            }
            i = 0;
            while i < h[11u16] {
                let mut count = h[11u16] - i;
                if count > 32 {
                    count = 32;
                }
                read(
                    scratch,
                    asset,
                    fix32::from_words(h[10u16] + i * 2, 0),
                    count * 2,
                );
                let mut j = 0u16;
                while j < count {
                    if scratch[j * 2 + 1] != 0 {
                        return false;
                    }
                    target[i + j] = scratch[j * 2];
                    j += 1;
                }
                i += count;
            }
            cursor += DRIVING_ENTITY_CAPACITY;
        } else if section != 0 && section != 4 && section != 5 {
            let source = h[4 + section * 2];
            let mut end = h[38u16];
            if section < 10 {
                end = h[6 + section * 2];
            }
            let source_words = end - source;
            let mut words = source_words;
            if section >= 6 {
                words = DRIVING_ENTITY_CAPACITY * 2;
                if section >= 9 {
                    words = DRIVING_OBJECT_CAPACITY * 2;
                }
            }
            if cursor > DRIVING_LIVE_WORDS || words > DRIVING_LIVE_WORDS - cursor {
                return false;
            }
            read(
                at(image, cursor as i16),
                asset,
                fix32::from_words(source, 0),
                source_words,
            );
            let mut target = at(image, cursor as i16);
            i = source_words;
            while i < words {
                target[i] = 0;
                i += 1;
            }
            cursor += words;
        }
        section += 1;
    }
    destination[38u16] = cursor;
    valid_live(image, true)
}
fn live_list(image: Array<u16>, section: u16) -> ordered::List {
    let mut capacity = image[13u16];
    if section >= 9 {
        capacity = DRIVING_OBJECT_CAPACITY;
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
            flag_words: 1,
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
fn loaded_list(
    data: Array<u16>,
    offset: u16,
    capacity: u16,
    source: Array<u16>,
    available: u16,
) -> ordered::List {
    if available == 0 {
        halt(0x7f23);
    }
    let count = source[0u16];
    if count > capacity || count > (available - 1) / 2 {
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
    world: &scene_world::World,
    identity: Array<u16>,
    lists: Array<u16>,
    records: Array<u16>,
) -> scene_lifecycle::Store {
    let temporary: Buf<u16, 256> = Buf::new([0; 256]);
    if DRIVING_LIFETIME_WORDS > 256 {
        halt(0x7f36);
    }
    let scratch = temporary.as_array();
    read(identity, DRIVING_IDENTITY, fix32::zero(), 12);
    if identity[0u16] != (DRIVING_IDENTITY_WORDS - 12) / 6 || identity[1u16] != 0 {
        halt(0x7f5b);
    }
    let mut ids = identity;
    let mut i = 12u16;
    while i < 12 + DRIVING_ENTITY_CAPACITY * 4 {
        ids[i] = 0;
        i += 1;
    }
    i = 0;
    while i < identity[0u16] {
        let mut count = identity[0u16] - i;
        if count > 32 {
            count = 32;
        }
        read(
            scratch,
            DRIVING_IDENTITY,
            fix32::from_words(12 + i * 6, 0),
            count * 6,
        );
        let mut j = 0u16;
        while j < count {
            let source = at(scratch, (j * 6) as i16);
            if source[3u16] != 0 || source[5u16] != 0 || source[4u16] >= DRIVING_ENTITY_CAPACITY {
                halt(0x7f5b);
            }
            let kind = (source[0u16] >> 2) & 7;
            if (kind == 1 && source[2u16] >= DRIVING_OBJECT_CAPACITY)
                || (kind == 2 && source[2u16] >= 8)
            {
                halt(0x7f5b);
            }
            let mut target = at(identity, (12 + (i + j) * 4) as i16);
            target[0u16] = source[0u16];
            target[1u16] = source[1u16];
            target[2u16] = source[2u16];
            target[3u16] = source[4u16];
            j += 1;
        }
        i += count;
    }
    read(
        scratch,
        DRIVING_LIFETIME,
        fix32::zero(),
        DRIVING_LIFETIME_WORDS,
    );
    let free = loaded_list(
        lists,
        DRIVING_LIFE_FREE_OFFSET,
        DRIVING_LIFE_FREE_CAPACITY,
        at(scratch, 2),
        DRIVING_LIFETIME_WORDS - 2,
    );
    let mut cursor = 3 + free.count * 2;
    let lap = loaded_list(
        lists,
        DRIVING_LIFE_LAP_OFFSET,
        DRIVING_LIFE_LAP_CAPACITY,
        at(scratch, cursor as i16),
        DRIVING_LIFETIME_WORDS - cursor,
    );
    cursor += 1 + lap.count * 2;
    let signs = loaded_list(
        lists,
        DRIVING_LIFE_SIGNS_OFFSET,
        DRIVING_LIFE_SIGNS_CAPACITY,
        at(scratch, cursor as i16),
        DRIVING_LIFETIME_WORDS - cursor,
    );
    cursor += 1 + signs.count * 2;
    let saved = spatial::select(&world.grid);
    let queue = world.properties;
    let mut guard = queue;
    guard[DRIVING_LIFE_ENTER_OFFSET - 1] = 0x1d47;
    guard[DRIVING_LIFE_QUEUE_END] = 0x74d1;
    let enter = loaded_list(
        queue,
        DRIVING_LIFE_ENTER_OFFSET,
        DRIVING_LIFE_ENTER_CAPACITY,
        at(scratch, cursor as i16),
        DRIVING_LIFETIME_WORDS - cursor,
    );
    cursor += 1 + enter.count * 2;
    let leave = loaded_list(
        queue,
        DRIVING_LIFE_LEAVE_OFFSET,
        DRIVING_LIFE_LEAVE_CAPACITY,
        at(scratch, cursor as i16),
        DRIVING_LIFETIME_WORDS - cursor,
    );
    cursor += 1 + leave.count * 2;
    unsafe {
        write_dseg_page(3, saved);
    }
    if cursor != DRIVING_LIFETIME_WORDS {
        halt(0x7f23);
    }
    scene_lifecycle::Store {
        identities: entities::State {
            row_words: 4,
            header: identity,
            rows: at(identity, 12),
            storage_capacity: DRIVING_ENTITY_CAPACITY,
        },
        records: records,
        object_count: world.count,
        object_capacity: DRIVING_OBJECT_CAPACITY,
        colliders: world.colliders,
        collider_stride: 60,
        fixed_count: world.fixed_count,
        dynamic_page: world.dynamic_page,
        generic_class: scratch[0u16],
        obstacle_class: scratch[1u16],
        limit: fix32::from_words(34464, 1),
        free_objects: free,
        lap_objects: lap,
        signs: signs,
        queue_page: world.grid.page,
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
    primary: u16,
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
            if kind == 2 && index.lo_bits() < 3 {
                return actors[index.lo_bits()].attitude;
            }
            if kind == 1 && index.lo_bits() < world.count {
                return at(world.colliders, (index.lo_bits() * 60 + 8) as i16);
            }
        }
        halt(0x7f19);
    }
    actors[primary].attitude
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
// Independent create/release input at the frame boundary. The following car
// update and full-world capture observe the same persistent owners.
pub fn apply_world_event(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    data: Array<u16>,
    asset: AssetBytes,
) {
    let event: Buf<u16, 4> = Buf::new([0; 4]);
    let e = event.as_array();
    let number = attitude::get(data, 26);
    if number.hi_bits() != 0 || number.lo_bits() == 0 {
        halt(0x7f5f);
    }
    read(
        e,
        asset,
        fix32::from_words((number.lo_bits() - 1) * 4, 0),
        4,
    );
    let mut i = 0u16;
    while i < e[1u16] {
        if e[0u16] == 1 {
            let created = entities::create_record(&state.lifetime.identities, state.lifetime.limit);
            if created.status != 0
                || entities::bind_class(&state.lifetime.identities, created.id, e[3u16]) != 0
            {
                halt(0x7f5c);
            }
            let allocated = scene_lifecycle::allocate_transient(
                &mut state.lifetime,
                world,
                &mut state.runtime,
                created.id,
            );
            if allocated.status != 0 {
                halt(0x7f5d);
            }
        } else if e[0u16] == 2 {
            let entity = fix32::from_words(e[2u16] + i, 0);
            let class = entities::class(&state.lifetime.identities, entity);
            let saved = attitude::get(state.runtime.slot, 0);
            attitude::put(state.runtime.slot, 0, integer::from_i16(3));
            if class < fix32::zero()
                || scene_lifecycle::parent_base(
                    scene,
                    &mut state.lifetime,
                    &mut state.runtime,
                    entity,
                    class.lo_bits(),
                ) != fix32::zero()
            {
                halt(0x7f5e);
            }
            attitude::put(state.runtime.slot, 0, saved);
        } else if e[0u16] == 3 {
            if weapon_mines::place(scene, state, world, player, data, e) < fix32::zero() {
                halt(0x7f6f);
            }
        } else if e[0u16] == 4 {
            if scene_creation::dispatch(
                scene,
                &mut state.lifetime,
                &mut state.runtime,
                world,
                &mut player.weapons,
                &scene_creation::Call {
                    entity: fix32::from_words(e[2u16] + i, 0),
                    slot: 8,
                },
            ) != fix32::zero()
            {
                halt(0x7f70);
            }
        } else if e[0u16] == 5 || e[0u16] == 6 {
            if weapon_ufos::place(scene, state, world, player, e) < fix32::zero() {
                halt(0x7f75);
            }
        } else {
            halt(0x7f5f);
        }
        i += 1;
    }
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
    let capture: Buf<u16, 8> = Buf::new([0; 8]);
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
    let ids = attitude::get(store.identities.header, 0).lo_bits();
    if 12 + ids * 6 != h[6u16] {
        halt(0x7f25);
    }
    let identity_base = base + fix32::from_words(h[5u16], 0);
    compare_block(store.identities.header, asset, identity_base, 12, scratch);
    let mut identity = 0u16;
    while identity < ids {
        identity_capture::write_row(&store.identities, identity, actual);
        compare_block(
            actual,
            asset,
            identity_base + fix32::from_words(12 + identity * 6, 0),
            6,
            scratch,
        );
        identity += 1;
    }
    identity = 0;
    while identity < runtime.ticks.flag_count {
        let mut count = runtime.ticks.flag_count - identity;
        if count > 32 {
            count = 32;
        }
        read(
            scratch,
            asset,
            base + fix32::from_words(h[7u16] + identity * 2, 0),
            count * 2,
        );
        let mut j = 0u16;
        while j < count {
            check_value(lifecycle::flags(&runtime.ticks, identity + j), scratch, j);
            j += 1;
        }
        identity += count;
    }
    // Only a live Rust Option has component state/configuration to compare.
    // Resource offsets describe each encoding and are not game state.
    let mut i = 0u16;
    while i < world.count {
        mark(11);
        let r = at(store.records, (i * 8) as i16);
        read(
            actual,
            asset,
            base + fix32::from_words(h[4u16] + i * 8, 0),
            8,
        );
        let e = actual;
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
        let saved = read_dseg_page(3);
        if i >= 8 && store.queue_page != 65535 && store.queue_page != saved {
            unsafe {
                write_dseg_page(3, store.queue_page);
            }
        }
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
        if read_dseg_page(3) != saved {
            unsafe {
                write_dseg_page(3, saved);
            }
        }
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
        let words = unsafe { view_of(&mut reference).as_ptr().as_u16_array() };
        if next == i {
            read(words, asset, base + fix32::from_words(cursor + 1, 0), 76);
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
        }
        if !scene_world::load_collider(world, i, scratch) {
            halt(0x7f2d);
        }
        check(scratch, words, 60, 0);
        if !scene_world::load_properties(world, i, scratch) {
            halt(0x7f2d);
        }
        check(scratch, at(words, 60), 16, 60);
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
    let expected: Buf<u16, 40> = Buf::new([0; 40]);
    read(
        expected.as_array(),
        asset,
        fix32::from_words(frame * DRIVING_DEFORM_COUNT * 4, 0),
        DRIVING_DEFORM_COUNT * 4,
    );
    let mut i = 0u16;
    while i < DRIVING_DEFORM_COUNT {
        let c = &components[i];
        let view = scene_meshes::view(&c.resources[0u16], c.resource, false);
        if !view.valid
            || view.page != c.mesh.vertex_page
            || view.page != c.sampling.vertex_page
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
