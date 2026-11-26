//! Complete post-collision motion: integrate, articulate, pose, then finish.
use crate::dsl_rt::*;
use crate::{attitude, body, integer, vehicle_pose};
pub struct State {
    pub poses: vehicle_pose::State,
    pub previous: Array<u16>,
    pub velocity: Array<u16>,
    pub angular: Array<u16>,
    pub acceleration: Array<u16>,
    pub pivot: Array<u16>,
    pub articulation: Array<u16>,
    pub lean: Array<u16>,
    pub contacts: Array<u16>,
    pub contact_stride: u16,
    pub contact_gap: u16,
    pub travel: Array<u16>,
    pub hull: Array<u16>,
    // One canonical boolean word, also bindable to a live RCC bool field.
    pub ready: Array<u16>,
    pub rates: Array<u16>,
    pub gun: Array<i16>,
    pub gun_binding: Array<u16>,
}
pub struct Config {
    pub attitude: Array<u16>,
    pub body: Array<u16>,
    pub pose: Array<u16>,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
pub struct Frame {
    pub dt: fix32,
    pub number: fix32,
    pub player: bool,
    pub mode: fix32,
    pub pose: vehicle_pose::Environment,
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
fn copy(mut to: Array<u16>, from: Array<u16>, words: u16) {
    let mut i = 0u16;
    while i < words {
        to[i] = from[i];
        i += 1;
    }
}
pub fn integrate_and_pose(state: &State, config: &Config, frame: &Frame) -> fix32 {
    if frame.dt <= n(0) {
        return n(-2);
    }
    copy(state.previous, state.poses.physical_position, 6);
    let mut i = 0u16;
    while i < 3 {
        p(
            state.poses.physical_position,
            i,
            g(state.poses.physical_position, i)
                + integer::trunc_shift(integer::mul(g(state.velocity, i), frame.dt), 14),
        );
        i += 1;
    }
    // The source commits this attitude transaction only on success. Integrated
    // physical/previous positions survive a normalization failure.
    let transaction: Buf<u16, 58> = Buf::new([0; 58]);
    let a = transaction.as_array();
    copy(a, state.poses.reference, 18);
    let mut axis = 0u16;
    while axis < 9 {
        p(a, 9 + axis, g(state.poses.body_basis, axis));
        axis += 1;
    }
    axis = 0;
    while axis < 3 {
        p(a, 18 + axis, g(state.poses.body_position, axis));
        p(a, 21 + axis, g(state.angular, axis));
        p(a, 26 + axis, g(state.pivot, axis));
        axis += 1;
    }
    p(a, 24, g(state.acceleration, 1));
    p(a, 25, g(state.acceleration, 0));
    let controls: Buf<u16, 3> = Buf::new([0; 3]);
    let mut c = controls.as_array();
    fix32::store_words(c, 0, frame.dt);
    c[2u16] = frame.number.lo_bits();
    let flags: Buf<u16, 1> = Buf::new([0; 1]);
    if !attitude::step(
        a,
        config.attitude,
        c,
        config.sine,
        config.angle,
        flags.as_array(),
    ) {
        return n(-2);
    }
    axis = 0;
    while axis < 9 {
        p(state.poses.body_basis, axis, g(a, 9 + axis));
        axis += 1;
    }
    axis = 0;
    while axis < 3 {
        p(state.poses.body_position, axis, g(a, 18 + axis));
        p(state.angular, axis, g(a, 21 + axis));
        axis += 1;
    }
    let body_config: Buf<u16, 14> = Buf::new([0; 14]);
    let b = body_config.as_array();
    copy(b, config.body, 12);
    p(b, 6, frame.dt);
    if body::articulate(state.articulation, b, state.gun, state.gun_binding) != 0 {
        return n(-1);
    }
    if frame.player && frame.mode != n(2) && frame.mode != n(3) {
        body::lean(state.lean, state.acceleration, b);
    }
    let pose_values: Buf<u16, 36> = Buf::new([0; 36]);
    let e = pose_values.as_array();
    copy(e, frame.pose.values, 36);
    // Articulation supplies these dependent values at this exact point.
    p(e, 6, g(state.articulation, 0));
    p(e, 7, g(state.articulation, 2));
    vehicle_pose::update(
        &state.poses,
        config.pose,
        state.travel,
        &vehicle_pose::Environment {
            timer: frame.pose.timer,
            values: e,
        },
        config.sine,
        config.angle,
    )
}
// Called only after the complete immediate effects stage succeeds (dt > 0).
pub fn finish(state: &State, dt: fix32) {
    body::hull_points(
        state.hull,
        state.poses.physical_position,
        state.previous,
        state.contacts,
        state.contact_stride,
        state.contact_gap,
    );
    let mut ready = state.ready;
    ready[0u16] = 1;
    let mut i = 0u16;
    while i < 4 {
        p(
            state.rates,
            i,
            integer::div(integer::mul(g(state.rates, i), n(40)), dt),
        );
        i += 1;
    }
}
