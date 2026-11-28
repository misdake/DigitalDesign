//! Finish route metadata shares the actor bank; geometry keeps its World owner.
#![link_cseg = "parents"]
use super::*;
pub const WORD: u16 = 5888;
pub const LIMIT: u16 = 7104;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
fn g(data: Array<u16>, column: u16) -> fix32 {
    attitude::get(data, column)
}
fn p(data: Array<u16>, column: u16, value: fix32) {
    attitude::put(data, column, value);
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}

pub fn entity(state: &scene_parent::State) -> fix32 {
    let mut id = 0u16;
    while id < g(state.lifetime.identities.header, 0).lo_bits() {
        let entity = fix32::from_words(id, 0);
        if entities::handle_kind(&state.lifetime.identities, entity) == 4 {
            return entity;
        }
        id += 1;
    }
    n(-1)
}

// One-time deep copy. The destination retains its own mutable views; immutable
// model bindings and the resource profile follow the source.
fn clone_car(target: &mut driving::Car, source: &mut driving::Car, entity: fix32) {
    copy(target.attitude, source.attitude, 266);
    copy(
        unsafe { view_of(&mut target.ground).as_ptr().as_u16_array() },
        unsafe { view_of(&mut source.ground).as_ptr().as_u16_array() },
        206,
    );
    copy(
        target.articulation.as_array(),
        source.articulation.as_array(),
        8,
    );
    copy(target.lean.as_array(), source.lean.as_array(), 8);
    copy(target.hull.as_array(), source.hull.as_array(), 37);
    copy(target.blast, source.blast, 14);
    let mut i = 0u16;
    let mut gun = target.gun;
    while i < DRIVING_GUN_WORDS {
        gun[i] = source.gun[i];
        i += 1;
    }
    target.entity = entity;
    target.profile = source.profile;
    target.slot = source.slot;
    target.player = true;
    target.visible = source.visible;
    target.ready = source.ready;
    target.turret_z = source.turret_z;
    target.gun_binding = source.gun_binding;
}

// Through the original immediate Tick dispatch boundary. An error preserves
// earlier shared writes and releases. The reserved car becomes live only after
// both light releases succeed, matching the source's local clone lifetime.
pub fn transfer(
    state: &mut scene_parent::State,
    player: &mut player_world::Player,
    complete: &mut car_runtime::State,
    pickups: &mut Pickups,
    own: u16,
    entity: fix32,
) -> fix32 {
    if state.car_count != 2 || entities::class(&state.lifetime.identities, entity) < n(0) {
        return n(-4);
    }
    if own >= state.car_count {
        return n(-1);
    }
    let old = state.cars[own].entity;
    let mut sources = state.cars;
    let mut targets = state.cars;
    clone_car(&mut targets[2u16], &mut sources[own], entity);
    copy(
        at(complete.auxiliary, 72),
        at(complete.auxiliary, (own * 36) as i16),
        36,
    );
    copy(
        at(player.prefixes, 100),
        at(player.prefixes, (own * 50) as i16),
        50,
    );
    copy(
        at(player.fields, 60),
        at(player.fields, (own * 30) as i16),
        30,
    );
    copy(
        at(player.inventory, 8),
        at(player.inventory, (own * 4) as i16),
        4,
    );
    copy(
        at(player.hit_fields.as_array(), 48),
        at(player.hit_fields.as_array(), (own * 24) as i16),
        24,
    );
    copy(
        at(player.hit_model.as_array(), 14),
        at(player.hit_model.as_array(), (own * 7) as i16),
        7,
    );
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, complete.page);
    }
    let metadata = ptr(0xc000 + car_runtime::METADATA_WORD);
    let input = ptr(0xc000 + car_runtime::INPUT_WORD);
    let effects = ptr(0xc000 + car_runtime::CAR_WORD);
    copy(at(metadata, 56), at(metadata, (own * 28) as i16), 28);
    copy(at(input, 52), at(input, (own * 26) as i16), 26);
    copy(at(effects, 1912), at(effects, (own * 956) as i16), 956);
    copy(
        ptr(0xc000 + player_world::TAIL_WORD + 14),
        ptr(0xc000 + player_world::TAIL_WORD + own * 7),
        7,
    );
    let body_words = DRIVING_BODY_VERTICES * 3 + DRIVING_BODY_FACES * 6 + 4;
    copy(
        at(player.body, (body_words * 2) as i16),
        at(player.body, (body_words * own) as i16),
        body_words,
    );
    copy(
        player_world::combat(2),
        player_world::combat(own),
        4 + player.hit_model[18u16] * 3 + player.hit_model[19u16] * 6,
    );
    p(at(input, 52), 3, g(at(input, (own * 26) as i16), 2));
    p(at(metadata, 56), 5, n(0));
    p(at(effects, 1912), 420, entity);
    let ai = ptr(0xc000 + car_runtime::FINISH_AI_WORD);
    let mut i = 0u16;
    let mut words = ai;
    while i < 60 {
        words[i] = 0;
        i += 1;
    }
    // The source's AI resource index is one-based; the bank stores cup at 9.
    let actor_page = read_dseg_page(2);
    unsafe {
        write_dseg_page(2, DRIVING_ACTORS.first_sector() >> 8);
    }
    let cup = ptr(0x8000 + WORD)[9u16];
    effect_pool::restore(actor_page);
    p(ai, 0, fix32::from_words(cup * 5 + 1, 0));
    p(ai, 2, n(32));
    p(ai, 7, g(at(player.fields, (own * 30) as i16), 0));
    p(ai, 8, g(at(player.fields, (own * 30) as i16), 1));
    p(ai, 9, g(ai, 7));
    p(ai, 10, g(ai, 8));
    i = 0;
    while i < 2 {
        let bank = ptr(0xc000 + i * 816);
        if g(bank, 0) != n(0) && g(bank, 4) != n(0) && g(bank, 5) == old {
            p(bank, 5, entity);
        }
        let debris = ptr(0xc000 + 8722 + i * 182);
        if g(debris, 89) != n(0) && g(debris, 90) == old {
            p(debris, 90, entity);
        }
        i += 1;
    }
    let lights = at(effects, 1912 + 830);
    i = 0;
    while i < 2 {
        if g(lights, 1 + i) != n(0) {
            let mut quads = complete.shared.quads;
            if quad_pool::release(&mut quads[0u16], &pickups.lines[0u16], g(lights, 3 + i)) != 0 {
                unsafe {
                    write_dseg_page(3, saved);
                }
                return n(-4);
            }
            p(lights, 1 + i, n(0));
        }
        i += 1;
    }
    state.car_count = 3;
    complete.shared.owners.count = 3;
    complete.shared.owner_slots = 3;
    let mut present = player.present;
    present[2u16] = 1;
    if entities::bind_handle(&state.lifetime.identities, entity, 2, n(2)) != 0 {
        unsafe {
            write_dseg_page(3, saved);
        }
        return n(-4);
    }
    let counts = ptr(0xc000 + weapon_ufos::COUNT_WORD);
    let mut visible = ordered::List {
        items: ptr(0xc000 + weapon_ufos::VISIBLE_WORD),
        count: counts[1u16],
        capacity: 9,
    };
    let status = ordered::remove_first(
        &mut visible,
        fix32::from_words(own, 0),
        state.lifetime.limit,
    );
    let mut counts = counts;
    counts[1u16] = visible.count;
    if status != 0
        || lifecycle::queue(
            &mut state.runtime.queues,
            &mut state.runtime.ticks,
            old,
            false,
            state.lifetime.limit,
        ) != 0
    {
        unsafe {
            write_dseg_page(3, saved);
        }
        return n(-3);
    }
    if ordered::append(&mut visible, n(2)) != 0 {
        unsafe {
            write_dseg_page(3, saved);
        }
        return n(-3);
    }
    counts[1u16] = visible.count;
    let mut slots = state.slots;
    slots[0u16] = 2;
    let mut flags = state.runtime.ticks.flags;
    flags[entity.lo_bits() * state.runtime.ticks.flag_words] &= 65503;
    player.finish_entity = entity;
    player.finish_old = own;
    player.finish_stage = 1;
    unsafe {
        write_dseg_page(3, saved);
    }
    n(0)
}

// The stopped Player retains its pose and receives stop controls only after
// the nested AITank Tick has succeeded. Activation belongs to the next frame.
pub fn started(state: &scene_parent::State, player: &mut player_world::Player) {
    player.finish_stage = 2;
    let own = player.finish_old;
    p(at(player.fields, (own * 30) as i16), 13, n(0));
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let input = ptr(0xc000 + car_runtime::METADATA_WORD + own * 28 + 2);
    let mut speed = g(state.cars[own].drive, 2);
    if speed < n(0) {
        speed = -speed;
    }
    let mut brake = n(0);
    if speed >= n(1000) {
        brake = fix32::from_words(65535, 0);
    }
    p(input, 0, n(0));
    p(input, 1, brake);
    p(input, 2, n(0));
    unsafe {
        write_dseg_page(3, saved);
    }
}

pub fn activate(state: &mut scene_parent::State, player: &mut player_world::Player) -> fix32 {
    if player.finish_stage == 2 {
        if lifecycle::enable(&mut state.runtime.ticks, player.finish_entity) >= 2 {
            return n(-1);
        }
        player.finish_stage = 3;
    }
    n(0)
}

pub fn load(world: &scene_world::World) -> finish_drive::Routes {
    let header: Buf<u16, 16> = Buf::new([0; 16]);
    let h = header.as_array();
    read(h, DRIVING_FINISH_SCENE, fix32::zero(), 16);
    if h[0u16] != 0x4644
        || h[1u16] != 1
        || h[2u16] > LIMIT - WORD
        || h[3u16] != world.fixed_count
        || h[3u16] > 240
        || h[4u16] > 36
        || h[5u16] != 16
        || h[6u16] != 16 + h[3u16]
        || h[7u16] != h[6u16] + h[4u16] * 22
        || h[8u16] != 124
        || h[2u16] != h[7u16] + 124
        || h[9u16] > 2
        || h[10u16] > 5
        || DRIVING_ACTORS.sector_count() * 64 < LIMIT
    {
        halt(0x7f90);
    }
    let saved = read_dseg_page(2);
    unsafe {
        write_dseg_page(2, DRIVING_ACTORS.first_sector() >> 8);
    }
    let data = unsafe { Ptr::from_addr(0x8000 + WORD).as_u16_array() };
    read(data, DRIVING_FINISH_SCENE, fix32::zero(), h[2u16]);
    let indices = at(data, h[5u16] as i16);
    let mut id = 0u16;
    while id < h[3u16] {
        if indices[id] != 65535 && indices[id] >= h[4u16] {
            halt(0x7f90);
        }
        id += 1;
    }
    let routes = finish_drive::Routes {
        indices,
        points: at(data, h[6u16] as i16),
        count: h[3u16],
        point_count: h[4u16],
        collider_page: saved,
    };
    effect_pool::restore(saved);
    routes
}

// Loaded identities must still refer to global obstacle IDs, including route
// points interspersed with scenery. This also checks actor/World page handoff.
pub fn check(
    routes: &finish_drive::Routes,
    state: &scene_parent::State,
    world: &scene_world::World,
) {
    let saved = read_dseg_page(2);
    unsafe {
        write_dseg_page(2, DRIVING_ACTORS.first_sector() >> 8);
    }
    let raw: Buf<u16, 60> = Buf::new([0; 60]);
    let collider = raw.as_array();
    let mut id = 0u16;
    let mut found = 0u16;
    while id < routes.count {
        if routes.indices[id] != 65535 {
            if !finish_drive::load_collider(routes, world, id, collider)
                || read_dseg_page(2) != DRIVING_ACTORS.first_sector() >> 8
                || finish_drive::point(
                    routes,
                    &state.lifetime.identities,
                    attitude::get(collider, 0),
                ) != fix32::from_words(id, 0)
            {
                halt(0x7f91);
            }
            found += 1;
        }
        id += 1;
    }
    if found != routes.point_count {
        halt(0x7f91);
    }
    effect_pool::restore(saved);
}
