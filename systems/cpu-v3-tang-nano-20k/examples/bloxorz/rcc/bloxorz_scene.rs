//! Bloxorz-specific Q16.16 scene preparation using the shared D2 transforms.
//! Source mesh coordinates are twice the host reference's local coordinates.
use crate::dsl_rt::*;

pub fn blox_camera(width: u16, depth: u16, pose: Array<u16>, view: Array<u16>) -> bool {
    if width == 0 || width > 15 || depth == 0 || depth > 10 {
        return false;
    }
    let one = fix32::from_int(1);
    let half = fix32::from_words(32768, 0);
    let target = fix32::from_words(39322, 0);
    let angles = vec3::new(
        fix32::from_words(25329, 2),
        fix32::from_words(17157, 0),
        fix32::zero(),
    );
    gpu_pose_set(pose, vec3::zero(), angles, vec3::new(one, one, one));
    if !gpu_camera_view(view, pose) {
        return false;
    }
    let vx = vec3::new(gpu_fload(view, 0), gpu_fload(view, 1), gpu_fload(view, 2));
    let vy = vec3::new(gpu_fload(view, 4), gpu_fload(view, 5), gpu_fload(view, 6));
    let vz = vec3::new(gpu_fload(view, 8), gpu_fload(view, 9), gpu_fload(view, 10));
    let hw = fix32::from_int(width as i16) * half;
    let hd = fix32::from_int(depth as i16) * half;
    let mut distance = fix32::from_int(8);
    let mut corner: u16 = 0;
    while corner < 8 {
        let x = if corner & 1 == 0 { -hw } else { hw };
        let y = if corner & 2 == 0 {
            -target
        } else {
            fix32::from_int(2) - target
        };
        let z = if corner & 4 == 0 { -hd } else { hd };
        let point = vec3::new(x, y, z);
        let forward = fdot(vz, point);
        let need_x = fdot(vx, point).abs() * fix32::from_words(26396, 3) - forward;
        let need_y = fdot(vy, point).abs() * fix32::from_words(43993, 5) - forward;
        if need_x > distance {
            distance = need_x;
        }
        if need_y > distance {
            distance = need_y;
        }
        corner += 1;
    }
    distance = distance * fix32::from_words(1966, 1);
    gpu_pose_set(
        pose,
        vec3::new(
            -vz.x() * distance,
            target - vz.y() * distance,
            -vz.z() * distance,
        ),
        angles,
        vec3::new(one, one, one),
    );
    gpu_camera_view(view, pose)
}

// x/z identify the minimum-footprint cell. Fraction is angular fraction, not
// elapsed time. The caller retains input/animation ownership until completion.
pub fn blox_roll_pose(
    out: Array<u16>,
    level: Array<u16>,
    state: Array<u16>,
    dir: u16,
    fraction: fix32,
    drop: fix32,
) -> bool {
    let width = level[0u16];
    let depth = level[1u16];
    let x = state[0u16] as i16;
    let z = state[1u16] as i16;
    let orient = state[2u16];
    if width == 0
        || width > 15
        || depth == 0
        || depth > 10
        || orient > 2
        || state[9u16] != 0
        || dir > 3
        || x < -2
        || x > (width as i16) + 1
        || z < -2
        || z > (depth as i16) + 1
        || fraction < fix32::zero()
        || fraction > fix32::from_int(1)
        || drop < fix32::from_int(-3)
        || drop > fix32::zero()
    {
        return false;
    }
    let one = fix32::from_int(1);
    let half = fix32::from_words(32768, 0);
    let sx = if orient == 1 { fix32::from_int(2) } else { one };
    let sy = if orient == 0 { fix32::from_int(2) } else { one };
    let sz = if orient == 2 { fix32::from_int(2) } else { one };
    let cx = sx * half;
    let cy = sy * half;
    let cz = sz * half;
    let mut px = half;
    let mut pz = half;
    if dir == 0 {
        px = sx;
    }
    if dir == 1 {
        px = fix32::zero();
    }
    if dir == 2 {
        pz = sz;
    }
    if dir == 3 {
        pz = fix32::zero();
    }
    let mut angle = fraction * fix32::from_words(37408, 1);
    if dir == 0 || dir == 3 {
        angle = -angle;
    }
    let sc = fsincos(angle);
    let s = sc.x();
    let c = sc.y();
    let mut tx = cx;
    let mut ty = cy;
    let mut tz = cz;
    let mut ax = fix32::zero();
    let mut az = fix32::zero();
    if dir < 2 {
        tx = px + c * (cx - px) - s * cy;
        ty = s * (cx - px) + c * cy;
        az = angle;
    } else {
        ty = c * cy - s * (cz - pz);
        tz = pz + s * cy + c * (cz - pz);
        ax = angle;
    }
    tx = tx + fix32::from_int(x) - fix32::from_int(width as i16) * half;
    ty = ty + drop;
    tz = tz + fix32::from_int(z) - fix32::from_int(depth as i16) * half;
    gpu_pose_set(
        out,
        vec3::new(tx, ty, tz),
        vec3::new(ax, fix32::zero(), az),
        vec3::new(sx * half, sy * half, sz * half),
    );
    true
}

// Normal is the shared helper's rigid-camera inverse-scale result. Lens
// magnification applies only to the position prefix, never to normal rows.
pub fn blox_instance(mut out: Array<u16>, pose: Array<u16>, camera: Array<u16>) -> bool {
    let mut result: Buf<u16, 42> = Buf::new([0; 42]);
    let mut lens: Buf<u16, 24> = Buf::new([0; 24]);
    if !gpu_instance_trs(result.as_array(), pose, camera) {
        return false;
    }
    gpu_affine_identity(lens.as_array());
    gpu_fstore(lens.as_array(), 0, fix32::from_words(43150, 2));
    gpu_fstore(lens.as_array(), 5, fix32::from_words(43150, 2));
    if !gpu_affine_compose(result.as_array(), lens.as_array(), result.as_array()) {
        return false;
    }
    let mut i: u16 = 0;
    while i < 42 {
        out[i] = result[i];
        i += 1;
    }
    true
}
