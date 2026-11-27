//! Authored paths and travel stay in the effects bank beside emitter state.
#![link_cseg = "parents"]
use super::*;
pub const STATE_WORD: u16 = 12086;
const OBJECT_WORD: u16 = 12206;
const SOUND_WORD: u16 = 12208;
pub const GUARD_WORD: u16 = 12210;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
pub fn contains(object: u16) -> bool {
    object == 125 || object == 239
}
pub fn load(player: &player_world::Player, h: Array<u16>) {
    if h[56u16] != h[7u16] + 124 {
        halt(0x7f7a);
    }
    let previous = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let mut i = 0u16;
    let mut objects = ptr(0xc000 + OBJECT_WORD);
    let mut sounds = ptr(0xc000 + SOUND_WORD);
    let header: Buf<u16, 2> = Buf::new([0; 2]);
    while i < 2 {
        read(
            header.as_array(),
            player.context,
            fix32::from_words(h[38u16] + h[7u16] + i * 62, 0),
            2,
        );
        objects[i] = header[0u16];
        sounds[i] = header[1u16];
        if !contains(objects[i]) || sounds[i] > 1 || (i > 0 && objects[i] <= objects[i - 1]) {
            halt(0x7f7a);
        }
        pickup_check::read_pickup_stage(
            ptr(0xc000 + STATE_WORD + i * 60),
            player.context,
            h[38u16] + h[7u16] + i * 62 + 2,
            60,
        );
        i += 1;
    }
    let mut words = ptr(0xc000);
    words[STATE_WORD - 1] = 0x2a31;
    words[GUARD_WORD] = 0x13a2;
    unsafe {
        write_dseg_page(3, previous);
    }
}
pub fn tick(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    entity: fix32,
) -> fix32 {
    let previous = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let records: Buf<scene_train::Record, 2> = Buf::new([
        scene_train::Record {
            object: ptr(0xc000 + OBJECT_WORD)[0u16],
            present: true,
            travel: ptr(0xc000 + STATE_WORD),
            motion: ptr(0xc000 + STATE_WORD + 38),
            sound: ptr(0xc000 + SOUND_WORD)[0u16] != 0,
        },
        scene_train::Record {
            object: ptr(0xc000 + OBJECT_WORD)[1u16],
            present: true,
            travel: ptr(0xc000 + STATE_WORD + 60),
            motion: ptr(0xc000 + STATE_WORD + 98),
            sound: ptr(0xc000 + SOUND_WORD)[1u16] != 0,
        },
    ]);
    let status = scene_train::dispatch(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &scene_train::Tick {
            call: scene_creation::Call { entity, slot: 12 },
            dt: attitude::get(frame, 0),
            number: attitude::get(frame, 26),
            simulation: true,
            player: scene_train::Car {
                vehicles: true,
                present: true,
                position: at(state.cars[0u16].attitude, 36),
            },
            records: records.as_array(),
            count: 2,
            sine: player.sine,
        },
    );
    unsafe {
        write_dseg_page(3, previous);
    }
    status
}
