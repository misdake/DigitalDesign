// Production impact/pool modules, independently checked against reference state.
fn run_impact(input: Array<u16>, out: Array<u16>, sine: Array<i16>, angle: Array<i16>) {
    let lines: Buf<u16, 1600> = Buf::new([0; 1600]);
    let tail: Buf<u16, 16> = Buf::new([0; 16]);
    let batches: Buf<impact::Batch, 4> = Buf::new([
        impact::batch_zero(),
        impact::batch_zero(),
        impact::batch_zero(),
        impact::batch_zero(),
    ]);
    let mut state = impact::State {
        page: 65535,
        batches: batches.as_array(),
        next: attitude::get(input, 7),
    };
    let cooldown: Buf<u16, 2> = Buf::new([0; 2]);
    attitude::put(cooldown.as_array(), 0, attitude::get(input, 2));
    let seed: Buf<u16, 2> = Buf::new([7, 0]);
    let mut vehicle = impact::Vehicle {
        entity: integer::from_i16(1),
        cooldown: cooldown.as_array(),
        contacts: unsafe { input.as_ptr().add(18).as_u16_array() },
        contact_stride: 3,
        forward: attitude::get(input, 5),
        basis: unsafe { input.as_ptr().add(42).as_u16_array() },
        has_basis: attitude::get(input, 4) != fix32::zero(),
    };
    let mut context = impact::Context {
        frame: integer::from_i16(17),
        seed: seed.as_array(),
        pool: effect_pool::Pool {
            page: 65535,
            lines: lines.as_array(),
            count: fix32::zero(),
            overflow: effect_pool::line_zero(),
            exhausted: tail.as_array(),
        },
        sine: sine,
        angle: angle,
        forward_offset: unsafe { input.as_ptr().add(60).as_u16_array() },
        reverse_offset: unsafe { input.as_ptr().add(66).as_u16_array() },
    };
    let layout = unsafe { Ptr::from_addr(0x1ffd).read(0) };
    let banked = layout != 0;
    let original_page = read_dseg_page(2);
    if banked {
        context.pool.page = 150;
        context.pool.lines = unsafe { Ptr::from_addr(0x9000).as_u16_array() };
        let saved = effect_pool::select(&context.pool);
        let mut records = context.pool.lines;
        let mut word = 0u16;
        while word < 1600 {
            records[word] = 0;
            word += 1;
        }
        effect_pool::restore(saved);
    }
    if layout == 2 {
        state.page = 150;
        state.batches = unsafe { Ptr::from_addr(0xa000).as_array::<impact::Batch>() };
        let saved = impact::select(&state);
        let mut rows = state.batches;
        let mut i = 0u16;
        while i < 4 {
            rows[i] = impact::batch_zero();
            i += 1;
        }
        effect_pool::restore(saved);
    }
    effect_pool::reset(&mut context.pool);
    let variant = attitude::get(input, 3);
    if variant == integer::from_i16(1) || variant == integer::from_i16(2) {
        let zero: Buf<u16, 12> = Buf::new([0; 12]);
        let colors: Buf<u16, 4> = Buf::new([0; 4]);
        let mut i = 0u16;
        while i < 100 {
            effect_pool::allocate(&mut context.pool, zero.as_array(), colors.as_array());
            i += 1;
        }
        if variant == integer::from_i16(2) {
            context.pool.count = integer::from_i16(99);
        }
    }
    if variant == integer::from_i16(3) {
        let saved = impact::select(&state);
        state.batches[0u16].active = true;
        state.batches[0u16].particles[3u16].line = integer::from_i16(101);
        effect_pool::restore(saved);
    }
    let phases: Buf<u16, 8> = Buf::new([0; 8]);
    let phase = phases.as_array();
    let mut i = 0u16;
    while i < 4 {
        attitude::put(phase, i, integer::from_i16(-99));
        i += 1;
    }
    let mut failed = false;
    i = 0;
    while i < attitude::get(input, 8).lo_bits() {
        let status = impact::emit(
            &mut state,
            &mut vehicle,
            attitude::get(input, 1),
            attitude::get(input, 0),
            &mut context,
        );
        attitude::put(phase, 0, fix32::from_words(status, 0));
        if status >= 2 {
            failed = true;
            break;
        }
        i += 1;
    }
    if !failed {
        let status = impact::tick(
            &mut state,
            &mut vehicle,
            attitude::get(input, 6),
            &mut context,
        );
        attitude::put(phase, 1, fix32::from_words(status, 0));
        failed = status != 0;
    }
    if !failed {
        context.frame = context.frame + integer::from_i16(1);
        let status = impact::tick(
            &mut state,
            &mut vehicle,
            attitude::get(input, 6),
            &mut context,
        );
        attitude::put(phase, 2, fix32::from_words(status, 0));
        failed = status != 0;
    }
    if !failed {
        context.frame = context.frame + integer::from_i16(1);
        let status = impact::tick(
            &mut state,
            &mut vehicle,
            attitude::get(input, 6),
            &mut context,
        );
        attitude::put(phase, 3, fix32::from_words(status, 0));
    }
    i = 0;
    while i < 4 {
        attitude::put(out, i, attitude::get(phase, i));
        i += 1;
    }
    impact_capture::write(&mut state, &vehicle, &mut context, out);
    if read_dseg_page(2) != original_page {
        halt(0x7f72);
    }
}
