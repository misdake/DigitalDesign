//! Pickup fixture installation and comparison around the shared owner.
#![link_cseg = "vehicle"]
use super::*;
pub fn load_pickups(
    case: u16,
    world: &scene_world::World,
    meta: Array<u16>,
    motion_page: u16,
    lines: Array<effect_pool::Pool>,
) -> Pickups {
    let effects_page = DRIVING_PICKUP_EFFECTS.first_sector() >> 8;
    let mut result = Pickups {
        enabled: driving_has_pickup(case) || driving_has_player(case),
        bindings: meta,
        flights: at(world.properties, FLIGHT_WORD as i16),
        meta: at(meta, 780),
        page: world.grid.page,
        motion_page: motion_page,
        effects_page: effects_page,
        quads: quad_pool::Pool {
            records: unsafe { Ptr::from_addr(0xc660).as_u16_array() },
            exhausted: unsafe { Ptr::from_addr(0xda54).as_u16_array() },
            count: fix32::zero(),
        },
        lines: lines,
        sine: unsafe { Ptr::from_addr(0).as_i16_array() },
    };
    if !result.enabled {
        return result;
    }
    let saved = spatial::select(&world.grid);
    let bindings = if driving_has_player(case) {
        DRIVING_PLAYER_PICKUP_BINDINGS
    } else {
        DRIVING_PICKUP_BINDINGS
    };
    read(result.bindings, bindings, fix32::zero(), 780);
    read(result.meta, DRIVING_PICKUP_CONFIG, fix32::zero(), 51);
    if driving_has_player(case) {
        if DRIVING_PLAYER_PICKUP_FLIGHTS.byte_len_hi != 0
            || DRIVING_PLAYER_PICKUP_FLIGHTS.byte_len_lo != FLIGHT_WORDS * 2
            || FLIGHT_WORD + FLIGHT_WORDS >= 10000
        {
            halt(0x7f84);
        }
        read_mapped(
            result.flights,
            DRIVING_PLAYER_PICKUP_FLIGHTS,
            0,
            FLIGHT_WORDS,
        );
        let mut guard = world.properties;
        guard[FLIGHT_WORD + FLIGHT_WORDS] = 0x6c25;
        unsafe {
            write_dseg_page(3, saved);
        }
        return result;
    }
    let asset = driving_pickup_initial(case);
    read_mapped(result.flights, asset, 0, 1620);
    let mut i = 0u16;
    while i < 2 {
        let row = at(result.meta, (51 + i * 4) as i16);
        let init: Buf<u16, 6> = Buf::new([0; 6]);
        read(
            init.as_array(),
            asset,
            fix32::from_words(1620 + i * 6, 0),
            6,
        );
        copy(row, init.as_array(), 3);
        i += 1;
    }
    unsafe {
        write_dseg_page(3, effects_page);
    }
    let data = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    read_mapped(data, asset, 1632, 6772);
    let mut guard = data;
    guard[6772u16] = 0x9a53;
    guard[16383u16] = 0x53a9;
    let count: Buf<u16, 2> = Buf::new([0; 2]);
    read(count.as_array(), asset, fix32::from_words(8404, 0), 2);
    result.quads.count = attitude::get(count.as_array(), 0);
    let pool = &mut result.lines[0u16];
    read(count.as_array(), asset, fix32::from_words(8406, 0), 2);
    pool.count = attitude::get(count.as_array(), 0);
    let line_page = effect_pool::select(pool);
    read(pool.lines, asset, fix32::from_words(8408, 0), 1600);
    effect_pool::restore(line_page);
    read(
        unsafe { view_of(&mut pool.overflow).as_ptr().as_u16_array() },
        asset,
        fix32::from_words(10008, 0),
        16,
    );
    read(pool.exhausted, asset, fix32::from_words(10024, 0), 16);
    unsafe {
        write_dseg_page(3, saved);
    }
    result
}
#[link_cseg = "frame"]
pub fn check_pickup_flights(store: &Pickups, asset: AssetBytes, frame: u16) {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, store.page);
    }
    let mut expected = reference(
        asset,
        fmul_lo32(
            fix32::from_words(frame, 0),
            fix32::from_words(FLIGHT_WORDS, 0),
        ),
        FLIGHT_WORDS,
    );
    expect_block(store.flights, &mut expected, 0, FLIGHT_WORDS);
    if unsafe { Ptr::from_addr(0xc000 + FLIGHT_WORD + FLIGHT_WORDS).read(0) } != 0x6c25 {
        halt(0x7f85);
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
pub fn check_pickups(
    store: &Pickups,
    state: &scene_parent::State,
    asset: AssetBytes,
    offset: fix32,
) {
    let mut expected = reference(asset, offset, 10040);
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, store.page);
    }
    let mut i = 0u16;
    while i < 30 {
        let f = at(store.flights, (i * 54) as i16);
        expect_block(f, &mut expected, i * 54, 28);
        let mut j = 0u16;
        while j < 24 {
            let mut value = 0u16;
            if attitude::get(f, 10) != fix32::zero() {
                value = f[28 + j];
            }
            expect_word(value, &mut expected, i * 54 + 28 + j);
            j += 1;
        }
        expect_block(at(f, 52), &mut expected, i * 54 + 52, 2);
        i += 1;
    }
    i = 0;
    while i < 2 {
        let inv = at(store.meta, (51 + i * 4) as i16);
        expect_block(inv, &mut expected, 1620 + i * 6, 2);
        expect_word(inv[2u16], &mut expected, 1622 + i * 6);
        expect_word(0, &mut expected, 1623 + i * 6);
        expect_block(
            at(state.cars[i].suspension, 66),
            &mut expected,
            1624 + i * 6,
            2,
        );
        i += 1;
    }
    unsafe {
        write_dseg_page(3, store.effects_page);
    }
    let guard = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    if guard[6772u16] != 0x9a53 || guard[16383u16] != 0x53a9 {
        halt(0x7f51);
    }
    expect_block(
        unsafe { Ptr::from_addr(0xc000).as_u16_array() },
        &mut expected,
        1632,
        6772,
    );
    expect_word(store.quads.count.lo_bits(), &mut expected, 8404);
    expect_word(store.quads.count.hi_bits(), &mut expected, 8405);
    let pool = &store.lines[0u16];
    expect_word(pool.count.lo_bits(), &mut expected, 8406);
    expect_word(pool.count.hi_bits(), &mut expected, 8407);
    let line_page = effect_pool::select(pool);
    expect_block(pool.lines, &mut expected, 8408, 1600);
    effect_pool::restore(line_page);
    let mut pools = store.lines;
    expect_block(
        unsafe { view_of(&mut pools[0u16].overflow).as_ptr().as_u16_array() },
        &mut expected,
        10008,
        16,
    );
    expect_block(pool.exhausted, &mut expected, 10024, 16);
    unsafe {
        write_dseg_page(3, saved);
    }
}
