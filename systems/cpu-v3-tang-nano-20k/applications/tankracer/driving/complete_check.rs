//! Install and compare vehicle fixtures around the shared runtime.
#![link_cseg = "vehicle"]
use super::*;
use car_runtime::*;
pub fn load(
    case: u16,
    pickups: &mut Pickups,
    impacts: Array<impact::State>,
    auxiliary: Array<u16>,
    prepared: Array<prefix::Shared>,
) -> State {
    let enabled = driving_has_complete(case);
    let page = pickups.effects_page;
    if enabled {
        let saved = read_dseg_page(3);
        unsafe {
            write_dseg_page(3, page);
        }
        if CAR_WORD <= scene_trains::GUARD_WORD + 1
            || CAR_WORD + CAR_CAPACITY * CAR_WORDS >= weapon_world::OVERLAY_WORD - 1
        {
            halt(0x7f5a);
        }
        let initial = driving_complete_initial(case);
        // Source diagnostics keep their original layout. Decode each segment
        // directly into its authoritative owner; the old car region is free.
        read_mapped(ptr(0xc000), initial, 0, 6810);
        read_mapped(row(0), initial, 6810, CAR_WORDS * 2);
        read_mapped(
            ptr(0xc000 + 8722),
            initial,
            8722,
            DRIVING_COMPLETE_WORDS - 8722,
        );
        let mut old = ptr(0xc000 + 6810);
        let mut i = 0u16;
        while i < CAR_WORDS * 2 {
            old[i] = 0xa6db;
            i += 1;
        }
        read_mapped(ptr(0xc000 + METADATA_WORD), initial, 10358, 56);
        read_mapped(ptr(0xc000 + INPUT_WORD), initial, 10414, 52);
        let mut metadata = ptr(0xc000 + METADATA_WORD + 56);
        i = 0;
        while i < 28 {
            metadata[i] = 0;
            i += 1;
        }
        let mut input = ptr(0xc000 + INPUT_WORD + 52);
        i = 0;
        while i < 26 {
            input[i] = 0;
            i += 1;
        }
        let mut old_metadata = ptr(0xc000 + 10358);
        i = 0;
        while i < 108 {
            old_metadata[i] = 0xa6db;
            i += 1;
        }
        let mut reserved = row(2);
        i = 0;
        while i < CAR_WORDS {
            reserved[i] = 0;
            i += 1;
        }
        let mut guards = ptr(0xc000);
        guards[CAR_WORD - 1] = 0x71c3;
        guards[CAR_WORD + CAR_CAPACITY * CAR_WORDS] = 0x3c17;
        pickups.quads.count = g(ptr(0xc000 + 6800), 0);
        let mut end = ptr(0xffff);
        end[0u16] = 0x53a9;
        unsafe {
            write_dseg_page(3, saved);
        }
    }
    let mut state = car_runtime::bind(
        page,
        view_of(&mut pickups.quads),
        impacts,
        auxiliary,
        prepared,
    );
    state.enabled = enabled;
    state
}
fn check_vehicle_fields(state: &State, expected: &mut Reference, end: u16) {
    expect_block(ptr(0xc000 + 6802), expected, 6802, 8);
    expect_block(row(0), expected, 6810, 952);
    expect_block(sub(state.prefixes, 8), expected, 7762, 4);
    expect_block(row(1), expected, 7766, 952);
    expect_block(sub(state.prefixes, 58), expected, 8718, 4);
    expect_block(ptr(0xc000 + 8722), expected, 8722, 10358 - 8722);
    expect_block(ptr(0xc000 + METADATA_WORD), expected, 10358, 56);
    let mut words = end - 10414;
    if words > 52 {
        words = 52;
    }
    expect_block(ptr(0xc000 + INPUT_WORD), expected, 10414, words);
    if end > 10466 {
        expect_block(ptr(0xc000 + 10466), expected, 10466, 2);
    }
}
pub fn check(state: &State, case: u16, offset: fix32) -> fix32 {
    check_asset(state, case, offset, driving_complete_frames(case))
}
pub fn check_asset(state: &State, case: u16, offset: fix32, asset: AssetBytes) -> fix32 {
    if !state.enabled {
        return offset;
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, state.page);
    }
    let mut expected = reference(asset, offset, DRIVING_COMPLETE_WORDS);
    // Count is owned by the shared pool descriptor. The source snapshot places
    // its diagnostic column between quad records and vehicle state.
    expect_block(ptr(0xc000), &mut expected, 0, 6800);
    let quads = state.shared.quads;
    expect_value(quads[0u16].count, &mut expected, 3400);
    if driving_has_player(case) {
        // These two diagnostic columns alias the authoritative route/racer rows.
        check_vehicle_fields(state, &mut expected, 10438);
        expect_value(g(state.racers, 13), &mut expected, 5219);
        expect_block(ptr(0xc000 + INPUT_WORD + 26), &mut expected, 10440, 24);
        expect_value(g(sub(state.racers, 30), 13), &mut expected, 5232);
        expect_block(ptr(0xc000 + 10466), &mut expected, 10466, 2);
    } else {
        check_vehicle_fields(state, &mut expected, DRIVING_COMPLETE_WORDS);
    }
    if ptr(0xffff)[0u16] != 0x53a9
        || ptr(0xc000 + CAR_WORD - 1)[0u16] != 0x71c3
        || ptr(0xc000 + CAR_WORD + CAR_CAPACITY * CAR_WORDS)[0u16] != 0x3c17
    {
        halt(0x7f51);
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    offset + fix32::from_words(DRIVING_COMPLETE_WORDS, 0)
}
