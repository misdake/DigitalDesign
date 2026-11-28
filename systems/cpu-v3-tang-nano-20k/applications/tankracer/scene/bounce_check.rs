//! Normal asset loading and connected updates for authored Bounce objects.
use crate::dsl_rt::*;
use crate::{
    attitude, bounce, ground, integer, object_query, ordered, scene_contact, scene_query,
    scene_world, spatial,
};
use crate::{
    compare, mark, read, SCENE_ANGLE, SCENE_BOUNCE_INITIAL, SCENE_BOUNCE_UPDATES,
    SCENE_BOUNCE_UPDATE_COUNT, SCENE_SAMPLE, SCENE_SINE,
};
static BOUNCE_CHECKS: u16 = 0;
pub fn run(
    scene: &scene_query::Scene,
    world: &mut scene_world::World,
    runtime: &mut scene_world::Runtime,
    map: &mut ground::World,
) {
    mark(8);
    let incoming = read_dseg_page(3);
    let header: Buf<u16, 47> = Buf::new([0; 47]);
    let h = header.as_array();
    read(h, SCENE_BOUNCE_INITIAL, fix32::zero(), 47);
    if h[0u16] != 0x424e || h[1u16] != 1 || h[2u16] > 32 {
        halt(0x7e50);
    }
    let components: Buf<u16, 1600> = Buf::new([0; 1600]);
    if h[2u16] != 0 {
        read(
            components.as_array(),
            SCENE_BOUNCE_INITIAL,
            integer::from_i16(47),
            h[2u16] * 50,
        );
    }
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let counters: Buf<u16, 4> = Buf::new([0; 4]);
    attitude::put(counters.as_array(), 0, attitude::get(h, 20));
    attitude::put(counters.as_array(), 1, attitude::get(h, 21));
    let env = scene_contact::Environment {
        mode: fix32::zero(),
        breakable_class: fix32::from_words(h[44u16], 0),
        sine: unsafe { addr_of(&SCENE_SINE).as_i16_array() },
        angle: unsafe { addr_of(&SCENE_ANGLE).as_i16_array() },
    };
    let record: Buf<u16, 2474> = Buf::new([0; 2474]);
    let result: Buf<u16, 2450> = Buf::new([0; 2450]);
    let mut i = 0u16;
    while i < SCENE_BOUNCE_UPDATE_COUNT {
        unsafe {
            addr_of(&SCENE_SAMPLE).write(0, i);
        }
        read(
            record.as_array(),
            SCENE_BOUNCE_UPDATES,
            fmul_lo32(fix32::from_words(i, 0), integer::from_i16(2474)),
            2474,
        );
        let data = record.as_array();
        let selected = data[1u16];
        if selected >= h[2u16] {
            halt(0x7e51);
        }
        let row = unsafe {
            components
                .as_array()
                .as_ptr()
                .add((selected * 50) as i16)
                .as_u16_array()
        };
        let id = row[0u16];
        if id >= world.count {
            halt(0x7e52);
        }
        let collider = unsafe {
            world
                .colliders
                .as_ptr()
                .add((id * 60) as i16)
                .as_u16_array()
        };
        let properties = unsafe {
            world
                .properties
                .as_ptr()
                .add((id * 16) as i16)
                .as_u16_array()
        };
        let state = unsafe { row.as_ptr().add(1).as_u16_array() };
        let mut motion = bounce::Motion {
            state: state,
            properties: properties,
            collider: collider,
            reference_alias: row[46u16] != 0,
            cell: spatial::Cell {
                x: row[43u16] as i16,
                z: row[44u16],
                present: row[45u16] != 0,
            },
            impact_threshold: attitude::get(unsafe { row.as_ptr().add(48).as_u16_array() }, 0),
        };
        let config = bounce::Config {
            data: unsafe { row.as_ptr().add(23).as_u16_array() },
            sine: env.sine,
            angle: env.angle,
        };
        let frame = bounce::Frame {
            entity: attitude::get(collider, 0),
            limit: integer::from_i16(10000),
            dt: attitude::get(data, 1),
            id: fix32::from_words(id, 0),
            has_mesh: row[47u16] != 0,
        };
        let mut hit = object_query::hit_zero();
        let mut phase = 0u16;
        if data[0u16] == 1 {
            let mut j = 0u16;
            let mut v = velocity.as_array();
            while j < 6 {
                v[j] = data[16 + j];
                j += 1;
            }
            let q = object_query::Query {
                start: unsafe { data.as_ptr().add(4).as_u16_array() },
                end: unsafe { data.as_ptr().add(10).as_u16_array() },
                self_entity: attitude::get(unsafe { h.as_ptr().add(45).as_u16_array() }, 0),
                object_count: world.count,
                deformable: false,
            };
            phase = scene_contact::obstacles(world, &q, v, counters.as_array(), &env, &mut hit);
            if phase != 1 || hit.object != frame.id {
                halt(0x7e53);
            }
            let method = scene.method(properties[0u16], 24);
            let activation = scene.method(properties[0u16], 4);
            if method.status != 1 || method.id != 79 || activation.status != 1 || activation.id != 2
            {
                halt(0x7e54);
            }
            let incoming_hit = bounce::Hit {
                position: q.start,
                velocity: v,
                forward: attitude::get(data, 11),
                reference: unsafe { h.as_ptr().add(3).as_u16_array() },
                external: unsafe { h.as_ptr().add(21).as_u16_array() },
                has_external: h[39u16] != 0,
            };
            let saved = attitude::get(runtime.slot, 0);
            attitude::put(runtime.slot, 0, integer::from_i16(24));
            let status = bounce::hit(&mut motion, &config, &incoming_hit, runtime, &frame);
            if status != 0 {
                halt(0x7e60 + status);
            }
            attitude::put(runtime.slot, 0, saved);
        } else if data[0u16] == 2 {
            let method = scene.method(properties[0u16], 12);
            if method.status != 1 || method.id != 80 {
                halt(0x7e55);
            }
            let saved = attitude::get(runtime.slot, 0);
            attitude::put(runtime.slot, 0, integer::from_i16(12));
            let status = bounce::tick(&mut motion, &config, runtime, &frame, &mut world.grid, map);
            if status != 0 {
                halt(0x7e70 + status);
            }
            attitude::put(runtime.slot, 0, saved);
        } else {
            halt(0x7e56);
        }
        map.terrain.finish();
        let mut metadata = row;
        metadata[43u16] = motion.cell.x as u16;
        metadata[44u16] = motion.cell.z;
        metadata[45u16] = 0;
        if motion.cell.present {
            metadata[45u16] = 1;
        }
        let output = result.as_array();
        attitude::put(output, 0, fix32::from_words(phase, 0));
        let raw = unsafe { view_of(&mut hit).as_ptr().as_u16_array() };
        let mut j = 0u16;
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
        while j < 11 {
            attitude::put(output, 15 + j, attitude::get(state, j));
            j += 1;
        }
        attitude::put(output, 26, integer::from_i16(motion.cell.x));
        attitude::put(output, 27, fix32::from_words(motion.cell.z, 0));
        attitude::put(output, 28, fix32::zero());
        if motion.cell.present {
            attitude::put(output, 28, integer::from_i16(1));
        }
        j = 0;
        while j < 5 {
            attitude::put(output, 29 + j, attitude::get(properties, j));
            j += 1;
        }
        j = 0;
        while j < 30 {
            attitude::put(output, 34 + j, attitude::get(collider, j));
            j += 1;
        }
        attitude::put(output, 64, attitude::get(runtime.slot, 0));
        attitude::put(output, 65, attitude::get(runtime.seed, 0));
        attitude::put(output, 66, map.seed);
        attitude::put(
            output,
            67,
            attitude::get(runtime.ticks.flags, frame.entity.lo_bits()),
        );
        let mut which = 0u16;
        let mut base = 68u16;
        while which < 5 {
            let mut list = ordered::List {
                items: runtime.ticks.order.items,
                count: runtime.ticks.order.count,
                capacity: 256,
            };
            let mut capacity = 256u16;
            if which == 1 {
                list.items = runtime.ordinary.items;
                list.count = runtime.ordinary.count;
            }
            if which == 2 {
                list.items = runtime.special.items;
                list.count = runtime.special.count;
            }
            if which == 3 {
                list.items = runtime.queues.notifications.items;
                list.count = runtime.queues.notifications.count;
                capacity = 32;
            }
            if which == 4 {
                list.items = runtime.queues.destruction.items;
                list.count = runtime.queues.destruction.count;
                capacity = 32;
            }
            if list.count > capacity {
                halt(0x7e57);
            }
            attitude::put(output, base, fix32::from_words(list.count, 0));
            j = 0;
            while j < capacity {
                let mut entity = integer::from_i16(-1);
                if j < list.count {
                    entity = attitude::get(list.items, j);
                }
                attitude::put(output, base + 1 + j, entity);
                j += 1;
            }
            base += capacity + 1;
            which += 1;
        }
        let mut cursor = 0u16;
        let mut cell = 0u16;
        while cell < 64 {
            let list = spatial::list(&world.grid, cell);
            attitude::put(output, 905 + cell, fix32::from_words(list.count, 0));
            j = 0;
            while j < list.count {
                if cursor >= 256 {
                    halt(0x7e58);
                }
                attitude::put(output, 969 + cursor, attitude::get(list.items, j));
                cursor += 1;
                j += 1;
            }
            cell += 1;
        }
        while cursor < 256 {
            attitude::put(output, 969 + cursor, integer::from_i16(-1));
            cursor += 1;
        }
        compare(
            output,
            unsafe { data.as_ptr().add(24).as_u16_array() },
            2450,
        );
        if read_dseg_page(3) != incoming {
            halt(0x7e59);
        }
        unsafe {
            addr_of(&BOUNCE_CHECKS).write(0, BOUNCE_CHECKS + 1);
        }
        i += 1;
    }
}
