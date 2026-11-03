//! Independent fixed-point stage oracle and mathematically generated H9 values.
use gpu_v2::lighting::{
    calendars::UnifiedCalendar,
    ports::*,
    sim::{counted, oracle},
    LightingQuantization,
};
pub fn light() -> [u16; 2] {
    // Numerical format options only: no candidate Program, schedule or tick.
    let o = UnifiedCalendar::SmoothH9Reference.options(LightingQuantization::CompensatedFloor);
    let config = oracle::Config::from_counted(counted::Config {
        smooth_h9: o.smooth_h9,
        h9_tail_dsp: o.h9_tail_dsp,
        rsqrt_q13: o.rsqrt_q13,
        lit_queue: o.lit_queue,
        quantization: o.quantization,
        scalar_norm: o.scalar_norm,
        weighted_view: o.weighted_view,
        scalar_normal: o.scalar_normal,
        exact_normal_gate: o.exact_normal_gate,
        block_prescale: o.block_prescale,
        direct_square: o.direct_square,
        direct_all_squares: o.direct_all_squares,
        square9: o.square9,
        dedicated_dots: o.dedicated_dots,
        ..counted::Config::architecture()
    });
    let result = oracle::evaluate(
        PixelInput {
            normal: [0, 0, 16384],
            ndc: [0, 0],
        },
        Material {
            unlit: false,
            specular_color: [255; 3],
            shininess_code: 3,
        },
        Light {
            direction: [-9830, 0, 13107],
            ambient: 64,
            directional: 96,
        },
        Projection {
            ray_scale: [0, 0],
            k: 8192,
        },
        config,
    )
    .unwrap();
    let cosine = result.stages.iter().find(|(n, _)| n == "x").unwrap().1;
    let exponent = 7;
    let mut shift = 0;
    while (1 << shift) * exponent < 6144 {
        shift += 1;
    }
    let distance = 32768 - cosine;
    let bin = distance >> shift;
    assert!((0..32).contains(&bin));
    let width = 1i128 << shift;
    let endpoint = |n: i128| {
        (256.0 * (1.0 - (n * width) as f64 / 32768.0).max(0.0).powi(exponent)).round_ties_even()
            as u32
    };
    let base = endpoint(bin);
    let delta = base - endpoint(bin + 1);
    let fraction = ((distance << 6) >> shift) as u32 & 63;
    let h = base - ((delta * fraction + 32) >> 6);
    let product = 96 * h;
    let q = product / 256;
    let r = product % 256;
    [
        result.g as u16,
        (q + u32::from(r > 128 || r == 128 && q & 1 != 0)) as u16,
    ]
}
pub fn texel(x: usize, y: usize) -> u16 {
    ((x + y * 64) as u16).wrapping_mul(149).wrapping_add(0x71b3)
}
pub fn sampled() -> u32 {
    // UV(15.75,7.75), half-texel bias -> (15.25,7.25); conserving floor weights.
    let bottom = 511 * 64 / 256;
    let top = 511 - bottom;
    let weights = [
        top - top * 64 / 256,
        top * 64 / 256,
        bottom - bottom * 64 / 256,
        bottom * 64 / 256,
    ];
    let mut channels = [0u32; 3];
    for (t, w) in weights.into_iter().enumerate() {
        let raw = texel(15 + t % 2, 7 + t / 2);
        let r = u32::from(raw >> 11);
        let g = u32::from(raw >> 5 & 63);
        let b = u32::from(raw & 31);
        for (c, v) in [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
            .into_iter()
            .enumerate()
        {
            channels[c] += (w * v + 256) / 512;
        }
    }
    channels[0] & 255 | (channels[1] & 255) << 8 | (channels[2] & 255) << 16
}
pub fn final_rgb(tint: u32, texture: u32, light: [u16; 2], specular: u32) -> u32 {
    (0..3).fold(0, |rgb, c| {
        let v = |n: u32| n >> (8 * c) & 255;
        let modulated = (v(tint) * v(texture) * 2 + 255) / 510;
        let channel = (modulated * u32::from(light[0]) / 256
            + v(specular) * u32::from(light[1]) / 256)
            .min(255);
        rgb | channel << (8 * c)
    })
}
pub fn rop(rgb: u32, old: u16, alpha: u8, blend: bool) -> u16 {
    let r = u32::from(old >> 11);
    let g = u32::from(old >> 5 & 63);
    let b = u32::from(old & 31);
    let a = if blend {
        (u32::from(alpha) + 2) / 4
    } else {
        64
    };
    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
        .into_iter()
        .enumerate()
        .fold(0, |raw, (c, d)| {
            let v = (((rgb >> (8 * c) & 255) * a + d * (64 - a) + 32) / 64)
                * if c == 1 { 63 } else { 31 };
            raw | (((v * 2 + 255) / 510) as u16) << [11, 5, 0][c]
        })
}
