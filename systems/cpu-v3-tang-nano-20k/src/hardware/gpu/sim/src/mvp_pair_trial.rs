//! Clocked arithmetic trial for one paired 18x18 DSP output and one small
//! digit-serial residual unit. Input vertex and meshlet plan are staged before
//! this unit starts; scratchpad loading and plan preparation are not hidden in
//! the reported vertex edge count. Pair latency is an explicit trial setting,
//! not a measured Gowin MULTADDALU timing contract.

use std::collections::VecDeque;

use crate::fixed::{round_shift_ties_even, Fx, NumericFault, WideFx, Q14, Q16};
use crate::mvp_width_trial::MeshletMvpPlan;
use crate::result_store::TransformedVertex;

type ClipSum = WideFx<66, true>;
type NormalSum = Fx<32, 0, true>;
type Alu54 = Fx<53, 0, true>;
type ShortProduct = Fx<18, 0, false>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairTrialError {
    Numeric(NumericFault),
    UnsupportedRow,
    InvalidProfile,
    Timeout,
}

impl From<NumericFault> for PairTrialError {
    fn from(value: NumericFault) -> Self {
        Self::Numeric(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PairProfile {
    /// Test both two-edge and three-edge candidates until the primitive is mapped.
    pub pair_latency: u8,
    /// One, two, four, or eight low-coefficient bits are consumed per edge.
    pub residual_digit_bits: u8,
}

impl PairProfile {
    fn validate(self) -> Result<(), PairTrialError> {
        if !(2..=3).contains(&self.pair_latency)
            || ![1, 2, 4, 8].contains(&self.residual_digit_bits)
        {
            return Err(PairTrialError::InvalidProfile);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairTag {
    Clip(u8),
    Normal(u8),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PairCycle {
    pub issue: Option<PairTag>,
    pub retire: Option<PairTag>,
    pub residual_row: Option<u8>,
    pub residual_retire: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PairWork {
    pub clip_dsp_issues: u32,
    pub normal_dsp_issues: u32,
    pub residual_products: u32,
    pub residual_digit_steps: u32,
    pub edges: u32,
}

impl PairWork {
    pub const fn dsp_issues(self) -> u32 {
        self.clip_dsp_issues + self.normal_dsp_issues
    }
}

#[derive(Clone, Copy, Debug)]
struct PairIssue {
    tag: PairTag,
    first: (i32, i32),
    second: (i32, i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PairResult {
    tag: PairTag,
    raw: Alu54,
}

#[derive(Clone, Debug)]
struct PairPipeline {
    stages: Vec<Option<PairResult>>,
}

impl PairPipeline {
    fn new(latency: u8) -> Self {
        Self {
            stages: vec![None; usize::from(latency)],
        }
    }

    fn tick(&mut self, issue: Option<PairIssue>) -> Result<Option<PairResult>, NumericFault> {
        let computed = issue
            .map(|issue| {
                for operand in [issue.first.0, issue.first.1, issue.second.0, issue.second.1] {
                    Fx::<17, 0, true>::from_raw(i128::from(operand))?;
                }
                let sum = i128::from(issue.first.0) * i128::from(issue.first.1)
                    + i128::from(issue.second.0) * i128::from(issue.second.1);
                Ok(PairResult {
                    tag: issue.tag,
                    raw: Alu54::from_raw(sum)?,
                })
            })
            .transpose()?;
        self.stages.rotate_right(1);
        self.stages[0] = computed;
        Ok(self.stages.last_mut().unwrap().take())
    }

    fn is_empty(&self) -> bool {
        self.stages.iter().all(Option::is_none)
    }
}

#[derive(Clone, Copy, Debug)]
struct ResidualTask {
    row: u8,
    tail: u8,
    bits: u8,
    next_bit: u8,
    shifted_delta: ShortProduct,
    sum: ShortProduct,
}

impl ResidualTask {
    fn new(row: u8, delta: u16, tail: u8, bits: u8) -> Self {
        Self {
            row,
            tail,
            bits,
            next_bit: 0,
            shifted_delta: ShortProduct::from_raw(i128::from(delta)).unwrap(),
            sum: ShortProduct::from_raw(0).unwrap(),
        }
    }

    fn step(&mut self, digit_bits: u8) -> Result<bool, NumericFault> {
        let mask = (1_u16 << digit_bits) - 1;
        let digit = (u16::from(self.tail) >> self.next_bit) & mask;
        // The shifted operand is a register advanced by a fixed shift; no
        // variable barrel shifter is needed in the one-bit implementation.
        let partial = i128::from(self.shifted_delta.raw()) * i128::from(digit);
        self.sum = ShortProduct::from_raw(self.sum.raw() as i128 + partial)?;
        self.next_bit = self.next_bit.saturating_add(digit_bits);
        let done = self.next_bit >= self.bits;
        if !done {
            self.shifted_delta =
                ShortProduct::from_raw(i128::from(self.shifted_delta.raw()) << digit_bits)?;
        }
        Ok(done)
    }
}

#[derive(Clone, Debug)]
pub struct PairedVertexUnit {
    profile: PairProfile,
    pipeline: PairPipeline,
    issues: VecDeque<PairIssue>,
    residuals: VecDeque<ResidualTask>,
    active_residual: Option<ResidualTask>,
    clip_sum: [ClipSum; 4],
    normal_sum: [NormalSum; 3],
    shifts: [u8; 4],
    rgba: [u8; 4],
    work: PairWork,
    completed: bool,
}

impl PairedVertexUnit {
    pub fn new(
        plan: &MeshletMvpPlan,
        xyz10: [u16; 3],
        normal: [Q14; 3],
        rgba: [u8; 4],
        normal_matrix: [[Q14; 3]; 3],
        profile: PairProfile,
    ) -> Result<Self, PairTrialError> {
        profile.validate()?;
        if xyz10.iter().any(|&value| value > 1023) {
            return Err(PairTrialError::Numeric(NumericFault::OutOfRange));
        }
        let rows = plan.paired_rows().ok_or(PairTrialError::UnsupportedRow)?;
        let mut issues = VecDeque::new();
        let mut residuals = VecDeque::new();
        let mut clip_sum = [ClipSum::from_raw(0)?; 4];
        let mut shifts = [0; 4];
        for (row, operands) in rows.iter().enumerate() {
            clip_sum[row] = ClipSum::from_raw(operands.base_raw)?;
            shifts[row] = operands.shift;
            let active: Vec<_> = (0..3)
                .filter(|&axis| operands.high[axis] != 0 && xyz10[axis] != 0)
                .map(|axis| (operands.high[axis], i32::from(xyz10[axis])))
                .collect();
            for pair in active.chunks(2) {
                issues.push_back(PairIssue {
                    tag: PairTag::Clip(row as u8),
                    first: pair[0],
                    second: pair.get(1).copied().unwrap_or((0, 0)),
                });
            }
            for (axis, &delta) in xyz10.iter().enumerate() {
                let tail = operands.tail[axis];
                if tail != 0 && delta != 0 {
                    residuals.push_back(ResidualTask::new(row as u8, delta, tail, operands.shift));
                }
            }
        }
        for (row, coefficients) in normal_matrix.iter().enumerate() {
            let active: Vec<_> = (0..3)
                .filter(|&axis| coefficients[axis].raw() != 0 && normal[axis].raw() != 0)
                .map(|axis| (coefficients[axis].raw() as i32, normal[axis].raw() as i32))
                .collect();
            for pair in active.chunks(2) {
                issues.push_back(PairIssue {
                    tag: PairTag::Normal(row as u8),
                    first: pair[0],
                    second: pair.get(1).copied().unwrap_or((0, 0)),
                });
            }
        }
        Ok(Self {
            profile,
            pipeline: PairPipeline::new(profile.pair_latency),
            issues,
            residuals,
            active_residual: None,
            clip_sum,
            normal_sum: [NormalSum::from_raw(0)?; 3],
            shifts,
            rgba,
            work: PairWork::default(),
            completed: false,
        })
    }

    pub const fn work(&self) -> PairWork {
        self.work
    }

    pub fn tick(&mut self) -> Result<(PairCycle, Option<TransformedVertex>), PairTrialError> {
        assert!(
            !self.completed,
            "completed vertex unit must not be ticked again"
        );
        self.work.edges += 1;
        let mut cycle = PairCycle::default();
        let issue = self.issues.pop_front();
        if let Some(issue) = issue {
            cycle.issue = Some(issue.tag);
            match issue.tag {
                PairTag::Clip(_) => self.work.clip_dsp_issues += 1,
                PairTag::Normal(_) => self.work.normal_dsp_issues += 1,
            }
        }
        if let Some(result) = self.pipeline.tick(issue)? {
            cycle.retire = Some(result.tag);
            match result.tag {
                PairTag::Clip(row) => {
                    let index = usize::from(row);
                    let shifted =
                        ClipSum::from_raw(i128::from(result.raw.raw()) << self.shifts[index])?;
                    self.clip_sum[index] = self.clip_sum[index].checked_add(shifted)?;
                }
                PairTag::Normal(row) => {
                    let index = usize::from(row);
                    let raw = NormalSum::from_raw(i128::from(result.raw.raw()))?;
                    self.normal_sum[index] = self.normal_sum[index].checked_add(raw)?;
                }
            }
        }
        if self.active_residual.is_none() {
            self.active_residual = self.residuals.pop_front();
            if self.active_residual.is_some() {
                self.work.residual_products += 1;
            }
        }
        if let Some(active) = &mut self.active_residual {
            cycle.residual_row = Some(active.row);
            self.work.residual_digit_steps += 1;
            if active.step(self.profile.residual_digit_bits)? {
                let row = usize::from(active.row);
                self.clip_sum[row] = self.clip_sum[row]
                    .checked_add(ClipSum::from_raw(i128::from(active.sum.raw()))?)?;
                cycle.residual_retire = Some(active.row);
                self.active_residual = None;
            }
        }
        if self.issues.is_empty()
            && self.pipeline.is_empty()
            && self.residuals.is_empty()
            && self.active_residual.is_none()
        {
            self.completed = true;
            let mut clip = [Q16::from_raw(0)?; 4];
            for (value, sum) in clip.iter_mut().zip(self.clip_sum) {
                *value = Q16::from_raw(round_shift_ties_even(sum.raw(), 16)?)?;
            }
            let mut normal = [Q14::from_raw(0)?; 3];
            for (value, sum) in normal.iter_mut().zip(self.normal_sum) {
                *value = Q14::from_raw(round_shift_ties_even(i128::from(sum.raw()), 14)?)?;
            }
            return Ok((
                cycle,
                Some(TransformedVertex {
                    clip,
                    normal,
                    rgba: self.rgba,
                }),
            ));
        }
        Ok((cycle, None))
    }

    pub fn run_bounded(
        &mut self,
        max_edges: u32,
    ) -> Result<(TransformedVertex, PairWork), PairTrialError> {
        for _ in 0..max_edges {
            if let Some(output) = self.tick()?.1 {
                return Ok((output, self.work));
            }
        }
        Err(PairTrialError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::CompactGrid;

    fn q16(raw: i32) -> Q16 {
        Q16::from_raw(i128::from(raw)).unwrap()
    }
    fn q14(raw: i32) -> Q14 {
        Q14::from_raw(i128::from(raw)).unwrap()
    }

    #[test]
    fn paired_pipeline_retires_on_selected_edge_and_checks_operands() {
        for latency in [2, 3] {
            let mut pipe = PairPipeline::new(latency);
            let op = PairIssue {
                tag: PairTag::Clip(0),
                first: (-131_072, 1023),
                second: (131_071, 1023),
            };
            assert!(pipe.tick(Some(op)).unwrap().is_none());
            for _ in 1..latency - 1 {
                assert!(pipe.tick(None).unwrap().is_none());
            }
            assert_eq!(pipe.tick(None).unwrap().unwrap().raw.raw(), -1023);
            assert!(pipe.is_empty());
        }
        let mut pipe = PairPipeline::new(2);
        assert_eq!(
            pipe.tick(Some(PairIssue {
                tag: PairTag::Clip(0),
                first: (131_072, 1),
                second: (0, 0),
            })),
            Err(NumericFault::OutOfRange)
        );
    }

    #[test]
    fn serial_residual_uses_bounded_digits_and_exact_product() {
        for digit_bits in [1, 2, 4, 8] {
            let mut task = ResidualTask::new(0, 1023, 255, 8);
            let steps = 8 / digit_bits;
            for edge in 1..=steps {
                assert_eq!(task.step(digit_bits).unwrap(), edge == steps);
            }
            assert_eq!(task.sum.raw(), 1023 * 255);
        }
    }

    #[test]
    fn complete_vertex_matches_exact_plan_for_both_pair_latencies_and_short_units() {
        let matrix = [
            [q16(143_668), q16(34_406), q16(94_531), q16(-1_229)],
            [q16(0), q16(274_682), q16(-99_976), q16(-30_453)],
            [q16(0), q16(0), q16(0), q16(0)],
            [q16(96_800), q16(-47_282), q16(-129_907), q16(72_172)],
        ];
        let grid = CompactGrid {
            origin: [-20_000, 10_000, 0],
            base_step: 1,
            meshlet_base: [0; 3],
            level: 3,
        };
        let plan = MeshletMvpPlan::new(matrix, grid).unwrap();
        let xyz = [101, 33, 17];
        let normal = [q14(3000), q14(-5000), q14(7000)];
        let normal_matrix = [
            [q14(11_000), q14(-8_000), q14(6_000)],
            [q14(7_000), q14(12_000), q14(-8_000)],
            [q14(-9_000), q14(6_000), q14(11_000)],
        ];
        let rgba = [17, 33, 65, 255];
        let clip_reference = plan.transform(xyz).unwrap().0;
        let normal_reference = std::array::from_fn::<_, 3, _>(|row| {
            let sum: i128 = (0..3)
                .map(|axis| {
                    i128::from(normal_matrix[row][axis].raw()) * i128::from(normal[axis].raw())
                })
                .sum();
            Q14::from_raw(round_shift_ties_even(sum, 14).unwrap()).unwrap()
        });
        for pair_latency in [2, 3] {
            let mut previous_edges = u32::MAX;
            for residual_digit_bits in [1, 2, 4, 8] {
                let mut unit = PairedVertexUnit::new(
                    &plan,
                    xyz,
                    normal,
                    rgba,
                    normal_matrix,
                    PairProfile {
                        pair_latency,
                        residual_digit_bits,
                    },
                )
                .unwrap();
                let (output, work) = unit.run_bounded(128).unwrap();
                assert_eq!(output.clip, clip_reference);
                assert_eq!(output.normal, normal_reference);
                assert_eq!(output.rgba, rgba);
                assert!(work.edges <= previous_edges);
                previous_edges = work.edges;
                assert_eq!(work.normal_dsp_issues, 6);
                assert!(work.clip_dsp_issues > 0);
            }
        }
    }

    #[test]
    fn bounded_run_reports_timeout_without_silent_completion() {
        let matrix = [[q16(65_536); 4]; 4];
        let grid = CompactGrid {
            origin: [0; 3],
            base_step: 1,
            meshlet_base: [0; 3],
            level: 0,
        };
        let plan = MeshletMvpPlan::new(matrix, grid).unwrap();
        let normal_matrix = [[q14(0); 3]; 3];
        let mut unit = PairedVertexUnit::new(
            &plan,
            [1, 2, 3],
            [q14(0); 3],
            [0; 4],
            normal_matrix,
            PairProfile {
                pair_latency: 3,
                residual_digit_bits: 1,
            },
        )
        .unwrap();
        assert_eq!(unit.run_bounded(1), Err(PairTrialError::Timeout));
    }
}
