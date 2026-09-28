//! Standalone color geometry reference for the meshlet microcore experiment.
//! This module has no connection to the fitted system GPU.

pub mod microcore_sim;

use super::rastersim::{clip, fixed::Q16, setup};

/// RGB components retain eight fractional bits through clipping and setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorVertex {
    pub clip: clip::ClipVertex,
    pub color: [i32; 3],
}

impl ColorVertex {
    pub fn from_clip_rgb565(position: [i32; 4], rgb565: u16) -> Self {
        let [x, y, z, w] = position.map(|c| Q16::from_raw(i64::from(c)));
        Self {
            clip: clip::ClipVertex { x, y, z, w },
            color: [
                i32::from(rgb565 >> 11),
                i32::from((rgb565 >> 5) & 63),
                i32::from(rgb565 & 31),
            ]
            .map(|c| c << 8),
        }
    }
}

/// Fixed-plane clipping carries color using the same quotient as XYZW.
pub fn clip_color_triangle(triangle: [ColorVertex; 3]) -> Vec<[ColorVertex; 3]> {
    let positions = triangle.map(|v| v.clip);
    match clip::classify(&positions) {
        clip::Classify::Accept => return vec![triangle],
        clip::Classify::Reject => return vec![],
        clip::Classify::Clip => {}
    }
    let mut polygon = triangle.to_vec();
    for plane in [0, 1, 6, 7, 8, 9] {
        if polygon.is_empty() {
            break;
        }
        let mut next = Vec::with_capacity(9);
        let mut previous = *polygon.last().unwrap();
        let mut previous_distance = clip::distances(&previous.clip)[plane];
        for current in polygon {
            let distance = clip::distances(&current.clip)[plane];
            if previous_distance.is_negative() != distance.is_negative() {
                let (outside, d_out, inside, d_in) = if previous_distance.is_negative() {
                    (previous, previous_distance, current, distance)
                } else {
                    (current, distance, previous, previous_distance)
                };
                let intersection = if let Some((num, den)) = clip::intersect_prepare(d_out, d_in) {
                    let t = clip::intersect_quotient(num, den);
                    let lerp = |a: i32, b: i32| {
                        let product = i128::from(i64::from(b) - i64::from(a)) * i128::from(t);
                        (i128::from(a) + ((product + (1 << 31)) >> 32)) as i32
                    };
                    let a = outside.clip;
                    let b = inside.clip;
                    ColorVertex {
                        clip: clip::ClipVertex {
                            x: clip::lerp_q16(a.x, b.x, t),
                            y: clip::lerp_q16(a.y, b.y, t),
                            z: clip::lerp_q16(a.z, b.z, t),
                            w: clip::lerp_q16(a.w, b.w, t),
                        },
                        color: std::array::from_fn(|i| lerp(outside.color[i], inside.color[i])),
                    }
                } else {
                    inside
                };
                next.push(intersection);
            }
            if !distance.is_negative() {
                next.push(current);
            }
            previous = current;
            previous_distance = distance;
        }
        assert!(next.len() <= 9, "six-plane clipped triangle bound");
        polygon = next;
    }
    (1..polygon.len().saturating_sub(1))
        .map(|i| [polygon[0], polygon[i], polygon[i + 1]])
        .collect()
}

/// Integer perspective-color oracle over snapped coverage geometry.
pub fn color_at(triangle: &[ColorVertex; 3], setup: &setup::TriangleSetup, x: u16, y: u16) -> u16 {
    use super::rastersim::fixed::S12_4;
    let px = S12_4::from_raw(i64::from(x) * 16 + 8);
    let py = S12_4::from_raw(i64::from(y) * 16 + 8);
    let e: [i128; 3] =
        std::array::from_fn(|i| i128::from(setup::edge_eval(setup, (i + 1) % 3, px, py).raw()));
    let w = triangle.map(|v| i128::from(v.clip.w.raw()));
    let weights = [e[0] * w[1] * w[2], e[1] * w[0] * w[2], e[2] * w[0] * w[1]];
    let denominator: i128 = weights.iter().sum();
    assert!(denominator > 0);
    let rgb: [u16; 3] = std::array::from_fn(|c| {
        let numerator: i128 = (0..3)
            .map(|i| weights[i] * i128::from(triangle[i].color[c]))
            .sum();
        ((numerator + denominator * 128) / (denominator * 256))
            .clamp(0, if c == 1 { 63 } else { 31 }) as u16
    });
    (rgb[0] << 11) | (rgb[1] << 5) | rgb[2]
}
