//! Conservative origin/quantizer-box culling before vertex admission.
//! Ineligible arithmetic falls through to the unchanged V/T path.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Keep,
    Behind,
    Left,
    Right,
}

/// Matches the shared MAC prologue, including its pre-output fractional bits.
/// The horizontal cone is deliberately wider than the 400x240 viewport.
pub fn classify(origin: [i32; 3], quant_shift: u8, matrix: &[i32; 12]) -> Decision {
    if quant_shift > 14 || origin.iter().any(|&x| !(-(1 << 24)..1 << 24).contains(&x)) {
        return Decision::Keep;
    }
    let extent = 1023_i64 << quant_shift;
    let eligible = |row: usize| {
        matrix[row..row + 3]
            .iter()
            .all(|&x| (-(1 << 19)..1 << 19).contains(&x))
            && (-(1 << 29)..1 << 29).contains(&matrix[row + 3])
    };
    let support = |row: usize, minimum: bool| {
        let mut accumulator = i64::from(matrix[row + 3]) << 16;
        for column in 0..3 {
            let coefficient = i64::from(matrix[row + column]);
            let upper = (coefficient < 0) == minimum;
            let position = i64::from(origin[column]) + if upper { extent } else { 0 };
            accumulator += position * coefficient;
        }
        accumulator
    };
    if !eligible(8) {
        return Decision::Keep;
    }
    let z = support(8, false) >> 16;
    if z <= 0 {
        return Decision::Behind;
    }
    if !eligible(0) {
        return Decision::Keep;
    }
    if support(0, false) + (z << 16) < -(1_i64 << 32) {
        Decision::Left
    } else if support(0, true) - (z << 16) > 1_i64 << 32 {
        Decision::Right
    } else {
        Decision::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_matches_independent_eight_corner_extrema() {
        let mut state = 0x13579bdf_u32;
        for _ in 0..2048 {
            let mut next = || {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                state
            };
            let origin = std::array::from_fn(|_| (next() % (32 << 16)) as i32 - (16 << 16));
            let shift = (next() % 11) as u8;
            let mut m = [0; 12];
            for x in &mut m {
                *x = (next() % (4 << 16)) as i32 - (2 << 16);
            }
            m[3] *= 16;
            m[11] *= 16;
            let mut points = Vec::new();
            for corner in 0..8 {
                let point: [i64; 3] = std::array::from_fn(|k| {
                    i64::from(origin[k])
                        + if corner & (1 << k) != 0 {
                            1023_i64 << shift
                        } else {
                            0
                        }
                });
                points.push([0, 8].map(|row| {
                    (i64::from(m[row + 3]) << 16)
                        + (0..3)
                            .map(|k| point[k] * i64::from(m[row + k]))
                            .sum::<i64>()
                }));
            }
            let z = points.iter().map(|p| p[1]).max().unwrap() >> 16;
            let expected = if z <= 0 {
                Decision::Behind
            } else if points.iter().map(|p| p[0]).max().unwrap() + (z << 16) < -(1_i64 << 32) {
                Decision::Left
            } else if points.iter().map(|p| p[0]).min().unwrap() - (z << 16) > 1_i64 << 32 {
                Decision::Right
            } else {
                Decision::Keep
            };
            assert_eq!(classify(origin, shift, &m), expected);
        }
    }
}
