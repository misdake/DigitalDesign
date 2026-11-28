//! Whole-world qualification of finish takeover and continuous driving.
#![link_cseg = "frame"]
use super::*;
pub struct Checkpoint {
    pub case: u16,
    pub index: u16,
    pub live: Array<u16>,
}
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
// Independent pre-takeover inputs, shared by the failure-prefix and success cases.
pub fn setup(
    state: &scene_parent::State,
    player: &player_world::Player,
    complete: &car_runtime::State,
    pickups: &Pickups,
    case: u16,
) {
    let asset = driving_finish_transfer_data(0);
    let header: Buf<u16, 32> = Buf::new([0; 32]);
    let h = header.as_array();
    read(h, asset, fix32::zero(), 32);
    if h[0u16] != 0x4654 || h[1u16] != 1 || h[3u16] != 0 {
        halt(0x7f96);
    }
    let target = finish_world::entity(state);
    if target != fix32::from_words(h[4u16], h[5u16]) || state.slots[0u16] != 0 {
        halt(0x7f96);
    }
    p(player.fields, 12, n(3));
    if driving_finish_frames() > 1 {
        p(state.cars[0u16].articulation.as_array(), 0, n(0));
        p(state.cars[0u16].articulation.as_array(), 1, n(0));
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, complete.page);
    }
    let metadata = ptr(0xc000 + car_runtime::METADATA_WORD);
    let input = ptr(0xc000 + car_runtime::INPUT_WORD);
    p(metadata, 5, n(1));
    p(input, 2, n(321));
    p(input, 3, n(12345));
    let lights = ptr(0xc000 + car_runtime::CAR_WORD + 830);
    p(lights, 1, n(1));
    p(lights, 2, n(1));
    p(lights, 3, n(0));
    p(lights, 4, n(1));
    let old = state.cars[0u16].entity;
    let mut pools = complete.shared.quads;
    let lines = &pickups.lines[0u16];
    let mut i = 0u16;
    while i < 2 {
        if quad_pool::get(&pools[0u16], lines, fix32::from_words(i, 0), 1) != effect_pool::free() {
            halt(0x7f96);
        }
        quad_pool::put(&pools[0u16], lines, fix32::from_words(i, 0), 1, n(0));
        pools[0u16].count = pools[0u16].count + n(1);
        let bank = ptr(0xc000 + i * 816);
        p(bank, 0, fix32::from_words(1 - i, 0));
        p(bank, 4, n(1));
        p(bank, 5, old);
        let debris = ptr(0xc000 + 8722 + i * 182);
        p(debris, 89, n(1));
        p(debris, 90, old);
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}

pub fn qualify(
    state: &mut scene_parent::State,
    world: &scene_world::World,
    player: &mut player_world::Player,
    complete: &mut car_runtime::State,
    pickups: &mut Pickups,
    checkpoint: &Checkpoint,
) {
    let case = checkpoint.case;
    setup(state, player, complete, pickups, case);
    let target = finish_world::entity(state);
    if finish_world::transfer(state, player, complete, pickups, 0, target) != n(0) {
        halt(0x7f97);
    }
    p(state.runtime.slot, 0, n(12));
    result(state, world, player, complete, checkpoint);
}

pub fn result(
    state: &mut scene_parent::State,
    world: &scene_world::World,
    player: &mut player_world::Player,
    complete: &car_runtime::State,
    checkpoint: &Checkpoint,
) {
    let case = checkpoint.case;
    let index = checkpoint.index;
    let asset = driving_finish_transfer_data(index);
    let header: Buf<u16, 32> = Buf::new([0; 32]);
    let h = header.as_array();
    read(h, asset, fix32::zero(), 32);
    let saved = read_dseg_page(3);
    let target = player.finish_entity;
    if state.car_count != 3
        || player.present[2u16] != 1
        || player.finish_stage
            != if index != 0 {
                3
            } else if driving_finish_tick(case) {
                2
            } else {
                1
            }
        || state.slots[0u16] != h[9u16]
        || !state.cars[2u16].player
        || state.cars[2u16].owner != 2
        || state.cars[2u16].profile != 0
        || state.cars[2u16].slot != 0
        || entities::class(&state.lifetime.identities, target) != fix32::from_words(h[6u16], 0)
        || g(state.runtime.seed, 0) != fix32::from_words(h[14u16], h[15u16])
    {
        halt(0x7f97);
    }
    // Compare both the nested Tick result and the retained source vehicle.
    let mut expected = reference(asset, fix32::zero(), h[2u16]);
    let mut cars = state.cars;
    frame_check::check_car(&mut cars[2u16], &mut expected, h[16u16]);
    if driving_finish_tick(case) {
        frame_check::check_car(&mut cars[0u16], &mut expected, h[29u16]);
        frame_check::check_car(&mut cars[1u16], &mut expected, h[30u16]);
        expect_block(player.auxiliary, &mut expected, h[31u16], 108);
        expect_word(player.finish_rank, &mut expected, h[31u16] + 108);
    } else {
        frame_check::check_car(&mut cars[0u16], &mut expected, h[16u16]);
    }
    expect_block(at(player.prefixes, 100), &mut expected, h[18u16], 50);
    expect_block(at(player.fields, 60), &mut expected, h[19u16], 30);
    expect_block(at(player.inventory, 8), &mut expected, h[20u16], 4);
    expect_block(
        at(player.hit_fields.as_array(), 48),
        &mut expected,
        h[22u16],
        24,
    );
    expect_block(
        at(player.hit_model.as_array(), 14),
        &mut expected,
        h[23u16],
        7,
    );
    unsafe {
        write_dseg_page(3, complete.page);
    }
    expect_block(
        ptr(0xc000 + car_runtime::CAR_WORD + 1912),
        &mut expected,
        h[17u16],
        952,
    );
    expect_block(at(player.prefixes, 108), &mut expected, h[17u16] + 952, 4);
    expect_block(
        ptr(0xc000 + player_world::TAIL_WORD + 14),
        &mut expected,
        h[21u16],
        7,
    );
    expect_block(
        at(player.body, (DRIVING_MESH_WORDS * 2) as i16),
        &mut expected,
        h[24u16],
        DRIVING_MESH_WORDS,
    );
    expect_block(
        player_world::combat(2),
        &mut expected,
        h[25u16],
        h[26u16] - h[25u16],
    );
    expect_block(
        ptr(0xc000 + car_runtime::METADATA_WORD + 56),
        &mut expected,
        h[26u16],
        28,
    );
    expect_block(
        ptr(0xc000 + car_runtime::INPUT_WORD + 52),
        &mut expected,
        h[26u16] + 28,
        24,
    );
    expect_value(
        g(at(player.fields, 60), 13),
        &mut expected,
        (h[26u16] + 52) / 2,
    );
    expect_block(
        ptr(0xc000 + car_runtime::FINISH_AI_WORD),
        &mut expected,
        h[27u16],
        60,
    );
    let count = ptr(0xc000 + weapon_ufos::COUNT_WORD);
    let visible = ptr(0xc000 + weapon_ufos::VISIBLE_WORD);
    if count[1u16] != h[10u16]
        || (h[10u16] > 0 && g(visible, 0) != fix32::from_words(h[12u16], 0))
        || (h[10u16] > 1 && g(visible, 1) != fix32::from_words(h[13u16], 0))
    {
        halt(0x7f97);
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    complete_check::check_asset(complete, case, fix32::from_words(h[28u16], 0), asset);
    let world_expected = ExpectedScene {
        asset: driving_finish_transfer_world(index),
        offset: fix32::zero(),
    };
    let scene = scene_query::Scene::begin(driving_scene(case));
    check_live(
        checkpoint.live,
        &state.lifetime,
        &state.runtime,
        world,
        &scene,
        &world_expected,
    );
}
