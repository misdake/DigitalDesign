// Persistent raw rows use the same layout as the debris kernel.
fn shatter_at(data: Array<u16>, offset: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(offset as i16).as_u16_array() }
}
fn shatter_copy(mut to: Array<u16>, from: Array<u16>, words: u16) {
    let mut i = 0u16;
    while i < words {
        to[i] = from[i];
        i += 1;
    }
}
pub fn run_shatter(
    input: Array<u16>,
    out: Array<u16>,
    storage: Array<u16>,
    mut info: Array<u16>,
    sine: Array<i16>,
    angle: Array<i16>,
) {
    let count = attitude::get(input, 0).lo_bits();
    if count > 64 {
        halt(0x7f50);
    }
    shatter_copy(storage, shatter_at(input, 12), 52);
    let mut part = 0u16;
    while part < count {
        let source = shatter_at(input, 64 + part * 54);
        let target = shatter_at(storage, 52 + part * 42);
        shatter_copy(target, source, 30);
        shatter_copy(shatter_at(target, 30), shatter_at(source, 36), 8);
        shatter_copy(shatter_at(target, 38), shatter_at(source, 48), 4);
        info[part * 3] = attitude::get(source, 22).lo_bits();
        info[part * 3 + 1] = attitude::get(source, 23).lo_bits();
        info[part * 3 + 2] = integer::sar(attitude::get(source, 26), 3).lo_bits();
        part += 1;
    }
    let state = shatter::State {
        header: storage,
        pieces: shatter_at(storage, 52),
        info: info,
        count: count,
        animation: shatter_at(storage, 18),
        has_animation: attitude::get(input, 1) != fix32::zero(),
        has_sprite: attitude::get(input, 2) != fix32::zero(),
        sprite_radius: attitude::get(input, 3),
    };
    let seed = shatter_at(input, 8);
    let controls = shatter_at(input, 64 + count * 54);
    let frames = attitude::get(input, 5).lo_bits();
    let incoming: Buf<u16, 6> = Buf::new([0; 6]);
    let mut frame = 0u16;
    while frame < frames {
        let c = shatter_at(controls, frame * 28);
        let action = attitude::get(c, 0);
        let mut status = fix32::zero();
        let mut prefix = fix32::zero();
        if action == integer::from_i16(0) {
            prefix = shatter::begin(
                &state,
                incoming.as_array(),
                shatter_at(c, 22),
                attitude::get(c, 4),
                sine,
            );
            status = prefix;
            if prefix == integer::from_i16(1) {
                let value = attitude::get(state.header, 0);
                let mask = attitude::get(c, 6);
                attitude::put(
                    state.header,
                    0,
                    fix32::from_words(
                        value.lo_bits() ^ mask.lo_bits(),
                        value.hi_bits() ^ mask.hi_bits(),
                    ),
                );
                let mut draw = 0u16;
                while draw < attitude::get(c, 7).lo_bits() {
                    attitude::put(seed, 0, random::next(attitude::get(seed, 0)));
                    draw += 1;
                }
                status = shatter::activate(
                    &state,
                    shatter_at(c, 16),
                    incoming.as_array(),
                    seed,
                    attitude::get(c, 5),
                    sine,
                );
            }
        } else if action == integer::from_i16(1) {
            let update = shatter::Frame {
                entity: attitude::get(c, 2),
                dt: attitude::get(c, 1),
                group: attitude::get(c, 3),
                limit: attitude::get(c, 4),
                sine: sine,
                angle: angle,
            };
            status = shatter::tick(&state, &update, seed);
        } else if action == integer::from_i16(2) {
            shatter::animate(state.animation, attitude::get(c, 1));
        } else if action == integer::from_i16(3) {
            if shatter::affected(shatter_at(c, 22), shatter_at(c, 16), attitude::get(c, 1)) {
                status = integer::from_i16(1);
            }
        } else {
            halt(0x7f51);
        }
        attitude::put(out, 0, status);
        attitude::put(out, 1, prefix);
        attitude::put(out, 2, attitude::get(seed, 0));
        shatter_copy(shatter_at(out, 6), state.header, 18);
        shatter_copy(shatter_at(out, 24), state.animation, 34);
        part = 0;
        while part < count {
            let source = shatter_at(input, 64 + part * 54);
            let current = shatter_at(state.pieces, part * 42);
            let target = shatter_at(out, 58 + part * 54);
            shatter_copy(target, current, 30);
            shatter_copy(shatter_at(target, 30), shatter_at(source, 30), 6);
            shatter_copy(shatter_at(target, 36), shatter_at(current, 30), 8);
            attitude::put(target, 22, integer::from_i16(info[part * 3] as i16));
            attitude::put(target, 23, integer::from_i16(info[part * 3 + 1] as i16));
            shatter_copy(shatter_at(target, 48), shatter_at(current, 38), 4);
            attitude::put(
                target,
                26,
                fmul_lo32(
                    fix32::from_words(info[part * 3 + 2], 0),
                    integer::from_i16(8),
                ),
            );
            part += 1;
        }
        let mut progress = unsafe { Ptr::from_addr(0xfffe).as_u16_array() };
        progress[0u16] = frame + 1;
        frame += 1;
    }
}
