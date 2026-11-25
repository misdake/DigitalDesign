// Persistent record state, siren and real queue retirement over three ticks.
fn run_crush(input: Array<u16>, out: Array<u16>) {
    let storage: Buf<deformation::Record, 3> = Buf::new([
        deformation::record_zero(),
        deformation::record_zero(),
        deformation::record_zero(),
    ]);
    let mut raw = unsafe { storage.as_array().as_ptr().as_u16_array() };
    let mut i = 0u16;
    while i < 66 {
        raw[i] = input[14 + i];
        i += 1;
    }
    let mut surface = deformation::disabled(input, storage.as_array());
    surface.record_count = 3;
    surface.attached = attitude::get(input, 3) != fix32::zero();
    surface.dirty = attitude::get(input, 4).lo_bits();
    let mut runtime = crush::Runtime {
        siren_timer: attitude::get(input, 5),
        sound_channel: attitude::get(input, 6),
        highlighted: true,
        private_mesh: true,
    };
    let frame = crush::Frame {
        dt: attitude::get(input, 0),
        limit: attitude::get(input, 1),
        entity: attitude::get(input, 2),
    };
    let flags: Buf<u16, 4> = Buf::new([0; 4]);
    let order: Buf<u16, 8> = Buf::new([0; 8]);
    let notifications: Buf<u16, 8> = Buf::new([0; 8]);
    let destruction: Buf<u16, 8> = Buf::new([0; 8]);
    let mut ticks = lifecycle::Ticks {
        flags: flags.as_array(),
        flag_count: 2,
        order: ordered::List {
            items: order.as_array(),
            count: 0,
            capacity: 4,
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
    let mut step = 0u16;
    while step < 3 {
        let status = crush::tick(&mut surface, &mut runtime, &frame, &mut ticks, &mut queues);
        let base = step * 41;
        attitude::put(out, base, fix32::from_words(status, 0));
        let mut attached = fix32::zero();
        if surface.attached {
            attached = integer::from_i16(1);
        }
        attitude::put(out, base + 1, attached);
        attitude::put(out, base + 2, runtime.siren_timer);
        attitude::put(out, base + 3, runtime.sound_channel);
        attitude::put(out, base + 4, attitude::get(flags.as_array(), 0));
        attitude::put(out, base + 5, attitude::get(flags.as_array(), 1));
        attitude::put(
            out,
            base + 6,
            fix32::from_words(queues.notifications.count, 0),
        );
        let mut front = integer::from_i16(-1);
        if queues.notifications.count != 0 {
            front = attitude::get(queues.notifications.items, 0);
        }
        attitude::put(out, base + 7, front);
        let mut destination = out;
        i = 0;
        while i < 66 {
            destination[(base + 8) * 2 + i] = raw[i];
            i += 1;
        }
        step += 1;
    }
}
