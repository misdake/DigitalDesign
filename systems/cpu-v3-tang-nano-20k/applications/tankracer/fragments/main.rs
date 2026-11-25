//! Source-authored fragment activation and geometry mutation via CB06 assets.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod gpu_window;
mod static_assets;
mod static_buffer;
use static_assets::AssetBytes;
use static_buffer::StaticBuffer;
#[static_buffer]
static FRAGMENTS_MOTION: StaticBuffer = StaticBuffer::new(32768, 32768);
mod attitude;
mod blast;
mod closer;
mod collision;
mod deformation;
mod driving;
mod entities;
mod falling;
mod fragment_case;
mod ground;
mod identity_capture;
mod integer;
mod inverse_basis;
mod lifecycle;
mod mesh_break;
mod mesh_catalog;
mod mesh_storage;
mod object_query;
mod ordered;
mod random;
mod sampling;
mod scene_debris;
mod scene_fragments;
mod scene_lifecycle;
mod scene_meshes;
mod scene_parent;
mod scene_query;
mod scene_world;
mod shatter;
mod spatial;
mod symbols;
mod terrain;
mod trig_lookup;
static FRAGMENTS_CASE: u16 = 0;
static FRAGMENTS_FIELD: u16 = 0;
static FRAGMENTS_CHECKS: u16 = 0;
static FRAGMENTS_FRAME: u16 = 0;
fn read(dst: Array<u16>, asset: AssetBytes, offset: fix32, count: u16) {
    if !gpu_window::gpu_window_asset_read(
        dst,
        asset.first_sector + (offset.hi_bits() << 10) + (offset.lo_bits() >> 6),
        offset.lo_bits() & 63,
        count,
    ) {
        halt(0x7f41);
    }
}
fn compare(
    asset: AssetBytes,
    out: Array<u16>,
    count: u16,
    offset: fix32,
    scratch: Array<u16>,
) -> fix32 {
    read(scratch, asset, offset, 1);
    if scratch[0u16] != count {
        halt(0x7f42);
    }
    let offset = offset + fix32::from_words(1, 0);
    let mut cursor = 0u16;
    while cursor < count {
        let mut chunk = count - cursor;
        if chunk > 256 {
            chunk = 256;
        }
        read(scratch, asset, offset + fix32::from_words(cursor, 0), chunk);
        let mut i = 0u16;
        while i < chunk {
            if out[cursor + i] != scratch[i] {
                unsafe {
                    addr_of(&FRAGMENTS_FIELD).write(0, cursor + i);
                }
                halt(0x8000 + cursor + i);
            }
            i += 1;
        }
        cursor += chunk;
    }
    offset + fix32::from_words(count, 0)
}
fn run_cases() {
    let incoming = read_dseg_page(3);
    let mut bank = scene_fragments::MotionBank {
        page: FRAGMENTS_MOTION.first_sector() >> 8,
        used: 0,
    };
    fragment_case::check_fragment_layout(
        &mut bank,
        FRAGMENTS_MOTION_LAYOUT.as_array(),
        FRAGMENTS_MOTION_OWNERS,
        FRAGMENTS_MOTION_WORDS,
    );
    let storage: FragmentsStorage = fragments_storage();
    let directory: Buf<u16, 6> = Buf::new([0; 6]);
    let d = directory.as_array();
    let input = storage.input.as_array();
    let output = storage.output.as_array();
    let scratch = storage.scratch.as_array();
    let sine = unsafe { addr_of(&FRAGMENTS_SINE).as_i16_array() };
    let scene = scene_query::Scene::begin(FRAGMENTS_SCENE);
    if !scene.valid {
        halt(0x7f44);
    }
    let mut case = 0u16;
    while case < FRAGMENTS_CASES {
        bank.used = FRAGMENTS_MOTION_WORDS;
        unsafe {
            addr_of(&FRAGMENTS_CASE).write(0, case);
        }
        read(d, FRAGMENTS_DIRECTORY, fix32::from_words(case * 6, 0), 6);
        read(
            input,
            FRAGMENTS_INPUTS,
            fix32::from_words(d[0u16], d[1u16]),
            d[2u16],
        );
        let mut expected = fix32::from_words(d[3u16], d[4u16]);
        if d[5u16] == 0 {
            let count = fragment_case::run_mesh_break(input, output, scratch, sine);
            expected = compare(
                FRAGMENTS_EXPECTED,
                output,
                count,
                expected,
                storage.comparison.as_array(),
            );
        } else if d[5u16] == 1 {
            let mut lifetime = storage.lifetime.as_array();
            let mut i = 0u16;
            while i < 11988 {
                lifetime[i] = 0;
                i += 1;
            }
            let mut world = fragment_case::load_scene_fragment_case(input, lifetime, scratch);
            fragment_case::place_fragment_motion(&mut world.object, &mut bank);
            let mut frame = 0u16;
            while frame < world.count {
                unsafe {
                    addr_of(&FRAGMENTS_FRAME).write(0, frame);
                }
                let count =
                    fragment_case::run_scene_fragment_step(&scene, &mut world, frame, output, sine);
                expected = compare(
                    FRAGMENTS_EXPECTED,
                    output,
                    count,
                    expected,
                    storage.comparison.as_array(),
                );
                frame += 1;
            }
        } else if d[5u16] == 2 {
            let mut lifetime = storage.lifetime.as_array();
            let mut i = 0u16;
            while i < 11988 {
                lifetime[i] = 0;
                i += 1;
            }
            let mut resources = fragment_case::load_mesh_store_case(input, lifetime);
            let mut frame = 0u16;
            while frame < resources.count {
                unsafe {
                    addr_of(&FRAGMENTS_FRAME).write(0, frame);
                }
                let count = fragment_case::run_mesh_store_step(&mut resources, frame, output);
                expected = compare(
                    FRAGMENTS_EXPECTED,
                    output,
                    count,
                    expected,
                    storage.comparison.as_array(),
                );
                frame += 1;
            }
            scene_meshes::finish(&mut resources.store);
        } else {
            let mut lifetime = storage.lifetime.as_array();
            let mut i = 0u16;
            while i < 11988 {
                lifetime[i] = 0;
                i += 1;
            }
            let directory: Buf<u16, 64> = Buf::new([0; 64]);
            let mut cells = directory.as_array();
            i = 0;
            while i < 64 {
                cells[i] = i;
                i += 1;
            }
            let mut map = fragment_case::auxiliary_map(FRAGMENTS_CELLS, cells);
            let mut hit = fragment_case::load_auxiliary_hit(input, lifetime, scratch);
            fragment_case::place_fragment_motion(&mut hit.object, &mut bank);
            if !scene_meshes::use_arena(&mut hit.resources, bank.page, bank.used, 16384 - bank.used)
            {
                halt(0x7f83);
            }
            fragment_case::check_fragment_terrain(&mut map, &hit.object);
            let mut frame = 0u16;
            while frame < hit.count {
                unsafe {
                    addr_of(&FRAGMENTS_FRAME).write(0, frame);
                }
                let count = fragment_case::run_auxiliary_hit(&mut hit, frame, output, &mut map);
                expected = compare(
                    FRAGMENTS_EXPECTED,
                    output,
                    count,
                    expected,
                    storage.comparison.as_array(),
                );
                frame += 1;
            }
            scene_meshes::finish(&mut hit.resources);
            map.terrain.finish();
        }
        unsafe {
            addr_of(&FRAGMENTS_CHECKS).write(0, FRAGMENTS_CHECKS + 1);
        }
        if read_dseg_page(3) != incoming {
            halt(0x7f43);
        }
        case += 1;
    }
}
fn run_catalog() {
    let incoming = read_dseg_page(3);
    let storage: CatalogStorage = catalog_storage();
    let scratch = storage.scratch.as_array();
    let output = storage.output.as_array();
    let mut catalog = mesh_catalog::load(
        FRAGMENTS_CATALOG,
        storage.memory.as_array(),
        FRAGMENTS_CATALOG_WORDS,
        scratch,
    );
    if !scene_meshes::use_arena(
        &mut catalog.store,
        FRAGMENTS_MOTION.first_sector() >> 8,
        FRAGMENTS_MOTION_WORDS,
        16384 - FRAGMENTS_MOTION_WORDS,
    ) {
        halt(0x7f83);
    }
    let mut expected = fix32::zero();
    let mut state = 0u16;
    while state < 2 {
        if state != 0 {
            fragment_case::run_catalog_controls(
                &mut catalog,
                FRAGMENTS_CATALOG_CONTROLS,
                FRAGMENTS_CATALOG_OPERATIONS,
                scratch,
            );
        }
        let mut payload_words = FRAGMENTS_CATALOG_INITIAL_PAYLOAD;
        if state != 0 {
            payload_words = FRAGMENTS_CATALOG_FINAL_PAYLOAD;
        }
        if fragment_case::catalog_payload_words(&catalog) != payload_words {
            halt(0x7f70);
        }
        if fragment_case::catalog_cpu_words(&catalog) != FRAGMENTS_CATALOG_INITIAL_PAYLOAD {
            halt(0x7f8e);
        }
        let used = mesh_storage::used(
            catalog.store.arena_page,
            catalog.store.arena_start,
            catalog.store.arena_words,
        );
        if (state == 0 && used != 0) || (state != 0 && used != FRAGMENTS_CATALOG_ARENA_USED) {
            halt(0x7f8f);
        }
        let count = fragment_case::capture_catalog_header(output, &catalog);
        expected = compare(
            FRAGMENTS_CATALOG_EXPECTED,
            output,
            count,
            expected,
            storage.comparison.as_array(),
        );
        let mut id = 0u16;
        while id < catalog.store.count {
            unsafe {
                addr_of(&FRAGMENTS_FRAME).write(0, id);
            }
            let count = fragment_case::capture_catalog_resource(output, &catalog, id);
            expected = compare(
                FRAGMENTS_CATALOG_EXPECTED,
                output,
                count,
                expected,
                storage.comparison.as_array(),
            );
            id += 1;
        }
        if fragment_case::catalog_payload_words(&catalog) != payload_words {
            halt(0x7f71);
        }
        state += 1;
    }
    let page = catalog.store.arena_page;
    let start = catalog.store.arena_start;
    let words = catalog.store.arena_words;
    scene_meshes::finish(&mut catalog.store);
    if mesh_storage::used(page, start, words) != 0 {
        halt(0x7f90);
    }
    fragment_case::check_fragment_prefix(page, FRAGMENTS_MOTION_WORDS);
    let mut restarted = mesh_catalog::load(
        FRAGMENTS_CATALOG,
        storage.memory.as_array(),
        FRAGMENTS_CATALOG_WORDS,
        scratch,
    );
    let count = fragment_case::capture_catalog_header(output, &restarted);
    expected = compare(
        FRAGMENTS_CATALOG_EXPECTED,
        output,
        count,
        expected,
        storage.comparison.as_array(),
    );
    let ids = FRAGMENTS_CATALOG_RESTART_IDS.as_array();
    let mut i = 0u16;
    while i < 3 {
        let count = fragment_case::capture_catalog_resource(output, &restarted, ids[i]);
        expected = compare(
            FRAGMENTS_CATALOG_EXPECTED,
            output,
            count,
            expected,
            storage.comparison.as_array(),
        );
        i += 1;
    }
    if fragment_case::catalog_payload_words(&restarted) != FRAGMENTS_CATALOG_INITIAL_PAYLOAD {
        halt(0x7f72);
    }
    scene_meshes::finish(&mut restarted.store);
    if expected
        != fix32::from_words(
            (FRAGMENTS_CATALOG_EXPECTED.byte_len_lo >> 1)
                | (FRAGMENTS_CATALOG_EXPECTED.byte_len_hi << 15),
            FRAGMENTS_CATALOG_EXPECTED.byte_len_hi >> 1,
        )
    {
        halt(0x7f73);
    }
    if read_dseg_page(3) != incoming {
        halt(0x7f43);
    }
}
fn run_parents() {
    let incoming = read_dseg_page(3);
    let storage: ParentStorage = parent_storage();
    let d: Buf<u16, 5> = Buf::new([0; 5]);
    let directory: Buf<u16, 64> = Buf::new([0; 64]);
    let mut cells = directory.as_array();
    let mut i = 0u16;
    while i < 64 {
        cells[i] = i;
        i += 1;
    }
    let scene = scene_query::Scene::begin(FRAGMENTS_SCENE);
    if !scene.valid {
        halt(0x7f44);
    }
    let tables = fragment_case::ParentTables {
        sine: unsafe { addr_of(&FRAGMENTS_SINE).as_i16_array() },
        angle: unsafe { addr_of(&FRAGMENTS_ANGLE).as_i16_array() },
        inverse: unsafe { addr_of(&FRAGMENTS_INVERSE_COSINE).as_i16_array() },
    };
    let mut index = 0u16;
    while index < PARENT_CASES {
        let mut memory = storage.lifetime.as_array();
        i = 0;
        while i < 11988 {
            memory[i] = 0;
            i += 1;
        }
        unsafe {
            addr_of(&FRAGMENTS_CASE).write(0, FRAGMENTS_CASES + 1 + index);
        }
        read(
            d.as_array(),
            FRAGMENTS_PARENT_DIRECTORY,
            fix32::from_words(index * 5, 0),
            5,
        );
        read(
            storage.input.as_array(),
            FRAGMENTS_PARENT_INPUTS,
            fix32::from_words(d[0u16], d[1u16]),
            d[2u16],
        );
        let mut expected = fix32::from_words(d[3u16], d[4u16]);
        let map = fragment_case::auxiliary_map(FRAGMENTS_CELLS, cells);
        let mut case = fragment_case::load_parent_case(
            storage.input.as_array(),
            storage.lifetime.as_array(),
            storage.resources.as_array(),
            storage.scratch.as_array(),
            map,
        );
        let mut bank = scene_fragments::MotionBank {
            page: FRAGMENTS_MOTION.first_sector() >> 8,
            used: 0,
        };
        fragment_case::place_fragment_motion(&mut case.actor.fragments, &mut bank);
        if !scene_meshes::use_arena(
            &mut case.state.resources,
            bank.page,
            bank.used,
            16384 - bank.used,
        ) {
            halt(0x7f83);
        }
        let mut frame = 0u16;
        while frame < case.count {
            unsafe {
                addr_of(&FRAGMENTS_FRAME).write(0, frame);
            }
            let count = fragment_case::run_parent_frame(
                &scene,
                &mut case,
                frame,
                storage.output.as_array(),
                &tables,
            );
            expected = compare(
                FRAGMENTS_PARENT_EXPECTED,
                storage.output.as_array(),
                count,
                expected,
                storage.comparison.as_array(),
            );
            frame += 1;
        }
        scene_meshes::finish(&mut case.state.resources);
        case.state.map.terrain.finish();
        if read_dseg_page(3) != incoming {
            halt(0x7f43);
        }
        index += 1;
    }
}
fn main() {
    check_storage();
    run_cases();
    unsafe {
        addr_of(&FRAGMENTS_CASE).write(0, FRAGMENTS_CASES);
    }
    run_catalog();
    run_parents();
    halt(0x600d);
}
fn parents_main() {
    run_parents();
    halt(0x600d);
}
fn check_storage() {
    let page = FRAGMENTS_MOTION.first_sector() >> 8;
    fragment_case::check_mesh_storage(page, 0, 16);
    fragment_case::check_mesh_storage(page, 16368, 16);
    fragment_case::check_mesh_storage(page, FRAGMENTS_MOTION_WORDS, 16384 - FRAGMENTS_MOTION_WORDS);
}
fn residency_main() {
    check_storage();
    let mut bank = scene_fragments::MotionBank {
        page: FRAGMENTS_MOTION.first_sector() >> 8,
        used: 0,
    };
    fragment_case::check_fragment_layout(
        &mut bank,
        FRAGMENTS_MOTION_LAYOUT.as_array(),
        FRAGMENTS_MOTION_OWNERS,
        FRAGMENTS_MOTION_WORDS,
    );
    run_catalog();
    run_parents();
    halt(0x600d);
}
