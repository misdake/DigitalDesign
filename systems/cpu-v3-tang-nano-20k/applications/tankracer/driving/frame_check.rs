//! Bounded source-answer reads and short-lived driving verification scratch.
#![link_cseg = "checks"]
use super::*;
pub struct Reference {
    pub asset: AssetBytes,
    pub offset: fix32,
    pub words: u16,
    pub block: u16,
    pub cache: Buf<u16, 64>,
}
pub fn reference(asset: AssetBytes, offset: fix32, words: u16) -> Reference {
    Reference {
        asset: asset,
        offset: offset,
        words: words,
        block: 65535,
        cache: Buf::new([0; 64]),
    }
}
pub fn word(expected: &mut Reference, index: u16) -> u16 {
    if index >= expected.words {
        halt(0x7f34);
    }
    let block = index & 65472;
    if block != expected.block {
        let mut count = expected.words - block;
        if count > 64 {
            count = 64;
        }
        read(
            expected.cache.as_array(),
            expected.asset,
            expected.offset + fix32::from_words(block, 0),
            count,
        );
        expected.block = block;
    }
    expected.cache[index - block]
}
fn value(expected: &mut Reference, index: u16) -> fix32 {
    let lo = word(expected, index * 2);
    fix32::from_words(lo, word(expected, index * 2 + 1))
}
pub fn expect_value(actual: fix32, expected: &mut Reference, index: u16) {
    if actual != value(expected, index) {
        unsafe {
            addr_of(&DRIVING_FIELD).write(0, index * 2);
        }
        halt(0x8000 + index * 2);
    }
}
pub fn expect_word(actual: u16, expected: &mut Reference, index: u16) {
    if actual != word(expected, index) {
        unsafe {
            addr_of(&DRIVING_FIELD).write(0, index);
        }
        halt(0x8000 + index);
    }
}
pub fn expect_block(actual: Array<u16>, expected: &mut Reference, base: u16, count: u16) {
    let mut i = 0u16;
    while i < count {
        if actual[i] != word(expected, base + i) {
            unsafe {
                addr_of(&DRIVING_FIELD).write(0, base + i);
            }
            halt(0x8000 + base + i);
        }
        i += 1;
    }
}
fn check_car(car: &mut driving::Car, expected: &mut Reference, base: u16) {
    if car.visible {
        expect_block(car.attitude, expected, base, 58);
        expect_block(car.suspension, expected, base + 58, 88);
        expect_block(car.drive, expected, base + 146, 32);
    } else {
        // Packed diagnostic columns alias these authoritative fields. Read them
        // at capture time instead of copying state between update branches.
        expect_block(car.attitude, expected, base, 48);
        expect_value(attitude::get(car.drive, 14), expected, base / 2 + 24);
        expect_value(attitude::get(car.drive, 13), expected, base / 2 + 25);
        expect_block(at(car.attitude, 52), expected, base + 52, 6);
        expect_block(car.suspension, expected, base + 58, 88);
        let mut i = 0u16;
        while i < 16 {
            let mut actual = attitude::get(car.drive, i);
            if i >= 3 && i < 6 {
                actual = attitude::get(car.attitude, 18 + i);
            } else if i == 9 {
                actual = attitude::get(car.motion, 15);
            } else if i == 12 {
                actual = attitude::get(car.motion, 14);
            }
            expect_value(actual, expected, base / 2 + 73 + i);
            i += 1;
        }
    }
    expect_block(car.motion, expected, base + 178, 32);
    expect_block(car.boost, expected, base + 210, 8);
    expect_block(car.tracks, expected, base + 218, 48);
    expect_block(
        unsafe { view_of(&mut car.ground).as_ptr().as_u16_array() },
        expected,
        base + 266,
        206,
    );
    expect_block(car.articulation.as_array(), expected, base + 472, 8);
    expect_block(car.lean.as_array(), expected, base + 480, 8);
    expect_block(car.hull.as_array(), expected, base + 488, 24);
    let mut i = 0u16;
    while i < 12 {
        expect_value(
            integer::from_i16(car.hull[24 + i] as i16),
            expected,
            base / 2 + 256 + i,
        );
        i += 1;
    }
    expect_value(
        fix32::from_words(car.hull[36u16], 0),
        expected,
        base / 2 + 268,
    );
    let mut ready = fix32::zero();
    if car.ready {
        ready = integer::from_i16(1);
    }
    expect_value(ready, expected, base / 2 + 269);
    expect_value(car.turret_z, expected, base / 2 + 270);
    i = 0;
    while i < DRIVING_GUN_WORDS {
        expect_value(integer::from_i16(car.gun[i]), expected, base / 2 + 271 + i);
        i += 1;
    }
}
fn check_mesh(target: &damage::Target, expected: &mut Reference, flags: u16, vertices: u16) {
    let mut i = 0u16;
    while i < 10 {
        expect_value(fix32::from_words(target.state[i], 0), expected, flags + i);
        i += 1;
    }
    i = 0;
    while i < DRIVING_BODY_VERTICES * 3 {
        expect_value(
            integer::from_i16(target.mesh.vertices[i]),
            expected,
            vertices + i,
        );
        i += 1;
    }
    i = 0;
    while i < DRIVING_BODY_FACES * 3 {
        expect_value(
            integer::from_i16(target.mesh.normals[i]),
            expected,
            vertices + DRIVING_BODY_VERTICES * 3 + i,
        );
        i += 1;
    }
}
pub fn check_surfaces(components: Array<deform_contact::Component>, expected: &mut Reference) {
    let mut i = 0u16;
    let mut offset = 0u16;
    while i < 7 {
        let c = &components[i];
        expect_value(c.state.dirty, expected, offset);
        expect_value(c.state.last_vehicle, expected, offset + 1);
        expect_value(c.state.sound_timer, expected, offset + 2);
        expect_value(fix32::from_words(c.sampling.dirty, 0), expected, offset + 3);
        let mut attached = fix32::zero();
        if c.sampling.attached {
            attached = integer::from_i16(1);
        }
        expect_value(attached, expected, offset + 4);
        offset += 5;
        let mut j = 0u16;
        while j < c.mesh.vertex_count * 3 {
            expect_value(integer::from_i16(c.mesh.vertices[j]), expected, offset);
            offset += 1;
            j += 1;
        }
        i += 1;
    }
    if offset != DRIVING_SURFACE_VALUES {
        halt(0x7f10);
    }
}
// Compare every canonical impact word while retaining only one particle row.
fn check_impacts(
    state: &mut impact::State,
    vehicle: &impact::Vehicle,
    context: &mut impact::Context,
    expected: &mut Reference,
) {
    let mut i = 0u16;
    while i < 4 {
        expect_value(fix32::zero(), expected, 295 + i);
        i += 1;
    }
    expect_value(attitude::get(context.seed, 0), expected, 299);
    expect_value(state.next, expected, 300);
    expect_value(attitude::get(vehicle.cooldown, 0), expected, 301);
    expect_value(context.pool.count, expected, 302);
    let saved = effect_pool::select(&context.pool);
    expect_block(context.pool.lines, expected, 606, 1600);
    effect_pool::restore(saved);
    expect_block(
        unsafe { view_of(&mut context.pool.overflow).as_ptr().as_u16_array() },
        expected,
        2206,
        16,
    );
    expect_block(context.pool.exhausted, expected, 2222, 16);
    let row: Buf<u16, 22> = Buf::new([0; 22]);
    let mut cursor = 2238u16;
    let batch_page = impact::select(state);
    i = 0;
    while i < 4 {
        let batch = &mut state.batches[i];
        impact_capture::write_batch(batch, row.as_array());
        expect_block(row.as_array(), expected, cursor, 12);
        cursor += 12;
        let mut j = 0u16;
        while j < 10 {
            impact_capture::write_particle(&mut batch.particles[j], row.as_array());
            expect_block(row.as_array(), expected, cursor, 22);
            cursor += 22;
            j += 1;
        }
        i += 1;
    }
    effect_pool::restore(batch_page);
}
#[inline(never)]
pub fn check_snapshot(
    actors: Array<driving::Car>,
    aux: Array<u16>,
    peers: &tank_response::World,
    impacts: &mut impact::State,
    effects: &mut impact::Context,
    expected: &mut Reference,
) {
    let mut actors = actors;
    check_car(&mut actors[0u16], expected, 0);
    check_car(&mut actors[1u16], expected, 3566);
    check_impacts(impacts, &peers.vehicles[0u16].impact, effects, expected);
    check_mesh(&peers.vehicles[0u16].damage, expected, 1583, 1596);
    check_mesh(&peers.vehicles[1u16].damage, expected, 2086, 2096);
    expect_value(attitude::get(aux, 1), expected, 1779);
    expect_value(attitude::get(aux, 18), expected, 2078);
    expect_value(attitude::get(aux, 19), expected, 2079);
    let mut i = 0u16;
    while i < 3 {
        expect_value(attitude::get(aux, 5 + i), expected, 1593 + i);
        expect_value(attitude::get(aux, 2 + i), expected, 1780 + i);
        expect_value(attitude::get(aux, 20 + i), expected, 2080 + i);
        expect_value(attitude::get(aux, 23 + i), expected, 2083 + i);
        expect_value(attitude::get(aux, 13 + i), expected, 2279 + i);
        expect_value(attitude::get(aux, 31 + i), expected, 2282 + i);
        i += 1;
    }
    expect_value(attitude::get(aux, 16), expected, 2286);
    expect_value(attitude::get(aux, 17), expected, 2287);
    expect_value(attitude::get(aux, 34), expected, 2288);
    expect_value(attitude::get(aux, 35), expected, 2289);
    if attitude::get(effects.seed, 0) != value(expected, DRIVING_STATE_VALUES) {
        unsafe {
            addr_of(&DRIVING_FIELD).write(0, DRIVING_STATE_VALUES * 2);
        }
        halt(0x7f05);
    }
}
