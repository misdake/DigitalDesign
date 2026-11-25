fn run_entities(input: Array<u16>, out: Array<u16>) {
    let header: Buf<u16, 12> = Buf::new([0; 12]);
    let rows: Buf<u16, 2400> = Buf::new([0; 2400]);
    let state = entities::State {
        header: header.as_array(),
        rows: rows.as_array(),
        storage_capacity: 400,
    };
    let mut h = state.header;
    let mut i = 0u16;
    while i < 12 {
        h[i] = input[i];
        i += 1;
    }
    let count = attitude::get(h, 0).lo_bits();
    let mut r = state.rows;
    i = 0;
    while i < count {
        let base = 10 + i * 5;
        let mut flags = attitude::get(input, base).lo_bits();
        let class = attitude::get(input, base + 1);
        if class != integer::from_i16(-1) {
            flags |= 2;
        }
        r[i * 6] = flags | (attitude::get(input, base + 2).lo_bits() << 2);
        r[i * 6 + 1] = class.lo_bits();
        attitude::put(r, i * 3 + 1, attitude::get(input, base + 3));
        attitude::put(r, i * 3 + 2, attitude::get(input, base + 4));
        i += 1;
    }
    let op = attitude::get(input, 6).lo_bits();
    let a = attitude::get(input, 7);
    let b = attitude::get(input, 8);
    let c = attitude::get(input, 9);
    let mut status = 0u16;
    let mut id = fix32::zero();
    if op == 0 {
        if a.hi_bits() != 0 {
            status = 4;
        } else {
            status = entities::reset(&state, a.lo_bits());
        }
    } else if op == 1 {
        status = entities::grow(&state);
    } else if op == 2 {
        let e = entities::unlink_free(&state, a, b);
        status = e.status;
        id = e.id;
    } else if op == 3 {
        let e = entities::new_record(&state, a, b);
        status = e.status;
        id = e.id;
    } else if op == 4 {
        let e = entities::create_record(&state, b);
        status = e.status;
        id = e.id;
    } else if op == 5 {
        status = entities::release_id(&state, a);
    } else if op == 6 {
        status = entities::detach(&state, a);
    } else if op == 7 {
        status = entities::bind_class(&state, a, b.lo_bits());
    } else if op == 8 {
        status = entities::bind_handle(&state, a, b.lo_bits(), c);
    } else if op == 9 {
        let mut sequential = fix32::zero();
        if a != fix32::zero() {
            sequential = integer::from_i16(1);
        }
        attitude::put(h, 4, sequential);
        attitude::put(h, 5, b);
    } else if op == 10 && entities::present(&state, a) {
        if entities::class(&state, a) != fix32::zero() {
            status = 6;
        } else {
            status = entities::release_id(&state, a);
            if status == 0 {
                entities::finish_destroy(&state);
            }
        }
    }
    attitude::put(out, 0, fix32::from_words(status, 0));
    attitude::put(out, 1, id);
    i = 0;
    while i < 6 {
        attitude::put(out, i + 2, attitude::get(h, i));
        i += 1;
    }
    i = 0;
    while i < attitude::get(h, 0).lo_bits() {
        let entity = fix32::from_words(i, 0);
        let base = 8 + i * 5;
        let mut present = fix32::zero();
        if entities::present(&state, entity) {
            present = integer::from_i16(1);
        }
        attitude::put(out, base, present);
        attitude::put(out, base + 1, entities::class(&state, entity));
        attitude::put(
            out,
            base + 2,
            fix32::from_words(entities::handle_kind(&state, entity), 0),
        );
        attitude::put(out, base + 3, entities::handle(&state, entity));
        attitude::put(out, base + 4, entities::next_free(&state, entity));
        i += 1;
    }
}
