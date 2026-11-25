//! Initial collision world: source records, ordered queries and method slots.
use crate::dsl_rt::*;
mod boot_resources;
use boot_resources::*;
mod gpu_api;
use gpu_api::*;
mod gpu_window;
use gpu_window::gpu_window_asset_read;
mod static_assets;
use static_assets::AssetBytes;
mod attitude;
mod bounce;
mod bounce_check;
mod collision;
mod deformation;
mod falling;
mod ground;
mod integer;
mod lifecycle;
mod object_query;
mod ordered;
mod random;
mod sampling;
mod scene_contact;
mod scene_query;
mod scene_world;
mod scenery_contact;
mod spatial;
mod terrain;
mod trig_lookup;
static SCENE_PHASE: u16 = 0;
static SCENE_SAMPLE: u16 = 0;
static SCENE_FIELD: u16 = 0;
static SCENE_CHECKS: u16 = 0;
static SCENE_HIT_CHECKS: u16 = 0;
static SCENE_TICK_CHECKS: u16 = 0;
struct FallFixture {
    metadata: Array<u16>,
    states: Array<u16>,
}
fn mark(phase: u16) {
    unsafe {
        addr_of(&SCENE_PHASE).write(0, phase);
    }
}
fn read(dst: Array<u16>, asset: AssetBytes, offset: fix32, count: u16) {
    if !gpu_window_asset_read(
        dst,
        asset.first_sector + (offset.hi_bits() << 10) + (offset.lo_bits() >> 6),
        offset.lo_bits() & 63,
        count,
    ) {
        halt(0x7e01);
    }
}
fn compare(actual: Array<u16>, expected: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        if actual[i] != expected[i] {
            unsafe {
                addr_of(&SCENE_FIELD).write(0, i);
            }
            halt(0x8000 + i);
        }
        i += 1;
    }
}
fn main() {
    let incoming = read_dseg_page(3);
    mark(1);
    let scene = scene_query::Scene::begin(SCENE_DATA);
    if !scene.valid {
        halt(0x7e02);
    }
    let invalid = scene.method(scene.header[4u16], 0);
    let beyond = scene.method(0, 65535);
    if invalid.status != 4 || beyond.status != 2 {
        halt(0x7e03);
    }
    let data: Buf<u16, 76> = Buf::new([0; 76]);
    let mut record = scene_query::record_zero();
    let mut i = 0u16;
    mark(2);
    while i < scene.header[3u16] {
        unsafe {
            addr_of(&SCENE_SAMPLE).write(0, i);
        }
        if !scene.load(i, &mut record) {
            halt(0x7e04);
        }
        read(
            data.as_array(),
            SCENE_RECORDS,
            fmul_lo32(fix32::from_words(i, 0), integer::from_i16(76)),
            76,
        );
        compare(
            unsafe { view_of(&mut record).as_ptr().as_u16_array() },
            data.as_array(),
            76,
        );
        if read_dseg_page(3) != incoming {
            halt(0x7e05);
        }
        i += 1;
    }
    let input: Buf<u16, 64> = Buf::new([0; 64]);
    let colliders: Buf<u16, 14400> = Buf::new([0; 14400]);
    let properties: Buf<u16, 3840> = Buf::new([0; 3840]);
    let memberships: Buf<u16, 4096> = Buf::new([0; 4096]);
    let range_storage: Buf<u16, 192> = Buf::new([0; 192]);
    let mut ranges = range_storage.as_array();
    let origin: Buf<u16, 4> = Buf::new([0; 4]);
    let deformables: Buf<u16, 16> = Buf::new([0; 16]);
    i = 0;
    while i < 64 {
        ranges[i * 3] = i * 64;
        ranges[i * 3 + 2] = 32;
        i += 1;
    }
    let mut world = scene_world::World {
        colliders: colliders.as_array(),
        properties: properties.as_array(),
        count: 240,
        grid: spatial::Grid {
            page: 65535,
            origin: origin.as_array(),
            shift_x: 0,
            shift_z: 0,
            items: memberships.as_array(),
            ranges: ranges,
        },
        deformables: ordered::List {
            items: deformables.as_array(),
            count: 0,
            capacity: 8,
        },
    };
    if scene.load_world(&mut world) != 0 {
        halt(0x7e09);
    }
    mark(5);
    i = 0;
    while i < world.count {
        if !scene.load(i, &mut record) {
            halt(0x7e04);
        }
        compare(
            unsafe { world.colliders.as_ptr().add((i * 60) as i16).as_u16_array() },
            unsafe { view_of(&mut record).as_ptr().as_u16_array() },
            60,
        );
        compare(
            unsafe {
                world
                    .properties
                    .as_ptr()
                    .add((i * 16) as i16)
                    .as_u16_array()
            },
            unsafe {
                view_of(&mut record)
                    .as_ptr()
                    .as_u16_array()
                    .as_ptr()
                    .add(60)
                    .as_u16_array()
            },
            16,
        );
        i += 1;
    }
    i = 0;
    while i < 64 {
        let row = spatial::list(&world.grid, i);
        if row.count != scene.cells[i * 2 + 1] {
            halt(0x7e0a);
        }
        if row.count != 0 {
            read(
                data.as_array(),
                SCENE_DATA,
                fix32::from_words(scene.cells[i * 2], 0),
                row.count,
            );
            let mut j = 0u16;
            while j < row.count {
                if attitude::get(row.items, j) != fix32::from_words(data[j], 0) {
                    halt(0x7e0b);
                }
                j += 1;
            }
        }
        i += 1;
    }
    if world.deformables.count != scene.header[9u16] {
        halt(0x7e0c);
    }
    if world.deformables.count != 0 {
        read(
            data.as_array(),
            SCENE_DATA,
            fix32::from_words(scene.header[8u16], 0),
            world.deformables.count,
        );
    }
    i = 0;
    while i < world.deformables.count {
        if attitude::get(world.deformables.items, i) != fix32::from_words(data[i], 0) {
            halt(0x7e0d);
        }
        i += 1;
    }
    let result: Buf<u16, 46> = Buf::new([0; 46]);
    let mut count = SCENE_QUERY_COUNT;
    if SCENE_QUERY_LIMIT != 0 && SCENE_QUERY_LIMIT < count {
        count = SCENE_QUERY_LIMIT;
    }
    i = 0;
    mark(3);
    while i < count {
        unsafe {
            addr_of(&SCENE_SAMPLE).write(0, i);
        }
        read(
            input.as_array(),
            SCENE_QUERIES,
            fmul_lo32(fix32::from_words(i, 0), integer::from_i16(64)),
            64,
        );
        let values = input.as_array();
        let q = object_query::Query {
            start: values,
            end: unsafe { values.as_ptr().add(6).as_u16_array() },
            self_entity: attitude::get(values, 6),
            object_count: scene.header[3u16],
            deformable: attitude::get(values, 7) != fix32::zero(),
        };
        let mut hit = object_query::hit_zero();
        let mut j = 0u16;
        while j < 3 {
            attitude::put(
                hit.point.as_array(),
                j,
                integer::from_i16(11 * ((j + 1) as i16)),
            );
            j += 1;
        }
        attitude::put(hit.normal.as_array(), 0, integer::from_i16(-44));
        attitude::put(hit.normal.as_array(), 1, integer::from_i16(55));
        attitude::put(hit.normal.as_array(), 2, integer::from_i16(-66));
        let status = scene_world::query(&world, &q, &mut hit);
        if status != values[17u16] {
            unsafe {
                addr_of(&SCENE_FIELD).write(0, status);
            }
            halt(0x7e06);
        }
        let mut output = result.as_array();
        j = 0;
        while j < 46 {
            output[j] = 0;
            j += 1;
        }
        let raw = unsafe { view_of(&mut hit).as_ptr().as_u16_array() };
        j = 0;
        while j < 18 {
            output[j] = raw[j];
            j += 1;
        }
        if hit.object != integer::from_i16(-1) {
            let row = unsafe {
                world
                    .properties
                    .as_ptr()
                    .add((hit.object.lo_bits() * 16) as i16)
                    .as_u16_array()
            };
            j = 0;
            while j < 5 {
                attitude::put(output, 9 + j, attitude::get(row, j));
                j += 1;
            }
            j = 0;
            while j < 6 {
                attitude::put(output, 14 + j, integer::from_i16(row[10 + j] as i16));
                j += 1;
            }
            let method = scene.method(row[0u16], values[16u16]);
            attitude::put(output, 20, fix32::from_words(method.status, 0));
            attitude::put(output, 21, fix32::from_words(method.id, 0));
            attitude::put(output, 22, method.maximum);
        }
        compare(
            output,
            unsafe { values.as_ptr().add(18).as_u16_array() },
            46,
        );
        if read_dseg_page(3) != incoming {
            halt(0x7e08);
        }
        unsafe {
            addr_of(&SCENE_CHECKS).write(0, SCENE_CHECKS + 1);
        }
        i += 1;
    }
    check_hits(&scene, &mut world);
    mark(4);
    halt(0x600d);
}
// Real class-table dispatch on the same rows used by the normal scene queries.
// Four plain Fall bindings qualify this connected phase; the full owner also
// needs the remaining class bindings and ordinary world tick/drain traversal.
fn check_hits(scene: &scene_query::Scene, world: &mut scene_world::World) {
    mark(6);
    let incoming = read_dseg_page(3);
    let header: Buf<u16, 76> = Buf::new([0; 76]);
    let h = header.as_array();
    read(h, SCENE_HIT_INITIAL, fix32::zero(), 60);
    if h[0u16] != 0x4846
        || h[1u16] != 2
        || h[2u16] != 4
        || h[3u16] > 512
        || h[4u16] > 252
        || h[58u16] > 28
        || h[59u16] > 28
    {
        halt(0x7e20);
    }
    let flags: Buf<u16, 1024> = Buf::new([0; 1024]);
    let order: Buf<u16, 512> = Buf::new([0; 512]);
    let mut offset = 60u16;
    if h[3u16] != 0 {
        read(
            flags.as_array(),
            SCENE_HIT_INITIAL,
            fix32::from_words(offset, 0),
            h[3u16] * 2,
        );
    }
    offset += h[3u16] * 2;
    if h[4u16] != 0 {
        read(
            order.as_array(),
            SCENE_HIT_INITIAL,
            fix32::from_words(offset, 0),
            h[4u16] * 2,
        );
    }
    offset += h[4u16] * 2;
    let notifications: Buf<u16, 64> = Buf::new([0; 64]);
    let destruction: Buf<u16, 64> = Buf::new([0; 64]);
    if h[58u16] != 0 {
        read(
            notifications.as_array(),
            SCENE_HIT_INITIAL,
            fix32::from_words(offset, 0),
            h[58u16] * 2,
        );
    }
    offset += h[58u16] * 2;
    if h[59u16] != 0 {
        read(
            destruction.as_array(),
            SCENE_HIT_INITIAL,
            fix32::from_words(offset, 0),
            h[59u16] * 2,
        );
    }
    offset += h[59u16] * 2;
    let queues = lifecycle::Queues {
        notifications: ordered::List {
            items: notifications.as_array(),
            count: h[58u16],
            capacity: 32,
        },
        destruction: ordered::List {
            items: destruction.as_array(),
            count: h[59u16],
            capacity: 32,
        },
    };
    let ticks = lifecycle::Ticks {
        flags: flags.as_array(),
        flag_count: h[3u16],
        order: ordered::List {
            items: order.as_array(),
            count: h[4u16],
            capacity: 256,
        },
        next: integer::from_i16(-1),
    };
    let states: Buf<u16, 40> = Buf::new([0; 40]);
    let initial: Buf<u16, 14> = Buf::new([0; 14]);
    let mut i = 0u16;
    while i < 4 {
        read(
            initial.as_array(),
            SCENE_HIT_INITIAL,
            fix32::from_words(offset, 0),
            14,
        );
        let mut metadata = h;
        metadata[60 + i * 4] = initial[0u16];
        if metadata[60 + i * 4] >= world.count {
            halt(0x7e21);
        }
        let mut j = 0u16;
        let mut state_words = states.as_array();
        while j < 10 {
            state_words[i * 10 + j] = initial[1 + j];
            j += 1;
        }
        j = 0;
        while j < 3 {
            metadata[61 + i * 4 + j] = initial[11 + j];
            j += 1;
        }
        offset += 14;
        i += 1;
    }
    let current: Buf<u16, 2> = Buf::new([0; 2]);
    attitude::put(current.as_array(), 0, attitude::get(h, 7));
    let directory: Buf<u16, 64> = Buf::new([0; 64]);
    let mut dir = directory.as_array();
    i = 0;
    while i < 64 {
        dir[i] = i;
        i += 1;
    }
    let mut map = ground::World {
        terrain: terrain::Terrain::begin(AssetBytes {
            first_sector: SCENE_CELLS.first_sector,
            sector_count: 256,
            byte_len_lo: 32768,
            byte_len_hi: 0,
        }),
        cells: SCENE_CELLS,
        directory: dir,
        seed: attitude::get(h, 6),
        noise: Buf::new([0; 16]),
    };
    let mut noise = map.noise.as_array();
    i = 0;
    while i < 16 {
        noise[i] = h[16 + i];
        i += 1;
    }
    map.terrain.finish();
    let list_header: Buf<u16, 2> = Buf::new([0; 2]);
    read(
        list_header.as_array(),
        SCENE_RUNTIME_LISTS,
        fix32::zero(),
        2,
    );
    if list_header[0u16] > 256 || list_header[1u16] > 256 {
        halt(0x7e27);
    }
    let ordinary: Buf<u16, 512> = Buf::new([0; 512]);
    let special: Buf<u16, 512> = Buf::new([0; 512]);
    if list_header[0u16] != 0 {
        read(
            ordinary.as_array(),
            SCENE_RUNTIME_LISTS,
            integer::from_i16(2),
            list_header[0u16] * 2,
        );
    }
    if list_header[1u16] != 0 {
        read(
            special.as_array(),
            SCENE_RUNTIME_LISTS,
            fix32::from_words(2 + list_header[0u16] * 2, 0),
            list_header[1u16] * 2,
        );
    }
    let mut runtime = scene_world::Runtime {
        ticks: ticks,
        queues: queues,
        slot: current.as_array(),
        seed: unsafe { view_of(&mut map.seed).as_ptr().as_u16_array() },
        ordinary: ordered::List {
            items: ordinary.as_array(),
            count: list_header[0u16],
            capacity: 256,
        },
        special: ordered::List {
            items: special.as_array(),
            count: list_header[1u16],
            capacity: 256,
        },
    };
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let counters: Buf<u16, 4> = Buf::new([0; 4]);
    let previous: Buf<u16, 6> = Buf::new([0; 6]);
    let bindings: Buf<u16, 1920> = Buf::new([0; 1920]);
    let mut rows = bindings.as_array();
    i = 0;
    while i < 4 {
        let id = h[60 + i * 4];
        rows[id * 8] = 1;
        rows[id * 8 + 1] = i * 10;
        i += 1;
    }
    let record: Buf<u16, 597> = Buf::new([0; 597]);
    let result: Buf<u16, 572> = Buf::new([0; 572]);
    i = 0;
    while i < SCENE_HIT_COUNT {
        unsafe {
            addr_of(&SCENE_SAMPLE).write(0, i);
        }
        read(
            record.as_array(),
            SCENE_HITS,
            fmul_lo32(fix32::from_words(i, 0), integer::from_i16(597)),
            597,
        );
        let data = record.as_array();
        let selected = data[0u16];
        if selected >= 4 {
            halt(0x7e22);
        }
        let mut j = 0u16;
        while j < 3 {
            attitude::put(
                velocity.as_array(),
                j,
                attitude::get(unsafe { data.as_ptr().add(17).as_u16_array() }, j),
            );
            attitude::put(
                previous.as_array(),
                j,
                fmul_lo32(
                    attitude::get(unsafe { data.as_ptr().add(5).as_u16_array() }, j),
                    integer::from_i16(2),
                ) - attitude::get(unsafe { data.as_ptr().add(11).as_u16_array() }, j),
            );
            j += 1;
        }
        let mut hit = object_query::hit_zero();
        let vehicle = scenery_contact::Vehicle {
            entity: attitude::get(h, 3),
            position: unsafe { data.as_ptr().add(5).as_u16_array() },
            previous: previous.as_array(),
            base_position: unsafe { data.as_ptr().add(5).as_u16_array() },
            velocity: velocity.as_array(),
            forward: attitude::get(unsafe { data.as_ptr().add(23).as_u16_array() }, 0),
            reference: unsafe { h.as_ptr().add(40).as_u16_array() },
            external: unsafe { h.as_ptr().add(40).as_u16_array() },
            has_external: true,
            counters: counters.as_array(),
            dt: attitude::get(unsafe { data.as_ptr().add(1).as_u16_array() }, 0),
            mode: fix32::zero(),
        };
        let components = scenery_contact::Components {
            records: rows,
            state: states.as_array(),
            config: unsafe { h.as_ptr().add(32).as_u16_array() },
            fall_class: fix32::from_words(h[5u16], 0),
            minimum_turn: attitude::get(h, 4),
            maximum_turn: attitude::get(h, 5),
            work_limit: attitude::get(unsafe { data.as_ptr().add(3).as_u16_array() }, 0),
            tables: scenery_contact::Tables {
                sine: unsafe { addr_of(&SCENE_SINE).as_i16_array() },
                angle: unsafe { addr_of(&SCENE_ANGLE).as_i16_array() },
            },
        };
        let status =
            scenery_contact::respond(&vehicle, scene, world, &components, &mut runtime, &mut hit);
        let id = h[60 + selected * 4];
        if hit.object != fix32::from_words(id, 0) || status != fix32::zero() {
            halt(0x7e23);
        }
        let properties = unsafe {
            world
                .properties
                .as_ptr()
                .add((id * 16) as i16)
                .as_u16_array()
        };
        let state = unsafe {
            states
                .as_array()
                .as_ptr()
                .add((selected * 10) as i16)
                .as_u16_array()
        };
        let output = result.as_array();
        attitude::put(output, 0, integer::from_i16(1));
        let raw = unsafe { view_of(&mut hit).as_ptr().as_u16_array() };
        j = 0;
        while j < 9 {
            attitude::put(output, 1 + j, attitude::get(raw, j));
            j += 1;
        }
        j = 0;
        while j < 3 {
            attitude::put(output, 10 + j, attitude::get(velocity.as_array(), j));
            j += 1;
        }
        attitude::put(output, 13, attitude::get(counters.as_array(), 0));
        attitude::put(output, 14, attitude::get(counters.as_array(), 1));
        j = 0;
        while j < 5 {
            attitude::put(output, 15 + j, attitude::get(state, j));
            attitude::put(output, 20 + j, attitude::get(properties, j));
            j += 1;
        }
        attitude::put(output, 25, attitude::get(current.as_array(), 0));
        attitude::put(output, 26, attitude::get(runtime.seed, 0));
        attitude::put(
            output,
            27,
            attitude::get(flags.as_array(), hit.entity.lo_bits()),
        );
        attitude::put(output, 28, fix32::from_words(runtime.ticks.order.count, 0));
        attitude::put(output, 29, fix32::zero());
        j = 0;
        while j < 256 {
            let mut entity = integer::from_i16(-1);
            if j < runtime.ticks.order.count {
                entity = attitude::get(runtime.ticks.order.items, j);
            }
            attitude::put(output, 30 + j, entity);
            j += 1;
        }
        compare(output, unsafe { data.as_ptr().add(25).as_u16_array() }, 572);
        if read_dseg_page(3) != incoming {
            halt(0x7e26);
        }
        unsafe {
            addr_of(&SCENE_HIT_CHECKS).write(0, SCENE_HIT_CHECKS + 1);
        }
        i += 1;
    }
    let fixture = FallFixture {
        metadata: h,
        states: states.as_array(),
    };
    check_fall_ticks(scene, world, &mut runtime, &mut map, &fixture);
    bounce_check::run(scene, world, &mut runtime, &mut map);
}
fn check_fall_ticks(
    scene: &scene_query::Scene,
    world: &mut scene_world::World,
    runtime: &mut scene_world::Runtime,
    map: &mut ground::World,
    fixture: &FallFixture,
) {
    mark(7);
    let h = fixture.metadata;
    let states = fixture.states;
    let current = runtime.slot;
    let incoming = read_dseg_page(3);
    let record: Buf<u16, 1377> = Buf::new([0; 1377]);
    let result: Buf<u16, 1374> = Buf::new([0; 1374]);
    let mut i = 0u16;
    while i < SCENE_FALL_TICK_COUNT {
        unsafe {
            addr_of(&SCENE_SAMPLE).write(0, i);
        }
        read(
            record.as_array(),
            SCENE_FALL_TICKS,
            fmul_lo32(fix32::from_words(i, 0), integer::from_i16(1377)),
            1377,
        );
        let data = record.as_array();
        let selected = data[0u16];
        if selected >= 4 {
            halt(0x7e30);
        }
        let id = h[60 + selected * 4];
        let properties = unsafe {
            world
                .properties
                .as_ptr()
                .add((id * 16) as i16)
                .as_u16_array()
        };
        let collider = unsafe {
            world
                .colliders
                .as_ptr()
                .add((id * 60) as i16)
                .as_u16_array()
        };
        let state = unsafe { states.as_ptr().add((selected * 10) as i16).as_u16_array() };
        let method = scene.method(properties[0u16], 12);
        if method.status != 1 || method.id != 65 {
            halt(0x7e31);
        }
        let step = falling::Step {
            id: fix32::from_words(id, 0),
            dt: attitude::get(unsafe { data.as_ptr().add(1).as_u16_array() }, 0),
            limit: integer::from_i16(10000),
            cell: spatial::Cell {
                x: h[61 + selected * 4] as i16,
                z: h[62 + selected * 4],
                present: h[63 + selected * 4] != 0,
            },
            vehicle_basis: unsafe { h.as_ptr().add(40).as_u16_array() },
            peer_valid: attitude::get(state, 4) == attitude::get(h, 3),
            effect_active: false,
            tuning: unsafe { h.as_ptr().add(32).as_u16_array() },
            sine: unsafe { addr_of(&SCENE_SINE).as_i16_array() },
            angle: unsafe { addr_of(&SCENE_ANGLE).as_i16_array() },
        };
        let saved = attitude::get(current, 0);
        attitude::put(current, 0, integer::from_i16(12));
        let status = falling::tick(state, world, &step, runtime, map);
        map.terrain.finish();
        if status != 0 {
            halt(0x7e40 + status);
        }
        attitude::put(current, 0, saved);
        let output = result.as_array();
        attitude::put(output, 0, fix32::from_words(status, 0));
        let mut j = 0u16;
        while j < 5 {
            attitude::put(output, 1 + j, attitude::get(state, j));
            attitude::put(output, 6 + j, attitude::get(properties, j));
            j += 1;
        }
        j = 0;
        while j < 30 {
            attitude::put(output, 11 + j, attitude::get(collider, j));
            j += 1;
        }
        attitude::put(output, 41, attitude::get(current, 0));
        attitude::put(output, 42, map.seed);
        attitude::put(
            output,
            43,
            attitude::get(runtime.ticks.flags, attitude::get(collider, 0).lo_bits()),
        );
        attitude::put(output, 44, fix32::from_words(runtime.ticks.order.count, 0));
        j = 0;
        while j < 256 {
            let mut entity = integer::from_i16(-1);
            if j < runtime.ticks.order.count {
                entity = attitude::get(runtime.ticks.order.items, j);
            }
            attitude::put(output, 45 + j, entity);
            j += 1;
        }
        let mut which = 0u16;
        while which < 2 {
            let mut list = ordered::List {
                items: runtime.queues.notifications.items,
                count: runtime.queues.notifications.count,
                capacity: runtime.queues.notifications.capacity,
            };
            if which == 1 {
                list.items = runtime.queues.destruction.items;
                list.count = runtime.queues.destruction.count;
                list.capacity = runtime.queues.destruction.capacity;
            }
            attitude::put(output, 301 + which * 33, fix32::from_words(list.count, 0));
            j = 0;
            while j < 32 {
                let mut entity = integer::from_i16(-1);
                if j < list.count {
                    entity = attitude::get(list.items, j);
                }
                attitude::put(output, 302 + which * 33 + j, entity);
                j += 1;
            }
            which += 1;
        }
        let mut cursor = 0u16;
        let mut cell = 0u16;
        while cell < 64 {
            let row = spatial::list(&world.grid, cell);
            attitude::put(output, 367 + cell, fix32::from_words(row.count, 0));
            j = 0;
            while j < row.count {
                if cursor >= 256 {
                    halt(0x7e32);
                }
                attitude::put(output, 431 + cursor, attitude::get(row.items, j));
                cursor += 1;
                j += 1;
            }
            cell += 1;
        }
        while cursor < 256 {
            attitude::put(output, 431 + cursor, integer::from_i16(-1));
            cursor += 1;
        }
        compare(output, unsafe { data.as_ptr().add(3).as_u16_array() }, 1374);
        if read_dseg_page(3) != incoming {
            halt(0x7e33);
        }
        unsafe {
            addr_of(&SCENE_TICK_CHECKS).write(0, SCENE_TICK_CHECKS + 1);
        }
        i += 1;
    }
}
