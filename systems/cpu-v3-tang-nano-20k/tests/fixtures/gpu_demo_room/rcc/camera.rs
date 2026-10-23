//! A 512-step yaw orbit; phase is a wrapping unsigned fraction of one turn.
use crate::dsl_rt::*;
mod gpu_transform;
use gpu_transform::*;

pub fn demo_camera(matrix: Array<u16>, phase: u16) {
    let sc = fsincos(fix32::from_words(phase, 0) * fix32::from_words(18559, 6));
    let s = sc.x();
    let c = sc.y();
    let k = fix32::from_words(51200, 0);
    let zero = fix32::zero();
    // Position: right/down/forward, focal=200. Eye=(8*sin,.22,8*cos).
    gpu_fstore(matrix, 0, -c * k);
    gpu_fstore(matrix, 1, zero);
    gpu_fstore(matrix, 2, s * k);
    gpu_fstore(matrix, 3, zero);
    gpu_fstore(matrix, 4, zero);
    gpu_fstore(matrix, 5, -k);
    gpu_fstore(matrix, 6, zero);
    gpu_fstore(matrix, 7, fix32::from_words(11264, 0));
    gpu_fstore(matrix, 8, -s);
    gpu_fstore(matrix, 9, zero);
    gpu_fstore(matrix, 10, -c);
    gpu_fstore(matrix, 11, fix32::from_int(8));
    // Normal basis: right/up/back; the room uses the existing unlit material.
    gpu_fstore(matrix, 12, -c);
    gpu_fstore(matrix, 13, zero);
    gpu_fstore(matrix, 14, s);
    gpu_fstore(matrix, 15, zero);
    gpu_fstore(matrix, 16, fix32::from_int(1));
    gpu_fstore(matrix, 17, zero);
    gpu_fstore(matrix, 18, s);
    gpu_fstore(matrix, 19, zero);
    gpu_fstore(matrix, 20, c);
}
