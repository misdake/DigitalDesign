fn bounce_tick_list(out: Array<u16>, base: u16, list: &ordered::List) {
    attitude::put(out, base, fix32::from_words(list.count, 0));
    let mut i = 0u16;
    while i < 4 {
        let mut id = integer::from_i16(-1);
        if i < list.count {
            id = attitude::get(list.items, i);
        }
        attitude::put(out, base + 1 + i, id);
        i += 1;
    }
}
fn run_bounce_tick(input: Array<u16>, out: Array<u16>, sine: Array<i16>, angle: Array<i16>) {
    let state_buffer: Buf<u16, 22> = Buf::new([0; 22]);
    let properties: Buf<u16, 16> = Buf::new([0; 16]);
    let mut state = state_buffer.as_array();
    let collider_buffer: Buf<u16, 60> = Buf::new([0; 60]);
    let mut collider = collider_buffer.as_array();
    let mut i = 0u16;
    while i < 22 {
        state[i] = input[i];
        i += 1;
    }
    attitude::put(properties.as_array(), 3, attitude::get(input, 11));
    // Contact threshold must not control settling. Keep the two values distinct.
    attitude::put(properties.as_array(), 2, integer::from_i16(-12345));
    i = 0;
    while i < 60 {
        collider[i] = input[24 + i];
        i += 1;
    }
    let id = attitude::get(input, 59);
    let other = attitude::get(input, 60);
    let entity = attitude::get(input, 58);
    let items: Buf<u16, 512> = Buf::new([0; 512]);
    let ranges_buffer: Buf<u16, 192> = Buf::new([0; 192]);
    let mut ranges = ranges_buffer.as_array();
    i = 0;
    while i < 64 {
        ranges[i * 3] = i * 8;
        ranges[i * 3 + 1] = 2;
        ranges[i * 3 + 2] = 4;
        attitude::put(items.as_array(), i * 4, other);
        attitude::put(items.as_array(), i * 4 + 1, id);
        i += 1;
    }
    let mut grid = spatial::Grid {
        page: 65535,
        origin: unsafe { input.as_ptr().add(124).as_u16_array() },
        shift_x: attitude::get(input, 64).lo_bits(),
        shift_z: attitude::get(input, 65).lo_bits(),
        items: items.as_array(),
        ranges: ranges,
    };
    let inserted = spatial::insert(
        &mut grid,
        id,
        unsafe { collider.as_ptr().add(2).as_u16_array() },
        true,
        attitude::get(collider, 28).lo_bits(),
    );
    if inserted.status != 0 {
        halt(0x7e20);
    }
    let mut motion = bounce::Motion {
        state: state,
        properties: properties.as_array(),
        collider: collider,
        reference_alias: attitude::get(input, 56) != fix32::zero(),
        cell: spatial::Cell {
            x: attitude::get(input, 52).lo_bits() as i16,
            z: attitude::get(input, 53).lo_bits(),
            present: attitude::get(input, 54) != fix32::zero(),
        },
        impact_threshold: attitude::get(input, 55),
    };
    let config = bounce::Config {
        data: unsafe { input.as_ptr().add(84).as_u16_array() },
        sine: sine,
        angle: angle,
    };
    let flags: Buf<u16, 1024> = Buf::new([0; 1024]);
    attitude::put(flags.as_array(), entity.lo_bits(), integer::from_i16(16));
    let order: Buf<u16, 8> = Buf::new([0; 8]);
    attitude::put(order.as_array(), 0, entity);
    let ordinary: Buf<u16, 8> = Buf::new([0; 8]);
    attitude::put(ordinary.as_array(), 0, other);
    attitude::put(ordinary.as_array(), 1, id);
    let special: Buf<u16, 8> = Buf::new([0; 8]);
    attitude::put(special.as_array(), 0, id);
    attitude::put(special.as_array(), 1, other);
    let notifications: Buf<u16, 8> = Buf::new([0; 8]);
    let destruction: Buf<u16, 8> = Buf::new([0; 8]);
    let slot: Buf<u16, 2> = Buf::new([24, 0]);
    let seed: Buf<u16, 2> = Buf::new([0; 2]);
    attitude::put(seed.as_array(), 0, attitude::get(input, 69));
    let mut runtime = scene_world::Runtime {
        ticks: lifecycle::Ticks {
            flag_words: 2,
            flags: flags.as_array(),
            flag_count: 512,
            order: ordered::List {
                items: order.as_array(),
                count: 1,
                capacity: 4,
            },
            next: integer::from_i16(-1),
        },
        slot: slot.as_array(),
        seed: seed.as_array(),
        queues: lifecycle::Queues {
            notifications: ordered::List {
                items: notifications.as_array(),
                count: 0,
                capacity: 4,
            },
            destruction: ordered::List {
                items: destruction.as_array(),
                count: 0,
                capacity: 4,
            },
        },
        ordinary: ordered::List {
            items: ordinary.as_array(),
            count: 2,
            capacity: 4,
        },
        special: ordered::List {
            items: special.as_array(),
            count: 2,
            capacity: 4,
        },
    };
    let directory: Buf<u16, 64> = Buf::new([0; 64]);
    let mut dir = directory.as_array();
    i = 0;
    while i < 64 {
        dir[i] = i;
        i += 1;
    }
    let mut map = ground::World::begin(
        static_assets::AssetBytes {
            first_sector: 16384,
            sector_count: 16384,
            byte_len_lo: 0,
            byte_len_hi: 32,
        },
        fix32::zero(),
        dir,
    );
    let mut step = 0u16;
    while step < 3 {
        let frame = bounce::Frame {
            entity: entity,
            limit: attitude::get(input, 61),
            dt: attitude::get(input, 66 + step),
            id: id,
            has_mesh: attitude::get(input, 57) != fix32::zero(),
        };
        let saved = attitude::get(slot.as_array(), 0);
        attitude::put(slot.as_array(), 0, integer::from_i16(12));
        let status = bounce::tick(
            &mut motion,
            &config,
            &mut runtime,
            &frame,
            &mut grid,
            &mut map,
        );
        let mut code = fix32::zero();
        if status == 0 {
            attitude::put(slot.as_array(), 0, saved);
        } else if status == 3 {
            code = integer::from_i16(-3);
        } else if status == 4 || status == 7 {
            code = integer::from_i16(-4);
        } else if status == 5 {
            code = integer::from_i16(-2);
        } else {
            code = integer::from_i16(-1);
        }
        let base = step * 330;
        i = 0;
        while i < 11 {
            attitude::put(out, base + i, attitude::get(state, i));
            i += 1;
        }
        attitude::put(out, base + 11, attitude::get(properties.as_array(), 3));
        i = 0;
        while i < 30 {
            attitude::put(out, base + 12 + i, attitude::get(collider, i));
            i += 1;
        }
        attitude::put(out, base + 42, integer::from_i16(motion.cell.x));
        attitude::put(out, base + 43, fix32::from_words(motion.cell.z, 0));
        let mut present = fix32::zero();
        if motion.cell.present {
            present = integer::from_i16(1);
        }
        attitude::put(out, base + 44, present);
        attitude::put(out, base + 45, code);
        attitude::put(out, base + 46, attitude::get(slot.as_array(), 0));
        attitude::put(out, base + 47, attitude::get(seed.as_array(), 0));
        attitude::put(
            out,
            base + 48,
            attitude::get(flags.as_array(), entity.lo_bits()),
        );
        bounce_tick_list(out, base + 49, &runtime.ticks.order);
        bounce_tick_list(out, base + 54, &runtime.ordinary);
        bounce_tick_list(out, base + 59, &runtime.special);
        bounce_tick_list(out, base + 64, &runtime.queues.notifications);
        bounce_tick_list(out, base + 69, &runtime.queues.destruction);
        i = 0;
        while i < 64 {
            let row = spatial::list(&grid, i);
            attitude::put(out, base + 74 + i * 4, fix32::from_words(row.count, 0));
            let mut j = 0u16;
            while j < 3 {
                let mut value = integer::from_i16(-1);
                if j < row.count {
                    value = attitude::get(row.items, j);
                }
                attitude::put(out, base + 75 + i * 4 + j, value);
                j += 1;
            }
            i += 1;
        }
        step += 1;
    }
    map.terrain.finish();
    if read_dseg_page(3) != 3 {
        halt(0x7e21);
    }
}
