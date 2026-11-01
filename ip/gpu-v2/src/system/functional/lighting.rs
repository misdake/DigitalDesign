//! The selected integer Lighting ALU, with no generic-renderer/oracle execution.
//! Only immutable ROMs are shared with the production numerical generator.
use super::geometry::rne;
use crate::lighting::{
    h9,
    ports::{Light, Projection},
    RSQRT_Q13_BIASES, RSQRT_Q13_RAW,
};

struct Normalized {
    value: [i128; 3],
    unit: [i128; 3],
    reciprocal: i128,
    zero: bool,
}
fn highest(value: i128) -> i32 {
    127 - value.leading_zeros() as i32
}

fn normalize(raw: [i128; 3], threshold: i128, bounded: bool) -> Normalized {
    let zero = raw.iter().map(|v| v.abs()).max().unwrap() < threshold;
    let raw = if zero { [8192, 0, 0] } else { raw };
    let magnitude = raw.iter().map(|v| v.abs()).max().unwrap();
    let mut shift = if bounded { 0 } else { 13 - highest(magnitude) };
    // The signed floor boundary must include the negative endpoint.
    if !bounded && shift < 0 && raw.iter().any(|&v| (v >> -shift).abs() >= 16384) {
        shift -= 1;
    }
    let value = raw.map(|v| if shift >= 0 { v << shift } else { v >> -shift });
    // Selected SQ uses the signed-chord result, expressed in magnitude form.
    let square: i128 = value
        .iter()
        .map(|v| {
            let magnitude = v.abs();
            let head = magnitude >> 7;
            let tail = magnitude & 127;
            ((head * head) << 14) + (((2 * head + 1) * tail) << 7)
        })
        .sum();
    let msb = highest(square);
    let exponent = msb - 28;
    let mantissa = if msb >= 14 {
        square >> (msb - 14)
    } else {
        square << (14 - msb)
    };
    let address = (exponent.rem_euclid(2) * 64 + ((mantissa - 16384) >> 8) as i32) as usize;
    let stored = RSQRT_Q13_RAW[address];
    let base = i128::from(stored & 16383) << 2;
    let delta = i128::from((stored >> 14) + RSQRT_Q13_BIASES[address >> 3]) << 2;
    let interpolation = base - ((delta * (mantissa & 255)) >> 8);
    let restore = -exponent.div_euclid(2);
    let reciprocal = if restore >= 0 {
        interpolation << restore
    } else {
        interpolation >> -restore
    };
    let unit = value.map(|v| {
        if zero {
            0
        } else {
            ((v * reciprocal) >> 15).clamp(-16384, 16384)
        }
    });
    Normalized {
        value,
        unit,
        reciprocal,
        zero,
    }
}

pub(super) fn evaluate(
    normal: [i16; 3],
    ndc: [i32; 2],
    light: Light,
    projection: Projection,
    code: u8,
    mode: u8,
) -> [u16; 2] {
    if mode == 0 {
        return [256, 0];
    }
    if mode == 1 || light.directional == 0 {
        return [light.ambient, 0];
    }
    let n = normalize(normal.map(i128::from), 4, false);
    let direction = light.direction.map(i128::from);
    let nl: i128 = (0..3).map(|c| n.unit[c] * direction[c]).sum();
    // S34F28 -> U9F8 uses half up. U18F16 -> U9F8 uses RNE.
    let diffuse = (nl.clamp(0, 1 << 28) + (1 << 19)) >> 20;
    let g = (i128::from(light.ambient) + rne(i128::from(light.directional) * diffuse, 256)).min(511)
        as u16;
    if mode == 2 {
        return [g, 0];
    }
    let ray = [
        (i128::from(ndc[0]) * i128::from(projection.ray_scale[0])) >> 14,
        (i128::from(ndc[1]) * i128::from(projection.ray_scale[1])) >> 14,
        i128::from(projection.k),
    ];
    let view = normalize(ray, 4, true);
    let half = normalize(
        std::array::from_fn(|c| (direction[c] + view.unit[c]) >> 1),
        64,
        false,
    );
    let dot: i128 = (0..3).map(|c| n.unit[c] * half.value[c]).sum();
    let cosine = if n.zero || half.zero {
        0
    } else {
        (((dot >> 12) * half.reciprocal) >> 16).clamp(0, 32768)
    };
    let distance = (32768 - cosine) >> h9::H9_SHIFTS_RAW[usize::from(code)];
    let index = distance >> 6;
    let power = if nl <= 0 || index >= 32 {
        0
    } else {
        let entry = h9::H9_RAW[usize::from(code) * 32 + index as usize];
        i128::from(entry & 511) - ((i128::from(entry >> 9) * (distance & 63) + 32) >> 6)
    };
    [g, rne(i128::from(light.directional) * power, 256) as u16]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lighting::{
        ports::{Material, PixelInput},
        sim::counted,
        LightingProfile, LightingQuantization,
    };
    #[test]
    fn selected_alu_matches_the_production_numerical_program_at_signed_boundaries() {
        let config = counted::Config {
            smooth_h9: true,
            h9_tail_dsp: true,
            rsqrt_q13: true,
            ..counted::Config::lit_queue_resource_profile(
                LightingProfile::Fast,
                LightingQuantization::CompensatedFloor,
            )
        };
        let normals = [
            [0, 0, 0],
            [0, 0, 3],
            [0, 0, 4],
            [0, 0, 16384],
            [0, 0, -16384],
            [32767, 0, 0],
            [-32767, 0, 0],
            [-32768, 0, 0],
            [8193, -8191, 16384],
            [16383, 16383, 16383],
        ];
        for normal in normals {
            for ndc in [[0, 0], [-16384, 16384], [16384, -16384], [8191, -8193]] {
                for code in [0, 12, 16] {
                    for direction in [
                        [0, 0, 16384],
                        [16384, 0, 0],
                        [9459, 9459, 9459],
                        [-5146, 8041, 13315],
                    ] {
                        let light = Light {
                            ambient: 31,
                            directional: 225,
                            direction,
                        };
                        let material = Material {
                            shininess_code: code,
                            ..Default::default()
                        };
                        let expected = counted::evaluate_with_config(
                            PixelInput { normal, ndc },
                            material,
                            light,
                            Projection::default(),
                            2048,
                            config,
                        )
                        .unwrap()
                        .output;
                        assert_eq!(
                            evaluate(normal, ndc, light, Projection::default(), code, 3),
                            [expected.g, expected.h],
                            "N={normal:?} ndc={ndc:?} L={direction:?} code={code}"
                        );
                    }
                }
            }
        }
    }
}
