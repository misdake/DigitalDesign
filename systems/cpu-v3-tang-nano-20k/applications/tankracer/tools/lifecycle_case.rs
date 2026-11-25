// Explicit operation loops exercise live cursor/queue changes without callbacks.
fn load_list(list: &mut ordered::List, input: Array<u16>, index: u16) {
    list.count = attitude::get(input, index).lo_bits();
    let mut i = 0u16;
    while i < list.count {
        attitude::put(list.items, i, attitude::get(input, index + 1 + i));
        i += 1;
    }
}
fn write_list(list: &ordered::List, out: Array<u16>, index: u16) {
    attitude::put(out, index, fix32::from_words(list.count, 0));
    let mut i = 0u16;
    while i < 8 {
        let mut value = integer::from_i16(-1);
        if i < list.count {
            value = attitude::get(list.items, i);
        }
        attitude::put(out, index + 1 + i, value);
        i += 1;
    }
}
fn run_lifecycle(input: Array<u16>, out: Array<u16>) {
    let flag_storage: Buf<u16, 12> = Buf::new([0; 12]);
    let flags = flag_storage.as_array();
    let order: Buf<u16, 32> = Buf::new([0; 32]);
    let notifications: Buf<u16, 32> = Buf::new([0; 32]);
    let destruction: Buf<u16, 32> = Buf::new([0; 32]);
    let visit_storage: Buf<u16, 32> = Buf::new([0; 32]);
    let mut ticks = lifecycle::Ticks {
        flags: flags,
        flag_count: 6,
        order: ordered::List {
            items: order.as_array(),
            count: 0,
            capacity: 16,
        },
        next: attitude::get(input, 15),
    };
    let mut queues = lifecycle::Queues {
        notifications: ordered::List {
            items: notifications.as_array(),
            count: 0,
            capacity: 16,
        },
        destruction: ordered::List {
            items: destruction.as_array(),
            count: 0,
            capacity: 16,
        },
    };
    let mut i = 0u16;
    while i < 6 {
        attitude::put(flags, i, attitude::get(input, i));
        i += 1;
    }
    load_list(&mut ticks.order, input, 6);
    load_list(&mut queues.notifications, input, 16);
    load_list(&mut queues.destruction, input, 25);
    let mut step = 0u16;
    while step < attitude::get(input, 34).lo_bits() {
        let row = 35 + step * 4;
        let op = attitude::get(input, row).lo_bits();
        let id = attitude::get(input, row + 1);
        let limit = attitude::get(input, row + 2);
        let fail = attitude::get(input, row + 3);
        let mut visited = ordered::List {
            items: visit_storage.as_array(),
            count: 0,
            capacity: 16,
        };
        let mut status = 0u16;
        if op == 0 {
            status = lifecycle::enable(&mut ticks, id);
        } else if op == 1 {
            status = lifecycle::disable(&mut ticks, id, limit);
        } else if op == 2 || op == 3 {
            status = lifecycle::queue(&mut queues, &mut ticks, id, op == 3, limit);
        } else if op == 4 {
            status = lifecycle::cancel(&mut queues, &mut ticks, id, limit);
        } else if op == 5 {
            let mut pass = lifecycle::begin(&ticks, limit);
            status = pass.status;
            if status == 0 {
                loop {
                    status = lifecycle::prepare(&mut ticks, &mut pass, limit);
                    if status != 1 {
                        break;
                    }
                    ordered::append(&mut visited, pass.current);
                    if fail != fix32::zero() && pass.current == integer::from_i16(1) {
                        status = 7;
                        break;
                    }
                    if pass.current == integer::from_i16(1) {
                        status = lifecycle::disable(&mut ticks, integer::from_i16(2), limit);
                        if status >= 2 {
                            break;
                        }
                    }
                    if pass.current == integer::from_i16(3) {
                        status = lifecycle::enable(&mut ticks, integer::from_i16(4));
                        if status >= 2 {
                            break;
                        }
                    }
                    lifecycle::resume(&mut pass, &ticks);
                }
            }
        } else {
            let mut drain = lifecycle::begin_drain(limit);
            status = drain.status;
            if status == 0 {
                loop {
                    let event = lifecycle::pop(&mut queues, &mut drain, limit);
                    status = event.status;
                    if status != 1 {
                        break;
                    }
                    ordered::append(&mut visited, fmul_lo32(event.id, integer::from_i16(2)));
                    status = lifecycle::disable(&mut ticks, event.id, limit);
                    if status >= 2 {
                        break;
                    }
                    if event.destroy {
                        ordered::append(
                            &mut visited,
                            fmul_lo32(event.id, integer::from_i16(2)) + integer::from_i16(1),
                        );
                        if event.id == integer::from_i16(4) {
                            status = lifecycle::queue(
                                &mut queues,
                                &mut ticks,
                                integer::from_i16(5),
                                false,
                                limit,
                            );
                            if status >= 2 {
                                break;
                            }
                        }
                    }
                }
            }
        }
        let base = step * 44;
        attitude::put(out, base, fix32::from_words(status, 0));
        attitude::put(out, base + 1, ticks.next);
        i = 0;
        while i < 6 {
            attitude::put(out, base + 2 + i, attitude::get(flags, i));
            i += 1;
        }
        write_list(&ticks.order, out, base + 8);
        write_list(&queues.notifications, out, base + 17);
        write_list(&queues.destruction, out, base + 26);
        write_list(&visited, out, base + 35);
        step += 1;
    }
}
