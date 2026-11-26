fn run_bounce_hit(input: Array<u16>, out: Array<u16>, sine: Array<i16>) {
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
    i = 0;
    while i < 60 {
        collider[i] = input[24 + i];
        i += 1;
    }
    let mut motion = bounce::Motion {
        state: state,
        properties: properties.as_array(),
        collider: collider,
        reference_alias: attitude::get(input, 78) != fix32::zero(),
        cell: spatial::Cell {
            x: 0,
            z: 0,
            present: false,
        },
        impact_threshold: fix32::zero(),
    };
    let config = bounce::Config {
        data: unsafe { input.as_ptr().add(84).as_u16_array() },
        sine: sine,
        angle: unsafe { Ptr::from_addr(0x8000).as_i16_array() },
    };
    let hit = bounce::Hit {
        position: unsafe { input.as_ptr().add(104).as_u16_array() },
        velocity: unsafe { input.as_ptr().add(110).as_u16_array() },
        forward: attitude::get(input, 58),
        reference: unsafe { input.as_ptr().add(118).as_u16_array() },
        external: unsafe { input.as_ptr().add(136).as_u16_array() },
        has_external: attitude::get(input, 77) != fix32::zero(),
    };
    let frame = bounce::Frame {
        entity: attitude::get(input, 79),
        limit: attitude::get(input, 82),
        dt: integer::from_i16(33),
        id: fix32::zero(),
        has_mesh: false,
    };
    let flags: Buf<u16, 1024> = Buf::new([0; 1024]);
    let order: Buf<u16, 16> = Buf::new([0; 16]);
    let mut count = 1u16;
    if attitude::get(input, 81) != fix32::zero() {
        count = 2;
        attitude::put(
            flags.as_array(),
            frame.entity.lo_bits(),
            integer::from_i16(16),
        );
        attitude::put(order.as_array(), 1, frame.entity);
    }
    let slot: Buf<u16, 2> = Buf::new([24, 0]);
    let seed: Buf<u16, 2> = Buf::new([0; 2]);
    attitude::put(seed.as_array(), 0, attitude::get(input, 83));
    let mut runtime = scene_world::Runtime {
        ticks: lifecycle::Ticks {
            flag_words: 2,
            flags: flags.as_array(),
            flag_count: attitude::get(input, 80).lo_bits(),
            order: ordered::List {
                items: order.as_array(),
                count: count,
                capacity: 8,
            },
            next: integer::from_i16(-1),
        },
        slot: slot.as_array(),
        seed: seed.as_array(),
        queues: lifecycle::Queues {
            notifications: ordered::List {
                items: order.as_array(),
                count: 0,
                capacity: 0,
            },
            destruction: ordered::List {
                items: order.as_array(),
                count: 0,
                capacity: 0,
            },
        },
        ordinary: ordered::List {
            items: order.as_array(),
            count: 0,
            capacity: 0,
        },
        special: ordered::List {
            items: order.as_array(),
            count: 0,
            capacity: 0,
        },
    };
    let mut step = 0u16;
    while step < 3 {
        let status = bounce::hit(&mut motion, &config, &hit, &mut runtime, &frame);
        let mut code = fix32::zero();
        if status != 0 {
            code = integer::from_i16(-1);
        }
        let base = step * 52;
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
        attitude::put(out, base + 42, code);
        attitude::put(out, base + 43, integer::from_i16(0x7182));
        attitude::put(out, base + 44, attitude::get(seed.as_array(), 0));
        attitude::put(out, base + 45, attitude::get(slot.as_array(), 0));
        let mut flag = fix32::zero();
        if frame.entity.lo_bits() < runtime.ticks.flag_count {
            flag = attitude::get(flags.as_array(), frame.entity.lo_bits());
        }
        attitude::put(out, base + 46, flag);
        attitude::put(
            out,
            base + 47,
            fix32::from_words(runtime.ticks.order.count, 0),
        );
        i = 0;
        while i < 4 {
            let mut id = integer::from_i16(-1);
            if i < runtime.ticks.order.count {
                id = attitude::get(runtime.ticks.order.items, i);
            }
            attitude::put(out, base + 48 + i, id);
            i += 1;
        }
        step += 1;
    }
}
