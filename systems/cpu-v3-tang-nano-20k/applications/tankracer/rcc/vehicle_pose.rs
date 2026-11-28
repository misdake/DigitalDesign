//! Complete part poses over caller-owned columns. Basis precedes XYZ in each pose.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, trig_lookup};

pub struct State {
    pub physical_position: Array<u16>,
    pub reference: Array<u16>,
    pub base_position: Array<u16>,
    pub body_basis: Array<u16>,
    pub body_position: Array<u16>,
    pub tracks: Array<u16>,
    pub turret: Array<u16>,
    pub gun: Array<u16>,
    pub attachments: Array<u16>,
    pub bottom_position: Array<u16>,
    // Factor, then shift, as extended byte fields.
    pub recoil: Array<u16>,
}
pub struct Environment {
    pub timer: u16,
    pub values: Array<u16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn at(v: Array<u16>, word: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(word as i16).as_u16_array() }
}
fn copy(mut out: Array<u16>, from: Array<u16>, words: u16) {
    let mut i = 0u16;
    while i < words {
        out[i] = from[i];
        i += 1;
    }
}
fn axis(basis: Array<u16>, local: Array<u16>, i: u16) -> fix32 {
    integer::sar(
        fmul_lo32(g(basis, i), g(local, 0))
            + fmul_lo32(g(basis, i + 3), g(local, 1))
            + fmul_lo32(g(basis, i + 6), g(local, 2)),
        12,
    )
}
fn wobble(table: Array<i16>, phase: fix32) -> fix32 {
    integer::sar(
        fmul_lo32(
            trig_lookup::index(table, ((phase.lo_bits() & 511) * 128) >> 2),
            n(3),
        ),
        7,
    )
}
// Config: travel gain/shift, track length, track offsets[2], turret/gun offsets,
// attachments[2], bottom offset, gun pivot (27 i32). Environment: timer,
// ambient/session flags, phase, recoil mode, signed turret yaw, recoil/pitch,
// external kind (0 none/1 reference/2 custom) and custom matrix (18 i32).
pub fn update(
    state: &State,
    config: Array<u16>,
    travel: Array<u16>,
    input: &Environment,
    sine: Array<i16>,
    angle: Array<i16>,
) -> fix32 {
    let env = input.values;
    copy(state.body_position, state.physical_position, 6);
    copy(state.reference, state.body_basis, 18);
    let mut wave = n(0);
    if input.timer != 0 {
        wave = wobble(sine, fix32::from_words(input.timer, 0));
        p(state.body_position, 1, g(state.body_position, 1) + wave);
    }
    if g(env, 1) != n(0) && g(env, 2) == n(0) {
        wave = wobble(sine, g(env, 3));
        p(state.body_position, 1, g(state.body_position, 1) + wave);
    }
    let local_buffer: Buf<u16, 6> = Buf::new([0; 6]);
    let local = local_buffer.as_array();
    let mut side = 0u16;
    while side < 2 {
        let front = integer::trunc_shift(g(travel, side * 2), g(config, 1).lo_bits());
        let rear = integer::trunc_shift(g(travel, side * 2 + 1), g(config, 1).lo_bits());
        let mut delta = front - rear;
        if delta < n(0) {
            delta = -delta;
        }
        let mut pitch = attitude::angle(angle, delta, g(config, 2));
        if pitch == n(-1) {
            return n(-2);
        }
        if front < rear {
            pitch = -pitch;
        }
        let track = at(state.tracks, side * 24);
        copy(track, state.body_basis, 18);
        p(local, 0, g(config, 3 + side * 3));
        p(
            local,
            1,
            g(config, 4 + side * 3) + integer::sar(front + rear, 1) - wave,
        );
        p(local, 2, g(config, 5 + side * 3));
        let mut i = 0u16;
        while i < 3 {
            p(
                track,
                9 + i,
                axis(track, local, i) + g(state.body_position, i),
            );
            i += 1;
        }
        attitude::rotate(track, 0, 0, pitch, sine);
        side += 1;
    }
    copy(state.turret, state.body_basis, 18);
    let mut shift = 1u16;
    let mut factor = n(1);
    if g(env, 4) == n(3) {
        shift = 2;
        factor = n(3);
    } else if g(env, 4) == n(5) {
        shift = 0;
    }
    p(state.recoil, 0, factor);
    p(state.recoil, 1, fix32::from_words(shift, 0));
    let amount = fmul_lo32(integer::trunc_shift(g(env, 6), shift), factor);
    let yaw = g(env, 5);
    let sin = trig_lookup::index(sine, (yaw.lo_bits() & 65532) >> 2);
    let cos = trig_lookup::index(sine, 4096 + ((yaw.lo_bits() & 65532) >> 2));
    p(local, 0, integer::trunc_shift(-fmul_lo32(amount, sin), 12));
    p(local, 1, n(0));
    p(local, 2, integer::trunc_shift(fmul_lo32(amount, cos), 12));
    if g(env, 8) == n(1) {
        collision::transform(local, state.reference, local, false);
    } else if g(env, 8) == n(2) {
        collision::transform(local, at(env, 18), local, false);
    }
    let mut i = 0u16;
    while i < 3 {
        p(
            state.turret,
            9 + i,
            axis(state.turret, at(config, 18), i) + g(state.body_position, i) + g(local, i),
        );
        i += 1;
    }
    attitude::rotate(state.turret, 0, 1, yaw, sine);
    copy(state.gun, state.turret, 18);
    i = 0;
    while i < 3 {
        p(local, i, g(config, 12 + i));
        i += 1;
    }
    p(local, 2, g(local, 2) + g(env, 6));
    attitude::rotate(state.gun, 0, 0, -g(env, 7), sine);
    // The unchanged turret basis is the gun basis before its pivot rotation.
    i = 0;
    while i < 3 {
        p(
            local,
            i,
            g(local, i) + axis(state.turret, at(config, 48), i)
                - axis(state.gun, at(config, 48), i),
        );
        i += 1;
    }
    i = 0;
    while i < 3 {
        p(
            state.gun,
            9 + i,
            axis(state.turret, local, i) + g(state.turret, 9 + i),
        );
        i += 1;
    }
    side = 0;
    while side < 2 {
        let attachment = at(state.attachments, side * 24);
        copy(attachment, state.body_basis, 18);
        i = 0;
        while i < 3 {
            p(
                attachment,
                9 + i,
                axis(attachment, at(config, 30 + side * 6), i) + g(state.body_position, i),
            );
            i += 1;
        }
        side += 1;
    }
    copy(state.base_position, state.physical_position, 6);
    let mut sum = n(0);
    i = 0;
    while i < 4 {
        sum = sum
            + integer::trunc_shift(
                fmul_lo32(g(travel, i), g(config, 0)),
                g(config, 1).lo_bits(),
            );
        i += 1;
    }
    p(
        state.base_position,
        1,
        g(state.base_position, 1) + integer::sar(sum, 3),
    );
    copy(state.physical_position, state.body_position, 6);
    i = 0;
    while i < 3 {
        p(
            state.bottom_position,
            i,
            axis(state.body_basis, at(config, 42), i) + g(state.body_position, i),
        );
        i += 1;
    }
    n(0)
}
