fn run_scene_contact(input: Array<u16>, out: Array<u16>, sine: Array<i16>, angle: Array<i16>) {
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let state: Buf<u16, 4> = Buf::new([0; 4]);
    let mut i = 0u16;
    while i < 3 {
        attitude::put(velocity.as_array(), i, attitude::get(input, 3 + i));
        i += 1;
    }
    attitude::put(state.as_array(), 0, attitude::get(input, 14));
    attitude::put(state.as_array(), 1, attitude::get(input, 15));
    let object = scene_contact::Object {
        class: attitude::get(input, 6),
        weight: attitude::get(input, 7),
        threshold: attitude::get(input, 8),
        activity: attitude::get(input, 9),
        flags: attitude::get(input, 10),
        radius: attitude::get(input, 11),
    };
    let env = scene_contact::Environment {
        mode: attitude::get(input, 12),
        breakable_class: attitude::get(input, 13),
        sine: sine,
        angle: angle,
    };
    let mut calls = fix32::zero();
    let mut step = 0u16;
    while step < 3 {
        let status =
            scene_contact::obstacle(velocity.as_array(), state.as_array(), input, &object, &env);
        if status == 2 {
            halt(0x7e11);
        }
        if status == 1 {
            calls = calls + integer::from_i16(1);
        }
        i = 0;
        while i < 3 {
            attitude::put(out, step * 6 + i, attitude::get(velocity.as_array(), i));
            i += 1;
        }
        attitude::put(out, step * 6 + 3, attitude::get(state.as_array(), 0));
        attitude::put(out, step * 6 + 4, attitude::get(state.as_array(), 1));
        attitude::put(out, step * 6 + 5, calls);
        step += 1;
    }
}
fn run_live_contact(input: Array<u16>, out: Array<u16>, sine: Array<i16>, angle: Array<i16>) {
    let colliders: Buf<u16, 60> = Buf::new([0; 60]);
    let properties: Buf<u16, 16> = Buf::new([0; 16]);
    let mut geometry = colliders.as_array();
    let mut p = properties.as_array();
    let mut i = 0u16;
    while i < 60 {
        geometry[i] = input[i];
        i += 1;
    }
    i = 0;
    while i < 10 {
        p[i] = input[60 + i];
        i += 1;
    }
    i = 0;
    while i < 6 {
        p[10 + i] = attitude::get(input, 35 + i).lo_bits();
        i += 1;
    }
    let origin: Buf<u16, 4> = Buf::new([0; 4]);
    let items: Buf<u16, 128> = Buf::new([0; 128]);
    let ranges_buffer: Buf<u16, 192> = Buf::new([0; 192]);
    let mut ranges = ranges_buffer.as_array();
    i = 0;
    while i < 64 {
        ranges[i * 3] = i * 2;
        ranges[i * 3 + 1] = 1;
        ranges[i * 3 + 2] = 1;
        i += 1;
    }
    let unused: Buf<u16, 2> = Buf::new([0; 2]);
    let world = scene_world::World {
        colliders: geometry,
        properties: p,
        count: 1,
        grid: spatial::Grid {
            page: 65535,
            origin: origin.as_array(),
            shift_x: 12,
            shift_z: 12,
            items: items.as_array(),
            ranges: ranges,
        },
        deformables: ordered::List {
            items: unused.as_array(),
            count: 0,
            capacity: 1,
        },
    };
    let segment: Buf<u16, 12> = Buf::new([500, 0, 0, 0, 0, 0, 65036, 65535, 0, 0, 0, 0]);
    let q = object_query::Query {
        start: segment.as_array(),
        end: unsafe { segment.as_array().as_ptr().add(6).as_u16_array() },
        self_entity: integer::from_i16(99),
        object_count: 1,
        deformable: false,
    };
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let state: Buf<u16, 4> = Buf::new([0; 4]);
    let env = scene_contact::Environment {
        mode: attitude::get(input, 41),
        breakable_class: integer::from_i16(1),
        sine: sine,
        angle: angle,
    };
    let mut calls = 0u16;
    let mut step = 0u16;
    while step < 6 {
        attitude::put(velocity.as_array(), 0, -fix32::from_words(53393, 3));
        attitude::put(velocity.as_array(), 1, fix32::zero());
        attitude::put(velocity.as_array(), 2, fix32::zero());
        if step == 4 {
            attitude::put(p, 2, fix32::zero());
            attitude::put(p, 0, fix32::zero());
        }
        if step == 5 {
            attitude::put(geometry, 2, integer::from_i16(10000));
        }
        let mut hit = object_query::hit_zero();
        let status = scene_contact::obstacles(
            &world,
            &q,
            velocity.as_array(),
            state.as_array(),
            &env,
            &mut hit,
        );
        if status == 1 {
            // Explicit qualification probe, matching the source Effects method.
            calls += 1;
            if calls == 1 {
                attitude::put(p, 3, integer::from_i16(1));
                attitude::put(p, 4, integer::from_i16(1));
            } else if calls == 2 {
                attitude::put(p, 1, integer::from_i16(128));
            } else if calls == 3 {
                attitude::put(p, 2, fix32::from_words(65535, 32767));
            }
        }
        let base = step * 27;
        attitude::put(out, base, fix32::from_words(status, 0));
        let raw = unsafe { view_of(&mut hit).as_ptr().as_u16_array() };
        i = 0;
        while i < 9 {
            attitude::put(out, base + 1 + i, attitude::get(raw, i));
            i += 1;
        }
        i = 0;
        while i < 3 {
            attitude::put(out, base + 10 + i, attitude::get(velocity.as_array(), i));
            i += 1;
        }
        attitude::put(out, base + 13, attitude::get(state.as_array(), 0));
        attitude::put(out, base + 14, attitude::get(state.as_array(), 1));
        attitude::put(out, base + 15, fix32::from_words(calls, 0));
        i = 0;
        while i < 5 {
            attitude::put(out, base + 16 + i, attitude::get(p, i));
            i += 1;
        }
        i = 0;
        while i < 6 {
            attitude::put(out, base + 21 + i, integer::from_i16(p[10 + i] as i16));
            i += 1;
        }
        step += 1;
    }
}
