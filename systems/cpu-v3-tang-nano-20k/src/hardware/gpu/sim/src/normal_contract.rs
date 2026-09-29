//! Host-side preflight for the provisional scale-preserving normal-matrix
//! promise. The GPU microkernel does not spend cycles on this check. A matrix
//! that passes is approximately orthogonal after Q2.14 quantization; this is
//! a matrix-wide guarantee for arbitrary unit normals, not a per-vertex test.

use crate::fixed::Q14;

pub const UNIT_SQUARED_RAW: i64 = 1_i64 << 28;
/// Trial bound in Q4.28 raw squared units. It covers nearest-quantized
/// rotations with coefficient error at most one half Q2.14 LSB.
pub const TRIAL_GRAM_TOLERANCE: i64 = 1_i64 << 15;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GramReport {
    /// `M^T M - I` in Q4.28 raw squared units.
    pub error: [[i64; 3]; 3],
    pub max_abs_error: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalScaleError {
    pub row: u8,
    pub column: u8,
    pub error: i64,
    pub tolerance: i64,
}

pub fn gram_report(matrix: [[Q14; 3]; 3]) -> GramReport {
    let error = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            let dot = (0..3)
                .map(|axis| matrix[axis][row].raw() * matrix[axis][column].raw())
                .sum::<i64>();
            dot - if row == column { UNIT_SQUARED_RAW } else { 0 }
        })
    });
    let max_abs_error = error
        .iter()
        .flatten()
        .map(|value| value.abs())
        .max()
        .unwrap();
    GramReport {
        error,
        max_abs_error,
    }
}

pub fn check_scale_preserving(
    matrix: [[Q14; 3]; 3],
    tolerance_raw_squared: i64,
) -> Result<GramReport, NormalScaleError> {
    assert!(tolerance_raw_squared >= 0);
    let report = gram_report(matrix);
    for row in 0..3 {
        for column in 0..3 {
            let error = report.error[row][column];
            if error.abs() > tolerance_raw_squared {
                return Err(NormalScaleError {
                    row: row as u8,
                    column: column as u8,
                    error,
                    tolerance: tolerance_raw_squared,
                });
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(raw: i16) -> Q14 {
        Q14::from_raw(i128::from(raw)).unwrap()
    }

    #[test]
    fn integer_rotations_pass_exactly() {
        let matrix = [
            [q(0), q(-0x4000), q(0)],
            [q(0x4000), q(0), q(0)],
            [q(0), q(0), q(0x4000)],
        ];
        assert_eq!(check_scale_preserving(matrix, 0).unwrap().max_abs_error, 0);
    }

    #[test]
    fn quantized_forty_five_degree_rotation_fits_trial_bound() {
        let matrix = [
            [q(11585), q(-11585), q(0)],
            [q(11585), q(11585), q(0)],
            [q(0), q(0), q(0x4000)],
        ];
        let report = check_scale_preserving(matrix, TRIAL_GRAM_TOLERANCE).unwrap();
        assert_eq!(report.max_abs_error, 11006);
        assert!(check_scale_preserving(matrix, 10000).is_err());
    }

    #[test]
    fn nonuniform_scale_and_shear_fail_matrix_wide_promise() {
        let scaled = [
            [q(0x2000), q(0), q(0)],
            [q(0), q(0x4000), q(0)],
            [q(0), q(0), q(0x4000)],
        ];
        assert_eq!(
            check_scale_preserving(scaled, TRIAL_GRAM_TOLERANCE)
                .unwrap_err()
                .row,
            0
        );
        let sheared = [
            [q(0x4000), q(0x1000), q(0)],
            [q(0), q(0x4000), q(0)],
            [q(0), q(0), q(0x4000)],
        ];
        assert!(check_scale_preserving(sheared, TRIAL_GRAM_TOLERANCE).is_err());
    }
}
