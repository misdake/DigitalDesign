//! Fresh vehicle state from model metadata and an authored spawn transform.
use crate::dsl_rt::*;
use crate::{attitude, integer, vehicle_pose};

// Word offsets in a contiguous vehicle row. Nine rows fit in one 32-KiB bank.
pub const WORDS: u16 = 1760;
pub const POWER: u16 = 339;
pub const SPRINGS: u16 = 357;
pub const SAMPLING: u16 = 437;
pub const PREFIX: u16 = 643;
pub const FIELDS: u16 = 701;
pub const EFFECTS: u16 = 921;
pub const POSE: u16 = 1661;
pub const POSE_INPUT: u16 = 1715;

pub struct Model {
    pub pose: Array<u16>,
    pub contacts: Array<u16>,
    pub shadow: Array<i16>,
    pub shadow_vertices: u16,
    pub shadow_radius: u16,
    // Reset sidegrip gain/shift, live gain/shift, alternate lateral/rotation.
    pub control: Array<u16>,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn at(v: Array<u16>, word: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(word as i16).as_u16_array() }
}
fn g(v: Array<u16>, index: u16) -> fix32 {
    attitude::get(v, index)
}
fn p(v: Array<u16>, index: u16, value: fix32) {
    attitude::put(v, index, value);
}
fn copy(mut dst: Array<u16>, src: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        dst[i] = src[i];
        i += 1;
    }
}
fn identity(dst: Array<u16>) {
    p(dst, 0, n(4096));
    p(dst, 4, n(4096));
    p(dst, 8, n(4096));
}
// Spawn is matrix9/position3. Input is entity/class/player, ambient/session,
// ambient phase and gun pivot XYZ. The caller publishes the row only on success.
pub fn construct(
    mut dst: Array<u16>,
    model: &Model,
    spawn: Array<u16>,
    input: Array<u16>,
) -> fix32 {
    if model.shadow_vertices < 4 {
        return n(-4);
    }
    let mut i = 0u16;
    while i < WORDS {
        dst[i] = 0;
        i += 1;
    }
    copy(dst, at(spawn, 18), 6);
    copy(at(dst, 6), spawn, 18);
    copy(at(dst, 24), at(spawn, 18), 6);
    copy(at(dst, 30), spawn, 18);
    identity(at(dst, 298));
    i = 0;
    while i < 12 {
        dst[316 + i] = model.shadow[i] as u16;
        i += 1;
    }
    dst[328u16] = model.shadow_radius;
    i = 0;
    while i < 4 {
        p(at(dst, SPRINGS), i * 6 + 2, fix32::from_words(i, 0));
        let point = at(dst, SAMPLING + i * 34);
        copy(point, at(model.contacts, i * 6), 6);
        p(point, 16, n(-1));
        i += 1;
    }
    p(at(dst, SAMPLING), 102, n(-1));
    let prefix = at(dst, PREFIX + 8);
    p(prefix, 0, n(1));
    p(prefix, 5, n(-1));
    copy(at(prefix, 12), model.control, 8);
    let fields = at(dst, FIELDS);
    copy(fields, input, 6);
    p(fields, 12, n(1));
    p(fields, 43, g(model.control, 4));
    p(fields, 44, g(model.control, 5));
    p(fields, 92, n(1));
    let pose = at(dst, POSE);
    copy(pose, model.pose, 54);
    copy(at(pose, 48), at(input, 12), 6);
    let env = at(dst, POSE_INPUT);
    p(env, 1, g(input, 3));
    p(env, 2, g(input, 4));
    p(env, 3, g(input, 5));
    p(env, 8, n(1));
    let state = vehicle_pose::State {
        physical_position: dst,
        reference: at(dst, 6),
        base_position: at(dst, 24),
        body_basis: at(dst, 30),
        body_position: at(dst, 48),
        tracks: at(dst, 54),
        turret: at(dst, 102),
        gun: at(dst, 126),
        attachments: at(dst, 150),
        bottom_position: at(dst, 198),
        recoil: at(dst, 204),
    };
    let mut count = 1u16;
    if g(input, 2) != n(0) {
        count = 2;
    }
    i = 0;
    while i < count {
        let status = vehicle_pose::update(
            &state,
            pose,
            at(dst, 284),
            &vehicle_pose::Environment {
                timer: 0,
                values: env,
            },
            model.sine,
            model.angle,
        );
        if status != n(0) {
            return status;
        }
        i += 1;
    }
    n(0)
}
