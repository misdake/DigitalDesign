fn run_blast(input: Array<u16>, out: Array<u16>) {
    let storage: Buf<u16, 30> = Buf::new([0; 30]);
    let mut state = storage.as_array();
    let mut i = 0u16;
    while i < 30 {
        state[i] = input[i];
        i += 1;
    }
    let seed = unsafe { state.as_ptr().add(28).as_u16_array() };
    let mut target = blast::Target {
        state: state,
        velocity: unsafe { state.as_ptr().add(14).as_u16_array() },
        angular: unsafe { state.as_ptr().add(20).as_u16_array() },
        marker: attitude::get(input, 13),
    };
    let frame = blast::Input {
        position: unsafe { input.as_ptr().add(30).as_u16_array() },
        source: unsafe { input.as_ptr().add(36).as_u16_array() },
        mode: attitude::get(input, 21),
        damage: attitude::get(input, 22) != fix32::zero(),
        max_health: attitude::get(input, 23),
        record_axes: attitude::get(input, 24) != fix32::zero(),
        mark: attitude::get(input, 25) != fix32::zero(),
    };
    let mut step = 0u16;
    while step < 3 {
        let status = blast::apply(&mut target, &frame, seed);
        attitude::put(out, step * 16, fix32::from_words(status, 0));
        i = 0;
        while i < 13 {
            attitude::put(out, step * 16 + 1 + i, attitude::get(state, i));
            i += 1;
        }
        attitude::put(out, step * 16 + 14, target.marker);
        attitude::put(out, step * 16 + 15, attitude::get(seed, 0));
        step += 1;
    }
}
