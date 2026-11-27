//! Three authored flocks share one small, independently mapped actor block.
#![link_cseg = "parents"]
use super::*;
const WORD: u16 = 0;
const WORDS: u16 = 1060;
const ROW: u16 = 348;
fn ptr(word: u16) -> Array<u16> {
    unsafe { Ptr::from_addr(word).as_u16_array() }
}
pub fn contains(object: u16) -> bool {
    object >= 183 && object <= 185
}
pub fn load() {
    let header: Buf<u16, 16> = Buf::new([0; 16]);
    let h = header.as_array();
    read(h, DRIVING_FLOCK, fix32::zero(), 16);
    if h[0u16] != 0x464c
        || h[1u16] != 1
        || h[2u16] != 3
        || h[3u16] != 2
        || h[4u16] != 288
        || h[5u16] != 24
        || h[6u16] != ROW
        || h[7u16] != WORDS
        || h[8u16] != 1188
        || WORDS + 1 > DRIVING_ACTORS.sector_count() * 64
        || DRIVING_FLOCK.byte_len_hi != 0
        || DRIVING_FLOCK.byte_len_lo != WORDS * 2
    {
        halt(0x7f7f);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, DRIVING_ACTORS.first_sector() >> 8);
    }
    pickup_check::read_pickup_stage(ptr(0xc000 + WORD), DRIVING_FLOCK, 0, WORDS);
    let mut i = 0u16;
    while i < 3 {
        let row = ptr(0xc000 + WORD + 16 + i * ROW);
        if row[0u16] != 183 + i || row[1u16] != 2 {
            halt(0x7f80);
        }
        let mut m = 0u16;
        while m < 2 {
            let model = at(row, 292 + (m * 28) as i16);
            if model[1u16] >= 8 || model[2u16] >= 8 || model[3u16] != 8 {
                halt(0x7f80);
            }
            m += 1;
        }
        i += 1;
    }
    let mut words = ptr(0xc000);
    words[WORD + WORDS] = 0x5f13;
    unsafe {
        write_dseg_page(3, saved);
    }
}
fn geometry(store: &scene_meshes::Store, id: u16) -> scene_flock::Geometry {
    if id >= store.count {
        return scene_flock::Geometry {
            exists: false,
            flags: ptr(0),
            revision: ptr(0),
            vertices: unsafe { Ptr::from_addr(0).as_i16_array() },
            page: 65535,
        };
    }
    let saved = scene_meshes::select(store, id);
    let view = scene_meshes::view(store, id, false);
    let row = at(store.records, (id * 13) as i16);
    let result = scene_flock::Geometry {
        exists: true,
        flags: row,
        revision: at(row, 4),
        vertices: view.mesh.vertices,
        page: view.page,
    };
    unsafe {
        write_dseg_page(3, saved);
    }
    result
}
pub fn tick(
    scene: &scene_query::Scene,
    state: &mut scene_parent::State,
    world: &mut scene_world::World,
    player: &mut player_world::Player,
    frame: Array<u16>,
    entity: fix32,
) -> fix32 {
    let object = scene_lifecycle::object(&state.lifetime, entity);
    if object.hi_bits() != 0 || !contains(object.lo_bits()) {
        return integer::from_i16(-1);
    }
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, DRIVING_ACTORS.first_sector() >> 8);
    }
    let row = ptr(0xc000 + WORD + 16 + (object.lo_bits() - 183) * ROW);
    let first = at(row, 292);
    let second = at(row, 320);
    let models: Buf<flock::Model, 2> = Buf::new([
        flock::Model {
            vertices: unsafe { at(first, 4).as_ptr().as_i16_array() },
            page: DRIVING_ACTORS.first_sector() >> 8,
            low: first[1u16],
            high: first[2u16],
        },
        flock::Model {
            vertices: unsafe { at(second, 4).as_ptr().as_i16_array() },
            page: DRIVING_ACTORS.first_sector() >> 8,
            low: second[1u16],
            high: second[2u16],
        },
    ]);
    let meshes: Buf<scene_flock::Geometry, 2> = Buf::new([
        geometry(&state.resources, first[0u16]),
        geometry(&state.resources, second[0u16]),
    ]);
    let records: Buf<scene_flock::Record, 1> = Buf::new([scene_flock::Record {
        object: object.lo_bits(),
        present: true,
        state: flock::State {
            flight: at(row, 4),
            birds: at(row, 68),
            models: models.as_array(),
            model_count: 2,
            range: attitude::get(row, 1),
        },
        meshes: meshes.as_array(),
        mesh_count: 2,
    }]);
    let status = scene_flock::dispatch(
        scene,
        &mut state.lifetime,
        &mut state.runtime,
        world,
        &mut player.weapons,
        &scene_flock::Tick {
            call: scene_creation::Call { entity, slot: 12 },
            dt: attitude::get(frame, 0),
            number: attitude::get(frame, 26),
            simulation: true,
            vehicles: true,
            player_present: true,
            player: at(state.cars[state.slots[0u16]].attitude, 36),
            records: records.as_array(),
            count: 1,
            tables: projectile::Tables {
                sine: player.sine,
                angle: player.angle,
            },
        },
    );
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
#[link_cseg = "checks"]
pub fn check(store: &scene_meshes::Store, expected: &mut Reference, cursor: u16) {
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, DRIVING_ACTORS.first_sector() >> 8);
    }
    let mut i = 0u16;
    let mut out = cursor;
    while i < 3 {
        let row = ptr(0xc000 + WORD + 16 + i * ROW);
        expect_block(at(row, 4), expected, out, 288);
        out += 288;
        expect_block(at(row, 296), expected, out, 24);
        out += 24;
        expect_block(at(row, 324), expected, out, 24);
        out += 24;
        let mut m = 0u16;
        while m < 2 {
            let id = at(row, 292 + (m * 28) as i16)[0u16];
            let exists = id < store.count;
            expect_word(exists as u16, expected, out);
            if !exists {
                halt(0x7f80);
            }
            let previous = scene_meshes::select(store, id);
            let view = scene_meshes::view(store, id, false);
            expect_word(view.valid as u16, expected, out + 1);
            expect_block(view.revision, expected, out + 2, 4);
            if view.valid {
                if view.mesh.vertex_count != 8 {
                    halt(0x7f80);
                }
                expect_block(
                    unsafe { view.mesh.vertices.as_ptr().as_u16_array() },
                    expected,
                    out + 6,
                    24,
                );
            }
            unsafe {
                write_dseg_page(3, previous);
            }
            out += 30;
            m += 1;
        }
        i += 1;
    }
    if ptr(0xc000)[WORD + WORDS] != 0x5f13 || out != cursor + 1188 {
        halt(0x7f81);
    }
    unsafe {
        write_dseg_page(3, saved);
    }
}
