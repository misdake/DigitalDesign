//! Authored emitter state over the live projectile factory and shared RNG.
#![link_cseg = "parents"]
use super::*;
pub const STATE_WORD: u16 = 11828;
const OBJECT_WORD: u16 = 12078;
pub const GUARD_WORD: u16 = 12084;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
pub fn contains(object: u16) -> bool {
    object == 81 || object == 112 || object == 113 || object == 114 || object == 115
}
pub fn load(player: &player_world::Player, h: Array<u16>) {
    if h[55u16] != 5 || h[7u16] != h[54u16] + 255 {
        halt(0x7f77);
    }
    let previous = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let mut i = 0u16;
    let mut objects = ptr(0xc000 + OBJECT_WORD);
    while i < 5 {
        read_mapped(
            at(objects, i as i16),
            player.context,
            h[38u16] + h[54u16] + i * 51,
            1,
        );
        if !contains(objects[i]) || (i > 0 && objects[i] <= objects[i - 1]) {
            halt(0x7f77);
        }
        read_mapped(
            ptr(0xc000 + STATE_WORD + i * 50),
            player.context,
            h[38u16] + h[54u16] + i * 51 + 1,
            50,
        );
        i += 1;
    }
    let mut words = ptr(0xc000);
    words[GUARD_WORD] = 0x5e19;
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
    let h = player.context_header.as_array();
    let resources = at(player.projectile_params.as_array(), 38);
    let previous = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, player.weapons.effect_page);
    }
    let camera: Buf<u16, 6> = Buf::new([0; 6]);
    copy(camera.as_array(), ptr(0xc000 + 10174), 6);
    let empty = scene_emitter::Record {
        object: 0,
        present: false,
        data: ptr(0),
    };
    let records: Buf<scene_emitter::Record, 5> = Buf::new([empty; 5]);
    let mut rows = records.as_array();
    let mut i = 0u16;
    while i < 5 {
        rows[i] = scene_emitter::Record {
            object: ptr(0xc000 + OBJECT_WORD)[i],
            present: true,
            data: ptr(0xc000 + STATE_WORD + i * 50),
        };
        i += 1;
    }
    let status = scene_emitter::dispatch(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &scene_emitter::Tick {
            call: scene_creation::Call { entity, slot: 12 },
            dt: attitude::get(frame, 0),
            simulation: true,
            camera: camera.as_array(),
            player: at(state.cars[state.slots[0u16]].motion, 18),
            player_present: true,
            records: records.as_array(),
            count: 5,
            resources_present: true,
            material: attitude::get(resources, 0),
            class: fix32::from_words(h[5u16], 0),
            sine: player.sine,
        },
    );
    unsafe {
        write_dseg_page(3, previous);
    }
    status
}
