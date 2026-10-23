//! Small Q16.16 affine helpers. GPU camera axes: +X right, +Y down, +Z forward.
//! Euler angles are radians, R=Rz*Ry*Rx. No perspective/4x4 inverse is implied.
use crate::dsl_rt::*;
// Pose: translation XYZ, Euler XYZ, scale XYZ (nine native Q16.16 pairs).
// Bounds: minimum XYZ, maximum XYZ, then one scale_s word.
pub fn gpu_pose_set(out: Array<u16>, translation: vec3, angles: vec3, scale: vec3) {
    gpu_fstore(out, 0, translation.x());
    gpu_fstore(out, 1, translation.y());
    gpu_fstore(out, 2, translation.z());
    gpu_fstore(out, 3, angles.x());
    gpu_fstore(out, 4, angles.y());
    gpu_fstore(out, 5, angles.z());
    gpu_fstore(out, 6, scale.x());
    gpu_fstore(out, 7, scale.y());
    gpu_fstore(out, 8, scale.z());
}
pub fn gpu_bounds_set(mut out: Array<u16>, minimum: vec3, maximum: vec3, scale_s: u16) {
    gpu_fstore(out, 0, minimum.x());
    gpu_fstore(out, 1, minimum.y());
    gpu_fstore(out, 2, minimum.z());
    gpu_fstore(out, 3, maximum.x());
    gpu_fstore(out, 4, maximum.y());
    gpu_fstore(out, 5, maximum.z());
    out[12u16] = scale_s;
}
fn gpu_fbounded(value: fix32, limit: fix32) -> bool {
    value >= -limit && value <= limit
}
fn gpu_fload(words: Array<u16>, index: u16) -> fix32 {
    fix32::from_words(words[index * 2], words[index * 2 + 1])
}
fn gpu_fstore(mut words: Array<u16>, index: u16, value: fix32) {
    words[index * 2] = value.lo_bits();
    words[index * 2 + 1] = value.hi_bits();
}
fn gpu_affine_valid(m: Array<u16>) -> bool {
    let mut i: u16 = 0;
    while i < 12 {
        let limit = if i & 3 == 3 {
            fix32::from_int(64)
        } else {
            fix32::from_int(4)
        };
        if !gpu_fbounded(gpu_fload(m, i), limit) {
            return false;
        }
        i += 1;
    }
    true
}
pub fn gpu_affine_identity(mut out: Array<u16>) {
    let mut i: u16 = 0;
    while i < 12 {
        let value = if i == 0 || i == 5 || i == 10 {
            fix32::from_int(1)
        } else {
            fix32::zero()
        };
        gpu_fstore(out, i, value);
        i += 1;
    }
}
pub fn gpu_affine_compose(mut out: Array<u16>, a: Array<u16>, b: Array<u16>) -> bool {
    if !gpu_affine_valid(a) || !gpu_affine_valid(b) {
        return false;
    }
    let mut tmp: Buf<u16, 24> = Buf::new([0; 24]);
    let mut row: u16 = 0;
    while row < 3 {
        let ar = vec4::new(
            gpu_fload(a, row * 4),
            gpu_fload(a, row * 4 + 1),
            gpu_fload(a, row * 4 + 2),
            gpu_fload(a, row * 4 + 3),
        );
        let mut col: u16 = 0;
        while col < 4 {
            let h = if col == 3 {
                fix32::from_int(1)
            } else {
                fix32::zero()
            };
            let bc = vec4::new(
                gpu_fload(b, col),
                gpu_fload(b, 4 + col),
                gpu_fload(b, 8 + col),
                h,
            );
            gpu_fstore(tmp.as_array(), row * 4 + col, fdot(ar, bc));
            col += 1;
        }
        row += 1;
    }
    if !gpu_affine_valid(tmp.as_array()) {
        return false;
    }
    let mut i: u16 = 0;
    while i < 24 {
        out[i] = tmp[i];
        i += 1;
    }
    true
}
fn gpu_pose_valid(p: Array<u16>) -> bool {
    gpu_fbounded(gpu_fload(p, 0), fix32::from_int(64))
        && gpu_fbounded(gpu_fload(p, 1), fix32::from_int(64))
        && gpu_fbounded(gpu_fload(p, 2), fix32::from_int(64))
        && gpu_fbounded(gpu_fload(p, 3), fix32::from_int(4))
        && gpu_fbounded(gpu_fload(p, 4), fix32::from_int(4))
        && gpu_fbounded(gpu_fload(p, 5), fix32::from_int(4))
        && gpu_fload(p, 6) >= fix32::from_words(32768, 0)
        && gpu_fload(p, 6) <= fix32::from_int(2)
        && gpu_fload(p, 7) >= fix32::from_words(32768, 0)
        && gpu_fload(p, 7) <= fix32::from_int(2)
        && gpu_fload(p, 8) >= fix32::from_words(32768, 0)
        && gpu_fload(p, 8) <= fix32::from_int(2)
}
fn gpu_rotation(mut out: Array<u16>, p: Array<u16>) {
    let x = fsincos(gpu_fload(p, 3));
    let y = fsincos(gpu_fload(p, 4));
    let z = fsincos(gpu_fload(p, 5));
    let sx = x.x();
    let cx = x.y();
    let sy = y.x();
    let cy = y.y();
    let sz = z.x();
    let cz = z.y();
    gpu_fstore(out, 0, cz * cy);
    gpu_fstore(out, 1, cz * sy * sx - sz * cx);
    gpu_fstore(out, 2, cz * sy * cx + sz * sx);
    gpu_fstore(out, 4, sz * cy);
    gpu_fstore(out, 5, sz * sy * sx + cz * cx);
    gpu_fstore(out, 6, sz * sy * cx - cz * sx);
    gpu_fstore(out, 8, -sy);
    gpu_fstore(out, 9, cy * sx);
    gpu_fstore(out, 10, cy * cx);
}
pub fn gpu_affine_trs(mut out: Array<u16>, p: Array<u16>) -> bool {
    if !gpu_pose_valid(p) {
        return false;
    }
    let mut r: Buf<u16, 24> = Buf::new([0; 24]);
    gpu_rotation(r.as_array(), p);
    let mut i: u16 = 0;
    while i < 3 {
        gpu_fstore(
            r.as_array(),
            i * 4,
            gpu_fload(r.as_array(), i * 4) * gpu_fload(p, 6),
        );
        gpu_fstore(
            r.as_array(),
            i * 4 + 1,
            gpu_fload(r.as_array(), i * 4 + 1) * gpu_fload(p, 7),
        );
        gpu_fstore(
            r.as_array(),
            i * 4 + 2,
            gpu_fload(r.as_array(), i * 4 + 2) * gpu_fload(p, 8),
        );
        i += 1;
    }
    gpu_fstore(r.as_array(), 3, gpu_fload(p, 0));
    gpu_fstore(r.as_array(), 7, gpu_fload(p, 1));
    gpu_fstore(r.as_array(), 11, gpu_fload(p, 2));
    i = 0;
    while i < 24 {
        out[i] = r[i];
        i += 1;
    }
    true
}
pub fn gpu_camera_view(mut out: Array<u16>, p: Array<u16>) -> bool {
    if !gpu_pose_valid(p)
        || gpu_fload(p, 6) != fix32::from_int(1)
        || gpu_fload(p, 7) != fix32::from_int(1)
        || gpu_fload(p, 8) != fix32::from_int(1)
    {
        return false;
    }
    let mut r: Buf<u16, 24> = Buf::new([0; 24]);
    gpu_rotation(r.as_array(), p);
    let mut v: Buf<u16, 24> = Buf::new([0; 24]);
    let mut i: u16 = 0;
    while i < 3 {
        let x = gpu_fload(r.as_array(), i);
        let y = gpu_fload(r.as_array(), 4 + i);
        let z = gpu_fload(r.as_array(), 8 + i);
        gpu_fstore(v.as_array(), i * 4, x);
        gpu_fstore(v.as_array(), i * 4 + 1, y);
        gpu_fstore(v.as_array(), i * 4 + 2, z);
        gpu_fstore(
            v.as_array(),
            i * 4 + 3,
            -fdot(
                vec3::new(x, y, z),
                vec3::new(gpu_fload(p, 0), gpu_fload(p, 1), gpu_fload(p, 2)),
            ),
        );
        i += 1;
    }
    if !gpu_affine_valid(v.as_array()) {
        return false;
    }
    i = 0;
    while i < 24 {
        out[i] = v[i];
        i += 1;
    }
    true
}
// Output is the native 21-u32 matrix prefix. Normal uses V*R*inverse(S),
// not the nonuniformly scaled position matrix. Camera is rigid only.
pub fn gpu_instance_trs(mut out: Array<u16>, model: Array<u16>, camera: Array<u16>) -> bool {
    let mut m: Buf<u16, 24> = Buf::new([0; 24]);
    let mut v: Buf<u16, 24> = Buf::new([0; 24]);
    let mut result: Buf<u16, 42> = Buf::new([0; 42]);
    if !gpu_affine_trs(m.as_array(), model)
        || !gpu_camera_view(v.as_array(), camera)
        || !gpu_affine_compose(result.as_array(), v.as_array(), m.as_array())
    {
        return false;
    }
    gpu_rotation(m.as_array(), model);
    let ix = frcp(gpu_fload(model, 6));
    let iy = frcp(gpu_fload(model, 7));
    let iz = frcp(gpu_fload(model, 8));
    let mut row: u16 = 0;
    while row < 3 {
        let vr = vec3::new(
            gpu_fload(v.as_array(), row * 4),
            gpu_fload(v.as_array(), row * 4 + 1),
            gpu_fload(v.as_array(), row * 4 + 2),
        );
        let mut col: u16 = 0;
        while col < 3 {
            let inv = if col == 0 {
                ix
            } else {
                if col == 1 {
                    iy
                } else {
                    iz
                }
            };
            let mc = vec3::new(
                gpu_fload(m.as_array(), col),
                gpu_fload(m.as_array(), 4 + col),
                gpu_fload(m.as_array(), 8 + col),
            );
            gpu_fstore(result.as_array(), 12 + row * 3 + col, fdot(vr, mc) * inv);
            col += 1;
        }
        row += 1;
    }
    let mut i: u16 = 0;
    while i < 42 {
        out[i] = result[i];
        i += 1;
    }
    true
}
// Conservative AABB corner admission for the fixed asset's scale_s. Keep
// all corners forward; this GPU does not construct near-clipped triangles.
pub fn gpu_instance_bounds(m: Array<u16>, bounds: Array<u16>) -> bool {
    if !gpu_affine_valid(m)
        || bounds[12u16] > 15
        || gpu_fload(bounds, 0) > gpu_fload(bounds, 3)
        || gpu_fload(bounds, 1) > gpu_fload(bounds, 4)
        || gpu_fload(bounds, 2) > gpu_fload(bounds, 5)
    {
        return false;
    }
    let mut corner: u16 = 0;
    while corner < 8 {
        let x = if corner & 1 == 0 {
            gpu_fload(bounds, 0)
        } else {
            gpu_fload(bounds, 3)
        };
        let y = if corner & 2 == 0 {
            gpu_fload(bounds, 1)
        } else {
            gpu_fload(bounds, 4)
        };
        let z = if corner & 4 == 0 {
            gpu_fload(bounds, 2)
        } else {
            gpu_fload(bounds, 5)
        };
        if !gpu_fbounded(x, fix32::from_int(64))
            || !gpu_fbounded(y, fix32::from_int(64))
            || !gpu_fbounded(z, fix32::from_int(64))
        {
            return false;
        }
        let point = vec4::new(x, y, z, fix32::from_int(1));
        let mut row: u16 = 0;
        while row < 3 {
            let value = fdot(
                point,
                vec4::new(
                    gpu_fload(m, row * 4),
                    gpu_fload(m, row * 4 + 1),
                    gpu_fload(m, row * 4 + 2),
                    gpu_fload(m, row * 4 + 3),
                ),
            );
            let low = value.lo_bits();
            let high = value.hi_bits();
            let limit = fix32::from_words(65535, 1);
            // Compare after the exact signed arithmetic shift using raw halves.
            let mut shifted_low = low;
            let mut shifted_high = high;
            let mut shift: u16 = 0;
            while shift < bounds[12u16] {
                shifted_low = (shifted_low >> 1) | (shifted_high << 15);
                shifted_high = ((shifted_high as i16) >> 1) as u16;
                shift += 1;
            }
            let shifted = fix32::from_words(shifted_low, shifted_high);
            if shifted > limit || shifted < -limit || (row == 2 && shifted <= fix32::zero()) {
                return false;
            }
            row += 1;
        }
        corner += 1;
    }
    true
}
