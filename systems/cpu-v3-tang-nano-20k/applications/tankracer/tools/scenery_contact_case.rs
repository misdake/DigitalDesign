fn scenery_snapshot(
    out: Array<u16>,
    base: u16,
    v: &scenery_contact::Vehicle,
    world: &scene_world::World,
    components: &scenery_contact::Components,
    runtime: &scene_world::Runtime,
) {
    let mut i = 0u16;
    while i < 3 {
        attitude::put(out, base + 1 + i, attitude::get(v.velocity, i));
        i += 1;
    }
    attitude::put(out, base + 4, attitude::get(v.counters, 0));
    attitude::put(out, base + 5, attitude::get(v.counters, 1));
    attitude::put(out, base + 6, attitude::get(runtime.slot, 0));
    attitude::put(out, base + 7, attitude::get(runtime.seed, 0));
    i = 0;
    while i < 30 {
        attitude::put(out, base + 8 + i, attitude::get(world.colliders, i));
        i += 1;
    }
    i = 0;
    while i < 5 {
        attitude::put(out, base + 38 + i, attitude::get(world.properties, i));
        i += 1;
    }
    i = 0;
    while i < 6 {
        attitude::put(
            out,
            base + 43 + i,
            integer::from_i16(world.properties[10 + i] as i16),
        );
        i += 1;
    }
    i = 0;
    while i < 11 {
        attitude::put(out, base + 49 + i, attitude::get(components.state, i));
        i += 1;
    }
    i = 0;
    while i < 3 {
        attitude::put(
            out,
            base + 60 + i,
            integer::from_i16(components.records[4 + i] as i16),
        );
        i += 1;
    }
    let entity = attitude::get(world.colliders, 0).lo_bits();
    let mut flag = fix32::zero();
    if entity < runtime.ticks.flag_count {
        flag = attitude::get(runtime.ticks.flags, entity);
    }
    attitude::put(out, base + 63, flag);
    attitude::put(out, base + 64, runtime.ticks.next);
    attitude::put(
        out,
        base + 65,
        fix32::from_words(runtime.ticks.order.count, 0),
    );
    i = 0;
    while i < 32 {
        let mut id = integer::from_i16(-1);
        if i < runtime.ticks.order.count {
            id = attitude::get(runtime.ticks.order.items, i);
        }
        attitude::put(out, base + 66 + i, id);
        i += 1;
    }
    attitude::put(
        out,
        base + 98,
        fix32::from_words(runtime.queues.notifications.count, 0),
    );
    attitude::put(
        out,
        base + 99,
        fix32::from_words(runtime.queues.destruction.count, 0),
    );
    i = 0;
    while i < 16 {
        let mut id = integer::from_i16(-1);
        if i < runtime.queues.notifications.count {
            id = attitude::get(runtime.queues.notifications.items, i);
        }
        attitude::put(out, base + 100 + i, id);
        id = integer::from_i16(-1);
        if i < runtime.queues.destruction.count {
            id = attitude::get(runtime.queues.destruction.items, i);
        }
        attitude::put(out, base + 116 + i, id);
        i += 1;
    }
    attitude::put(
        out,
        base + 132,
        fix32::from_words(runtime.ordinary.count, 0),
    );
    attitude::put(out, base + 133, fix32::from_words(runtime.special.count, 0));
    i = 0;
    while i < 64 {
        let list = spatial::list(&world.grid, i);
        attitude::put(out, base + 134 + i, fix32::from_words(list.count, 0));
        i += 1;
    }
}
fn run_car_scenery(input: Array<u16>, out: Array<u16>, sine: Array<i16>, angle: Array<i16>) {
    let scene = scene_query::Scene::begin(static_assets::AssetBytes {
        first_sector: 40960,
        sector_count: attitude::get(input, 659).lo_bits(),
        byte_len_lo: attitude::get(input, 657).lo_bits(),
        byte_len_hi: attitude::get(input, 658).lo_bits(),
    });
    if !scene.valid {
        halt(0x7f13);
    }
    let geometry: Buf<u16, 60> = Buf::new([0; 60]);
    let mut collider = geometry.as_array();
    let properties: Buf<u16, 16> = Buf::new([0; 16]);
    let mut props = properties.as_array();
    let states: Buf<u16, 22> = Buf::new([0; 22]);
    let mut state = states.as_array();
    let configs: Buf<u16, 22> = Buf::new([0; 22]);
    let mut cfg = configs.as_array();
    let records: Buf<u16, 8> = Buf::new([0; 8]);
    let mut row = records.as_array();
    let mut i = 0u16;
    while i < 60 {
        collider[i] = input[86 + i];
        i += 1;
    }
    i = 0;
    while i < 5 {
        attitude::put(props, i, attitude::get(input, 73 + i));
        i += 1;
    }
    i = 0;
    while i < 6 {
        props[10 + i] = attitude::get(input, 78 + i).lo_bits();
        i += 1;
    }
    i = 0;
    while i < 22 {
        state[i] = input[168 + i];
        cfg[i] = input[190 + i];
        i += 1;
    }
    row[0u16] = attitude::get(input, 0).lo_bits() & 3;
    row[3u16] = attitude::get(input, 106).lo_bits();
    i = 0;
    while i < 3 {
        row[4 + i] = attitude::get(input, 107 + i).lo_bits();
        i += 1;
    }
    let mut components = scenery_contact::Components {
        records: row,
        state: state,
        config: cfg,
        fall_class: integer::from_i16(5),
        minimum_turn: attitude::get(input, 10),
        maximum_turn: attitude::get(input, 11),
        work_limit: attitude::get(input, 1276),
        tables: scenery_contact::Tables {
            sine: sine,
            angle: angle,
        },
    };
    let flags: Buf<u16, 1024> = Buf::new([0; 1024]);
    let mut fl = flags.as_array();
    let order: Buf<u16, 64> = Buf::new([0; 64]);
    let mut ord = order.as_array();
    let notifications: Buf<u16, 32> = Buf::new([0; 32]);
    let mut notices = notifications.as_array();
    let destruction: Buf<u16, 32> = Buf::new([0; 32]);
    let mut dead = destruction.as_array();
    let classes: Buf<u16, 512> = Buf::new([0; 512]);
    let mut cl = classes.as_array();
    i = 0;
    while i < 1024 {
        fl[i] = input[226 + i];
        i += 1;
    }
    i = 0;
    while i < 64 {
        ord[i] = input[1250 + i];
        i += 1;
    }
    i = 0;
    while i < 32 {
        notices[i] = input[1324 + i];
        dead[i] = input[1356 + i];
        i += 1;
    }
    i = 0;
    while i < 512 {
        cl[i] = attitude::get(input, 694 + i).lo_bits();
        i += 1;
    }
    let ordinary: Buf<u16, 2> = Buf::new([0; 2]);
    let special: Buf<u16, 2> = Buf::new([0; 2]);
    let ranges: Buf<u16, 192> = Buf::new([0; 192]);
    let mut buckets = ranges.as_array();
    let grid_items: Buf<u16, 512> = Buf::new([0; 512]);
    i = 0;
    while i < 64 {
        buckets[i * 3] = i * 8;
        buckets[i * 3 + 1] = attitude::get(input, 1206 + i).lo_bits();
        buckets[i * 3 + 2] = 4;
        i += 1;
    }
    let mut world = scene_world::World {
        colliders: collider,
        properties: props,
        count: 1,
        fixed_count: 1,
        dynamic_page: 0,
        dynamic_capacity: 0,
        grid: spatial::Grid {
            page: 65535,
            origin: unsafe { input.as_ptr().add(2544).as_u16_array() },
            shift_x: attitude::get(input, 1274).lo_bits(),
            shift_z: attitude::get(input, 1275).lo_bits(),
            items: grid_items.as_array(),
            ranges: buckets,
        },
        deformables: ordered::List {
            items: ordinary.as_array(),
            count: 0,
            capacity: 0,
        },
    };
    let directory: Buf<u16, 64> = Buf::new([0; 64]);
    let mut dir = directory.as_array();
    i = 0;
    while i < 64 {
        dir[i] = i;
        i += 1;
    }
    let mut map = ground::World {
        terrain: terrain::Terrain::begin(static_assets::AssetBytes {
            first_sector: 16384,
            sector_count: 256,
            byte_len_lo: 32768,
            byte_len_hi: 0,
        }),
        cells: static_assets::AssetBytes {
            first_sector: 16384,
            sector_count: 16384,
            byte_len_lo: 0,
            byte_len_hi: 32,
        },
        directory: dir,
        seed: attitude::get(input, 6),
        noise: Buf::new([0; 16]),
        shield_page: 65535,
        shield: unsafe { Ptr::from_addr(0).as_u16_array() },
    };
    let slot: Buf<u16, 2> = Buf::new([0; 2]);
    attitude::put(slot.as_array(), 0, attitude::get(input, 9));
    let mut runtime = scene_world::Runtime {
        ticks: lifecycle::Ticks {
            flag_words: 2,
            flags: fl,
            flag_count: attitude::get(input, 110).lo_bits(),
            order: ordered::List {
                items: ord,
                count: attitude::get(input, 111).lo_bits(),
                capacity: 32,
            },
            next: attitude::get(input, 112),
        },
        slot: slot.as_array(),
        seed: unsafe { view_of(&mut map.seed).as_ptr().as_u16_array() },
        queues: lifecycle::Queues {
            notifications: ordered::List {
                items: notices,
                count: attitude::get(input, 660).lo_bits(),
                capacity: 16,
            },
            destruction: ordered::List {
                items: dead,
                count: attitude::get(input, 661).lo_bits(),
                capacity: 16,
            },
        },
        ordinary: ordered::List {
            items: ordinary.as_array(),
            count: attitude::get(input, 1270).lo_bits(),
            capacity: 1,
        },
        special: ordered::List {
            items: special.as_array(),
            count: attitude::get(input, 1271).lo_bits(),
            capacity: 1,
        },
    };
    let counters: Buf<u16, 4> = Buf::new([0; 4]);
    attitude::put(counters.as_array(), 0, attitude::get(input, 7));
    attitude::put(counters.as_array(), 1, attitude::get(input, 8));
    let v = scenery_contact::Vehicle {
        entity: attitude::get(input, 4),
        position: unsafe { input.as_ptr().add(24).as_u16_array() },
        previous: unsafe { input.as_ptr().add(30).as_u16_array() },
        base_position: unsafe { input.as_ptr().add(36).as_u16_array() },
        velocity: unsafe { input.as_ptr().add(42).as_u16_array() },
        forward: attitude::get(input, 5),
        reference: unsafe { input.as_ptr().add(48).as_u16_array() },
        external: unsafe { input.as_ptr().add(66).as_u16_array() },
        has_external: attitude::get(input, 42) != fix32::zero(),
        counters: counters.as_array(),
        dt: attitude::get(input, 2),
        mode: attitude::get(input, 3),
    };
    let mut action = 0u16;
    while action < attitude::get(input, 1).lo_bits() {
        let mut status = fix32::zero();
        if attitude::get(input, 0).lo_bits() & 4 != 0 {
            let mut pass = lifecycle::begin(&runtime.ticks, components.work_limit);
            loop {
                let progress =
                    lifecycle::prepare(&mut runtime.ticks, &mut pass, components.work_limit);
                if progress == 0 {
                    break;
                }
                if progress != 1 {
                    halt(0x7f15);
                }
                if pass.current == v.entity {
                    let saved = attitude::get(runtime.slot, 0);
                    attitude::put(runtime.slot, 0, integer::from_i16(12));
                    let mut hit = object_query::hit_zero();
                    status = scenery_contact::respond(
                        &v,
                        &scene,
                        &world,
                        &components,
                        &mut runtime,
                        &mut hit,
                    );
                    if status == fix32::zero() {
                        attitude::put(runtime.slot, 0, saved);
                    }
                } else if pass.current == attitude::get(collider, 0) {
                    let frame = scenery_contact::Frame {
                        object: 0,
                        dt: integer::from_i16(33),
                        peer_basis: v.reference,
                        peer_valid: true,
                    };
                    status = scenery_contact::tick(
                        &scene,
                        &mut world,
                        &components,
                        &mut runtime,
                        &mut map,
                        &frame,
                    );
                } else {
                    halt(0x7f16);
                }
                if status != fix32::zero() {
                    break;
                }
                lifecycle::resume(&mut pass, &runtime.ticks);
            }
            if status == fix32::zero() {
                status =
                    scenery_contact::drain(&scene, &mut runtime, cl, 512, components.work_limit);
            }
        } else if action == 0 {
            let mut hit = object_query::hit_zero();
            status =
                scenery_contact::respond(&v, &scene, &world, &components, &mut runtime, &mut hit);
        } else {
            components.work_limit = attitude::get(input, 1277);
            if action == 4 {
                let s = lifecycle::queue(
                    &mut runtime.queues,
                    &mut runtime.ticks,
                    attitude::get(collider, 0),
                    false,
                    components.work_limit,
                );
                if s != 0 {
                    halt(0x7f14);
                }
            } else {
                let frame = scenery_contact::Frame {
                    object: 0,
                    dt: integer::from_i16(33),
                    peer_basis: v.reference,
                    peer_valid: true,
                };
                status = scenery_contact::tick(
                    &scene,
                    &mut world,
                    &components,
                    &mut runtime,
                    &mut map,
                    &frame,
                );
            }
            if status == fix32::zero() {
                status =
                    scenery_contact::drain(&scene, &mut runtime, cl, 512, components.work_limit);
            }
        }
        attitude::put(out, action * 198, status);
        scenery_snapshot(out, action * 198, &v, &world, &components, &runtime);
        action += 1;
    }
    i = 0;
    while i < 512 {
        attitude::put(out, action * 198 + i, attitude::get(fl, i));
        i += 1;
    }
    map.terrain.finish();
}
