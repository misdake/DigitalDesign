//! One 36x36 and one 18x18 product per edge, with independent accumulators.
//! Matrix and vertex operands have already crossed the synchronous BSRAM read
//! boundary into local staging registers before the first product is issued.

use crate::fixed::{round_shift_ties_even, Fx, NumericFault, WideFx, Q14, Q16};
use crate::format::{InputVertex, Uniform};
use crate::result_store::TransformedVertex;
use crate::timing::{Mult18Pipeline, Mult36Pipeline, RamRead};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransformError {
    DspOperand(NumericFault),
    ClipOverflow { row: u8 },
    NormalOverflow { row: u8 },
    MatrixCollision,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TransformCycle {
    pub matrix_read: Option<usize>,
    pub wide_issue: Option<u8>,
    pub small_issue: Option<u8>,
    pub wide_retire: Option<u8>,
    pub small_retire: Option<u8>,
}

#[derive(Clone, Debug)]
enum MvpSource {
    Staged([[Q16; 4]; 4]),
    Streaming {
        base_word: usize,
        primed: bool,
        high_coefficient: i32,
    },
}

#[derive(Clone, Debug)]
pub struct TransformUnit {
    mvp_source: MvpSource,
    normal_matrix: [[Q14; 3]; 3],
    input: InputVertex,
    wide: Mult36Pipeline,
    small: Mult18Pipeline,
    wide_next: u8,
    small_next: u8,
    wide_retired: u8,
    small_retired: u8,
    clip_sums: [WideFx<66, true>; 4],
    normal_sums: [Fx<32, 0, true>; 3],
    clip: [Q16; 4],
    normal: [Q14; 3],
}

impl TransformUnit {
    pub fn new(uniform: Uniform, input: InputVertex) -> Self {
        Self::with_source(MvpSource::Staged(uniform.mvp), uniform.normal, input)
    }

    pub fn new_streaming(
        normal_matrix: [[Q14; 3]; 3],
        input: InputVertex,
        mvp_base_word: usize,
    ) -> Self {
        Self::with_source(
            MvpSource::Streaming {
                base_word: mvp_base_word,
                primed: false,
                high_coefficient: 0,
            },
            normal_matrix,
            input,
        )
    }

    fn with_source(
        mvp_source: MvpSource,
        normal_matrix: [[Q14; 3]; 3],
        input: InputVertex,
    ) -> Self {
        Self {
            mvp_source,
            normal_matrix,
            input,
            wide: Mult36Pipeline::default(),
            small: Mult18Pipeline::default(),
            wide_next: 0,
            small_next: 0,
            wide_retired: 0,
            small_retired: 0,
            clip_sums: [WideFx::from_raw(0).unwrap(); 4],
            normal_sums: [Fx::from_raw(0).unwrap(); 3],
            clip: [Q16::from_raw(0).unwrap(); 4],
            normal: [Q14::from_raw(0).unwrap(); 3],
        }
    }

    pub fn tick(&mut self) -> Result<(TransformCycle, Option<TransformedVertex>), TransformError> {
        assert!(matches!(&self.mvp_source, MvpSource::Staged(_)));
        self.tick_with_ram(RamRead::Data(0))
    }

    pub fn tick_with_ram(
        &mut self,
        matrix_output: RamRead,
    ) -> Result<(TransformCycle, Option<TransformedVertex>), TransformError> {
        let mut cycle = TransformCycle::default();
        let mut coefficient = None;
        if self.wide_next < 16 {
            match &mut self.mvp_source {
                MvpSource::Staged(matrix) => {
                    let tag = self.wide_next;
                    coefficient = Some(matrix[usize::from(tag / 4)][usize::from(tag % 4)].raw());
                }
                MvpSource::Streaming {
                    base_word,
                    primed,
                    high_coefficient,
                } => {
                    if !*primed {
                        cycle.matrix_read = Some(*base_word);
                        *primed = true;
                    } else if self.wide_next & 1 == 0 {
                        let pair = match matrix_output {
                            RamRead::Data(word) => word,
                            RamRead::Collision => return Err(TransformError::MatrixCollision),
                        };
                        *high_coefficient = (pair >> 32) as u32 as i32;
                        coefficient = Some(i64::from(pair as u32 as i32));
                        let next_pair = usize::from(self.wide_next / 2 + 1);
                        if next_pair < 8 {
                            cycle.matrix_read = Some(*base_word + next_pair);
                        }
                    } else {
                        coefficient = Some(i64::from(*high_coefficient));
                    }
                }
            }
        }
        let wide_issue = if let Some(coefficient) = coefficient {
            let tag = self.wide_next;
            self.wide_next += 1;
            cycle.wide_issue = Some(tag);
            Some((
                u64::from(tag),
                coefficient,
                self.input.position[usize::from(tag % 4)].raw(),
            ))
        } else {
            None
        };
        let small_issue = if self.small_next < 9 {
            let tag = self.small_next;
            self.small_next += 1;
            cycle.small_issue = Some(tag);
            Some((
                u64::from(tag),
                self.normal_matrix[usize::from(tag / 3)][usize::from(tag % 3)].raw() as i32,
                self.input.normal[usize::from(tag % 3)].raw() as i32,
            ))
        } else {
            None
        };
        if let Some((tag, product)) = self
            .wide
            .tick(wide_issue)
            .map_err(TransformError::DspOperand)?
        {
            let tag = tag as u8;
            cycle.wide_retire = Some(tag);
            self.wide_retired += 1;
            let row = usize::from(tag / 4);
            self.clip_sums[row] = self.clip_sums[row]
                .checked_add(WideFx::from_raw(product).map_err(TransformError::DspOperand)?)
                .map_err(TransformError::DspOperand)?;
            if tag % 4 == 3 {
                let rounded = round_shift_ties_even(self.clip_sums[row].raw(), 16)
                    .map_err(TransformError::DspOperand)?;
                self.clip[row] = Q16::from_raw(rounded)
                    .map_err(|_| TransformError::ClipOverflow { row: row as u8 })?;
            }
        }
        if let Some((tag, product)) = self
            .small
            .tick(small_issue)
            .map_err(TransformError::DspOperand)?
        {
            let tag = tag as u8;
            cycle.small_retire = Some(tag);
            self.small_retired += 1;
            let row = usize::from(tag / 3);
            self.normal_sums[row] = self.normal_sums[row]
                .checked_add(Fx::from_raw(i128::from(product)).map_err(TransformError::DspOperand)?)
                .map_err(TransformError::DspOperand)?;
            if tag % 3 == 2 {
                let rounded = round_shift_ties_even(i128::from(self.normal_sums[row].raw()), 14)
                    .map_err(TransformError::DspOperand)?;
                self.normal[row] = Q14::from_raw(rounded)
                    .map_err(|_| TransformError::NormalOverflow { row: row as u8 })?;
            }
        }
        let result = if self.wide_retired == 16 && self.small_retired == 9 {
            Some(TransformedVertex {
                clip: self.clip,
                normal: self.normal,
                rgba: self.input.rgba,
            })
        } else {
            None
        };
        Ok((cycle, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::SyncRam64;

    #[test]
    fn accumulator_widths_cover_raw_operand_extrema() {
        let clip_product = i128::from(i32::MIN) * i128::from(i32::MIN);
        let clip_sum = 4 * clip_product;
        assert!(WideFx::<65, true>::from_raw(clip_sum).is_err());
        assert!(WideFx::<66, true>::from_raw(clip_sum).is_ok());

        let normal_product = i128::from(i16::MIN) * i128::from(i16::MIN);
        let normal_sum = 3 * normal_product;
        assert!(Fx::<31, 0, true>::from_raw(normal_sum).is_err());
        assert!(Fx::<32, 0, true>::from_raw(normal_sum).is_ok());
    }

    fn q16(raw: i32) -> Q16 {
        Q16::from_raw(i128::from(raw)).unwrap()
    }
    fn q14(raw: i16) -> Q14 {
        Q14::from_raw(i128::from(raw)).unwrap()
    }

    #[test]
    fn mixed_lanes_finish_at_edge_eighteen_and_round_once() {
        let mut uniform = Uniform {
            mvp: [[q16(0); 4]; 4],
            normal: [[q14(0); 3]; 3],
        };
        for row in 0..4 {
            uniform.mvp[row][row] = q16(0x10000);
        }
        for row in 0..3 {
            uniform.normal[row][row] = q14(0x4000);
        }
        uniform.mvp[0][0] = q16(0x8000);
        uniform.mvp[0][1] = q16(0x8000);
        let input = InputVertex {
            position: [q16(1), q16(1), q16(-0x20000), q16(0x10000)],
            normal: [q14(0x2000), q14(-0x2000), q14(0x1000)],
            rgba: [9, 8, 7, 6],
        };
        let mut unit = TransformUnit::new(uniform, input);
        let mut finish = None;
        let mut wide_issues = 0;
        let mut small_issues = 0;
        for edge in 1..=24 {
            let (cycle, result) = unit.tick().unwrap();
            wide_issues += usize::from(cycle.wide_issue.is_some());
            small_issues += usize::from(cycle.small_issue.is_some());
            if let Some(result) = result {
                finish = Some((edge, result));
                break;
            }
        }
        let (edge, result) = finish.unwrap();
        assert_eq!(edge, 18);
        assert_eq!((wide_issues, small_issues), (16, 9));
        assert_eq!(result.clip.map(Q16::raw), [1, 1, -0x20000, 0x10000]);
        assert_eq!(result.normal.map(Q14::raw), [0x2000, -0x2000, 0x1000]);
        assert_eq!(result.rgba, [9, 8, 7, 6]);
    }

    #[test]
    fn clip_overflow_is_fault_instead_of_wrapped_vertex() {
        let mut uniform = Uniform {
            mvp: [[q16(0); 4]; 4],
            normal: [[q14(0); 3]; 3],
        };
        uniform.mvp[0][0] = q16(i32::MAX);
        uniform.mvp[0][1] = q16(i32::MAX);
        let input = InputVertex {
            position: [q16(i32::MAX), q16(i32::MAX), q16(0), q16(0)],
            normal: [q14(0); 3],
            rgba: [0; 4],
        };
        let mut unit = TransformUnit::new(uniform, input);
        let mut fault = None;
        for _ in 0..20 {
            if let Err(error) = unit.tick() {
                fault = Some(error);
                break;
            }
        }
        assert_eq!(fault, Some(TransformError::ClipOverflow { row: 0 }));
    }

    #[test]
    fn streamed_matrix_uses_one_synchronous_port_and_eight_pair_reads() {
        let mut matrix = [[q16(0); 4]; 4];
        for (row, coefficients) in matrix.iter_mut().enumerate() {
            coefficients[row] = q16(0x10000);
        }
        matrix[0][1] = q16(0x8000);
        let mut normal = [[q14(0); 3]; 3];
        for (row, coefficients) in normal.iter_mut().enumerate() {
            coefficients[row] = q14(0x4000);
        }
        let input = InputVertex {
            position: [q16(0x10000), q16(0x8000), q16(-0x20000), q16(0x10000)],
            normal: [q14(0x2000), q14(-0x1000), q14(0x0800)],
            rgba: [11, 22, 33, 44],
        };
        let mut words = vec![0; 11];
        for pair in 0..8 {
            let row = pair / 2;
            let column = pair % 2 * 2;
            words[3 + pair] = u64::from(matrix[row][column].raw() as i32 as u32)
                | (u64::from(matrix[row][column + 1].raw() as i32 as u32) << 32);
        }
        let mut ram = SyncRam64::new(words);
        let mut unit = TransformUnit::new_streaming(normal, input, 3);
        let mut read_addresses = Vec::new();
        let mut result = None;
        for edge in 1..=24 {
            let (cycle, output) = unit.tick_with_ram(ram.output()).unwrap();
            if let Some(address) = cycle.matrix_read {
                read_addresses.push(address);
            }
            ram.tick(cycle.matrix_read, None);
            if let Some(output) = output {
                result = Some((edge, output));
                break;
            }
        }
        assert_eq!(read_addresses, (3..11).collect::<Vec<_>>());
        let (edge, output) = result.unwrap();
        assert_eq!(edge, 19);
        assert_eq!(
            output.clip.map(Q16::raw),
            [0x14000, 0x8000, -0x20000, 0x10000]
        );
        assert_eq!(output.normal.map(Q14::raw), [0x2000, -0x1000, 0x0800]);
        assert_eq!(output.rgba, [11, 22, 33, 44]);

        let mut collision = TransformUnit::new_streaming(normal, input, 3);
        assert_eq!(
            collision
                .tick_with_ram(RamRead::Data(0))
                .unwrap()
                .0
                .matrix_read,
            Some(3)
        );
        assert_eq!(
            collision.tick_with_ram(RamRead::Collision),
            Err(TransformError::MatrixCollision)
        );
    }

    #[test]
    fn normal_row_rounds_once_and_reports_range_fault() {
        let mut normal = [[q14(0); 3]; 3];
        normal[0][0] = q14(0x2000);
        normal[0][1] = q14(0x2000);
        let uniform = Uniform {
            mvp: [[q16(0); 4]; 4],
            normal,
        };
        let input = InputVertex {
            position: [q16(0); 4],
            normal: [q14(1), q14(1), q14(0)],
            rgba: [0; 4],
        };
        let mut unit = TransformUnit::new(uniform, input);
        let mut result = None;
        for _ in 0..18 {
            result = unit.tick().unwrap().1;
        }
        assert_eq!(result.unwrap().normal[0].raw(), 1);

        normal[0][0] = q14(0x4000);
        normal[0][1] = q14(0x4000);
        let input = InputVertex {
            normal: [q14(i16::MAX), q14(i16::MAX), q14(0)],
            ..input
        };
        let mut unit = TransformUnit::new(
            Uniform {
                mvp: [[q16(0); 4]; 4],
                normal,
            },
            input,
        );
        let mut fault = None;
        for _ in 0..18 {
            if let Err(error) = unit.tick() {
                fault = Some(error);
                break;
            }
        }
        assert_eq!(fault, Some(TransformError::NormalOverflow { row: 0 }));
    }
}
