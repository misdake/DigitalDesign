fn run_scene_world(input: Array<u16>, out: Array<u16>, sine: Array<i16>) {
    let storage: Buf<u16, 120> = Buf::new([0; 120]);
    let properties: Buf<u16, 32> = Buf::new([0; 32]);
    let mut colliders = storage.as_array();
    let mut i = 0u16;
    while i < 120 {
        colliders[i] = input[i];
        i += 1;
    }
    let items: Buf<u16, 512> = Buf::new([0; 512]);
    let ranges_buffer: Buf<u16, 192> = Buf::new([0; 192]);
    let mut ranges = ranges_buffer.as_array();
    i = 0;
    while i < 64 {
        ranges[i * 3] = i * 8;
        ranges[i * 3 + 1] = 2;
        ranges[i * 3 + 2] = 4;
        attitude::put(items.as_array(), i * 4, attitude::get(input, 68));
        attitude::put(items.as_array(), i * 4 + 1, integer::from_i16(1));
        i += 1;
    }
    let origin: Buf<u16, 4> = Buf::new([61440, 65535, 61440, 65535]);
    let deformables: Buf<u16, 4> = Buf::new([0, 0, 1, 0]);
    let mut world = scene_world::World {
        colliders: colliders,
        properties: properties.as_array(),
        count: 2,
        fixed_count: 2,
        dynamic_page: 0,
        dynamic_capacity: 0,
        grid: spatial::Grid {
            page: 65535,
            origin: origin.as_array(),
            shift_x: 12,
            shift_z: 12,
            items: items.as_array(),
            ranges: ranges,
        },
        deformables: ordered::List {
            items: deformables.as_array(),
            count: 2,
            capacity: 2,
        },
    };
    let mode = unsafe { Ptr::from_addr(0x1ffd).read(0) };
    if mode > 2 {
        halt(0x7e40);
    }
    world.fixed_count = 2 - mode;
    world.dynamic_page = 120;
    world.dynamic_capacity = mode;
    let caller_page = read_dseg_page(3);
    if mode == 2 {
        // Candidate IDs occupy a different window page from dynamic geometry.
        unsafe {
            write_dseg_page(3, 119);
        }
        let mut mapped = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
        i = 0;
        while i < 4 {
            mapped[i] = origin[i];
            i += 1;
        }
        i = 0;
        while i < 192 {
            mapped[4 + i] = ranges[i];
            i += 1;
        }
        i = 0;
        while i < 512 {
            mapped[196 + i] = items[i];
            i += 1;
        }
        world.grid.page = 119;
        world.grid.origin = mapped;
        world.grid.ranges = unsafe { mapped.as_ptr().add(4).as_u16_array() };
        world.grid.items = unsafe { mapped.as_ptr().add(196).as_u16_array() };
    }
    let incoming = read_dseg_page(3);
    let mut hidden = world.fixed_count;
    while hidden < 2 {
        let source = unsafe { colliders.as_ptr().add((hidden * 60) as i16).as_u16_array() };
        if !scene_world::store_collider(&world, hidden, source) {
            halt(0x7e41);
        }
        // Stale fixed rows must never satisfy a banked query.
        i = 0;
        while i < 60 {
            colliders[hidden * 60 + i] = 0x7777;
            i += 1;
        }
        hidden += 1;
    }
    if read_dseg_page(3) != incoming {
        halt(0x7e42);
    }
    let q = object_query::Query {
        start: unsafe { input.as_ptr().add(120).as_u16_array() },
        end: unsafe { input.as_ptr().add(126).as_u16_array() },
        self_entity: attitude::get(input, 66),
        object_count: 2,
        deformable: attitude::get(input, 67) != fix32::zero(),
    };
    let cell = spatial::Cell {
        x: 1,
        z: 1,
        present: true,
    };
    let mut second = object_query::collider_zero();
    let mut raw = unsafe { view_of(&mut second).as_ptr().as_u16_array() };
    i = 0;
    while i < 60 {
        raw[i] = input[60 + i];
        i += 1;
    }
    let mut step = 0u16;
    let current: Buf<u16, 60> = Buf::new([0; 60]);
    let mut words = current.as_array();
    while step < 6 {
        if step == 1 {
            if !scene_world::load_collider(&world, 0, words) {
                halt(0x7e43);
            }
            attitude::put(words, 2, integer::from_i16(10000));
            if !scene_world::store_collider(&world, 0, words) {
                halt(0x7e44);
            }
        }
        if step == 2 {
            if !scene_world::load_collider(&world, 0, words) {
                halt(0x7e45);
            }
            attitude::put(words, 2, fix32::zero());
            attitude::rotate(words, 4, 1, integer::from_i16(8192), sine);
            if !scene_world::store_collider(&world, 0, words) {
                halt(0x7e46);
            }
            spatial::remove(
                &mut world.grid,
                integer::from_i16(1),
                &cell,
                integer::from_i16(1000),
            );
        }
        if step == 3 {
            spatial::remove(
                &mut world.grid,
                fix32::zero(),
                &cell,
                integer::from_i16(1000),
            );
        }
        if step == 4 {
            let inserted = spatial::insert(
                &mut world.grid,
                integer::from_i16(1),
                second.position.as_array(),
                true,
                20,
            );
            if inserted.status != 0 {
                halt(0x7e10);
            }
        }
        if step == 5 {
            // Original Matrix::from_angles uses a different composition from
            // successive rotations; the independent source matrix is appended.
            if !scene_world::load_collider(&world, 1, words) {
                halt(0x7e47);
            }
            i = 0;
            while i < 18 {
                words[26 + i] = input[138 + i];
                i += 1;
            }
            attitude::put(words, 29, integer::from_i16(1));
            if !scene_world::store_collider(&world, 1, words) {
                halt(0x7e48);
            }
        }
        let mut hit = object_query::hit_zero();
        attitude::put(hit.point.as_array(), 0, integer::from_i16(11));
        attitude::put(hit.point.as_array(), 1, integer::from_i16(22));
        attitude::put(hit.point.as_array(), 2, integer::from_i16(33));
        attitude::put(hit.normal.as_array(), 0, integer::from_i16(-44));
        attitude::put(hit.normal.as_array(), 1, integer::from_i16(55));
        attitude::put(hit.normal.as_array(), 2, integer::from_i16(-66));
        let status = scene_world::query(&world, &q, &mut hit);
        if read_dseg_page(3) != incoming {
            halt(0x7e49);
        }
        let base = step * 14;
        attitude::put(out, base, fix32::from_words(status, 0));
        let values = unsafe { view_of(&mut hit).as_ptr().as_u16_array() };
        i = 0;
        while i < 18 {
            let mut dst = out;
            dst[base * 2 + 2 + i] = values[i];
            i += 1;
        }
        let row = spatial::list(&world.grid, 9);
        attitude::put(out, base + 10, fix32::from_words(row.count, 0));
        i = 0;
        while i < 3 {
            let mut id = integer::from_i16(-1);
            if i < row.count {
                id = attitude::get(row.items, i);
            }
            attitude::put(out, base + 11 + i, id);
            i += 1;
        }
        step += 1;
    }
    // The final complete row fits; a following row cannot touch the page guard.
    world.fixed_count = 0;
    world.dynamic_capacity = 215;
    world.count = 215;
    unsafe {
        write_dseg_page(3, 120);
        Ptr::from_addr(0xffff).write(0, 0xa55a);
        write_dseg_page(3, incoming);
    }
    if !scene_world::store_collider(&world, 214, raw) {
        halt(0x7e4a);
    }
    if !scene_world::load_collider(&world, 214, words) {
        halt(0x7e4b);
    }
    i = 0;
    while i < 60 {
        if words[i] != raw[i] {
            halt(0x7e4c);
        }
        i += 1;
    }
    i = 0;
    while i < 16 {
        words[i] = 0xa500 + i;
        i += 1;
    }
    if !scene_world::store_properties(&world, 214, words) {
        halt(0x7e4d);
    }
    i = 0;
    while i < 16 {
        words[i] = 0;
        i += 1;
    }
    if !scene_world::load_properties(&world, 214, words) {
        halt(0x7e4e);
    }
    i = 0;
    while i < 16 {
        if words[i] != 0xa500 + i {
            halt(0x7e4f);
        }
        i += 1;
    }
    world.count = 216;
    if scene_world::store_collider(&world, 215, raw)
        || scene_world::load_properties(&world, 215, words)
    {
        halt(0x7e50);
    }
    if read_dseg_page(3) != incoming {
        halt(0x7e51);
    }
    unsafe {
        write_dseg_page(3, 120);
    }
    if unsafe { Ptr::from_addr(0xffff).read(0) } != 0xa55a {
        halt(0x7e52);
    }
    unsafe {
        write_dseg_page(3, incoming);
    }
    if caller_page != incoming {
        unsafe {
            write_dseg_page(3, caller_page);
        }
    }
}
