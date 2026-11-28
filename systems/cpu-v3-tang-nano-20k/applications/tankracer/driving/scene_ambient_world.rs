//! Ambient owners update in the resident property-page context.
#![link_cseg = "parents"]
use super::*;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
pub fn contains(object: u16) -> bool {
    object == 0 || object == 1
}
pub fn load(player: &mut player_world::Player, h: Array<u16>) {
    if h[58u16] != 1
        || h[59u16] != 0
        || h[56u16] != h[7u16] + 124
        || h[39u16] != h[56u16] + h[57u16]
        || h[57u16] < 432
    {
        halt(0x7f7d);
    }
    player.ambient_word = 10000 + h[38u16] + h[56u16];
    player.context_end = 10000 + h[42u16];
    let header = ptr(0xc000 + player.ambient_word);
    if header[0u16] != 0x414d
        || header[1u16] != 1
        || header[2u16] > 8
        || header[3u16] >= 10
        || h[57u16] != 432 + header[2u16] * 10
    {
        halt(0x7f7d);
    }
    player.ambient_sounds = header[2u16];
    let mut words = ptr(0xc000);
    words[player.context_end] = 0x4b17;
}
pub fn tick(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    entity: fix32,
) -> fix32 {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let camera: Buf<u16, 6> = Buf::new([0; 6]);
    copy(camera.as_array(), ptr(0xc000 + 10174), 6);
    spatial::select(&world.grid);
    let base = 0xc000 + player.ambient_word;
    let smoke = base + 6 + player.ambient_sounds * 10;
    let records: Buf<scene_ambient::Smoke, 1> = Buf::new([scene_ambient::Smoke {
        object: 1,
        present: true,
        state: smoker::State {
            header: ptr(smoke),
            particles: ptr(smoke + 10),
            sequence: ptr(base + 3),
        },
    }]);
    let noisy: Buf<scene_ambient::Noisy, 1> = Buf::new([scene_ambient::Noisy {
        object: 0,
        present: true,
        selected: ptr(base + 4),
        sounds: ptr(base + 6),
        count: player.ambient_sounds,
    }]);
    let status = scene_ambient::dispatch(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &scene_ambient::Tick {
            call: scene_creation::Call { entity, slot: 12 },
            dt: attitude::get(frame, 0),
            number: attitude::get(frame, 26),
            camera: camera.as_array(),
            vehicles: true,
            player_present: true,
            simulation: true,
            smoke: records.as_array(),
            smoke_count: 1,
            noisy: noisy.as_array(),
            noisy_count: 1,
        },
    );
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
#[link_cseg = "frame"]
pub fn check(
    player: &player_world::Player,
    world: &scene_world::World,
    expected: &mut Reference,
    cursor: u16,
) {
    let saved = spatial::select(&world.grid);
    let base = 0xc000 + player.ambient_word;
    expect_block(ptr(base + 4), expected, cursor, 2);
    expect_word(ptr(base)[3u16], expected, cursor + 2);
    expect_word(0, expected, cursor + 3);
    expect_block(
        ptr(base + 6 + player.ambient_sounds * 10),
        expected,
        cursor + 4,
        426,
    );
    if ptr(0xc000)[player.context_end] != 0x4b17 {
        halt(0x7f7e);
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
