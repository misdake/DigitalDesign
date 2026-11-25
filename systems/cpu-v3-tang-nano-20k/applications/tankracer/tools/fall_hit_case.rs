fn run_fall_hit(input: Array<u16>, out: Array<u16>) {
    let storage: Buf<u16, 10> = Buf::new([0; 10]);
    let properties: Buf<u16, 16> = Buf::new([0; 16]);
    let mut state = storage.as_array();
    let mut i = 0u16;
    while i < 8 {
        state[i] = input[i];
        i += 1;
    }
    attitude::put(state, 4, attitude::get(input, 6));
    attitude::put(properties.as_array(), 4, attitude::get(input, 4));
    attitude::put(properties.as_array(), 3, attitude::get(input, 5));
    let flags: Buf<u16, 1024> = Buf::new([0; 1024]);
    let order: Buf<u16, 16> = Buf::new([0; 16]);
    let mut count = 1u16;
    let entity = attitude::get(input, 13);
    if attitude::get(input, 15) != fix32::zero() {
        attitude::put(flags.as_array(), entity.lo_bits(), integer::from_i16(16));
        attitude::put(order.as_array(), 1, entity);
        count = 2;
    }
    let mut ticks = lifecycle::Ticks {
        flags: flags.as_array(),
        flag_count: attitude::get(input, 14).lo_bits(),
        order: ordered::List {
            items: order.as_array(),
            count: count,
            capacity: 8,
        },
        next: integer::from_i16(-1),
    };
    let current: Buf<u16, 2> = Buf::new([24, 0]);
    let hit = falling::Hit {
        velocity: unsafe { input.as_ptr().add(14).as_u16_array() },
        forward: attitude::get(input, 10),
        vehicle: integer::from_i16(99),
        entity: entity,
        minimum_turn_rate: attitude::get(input, 11),
        maximum_turn_rate: attitude::get(input, 12),
    };
    let mut step = 0u16;
    while step < 3 {
        let status = falling::strike(
            state,
            properties.as_array(),
            &hit,
            &mut ticks,
            current.as_array(),
            attitude::get(input, 16),
        );
        let mut code = fix32::zero();
        if status >= 2 {
            code = integer::from_i16(-1);
        }
        let base = step * 17;
        i = 0;
        while i < 4 {
            attitude::put(out, base + i, attitude::get(state, i));
            i += 1;
        }
        attitude::put(out, base + 4, attitude::get(properties.as_array(), 4));
        attitude::put(out, base + 5, attitude::get(properties.as_array(), 3));
        attitude::put(out, base + 6, attitude::get(state, 4));
        attitude::put(out, base + 7, code);
        attitude::put(out, base + 8, fix32::zero());
        attitude::put(out, base + 9, integer::from_i16(7));
        attitude::put(out, base + 10, attitude::get(current.as_array(), 0));
        let mut flag = fix32::zero();
        if entity.lo_bits() < ticks.flag_count {
            flag = attitude::get(flags.as_array(), entity.lo_bits());
        }
        attitude::put(out, base + 11, flag);
        attitude::put(out, base + 12, fix32::from_words(ticks.order.count, 0));
        i = 0;
        while i < 4 {
            let mut id = integer::from_i16(-1);
            if i < ticks.order.count {
                id = attitude::get(ticks.order.items, i);
            }
            attitude::put(out, base + 13 + i, id);
            i += 1;
        }
        step += 1;
    }
}
