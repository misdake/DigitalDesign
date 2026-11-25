// Real Fall method boundaries: validate, advance, terrain/retirement, publish.
fn run_falling(input: Array<u16>, out: Array<u16>, sine: Array<i16>, angle: Array<i16>) {
    let storage: Buf<u16, 48> = Buf::new([0; 48]);
    let state = storage.as_array();
    let properties: Buf<u16, 16> = Buf::new([0; 16]);
    let pose = unsafe { state.as_ptr().add(12).as_u16_array() };
    let external = unsafe { state.as_ptr().add(30).as_u16_array() };
    let mut i = 0u16;
    while i < 48 {
        let mut words = state;
        words[i] = input[i];
        i += 1;
    }
    attitude::put(properties.as_array(), 4, attitude::get(state, 4));
    attitude::put(properties.as_array(), 3, attitude::get(state, 5));
    let present = attitude::get(input, 24) != fix32::zero();
    let peer = unsafe { input.as_ptr().add(50).as_u16_array() };
    let position = unsafe { input.as_ptr().add(68).as_u16_array() };
    let tuning = unsafe { input.as_ptr().add(76).as_u16_array() };
    let tables = falling::Tables {
        sine: sine,
        angle: angle,
    };
    let id = attitude::get(input, 43);
    let entity = attitude::get(input, 44);
    let limit = attitude::get(input, 42);
    let items: Buf<u16, 512> = Buf::new([0; 512]);
    let range_storage: Buf<u16, 192> = Buf::new([0; 192]);
    let mut ranges = range_storage.as_array();
    i = 0;
    while i < 64 {
        ranges[i * 3] = i * 8;
        ranges[i * 3 + 2] = 4;
        i += 1;
    }
    ranges[1u16] = 2;
    ranges[4u16] = 2;
    ranges[190u16] = 1;
    attitude::put(items.as_array(), 0, id);
    attitude::put(items.as_array(), 1, id);
    attitude::put(items.as_array(), 4, id + integer::from_i16(1));
    attitude::put(items.as_array(), 5, id);
    attitude::put(items.as_array(), 252, id);
    let origin: Buf<u16, 4> = Buf::new([0; 4]);
    let mut grid = spatial::Grid {
        origin: origin.as_array(),
        shift_x: 12,
        shift_z: 12,
        items: items.as_array(),
        ranges: ranges,
    };
    let cell = spatial::Cell {
        x: 0,
        z: 0,
        present: true,
    };
    let flags: Buf<u16, 1024> = Buf::new([0; 1024]);
    attitude::put(flags.as_array(), entity.lo_bits(), integer::from_i16(16));
    let order: Buf<u16, 2> = Buf::new([0; 2]);
    attitude::put(order.as_array(), 0, entity);
    let notifications: Buf<u16, 8> = Buf::new([0; 8]);
    let destruction: Buf<u16, 8> = Buf::new([0; 8]);
    let mut ticks = lifecycle::Ticks {
        flags: flags.as_array(),
        flag_count: 512,
        order: ordered::List {
            items: order.as_array(),
            count: 1,
            capacity: 1,
        },
        next: integer::from_i16(-1),
    };
    let mut queues = lifecycle::Queues {
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
    };
    let mut slot = integer::from_i16(777);
    let mut step = 0u16;
    while step < 4 {
        let mut status = fix32::zero();
        let mut query_x = fix32::zero();
        let mut query_z = fix32::zero();
        let saved = slot;
        slot = integer::from_i16(12);
        let active = attitude::get(properties.as_array(), 3) != fix32::zero();
        let aligning = attitude::get(properties.as_array(), 4).lo_bits() & 2 != 0;
        if limit == fix32::zero()
            || (active && aligning && attitude::get(input, 45) == fix32::zero())
            || (active && !present)
        {
            status = integer::from_i16(-1);
        } else {
            let mut matrix = pose;
            if present {
                matrix = external;
            }
            let mut alias = present;
            i = 0;
            while i < 9 {
                if attitude::get(pose, i) != attitude::get(external, i) {
                    alias = false;
                }
                i += 1;
            }
            let mut motion = falling::Motion {
                state: state,
                properties: properties.as_array(),
                matrix: matrix,
                vehicle_basis: peer,
                position: position,
                width: attitude::get(input, 37),
                effect_active: false,
            };
            let action = falling::advance(
                &mut motion,
                tuning,
                attitude::get(input, 46 + step),
                &tables,
            );
            if action.status != 0 {
                status = integer::from_i16(-2);
            }
            if status == fix32::zero() && action.terrain {
                query_x = action.x;
                query_z = action.z;
                status = attitude::get(input, 51 + step * 2);
                if status == fix32::zero()
                    && falling::slope(&mut motion, attitude::get(input, 50 + step * 2), angle) != 0
                {
                    status = integer::from_i16(-2);
                }
            }
            if status == fix32::zero() && action.retire {
                let mut code = spatial::remove(&mut grid, id, &cell, limit);
                if code == 0 {
                    code = lifecycle::queue(&mut queues, &mut ticks, entity, false, limit);
                }
                if code == 3 {
                    status = integer::from_i16(-3);
                } else if code != 0 {
                    status = integer::from_i16(-1);
                }
            }
            if status == fix32::zero() {
                falling::finish(&mut motion, &action, sine);
            }
            if alias {
                i = 0;
                while i < 9 {
                    attitude::put(pose, i, attitude::get(external, i));
                    i += 1;
                }
            }
        }
        if status == fix32::zero() {
            slot = saved;
        }
        let base = step * 288;
        i = 0;
        while i < 24 {
            attitude::put(out, base + i, attitude::get(state, i));
            i += 1;
        }
        attitude::put(out, base + 4, attitude::get(properties.as_array(), 4));
        attitude::put(out, base + 5, attitude::get(properties.as_array(), 3));
        let mut has_external = fix32::zero();
        if present {
            has_external = integer::from_i16(1);
        }
        attitude::put(out, base + 24, has_external);
        attitude::put(out, base + 25, status);
        attitude::put(out, base + 26, slot);
        attitude::put(
            out,
            base + 27,
            attitude::get(flags.as_array(), entity.lo_bits()),
        );
        attitude::put(
            out,
            base + 28,
            fix32::from_words(queues.notifications.count, 0),
        );
        let mut first = integer::from_i16(-1);
        if queues.notifications.count != 0 {
            first = attitude::get(queues.notifications.items, 0);
        }
        attitude::put(out, base + 29, first);
        i = 0;
        while i < 64 {
            let list = spatial::list(&grid, i);
            attitude::put(out, base + 30 + i * 4, fix32::from_words(list.count, 0));
            let mut j = 0u16;
            while j < 3 {
                let mut value = integer::from_i16(-1);
                if j < list.count {
                    value = attitude::get(list.items, j);
                }
                attitude::put(out, base + 31 + i * 4 + j, value);
                j += 1;
            }
            i += 1;
        }
        attitude::put(out, base + 286, query_x);
        attitude::put(out, base + 287, query_z);
        step += 1;
    }
}
