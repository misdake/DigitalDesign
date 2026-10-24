//! First-person camera using the classic scene's right/up/back lighting basis.
use crate::dsl_rt::*;
mod gpu_window;
use gpu_window::*;
mod boot_resources;
use boot_resources::*;
mod gpu_transform;
use gpu_transform::*;
mod stick;
use stick::*;

static CAMERA_STICK_CONFIG: StickConfig = StickConfig {
    min_x: 0,
    center_x: 128,
    max_x: 255,
    min_y: 0,
    center_y: 128,
    max_y: 255,
    inner: 24,
    outer: 256,
};
static CAMERA_STICK: StickPrepared = StickPrepared {
    min_x: 0,
    center_x: 0,
    max_x: 0,
    min_y: 0,
    center_y: 0,
    max_y: 0,
    negative_x: fix32::zero(),
    positive_x: fix32::zero(),
    negative_y: fix32::zero(),
    positive_y: fix32::zero(),
    inner: fix32::zero(),
    inverse_range: fix32::zero(),
    valid: 0,
};

// State[12]: eye.xyz, yaw, pitch as five Q16 pairs, last wall ms, initialized.
pub fn demo_camera_reset(mut state: Array<u16>, now: u16) {
    gpu_fstore(state, 0, fix32::zero());
    gpu_fstore(state, 1, fix32::from_words(14418, 0));
    gpu_fstore(state, 2, fix32::from_words(52429, 2));
    gpu_fstore(state, 3, fix32::zero());
    gpu_fstore(state, 4, fix32::from_words(59932, 65535));
    state[10u16] = now;
    state[11u16] = 1;
    CAMERA_STICK = stick_prepare(view_of(&mut CAMERA_STICK_CONFIG));
}

pub fn demo_camera_init(out: Array<u16>, state: Array<u16>, now: u16) -> bool {
    demo_camera_reset(state, now);
    // Preserve the exact original phase0 matrix/light while stationary.
    gpu_window_copy(out, DEMO_CAMERA_FIRST, 0, 45, true)
}

fn demo_light_unit(code: u16) -> fix32 {
    fix32::from_words(code << 2, ((code as i16) >> 14) as u16)
}
fn demo_light_quantize(mut out: Array<u16>) -> bool {
    let mut largest: u16 = 42;
    let mut i: u16 = 43;
    while i < 45 {
        if fix32::from_int(out[i] as i16).abs() > fix32::from_int(out[largest] as i16).abs() {
            largest = i;
        }
        i += 1;
    }
    let mut attempt: u16 = 0;
    while attempt < 16 {
        let light = vec3::new(
            demo_light_unit(out[42u16]),
            demo_light_unit(out[43u16]),
            demo_light_unit(out[44u16]),
        );
        let length2 = fdot(light, light);
        // DOTSTORE floors the squared F14-code sum /4096. This inner window
        // satisfies the encoder's exact 2^28 +/-32768 unit-vector contract.
        if length2 >= fix32::from_words(65529, 0) && length2 <= fix32::from_words(7, 1) {
            return true;
        }
        let delta: i16 = if length2 < fix32::from_int(1) { 1 } else { -1 };
        let code = out[largest] as i16;
        out[largest] = if code < 0 {
            (code - delta) as u16
        } else {
            (code + delta) as u16
        };
        attempt += 1;
    }
    false
}
fn demo_camera_row(out: Array<u16>, first: u16) -> vec3 {
    vec3::new(
        gpu_fload(out, first),
        gpu_fload(out, first + 1),
        gpu_fload(out, first + 2),
    )
}
fn demo_camera_basis(mut out: Array<u16>, yaw: fix32, pitch: fix32) -> bool {
    let y = fsincos(yaw);
    let p = fsincos(pitch);
    let right = vec3::new(-y.y(), fix32::zero(), y.x());
    let up = vec3::new(y.x() * p.x(), p.y(), y.y() * p.x());
    let back = vec3::new(y.x() * p.y(), -p.x(), y.y() * p.y());
    let k = fix32::from_words(51200, 0);
    let mut col: u16 = 0;
    while col < 3 {
        let r = if col == 0 {
            right.x()
        } else {
            if col == 1 {
                right.y()
            } else {
                right.z()
            }
        };
        let u = if col == 0 {
            up.x()
        } else {
            if col == 1 {
                up.y()
            } else {
                up.z()
            }
        };
        let b = if col == 0 {
            back.x()
        } else {
            if col == 1 {
                back.y()
            } else {
                back.z()
            }
        };
        gpu_fstore(out, col, k * r);
        gpu_fstore(out, 4 + col, -k * u);
        gpu_fstore(out, 8 + col, -b);
        gpu_fstore(out, 12 + col, r);
        gpu_fstore(out, 15 + col, u);
        gpu_fstore(out, 18 + col, b);
        col += 1;
    }
    let world_light = vec3::new(
        fix32::from_words(44951, 65535),
        fix32::from_words(36596, 0),
        fix32::from_words(50319, 0),
    );
    let scale = fix32::from_int(16384);
    out[42u16] = (fdot(right, world_light) * scale).round().to_int() as u16;
    out[43u16] = (fdot(up, world_light) * scale).round().to_int() as u16;
    out[44u16] = (fdot(back, world_light) * scale).round().to_int() as u16;
    demo_light_quantize(out)
}
fn demo_camera_position(value: fix32) -> fix32 {
    if value < -fix32::from_int(16) {
        return -fix32::from_int(16);
    }
    if value > fix32::from_int(16) {
        return fix32::from_int(16);
    }
    value
}

pub fn demo_camera_update(
    mut out: Array<u16>,
    mut state: Array<u16>,
    input: Array<u16>,
    now: u16,
) -> bool {
    let mut elapsed = now - state[10u16];
    state[10u16] = now;
    if elapsed > 100 {
        elapsed = 100;
    }
    if state[11u16] == 0 || input[0u16] == 0 || elapsed == 0 {
        return true;
    }
    let analog = input[1u16] == 115 || input[1u16] == 121;
    let left = stick_read(view_of(&mut CAMERA_STICK), input[5u16], input[6u16], analog);
    let right = stick_read(view_of(&mut CAMERA_STICK), input[3u16], input[4u16], analog);
    // Scale the raw millisecond fraction, avoiding a coarse Q16 1/1000 seed.
    let distance = fix32::from_words(elapsed, 0) * fix32::from_words(4719, 131);
    let angle = fix32::from_words(elapsed, 0) * fix32::from_words(39846, 196);
    let old_yaw = gpu_fload(state, 3);
    let old_pitch = gpu_fload(state, 4);
    let mut yaw = old_yaw + right.x * angle;
    let mut pitch = old_pitch - right.y * angle;
    let pi = fix32::from_words(9279, 3);
    let turn = fix32::from_words(18559, 6);
    if yaw > pi {
        yaw -= turn;
    }
    if yaw < -pi {
        yaw += turn;
    }
    let limit = fix32::from_words(32768, 1);
    if pitch > limit {
        pitch = limit;
    }
    if pitch < -limit {
        pitch = -limit;
    }
    let rotated = yaw != old_yaw || pitch != old_pitch;
    if rotated {
        if !demo_camera_basis(out, yaw, pitch) {
            return false;
        }
        gpu_fstore(state, 3, yaw);
        gpu_fstore(state, 4, pitch);
    }
    let shoulders = input[2u16] & 3072;
    let strafe = left.x * distance;
    let forward = -left.y * distance;
    let mut vertical = fix32::zero();
    if shoulders == 1024 {
        vertical = -distance;
    }
    if shoulders == 2048 {
        vertical = distance;
    }
    if !rotated && strafe == fix32::zero() && forward == fix32::zero() && vertical == fix32::zero()
    {
        return true;
    }
    let y = fsincos(yaw);
    let eye = vec3::new(
        demo_camera_position(gpu_fload(state, 0) - y.y() * strafe - y.x() * forward),
        demo_camera_position(gpu_fload(state, 1) + vertical),
        demo_camera_position(gpu_fload(state, 2) + y.x() * strafe - y.y() * forward),
    );
    gpu_fstore(state, 0, eye.x());
    gpu_fstore(state, 1, eye.y());
    gpu_fstore(state, 2, eye.z());
    let k = fix32::from_words(51200, 0);
    gpu_fstore(out, 3, -k * fdot(demo_camera_row(out, 12), eye));
    gpu_fstore(out, 7, k * fdot(demo_camera_row(out, 15), eye));
    gpu_fstore(out, 11, fdot(demo_camera_row(out, 18), eye));
    true
}
