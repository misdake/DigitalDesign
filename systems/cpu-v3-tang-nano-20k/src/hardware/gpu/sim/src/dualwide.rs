//! Experimental two-wide 36x36 vertex unit. The single synchronous 64-bit
//! matrix port supplies both MVP coefficients of a pair. The priming edge
//! issues two normal products, hiding the BSRAM read latency. This candidate
//! requires two independent product retirement/accumulation paths.

use crate::fixed::{round_shift_ties_even, Q14, Q16};
use crate::format::InputVertex;
use crate::result_store::TransformedVertex;
use crate::timing::{Mult36Pipeline, RamRead};
use crate::transform::TransformError;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DualWideCycle {
    pub matrix_read: Option<usize>,
    pub issue: [Option<u8>; 2],
    pub retire: [Option<u8>; 2],
}

#[derive(Clone, Debug)]
pub struct DualWideUnit {
    normal_matrix: [[Q14; 3]; 3],
    input: InputVertex,
    matrix_base: usize,
    primed: bool,
    mvp_pair: u8,
    normal_next: u8,
    retired: u8,
    lanes: [Mult36Pipeline; 2],
    clip_sums: [i128; 4],
    normal_sums: [i128; 3],
    clip: [Q16; 4],
    normal: [Q14; 3],
}

impl DualWideUnit {
    pub fn new(normal_matrix: [[Q14; 3]; 3], input: InputVertex, matrix_base: usize) -> Self {
        Self {
            normal_matrix,
            input,
            matrix_base,
            primed: false,
            mvp_pair: 0,
            normal_next: 0,
            retired: 0,
            lanes: std::array::from_fn(|_| Mult36Pipeline::default()),
            clip_sums: [0; 4],
            normal_sums: [0; 3],
            clip: [Q16::from_raw(0).unwrap(); 4],
            normal: [Q14::from_raw(0).unwrap(); 3],
        }
    }

    fn normal_issue(&mut self) -> Option<(u64, i64, i64)> {
        if self.normal_next == 9 {
            return None;
        }
        let tag = self.normal_next;
        self.normal_next += 1;
        Some((
            u64::from(16 + tag),
            self.normal_matrix[usize::from(tag / 3)][usize::from(tag % 3)].raw(),
            self.input.normal[usize::from(tag % 3)].raw(),
        ))
    }

    pub fn tick(
        &mut self,
        matrix_output: RamRead,
    ) -> Result<(DualWideCycle, Option<TransformedVertex>), TransformError> {
        let mut cycle = DualWideCycle::default();
        let issue = if !self.primed {
            self.primed = true;
            cycle.matrix_read = Some(self.matrix_base);
            [self.normal_issue(), self.normal_issue()]
        } else if self.mvp_pair < 8 {
            let pair = match matrix_output {
                RamRead::Data(pair) => pair,
                RamRead::Collision => return Err(TransformError::MatrixCollision),
            };
            let tag = self.mvp_pair * 2;
            let issue = [
                Some((
                    u64::from(tag),
                    i64::from(pair as u32 as i32),
                    self.input.position[usize::from(tag % 4)].raw(),
                )),
                Some((
                    u64::from(tag + 1),
                    i64::from((pair >> 32) as u32 as i32),
                    self.input.position[usize::from((tag + 1) % 4)].raw(),
                )),
            ];
            self.mvp_pair += 1;
            if self.mvp_pair < 8 {
                cycle.matrix_read = Some(self.matrix_base + usize::from(self.mvp_pair));
            }
            issue
        } else {
            [self.normal_issue(), self.normal_issue()]
        };
        for (lane, operation) in issue.into_iter().enumerate() {
            cycle.issue[lane] = operation.map(|(tag, _, _)| tag as u8);
            if let Some((tag, product)) = self.lanes[lane]
                .tick(operation)
                .map_err(TransformError::DspOperand)?
            {
                let tag = tag as u8;
                cycle.retire[lane] = Some(tag);
                self.retired += 1;
                if tag < 16 {
                    let row = usize::from(tag / 4);
                    self.clip_sums[row] += product;
                    if tag % 4 == 3 {
                        let rounded = round_shift_ties_even(self.clip_sums[row], 16)
                            .map_err(TransformError::DspOperand)?;
                        self.clip[row] = Q16::from_raw(rounded)
                            .map_err(|_| TransformError::ClipOverflow { row: row as u8 })?;
                    }
                } else {
                    let normal_tag = tag - 16;
                    let row = usize::from(normal_tag / 3);
                    self.normal_sums[row] += product;
                    if normal_tag % 3 == 2 {
                        let rounded = round_shift_ties_even(self.normal_sums[row], 14)
                            .map_err(TransformError::DspOperand)?;
                        self.normal[row] = Q14::from_raw(rounded)
                            .map_err(|_| TransformError::NormalOverflow { row: row as u8 })?;
                    }
                }
            }
        }
        let result = (self.retired == 25).then_some(TransformedVertex {
            clip: self.clip,
            normal: self.normal,
            rgba: self.input.rgba,
        });
        Ok((cycle, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::{Q14, Q16};
    use crate::timing::SyncRam64;
    use crate::transform::TransformUnit;

    fn q16(value: i32) -> Q16 {
        Q16::from_raw(i128::from(value)).unwrap()
    }
    fn q14(value: i16) -> Q14 {
        Q14::from_raw(i128::from(value)).unwrap()
    }

    #[test]
    fn dual_wide_hides_matrix_prime_with_normal_work_and_matches_mixed() {
        let mvp = [
            [q16(0x10000), q16(0x8000), q16(-0x4000), q16(0)],
            [q16(0), q16(0x10000), q16(0), q16(0x4000)],
            [q16(0x2000), q16(0), q16(0x10000), q16(0)],
            [q16(0), q16(0), q16(0), q16(0x10000)],
        ];
        let normal = [
            [q14(0), q14(-0x4000), q14(0)],
            [q14(0x4000), q14(0), q14(0)],
            [q14(0), q14(0), q14(0x4000)],
        ];
        let input = InputVertex {
            position: [q16(0x18000), q16(-0x08000), q16(0x04000), q16(0x10000)],
            normal: [q14(0x2000), q14(-0x1000), q14(0x0800)],
            rgba: [4, 3, 2, 1],
        };
        let mut words = vec![0; 11];
        for pair in 0..8 {
            let row = pair / 2;
            let column = pair % 2 * 2;
            words[3 + pair] = u64::from(mvp[row][column].raw() as i32 as u32)
                | (u64::from(mvp[row][column + 1].raw() as i32 as u32) << 32);
        }
        let mut dual_ram = SyncRam64::new(words.clone());
        let mut dual = DualWideUnit::new(normal, input, 3);
        let mut dual_result = None;
        let mut reads = Vec::new();
        let mut issues = Vec::new();
        for edge in 1..=20 {
            let (cycle, result) = dual.tick(dual_ram.output()).unwrap();
            if let Some(address) = cycle.matrix_read {
                reads.push(address);
            }
            issues.extend(cycle.issue.into_iter().flatten());
            dual_ram.tick(cycle.matrix_read, None);
            if let Some(result) = result {
                dual_result = Some((edge, result));
                break;
            }
        }
        assert_eq!(reads, (3..11).collect::<Vec<_>>());
        assert_eq!(issues.len(), 25);
        assert_eq!(&issues[..2], &[16, 17]);
        assert_eq!(dual_result.unwrap().0, 15);

        let mut mixed_ram = SyncRam64::new(words);
        let mut mixed = TransformUnit::new_streaming(normal, input, 3);
        let mut mixed_result = None;
        for edge in 1..=24 {
            let (cycle, result) = mixed.tick_with_ram(mixed_ram.output()).unwrap();
            mixed_ram.tick(cycle.matrix_read, None);
            if let Some(result) = result {
                mixed_result = Some((edge, result));
                break;
            }
        }
        assert_eq!(mixed_result.unwrap().0, 19);
        assert_eq!(dual_result.unwrap().1, mixed_result.unwrap().1);
        assert_eq!(
            dual_result.unwrap().1.clip.map(Q16::raw),
            [0x13000, -0x4000, 0x7000, 0x10000]
        );
        assert_eq!(
            dual_result.unwrap().1.normal.map(Q14::raw),
            [0x1000, 0x2000, 0x0800]
        );
    }

    #[test]
    fn dual_wide_signed_corpus_matches_independent_integer_golden() {
        let mut state = 0x25f1_732a_d182_c5b7_u64;
        let mut next = || {
            state = state
                .wrapping_mul(2862933555777941757)
                .wrapping_add(3037000493);
            (state >> 32) as u32
        };
        let rne = |value: i128, shift: u32| {
            let unit = 1_i128 << shift;
            let floor = value.div_euclid(unit);
            let fraction = value.rem_euclid(unit);
            floor + i128::from(fraction * 2 > unit || (fraction * 2 == unit && floor & 1 != 0))
        };
        for case in 0..16 {
            let mvp: [[Q16; 4]; 4] = std::array::from_fn(|_| {
                std::array::from_fn(|_| q16((next() % 0x30001) as i32 - 0x18000))
            });
            let mut normal = [[q14(0); 3]; 3];
            for (row, coefficients) in normal.iter_mut().enumerate() {
                coefficients[(row + case % 3) % 3] = q14(if (row + case) & 1 == 0 {
                    0x4000
                } else {
                    -0x4000
                });
            }
            let input = InputVertex {
                position: std::array::from_fn(|_| q16((next() % 0x50001) as i32 - 0x28000)),
                normal: std::array::from_fn(|_| q14((next() % 0x6001) as i16 - 0x3000)),
                rgba: next().to_le_bytes(),
            };
            let words = (0..8)
                .map(|pair| {
                    let row = pair / 2;
                    let column = pair % 2 * 2;
                    u64::from(mvp[row][column].raw() as i32 as u32)
                        | (u64::from(mvp[row][column + 1].raw() as i32 as u32) << 32)
                })
                .collect::<Vec<_>>();
            let mut ram = SyncRam64::new(words);
            let mut unit = DualWideUnit::new(normal, input, 0);
            let mut output = None;
            for edge in 1..=18 {
                let (cycle, result) = unit.tick(ram.output()).unwrap();
                ram.tick(cycle.matrix_read, None);
                if let Some(result) = result {
                    output = Some((edge, result));
                    break;
                }
            }
            let (edge, output) = output.unwrap();
            assert_eq!(edge, 15, "case={case}");
            for (row, coefficients) in mvp.iter().enumerate() {
                let sum = (0..4)
                    .map(|column| {
                        i128::from(coefficients[column].raw())
                            * i128::from(input.position[column].raw())
                    })
                    .sum();
                assert_eq!(
                    output.clip[row].raw(),
                    rne(sum, 16) as i64,
                    "case={case} clip row={row}"
                );
            }
            for (row, coefficients) in normal.iter().enumerate() {
                let sum = (0..3)
                    .map(|column| {
                        i128::from(coefficients[column].raw())
                            * i128::from(input.normal[column].raw())
                    })
                    .sum();
                assert_eq!(
                    output.normal[row].raw(),
                    rne(sum, 14) as i64,
                    "case={case} normal row={row}"
                );
            }
            assert_eq!(output.rgba, input.rgba);
        }
    }
}
