//! Exact arithmetic candidates for moving compact-vertex MVP work below the
//! Gowin 36x36 multiplier boundary. This is an operation/width model, not a
//! scheduled replacement for `TransformUnit` or an RTL resource result.

use crate::fixed::{round_shift_ties_even, Fx, NumericFault, WideFx, Q16};
use crate::format::CompactGrid;

type Signed18 = Fx<17, 0, true>;
type Unsigned4 = Fx<4, 0, false>;
type Unsigned8 = Fx<8, 0, false>;
type Product36x18 = Fx<49, 0, true>;
type Product32x4 = Fx<35, 0, true>;
type Product36x10 = Fx<45, 0, true>;
type Product18x10 = Fx<27, 0, true>;
type Product10x8 = Fx<18, 0, false>;
type Accumulator = WideFx<66, true>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MvpWork {
    /// Each result occupies two 18x18 DSP macro issue equivalents.
    pub wide36x36: u32,
    /// Each result occupies one 18x18 DSP macro issue equivalent.
    pub narrow36x18: u32,
    /// Small products are shift/add work outside the DSP macro.
    pub residual32x4: u32,
    pub translation_adds: u32,
}

impl MvpWork {
    pub const fn macro_issues(self) -> u32 {
        self.wide36x36 * 2 + self.narrow36x18
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinearTerm {
    Narrow {
        high: Signed18,
        tail: Unsigned4,
        shift: u8,
    },
    Wide(Q16),
}

impl LinearTerm {
    fn new(coefficient: Q16) -> Self {
        let raw = coefficient.raw();
        for shift in 0..=4 {
            // Arithmetic shift is floor division, including negative values.
            let high_raw = raw >> shift;
            if let Ok(high) = Signed18::from_raw(i128::from(high_raw)) {
                let tail = raw - (high_raw << shift);
                return Self::Narrow {
                    high,
                    tail: Unsigned4::from_raw(i128::from(tail)).unwrap(),
                    shift,
                };
            }
        }
        Self::Wide(coefficient)
    }

    fn product(self, position: Q16, work: &mut MvpWork) -> Result<Accumulator, NumericFault> {
        let x = position.raw();
        let raw = match self {
            Self::Narrow { high, tail, shift } => {
                let main = Product36x18::from_raw(i128::from(x) * i128::from(high.raw()))?;
                if high.raw() != 0 {
                    work.narrow36x18 += 1;
                }
                let remainder = Product32x4::from_raw(i128::from(x) * i128::from(tail.raw()))?;
                if tail.raw() != 0 {
                    work.residual32x4 += 1;
                }
                (i128::from(main.raw()) << shift) + i128::from(remainder.raw())
            }
            Self::Wide(coefficient) => {
                if coefficient.raw() != 0 {
                    work.wide36x36 += 1;
                }
                i128::from(x) * i128::from(coefficient.raw())
            }
        };
        Accumulator::from_raw(raw)
    }
}

/// At most four coefficient bits are assigned to shift/add correction.
/// Coefficients beyond that budget retain the exact 36x36 route.
#[derive(Clone, Debug)]
pub struct AffineMvpPlan {
    matrix: [[Q16; 4]; 4],
    linear: [[LinearTerm; 3]; 4],
}

impl AffineMvpPlan {
    pub fn new(matrix: [[Q16; 4]; 4]) -> Self {
        let linear = std::array::from_fn(|row| {
            std::array::from_fn(|axis| LinearTerm::new(matrix[row][axis]))
        });
        Self { matrix, linear }
    }

    pub fn transform(&self, position: [Q16; 4]) -> Result<([Q16; 4], MvpWork), NumericFault> {
        let mut work = MvpWork::default();
        let mut result = [Q16::from_raw(0).unwrap(); 4];
        for (row, output) in result.iter_mut().enumerate() {
            let mut sum = if position[3].raw() == 1 << 16 {
                work.translation_adds += 1;
                Accumulator::from_raw(i128::from(self.matrix[row][3].raw()) << 16)?
            } else {
                // Expanded homogeneous vertices may have arbitrary w.
                work.wide36x36 += 1;
                Accumulator::from_raw(
                    i128::from(self.matrix[row][3].raw()) * i128::from(position[3].raw()),
                )?
            };
            for (axis, &component) in position.iter().enumerate().take(3) {
                sum = sum.checked_add(self.linear[row][axis].product(component, &mut work)?)?;
            }
            *output = Q16::from_raw(round_shift_ties_even(sum.raw(), 16)?)?;
        }
        Ok((result, work))
    }

    pub fn coefficient_shifts(&self) -> [[Option<u8>; 3]; 4] {
        self.linear.map(|row| {
            row.map(|term| match term {
                LinearTerm::Narrow { shift, .. } => Some(shift),
                LinearTerm::Wide(_) => None,
            })
        })
    }
}

/// A second, meshlet-local exact factorization:
/// M * (base + xyz10 * stride, 1) = M * (base, 1) + xyz10 * (M * stride).
/// The precomputed slope must fit signed 36. A row-shared shift of at most
/// eight bits then moves every slope to signed 18, with exact short residuals.
/// Wider rows retain the 36x10 route. The row-shared shift makes two high
/// products in the same row eligible for one paired DSP ALU result.
#[derive(Clone, Debug)]
pub struct MeshletMvpPlan {
    base_sum: [Accumulator; 4],
    slopes: [[Fx<35, 0, true>; 3]; 4],
    row_shift: [Option<u8>; 4],
    high: [[Signed18; 3]; 4],
    tail: [[Unsigned8; 3]; 4],
    precompute_work: MvpWork,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeshletVertexWork {
    pub products18x10: u32,
    pub products36x10: u32,
    pub residual10x8: u32,
    /// Optimistic issue count if two high products share one ALU54 sum.
    pub paired18_issue_lower_bound: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PairedRowOperands {
    pub base_raw: i128,
    pub shift: u8,
    pub high: [i32; 3],
    pub tail: [u8; 3],
}

impl MeshletMvpPlan {
    /// Returns `None` when this meshlet cannot use the signed-36 slope path.
    pub fn new(matrix: [[Q16; 4]; 4], grid: CompactGrid) -> Option<Self> {
        if !grid.valid() {
            return None;
        }
        let stride = i128::from(grid.base_step) << grid.level;
        let base = std::array::from_fn::<_, 3, _>(|axis| {
            let raw = i128::from(grid.origin[axis])
                + i128::from(grid.meshlet_base[axis]) * i128::from(grid.base_step);
            Q16::from_raw(raw)
        });
        let base = [base[0].ok()?, base[1].ok()?, base[2].ok()?];
        let mut base_sum = [Accumulator::from_raw(0).unwrap(); 4];
        let mut slopes = [[Fx::from_raw(0).unwrap(); 3]; 4];
        let mut row_shift = [None; 4];
        let mut high = [[Signed18::from_raw(0).unwrap(); 3]; 4];
        let mut tail = [[Unsigned8::from_raw(0).unwrap(); 3]; 4];
        let affine = AffineMvpPlan::new(matrix);
        let mut precompute_work = MvpWork::default();
        for row in 0..4 {
            precompute_work.translation_adds += 1;
            let mut sum = Accumulator::from_raw(i128::from(matrix[row][3].raw()) << 16).ok()?;
            for axis in 0..3 {
                sum = sum
                    .checked_add(
                        affine.linear[row][axis]
                            .product(base[axis], &mut precompute_work)
                            .ok()?,
                    )
                    .ok()?;
                slopes[row][axis] =
                    Fx::from_raw(i128::from(matrix[row][axis].raw()) * stride).ok()?;
            }
            base_sum[row] = sum;
            for shift in 0..=8 {
                if (0..3).all(|axis| {
                    Signed18::from_raw(i128::from(slopes[row][axis].raw() >> shift)).is_ok()
                }) {
                    row_shift[row] = Some(shift);
                    for axis in 0..3 {
                        let raw = slopes[row][axis].raw();
                        let upper = raw >> shift;
                        high[row][axis] = Signed18::from_raw(i128::from(upper)).unwrap();
                        tail[row][axis] =
                            Unsigned8::from_raw(i128::from(raw - (upper << shift))).unwrap();
                    }
                    break;
                }
            }
        }
        Some(Self {
            base_sum,
            slopes,
            row_shift,
            high,
            tail,
            precompute_work,
        })
    }

    pub const fn row_shifts(&self) -> [Option<u8>; 4] {
        self.row_shift
    }

    pub const fn precompute_work(&self) -> MvpWork {
        self.precompute_work
    }

    pub(crate) fn paired_rows(&self) -> Option<[PairedRowOperands; 4]> {
        let mut rows = [PairedRowOperands {
            base_raw: 0,
            shift: 0,
            high: [0; 3],
            tail: [0; 3],
        }; 4];
        for (index, row) in rows.iter_mut().enumerate() {
            row.base_raw = self.base_sum[index].raw();
            row.shift = self.row_shift[index]?;
            row.high = self.high[index].map(|value| value.raw() as i32);
            row.tail = self.tail[index].map(|value| value.raw() as u8);
        }
        Some(rows)
    }

    pub fn transform(
        &self,
        xyz10: [u16; 3],
    ) -> Result<([Q16; 4], MeshletVertexWork), NumericFault> {
        if xyz10.iter().any(|&value| value > 1023) {
            return Err(NumericFault::OutOfRange);
        }
        let mut work = MeshletVertexWork::default();
        let mut output = [Q16::from_raw(0).unwrap(); 4];
        for (row, value) in output.iter_mut().enumerate() {
            let mut sum = self.base_sum[row];
            let mut active_high = 0_u32;
            for (axis, &delta) in xyz10.iter().enumerate() {
                let product = if let Some(shift) = self.row_shift[row] {
                    let upper = self.high[row][axis].raw();
                    let lower = self.tail[row][axis].raw();
                    let main = Product18x10::from_raw(i128::from(upper) * i128::from(delta))?;
                    let remainder = Product10x8::from_raw(i128::from(lower) * i128::from(delta))?;
                    if upper != 0 && delta != 0 {
                        work.products18x10 += 1;
                        active_high += 1;
                    }
                    if lower != 0 && delta != 0 {
                        work.residual10x8 += 1;
                    }
                    (i128::from(main.raw()) << shift) + i128::from(remainder.raw())
                } else {
                    let slope = self.slopes[row][axis].raw();
                    if slope != 0 && delta != 0 {
                        work.products36x10 += 1;
                    }
                    i128::from(Product36x10::from_raw(i128::from(slope) * i128::from(delta))?.raw())
                };
                sum = sum.checked_add(Accumulator::from_raw(product)?)?;
            }
            work.paired18_issue_lower_bound += active_high.div_ceil(2);
            *value = Q16::from_raw(round_shift_ties_even(sum.raw(), 16)?)?;
        }
        Ok((output, work))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::Q14;
    use crate::format::{InputVertex, Uniform};
    use crate::transform::TransformUnit;

    fn q(raw: i32) -> Q16 {
        Q16::from_raw(i128::from(raw)).unwrap()
    }

    fn reference(matrix: [[Q16; 4]; 4], position: [Q16; 4]) -> Result<[Q16; 4], NumericFault> {
        let mut output = [q(0); 4];
        for (row, value) in output.iter_mut().enumerate() {
            let mut sum = Accumulator::from_raw(0)?;
            for axis in 0..4 {
                let product =
                    i128::from(matrix[row][axis].raw()) * i128::from(position[axis].raw());
                sum = sum.checked_add(Accumulator::from_raw(product)?)?;
            }
            *value = Q16::from_raw(round_shift_ties_even(sum.raw(), 16)?)?;
        }
        Ok(output)
    }

    #[test]
    fn affine_split_matches_full_product_including_negative_ties_and_fallback() {
        let mut seed = 0x71ed_cafe_u64;
        for iteration in 0..10_000 {
            let mut next = || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as i32
            };
            let matrix = std::array::from_fn(|_| std::array::from_fn(|_| q(next() % 2_000_001)));
            let mut position = std::array::from_fn(|_| q(next() % 4_000_001));
            if iteration % 5 != 0 {
                position[3] = q(1 << 16);
            }
            let plan = AffineMvpPlan::new(matrix);
            assert_eq!(
                plan.transform(position).map(|(clip, _)| clip),
                reference(matrix, position)
            );
        }
        let matrix = [[q(i32::MAX); 4]; 4];
        assert!(AffineMvpPlan::new(matrix).coefficient_shifts()[0][0].is_none());
        assert_eq!(
            AffineMvpPlan::new(matrix)
                .transform([q(1); 4])
                .map(|(clip, _)| clip),
            reference(matrix, [q(1); 4])
        );
    }

    #[test]
    fn meshlet_factorization_matches_global_transform_and_width_classes() {
        let matrix = [
            [q(143_668), q(34_406), q(94_531), q(-1_229)],
            [q(0), q(274_682), q(-99_976), q(-30_453)],
            [q(0), q(0), q(0), q(0)],
            [q(96_800), q(-47_282), q(-129_907), q(72_172)],
        ];
        let grid = CompactGrid {
            origin: [-20_000, 10_000, 0],
            base_step: 1,
            meshlet_base: [0; 3],
            level: 0,
        };
        let plan = MeshletMvpPlan::new(matrix, grid).unwrap();
        for xyz in [[0, 0, 0], [1, 23, 1023], [1023, 1023, 1023]] {
            let position = [
                q(grid.origin[0] + i32::from(xyz[0])),
                q(grid.origin[1] + i32::from(xyz[1])),
                q(i32::from(xyz[2])),
                q(1 << 16),
            ];
            let (actual, work) = plan.transform(xyz).unwrap();
            assert_eq!(actual, reference(matrix, position).unwrap());
            assert_eq!(work.products36x10, 0);
            assert_eq!(work.products18x10, if xyz == [0, 0, 0] { 0 } else { 8 });
        }
        let coarse = CompactGrid { level: 24, ..grid };
        assert!(MeshletMvpPlan::new(matrix, coarse).is_none());
    }

    #[test]
    fn staged_transform_unit_and_local_plan_agree_on_the_same_vertex() {
        let matrix = [
            [q(143_668), q(34_406), q(94_531), q(-1_229)],
            [q(0), q(274_682), q(-99_976), q(-30_453)],
            [q(0), q(0), q(0), q(0)],
            [q(96_800), q(-47_282), q(-129_907), q(72_172)],
        ];
        let grid = CompactGrid {
            origin: [-20_000, 10_000, 0],
            base_step: 1,
            meshlet_base: [0; 3],
            level: 3,
        };
        let xyz = [101, 33, 17];
        let position = std::array::from_fn::<_, 4, _>(|axis| {
            if axis == 3 {
                q(1 << 16)
            } else {
                q(grid.origin[axis] + i32::from(xyz[axis]) * 8)
            }
        });
        let input = InputVertex {
            position,
            normal: [Q14::from_raw(0).unwrap(); 3],
            rgba: [0; 4],
        };
        let mut unit = TransformUnit::new(
            Uniform {
                mvp: matrix,
                normal: [[Q14::from_raw(0).unwrap(); 3]; 3],
            },
            input,
        );
        let mut old = None;
        for _ in 0..20 {
            let (_, output) = unit.tick().unwrap();
            if output.is_some() {
                old = output;
                break;
            }
        }
        let old = old.unwrap().clip;
        assert_eq!(
            AffineMvpPlan::new(matrix).transform(position).unwrap().0,
            old
        );
        assert_eq!(
            MeshletMvpPlan::new(matrix, grid)
                .unwrap()
                .transform(xyz)
                .unwrap()
                .0,
            old
        );
    }

    #[test]
    fn local_slope_splits_cover_random_signed_rows_and_wide_row_fallback() {
        let mut state = 0x9234_7bca_6d01_e451_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as i32
        };
        let mut saw_wide_row = false;
        for iteration in 0..2_000 {
            let level = (iteration % 8) as u8;
            let grid = CompactGrid {
                origin: std::array::from_fn(|_| next() % 10_001),
                base_step: 1,
                meshlet_base: [0; 3],
                level,
            };
            let matrix = std::array::from_fn(|_| {
                std::array::from_fn(|_| {
                    q(if iteration % 19 == 0 {
                        next() % 40_000_001
                    } else {
                        next() % 1_000_001
                    })
                })
            });
            let xyz = std::array::from_fn(|_| (next() as u32 % 1024) as u16);
            let position = std::array::from_fn::<_, 4, _>(|axis| {
                if axis == 3 {
                    q(1 << 16)
                } else {
                    q(grid.origin[axis] + (i32::from(xyz[axis]) << level))
                }
            });
            let plan = MeshletMvpPlan::new(matrix, grid).unwrap();
            saw_wide_row |= plan.row_shifts().contains(&None);
            assert_eq!(
                plan.transform(xyz).map(|(clip, _)| clip),
                reference(matrix, position)
            );
        }
        assert!(saw_wide_row);
    }
}
