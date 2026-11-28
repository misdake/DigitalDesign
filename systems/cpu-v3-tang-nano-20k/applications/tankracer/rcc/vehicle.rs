//! Physical single-vehicle chain with frozen classified geometry inputs.
//! Motion i32 slots: world velocity0..3 physical3..6 previous6..9 base9..12;
//! successful-frame rates12..16. Boost: extra_speed0 power1 loss2 timer3.
//! No collision actors, terrain lookup, sampling effects, RNG or rendered parts.
use crate::dsl_rt::*;
use crate::{attitude, drive, integer, suspension};
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(a: Array<u16>, i: u16) -> fix32 {
    attitude::get(a, i)
}
fn p(a: Array<u16>, i: u16, v: fix32) {
    attitude::put(a, i, v);
}
pub fn boost(s: Array<u16>, c: Array<u16>, f: Array<u16>) {
    if g(f, 27) != n(0) {
        let mut extra = g(s, 0) + g(c, 47);
        let maximum = fix32::from_words(41248, 7);
        if extra > maximum {
            extra = maximum;
        }
        p(s, 0, extra);
        p(s, 1, g(c, 258));
        p(s, 2, g(c, 259));
        p(s, 3, n(5000));
    }
    if g(s, 3) > n(0) {
        let dt = g(f, 0);
        let timer = (g(s, 3).lo_bits() as i16) - (dt.lo_bits() as i16);
        p(s, 3, n(timer));
        let mut power = g(s, 1) - integer::mul(g(s, 2), dt);
        if power < n(0) {
            power = n(0);
        }
        p(s, 1, power);
    }
    p(f, 16, g(s, 3));
    p(f, 17, g(s, 0));
    p(f, 18, g(s, 1));
}
// Publish only shared physical fields after the four contact responses finish.
pub fn contact_stage(
    a: Array<u16>,
    s: Array<u16>,
    m: Array<u16>,
    c: Array<u16>,
    e: Array<u16>,
    events: Array<u16>,
) {
    p(s, 24, g(a, 21));
    p(s, 25, g(a, 23));
    p(s, 31, g(m, 1));
    p(s, 32, g(m, 4));
    suspension::step(s, c, e, events);
    p(a, 21, g(s, 24));
    p(a, 23, g(s, 25));
    p(m, 1, g(s, 31));
    p(m, 4, g(s, 32));
    let mut i = 0u16;
    while i < 3 {
        p(a, 26 + i, g(s, 41 + i));
        i += 1;
    }
    p(m, 12, g(s, 29));
    p(m, 13, g(s, 28));
}
pub fn power_stage(
    a: Array<u16>,
    d: Array<u16>,
    m: Array<u16>,
    c: Array<u16>,
    f: Array<u16>,
    vertical: fix32,
) -> u16 {
    if g(f, 0) <= n(0) {
        return 4;
    }
    let big = drive::large(m);
    drive::transform(m, a, d, false, big);
    let mut i = 0u16;
    while i < 3 {
        p(d, 3 + i, g(a, 21 + i));
        i += 1;
    }
    p(d, 9, g(m, 15));
    p(d, 12, g(m, 14));
    let status = drive::step(d, c, f);
    i = 0;
    while i < 3 {
        p(a, 21 + i, g(d, 3 + i));
        i += 1;
    }
    p(m, 14, g(d, 12));
    p(m, 15, g(d, 9));
    if status != 0 {
        return status;
    }
    if g(f, 23) == n(0) && g(f, 28) < n(1) {
        drive::correct_yaw(d, c);
        p(a, 22, g(d, 4));
    }
    drive::transform(d, a, m, true, big);
    p(m, 1, g(m, 1) + vertical);
    0
}
pub fn integrate_position(m: Array<u16>, f: Array<u16>) {
    let dt = g(f, 0);
    let mut i = 0u16;
    while i < 3 {
        p(m, 6 + i, g(m, 3 + i));
        p(
            m,
            3 + i,
            g(m, 3 + i) + integer::trunc_shift(integer::mul(g(m, i), dt), 14),
        );
        i += 1;
    }
}
// Motion owns the attitude transaction: failure retains the old body/angles.
pub fn attitude_stage(
    a: Array<u16>,
    d: Array<u16>,
    c: Array<u16>,
    f: Array<u16>,
    sine: Array<i16>,
    angle: Array<i16>,
) -> bool {
    let local: Buf<u16, 58> = Buf::new([0; 58]);
    let la = local.as_array();
    let mut i = 0u16;
    while i < 29 {
        p(la, i, g(a, i));
        i += 1;
    }
    p(la, 24, g(d, 14));
    p(la, 25, g(d, 13));
    let control: Buf<u16, 3> = Buf::new([0; 3]);
    let mut input = control.as_array();
    fix32::store_words(input, 0, g(f, 0));
    let number = g(f, 26);
    input[2u16] = (number.lo_bits() % 10 + (number.hi_bits() % 10) * 6) % 10;
    let flags: Buf<u16, 1> = Buf::new([0; 1]);
    if !attitude::step(la, c, input, sine, angle, flags.as_array()) {
        return false;
    }
    i = 9;
    while i < 26 {
        p(a, i, g(la, i));
        i += 1;
    }
    true
}
// Physical fields of pose::update, then the successful-frame 40/dt rate scale.
pub fn finish(s: Array<u16>, a: Array<u16>, m: Array<u16>, pose: Array<u16>, f: Array<u16>) {
    publish_pose(s, a, m, pose);
    finish_rates(m, g(f, 0));
}
pub fn publish_pose(s: Array<u16>, a: Array<u16>, m: Array<u16>, pose: Array<u16>) {
    let mut i = 0u16;
    while i < 9 {
        p(a, i, g(a, 9 + i));
        i += 1;
    }
    i = 0;
    while i < 3 {
        p(a, 18 + i, g(m, 3 + i));
        p(m, 9 + i, g(m, 3 + i));
        i += 1;
    }
    let mut sum = n(0);
    i = 0;
    while i < 4 {
        sum = sum
            + integer::trunc_shift(
                integer::mul(g(s, i * 6 + 3), g(pose, 0)),
                g(pose, 1).lo_bits(),
            );
        i += 1;
    }
    p(m, 10, g(m, 10) + integer::sar(sum, 3));
}
pub fn finish_rates(m: Array<u16>, dt: fix32) {
    let mut i = 0;
    while i < 4 {
        p(
            m,
            12 + i,
            integer::div(integer::mul(g(m, 12 + i), n(40)), dt),
        );
        i += 1;
    }
}
