//! Frozen pre-optimization arithmetic from accepted 4e14c791.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
pub fn gpu_scene_fix_read(data: Array<u16>, at: u16) -> fix32 {
    fix32::from_words(data[at], data[at + 1])
}

pub fn gpu_scene_fix_write(mut data: Array<u16>, at: u16, value: fix32) {
    data[at] = value.lo_bits();
    data[at + 1] = value.hi_bits();
}

pub fn gpu_scene_vec_read(data: Array<u16>, at: u16) -> vec3 {
    vec3::new(
        fix32::from_words(data[at], data[at + 1]),
        fix32::from_words(data[at + 2], data[at + 3]),
        fix32::from_words(data[at + 4], data[at + 5]),
    )
}

pub fn gpu_scene_matrix_prepare(desired: Array<u16>, model: Array<u16>, mut out: Array<u16>) {
    let k = gpu_scene_fix_read(desired, GPU_CAMERA_FOCAL) * fix32::from_words(256, 0);
    let mut row: u16 = 0;
    while row < 3 {
        let basis = gpu_scene_vec_read(desired, GPU_CAMERA_VIEW + row * 8);
        let mut factor = -k;
        if row == 2 {
            factor = -fix32::from_int(1);
        }
        let mut col: u16 = 0;
        while col < 4 {
            let world = vec3::new(
                gpu_scene_fix_read(model, GPU_MODEL_WORLD + col * 2),
                gpu_scene_fix_read(model, GPU_MODEL_WORLD + 8 + col * 2),
                gpu_scene_fix_read(model, GPU_MODEL_WORLD + 16 + col * 2),
            );
            let mut value = fdot(basis, world);
            if col == 3 {
                value = value + gpu_scene_fix_read(desired, GPU_CAMERA_VIEW + row * 8 + 6);
            }
            gpu_scene_fix_write(out, row * 8 + col * 2, value * factor);
            col += 1;
        }
        col = 0;
        while col < 3 {
            let normal = vec3::new(
                gpu_scene_fix_read(model, GPU_MODEL_NORMAL + col * 2),
                gpu_scene_fix_read(model, GPU_MODEL_NORMAL + 6 + col * 2),
                gpu_scene_fix_read(model, GPU_MODEL_NORMAL + 12 + col * 2),
            );
            let mut value = fdot(basis, normal);
            if row == 0 {
                value = -value;
            }
            gpu_scene_fix_write(out, 24 + (row * 3 + col) * 2, value);
            col += 1;
        }
        row += 1;
    }
}
