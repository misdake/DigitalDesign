//! Post-DMA port schedule for one GV2M v6 meshlet. The scratchpad core port
//! loads MVP, normal matrix, AABB, and interleaved 32-bit stream cells. The
//! paired arithmetic trial feeds three synchronous transformed rows; a vertex
//! becomes visible only on a later publish edge. Setup queue credit can stall
//! stream retirement. DMA and base-plan arithmetic are not yet edge-accurate.

use std::collections::VecDeque;

use crate::fixed::{NumericFault, Q16};
use crate::format::{CompactGrid, FormatError, MeshletBounds, Uniform};
use crate::mvp_pair_trial::{PairCycle, PairProfile, PairTrialError, PairedVertexUnit};
use crate::mvp_width_trial::MeshletMvpPlan;
use crate::result_store::{
    ResultRead, ResultStore, TransformedVertex, TriangleRef, RESULT_ROWS, ROWS_PER_VERTEX,
};
use crate::scratchpad::{Scratchpad, SCRATCH_WORDS};
use crate::stream96::{decode_triangle_cell, decode_vertex_cells, Record, StreamFault};
use crate::timing::RamRead;

const UNIFORM_READ_WORDS: usize = 11;
const AABB_READ_WORDS: usize = 3;
const HEADER_READ_WORDS: usize = UNIFORM_READ_WORDS + AABB_READ_WORDS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortFault {
    Format(FormatError),
    Stream(StreamFault),
    Pair(PairTrialError),
    Numeric(NumericFault),
    UnsupportedPlan,
    Bounds,
    Capacity,
    UnpublishedRead,
    ReadCollision,
    Timeout,
}

impl From<FormatError> for PortFault {
    fn from(value: FormatError) -> Self {
        Self::Format(value)
    }
}
impl From<StreamFault> for PortFault {
    fn from(value: StreamFault) -> Self {
        Self::Stream(value)
    }
}
impl From<PairTrialError> for PortFault {
    fn from(value: PairTrialError) -> Self {
        Self::Pair(value)
    }
}
impl From<NumericFault> for PortFault {
    fn from(value: NumericFault) -> Self {
        Self::Numeric(value)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PortCycle {
    pub core_read: Option<usize>,
    pub stream_cell: Option<u32>,
    pub pair: PairCycle,
    pub result_write: Option<usize>,
    pub setup_read: Option<usize>,
    pub setup_output: Option<ResultRead>,
    pub publish: Option<usize>,
    pub queue_push: Option<TriangleRef>,
    pub credit_stall: bool,
    pub prepare_issue: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingRead {
    Header(usize),
    Stream,
}

#[derive(Clone, Debug)]
enum Stage {
    Load,
    Prepare(u32),
    Fetch {
        first: Option<u32>,
        second: Option<u32>,
    },
    Execute(PairedVertexUnit),
    Write {
        vertex: TransformedVertex,
        row: usize,
    },
    Publish,
    Queue(TriangleRef),
    Done,
}

pub struct MvpPortTrial {
    scratch: Scratchpad,
    results: ResultStore,
    queue: VecDeque<TriangleRef>,
    queue_capacity: usize,
    grid: CompactGrid,
    profile: PairProfile,
    stream_start: usize,
    stream_words: usize,
    useful_cells: usize,
    next_header: usize,
    header: [u64; HEADER_READ_WORDS],
    header_captured: usize,
    next_stream_word: usize,
    pending_read: Option<PendingRead>,
    cells: VecDeque<u32>,
    cells_used: usize,
    plan: Option<MeshletMvpPlan>,
    normal_matrix: Option<[[crate::fixed::Q14; 3]; 3]>,
    bounds: Option<MeshletBounds>,
    published: usize,
    triangles: usize,
    stage: Stage,
    edges: usize,
}

impl MvpPortTrial {
    /// Scratchpad has been filled by DMA before the first trial edge. Uniform
    /// occupies words 0..16, AABB words 16..19, and stream starts at >=19.
    pub fn new(
        scratch: Scratchpad,
        stream_start: usize,
        stream_words: usize,
        useful_cells: usize,
        grid: CompactGrid,
        profile: PairProfile,
        queue_capacity: usize,
    ) -> Result<Self, PortFault> {
        if !grid.valid()
            || queue_capacity == 0
            || stream_start < 19
            || stream_start
                .checked_add(stream_words)
                .is_none_or(|end| end > SCRATCH_WORDS)
            || useful_cells == 0
            || useful_cells.div_ceil(2) != stream_words
        {
            return Err(PortFault::Capacity);
        }
        Ok(Self {
            scratch,
            results: ResultStore::new(0),
            queue: VecDeque::new(),
            queue_capacity,
            grid,
            profile,
            stream_start,
            stream_words,
            useful_cells,
            next_header: 0,
            header: [0; HEADER_READ_WORDS],
            header_captured: 0,
            next_stream_word: 0,
            pending_read: None,
            cells: VecDeque::new(),
            cells_used: 0,
            plan: None,
            normal_matrix: None,
            bounds: None,
            published: 0,
            triangles: 0,
            stage: Stage::Load,
            edges: 0,
        })
    }

    pub const fn edges(&self) -> usize {
        self.edges
    }
    pub const fn published_vertices(&self) -> usize {
        self.published
    }
    pub const fn triangles_queued(&self) -> usize {
        self.triangles
    }
    pub fn done(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }
    pub fn results(&self) -> &ResultStore {
        &self.results
    }
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }
    pub fn pop_triangle(&mut self) -> Option<TriangleRef> {
        self.queue.pop_front()
    }

    fn receive_read(&mut self) -> Result<(), PortFault> {
        let Some(pending) = self.pending_read.take() else {
            return Ok(());
        };
        let RamRead::Data(word) = self.scratch.core_output() else {
            return Err(PortFault::ReadCollision);
        };
        match pending {
            PendingRead::Header(slot) => {
                self.header[slot] = word;
                self.header_captured += 1;
            }
            PendingRead::Stream => {
                self.cells.push_back(word as u32);
                self.cells.push_back((word >> 32) as u32);
            }
        }
        Ok(())
    }

    fn start_plan(&mut self) -> Result<Stage, PortFault> {
        let mut words = [0_u64; 16];
        words[..8].copy_from_slice(&self.header[..8]);
        words[8..11].copy_from_slice(&self.header[8..11]);
        let uniform = Uniform::decode(words)?;
        self.normal_matrix = Some(uniform.normal);
        self.bounds = Some(MeshletBounds::decode(
            self.header[11..14].try_into().unwrap(),
        )?);
        let plan = MeshletMvpPlan::new(uniform.mvp, self.grid).ok_or(PortFault::UnsupportedPlan)?;
        if plan.paired_rows().is_none() {
            return Err(PortFault::UnsupportedPlan);
        }
        // Occupancy estimate only: eager Rust plan construction above has no
        // clocked datapath, and residual/row-shift preparation is not charged.
        let issues = plan.precompute_work().macro_issues();
        self.plan = Some(plan);
        Ok(Stage::Prepare(issues + u32::from(issues != 0) * 3))
    }

    fn check_bounds(&self, xyz10: [u16; 3]) -> Result<(), PortFault> {
        let mut position = [Q16::from_raw(0)?; 4];
        for axis in 0..3 {
            let raw = i128::from(self.grid.origin[axis])
                + (i128::from(self.grid.meshlet_base[axis])
                    + (i128::from(xyz10[axis]) << self.grid.level))
                    * i128::from(self.grid.base_step);
            position[axis] = Q16::from_raw(raw)?;
        }
        position[3] = Q16::from_raw(1 << 16)?;
        if !self.bounds.unwrap().contains(position) {
            return Err(PortFault::Bounds);
        }
        Ok(())
    }

    fn accept_cell(
        &mut self,
        cell: u32,
        first: Option<u32>,
        second: Option<u32>,
    ) -> Result<Stage, PortFault> {
        if let Some(first) = first {
            if let Some(second) = second {
                let Record::Vertex {
                    xyz10,
                    normal,
                    color565,
                    ..
                } = decode_vertex_cells(first, second, cell)?
                else {
                    unreachable!()
                };
                if self.published >= 64 {
                    return Err(PortFault::Capacity);
                }
                self.check_bounds(xyz10)?;
                let r = ((color565 >> 11) & 31) as u8;
                let g = ((color565 >> 5) & 63) as u8;
                let b = (color565 & 31) as u8;
                let rgba = [
                    (r << 3) | (r >> 2),
                    (g << 2) | (g >> 4),
                    (b << 3) | (b >> 2),
                    255,
                ];
                let unit = PairedVertexUnit::new(
                    self.plan.as_ref().unwrap(),
                    xyz10,
                    normal,
                    rgba,
                    self.normal_matrix.unwrap(),
                    self.profile,
                )?;
                return Ok(Stage::Execute(unit));
            }
            return Ok(Stage::Fetch {
                first: Some(first),
                second: Some(cell),
            });
        }
        match cell & 3 {
            0 => Ok(Stage::Fetch {
                first: Some(cell),
                second: None,
            }),
            1 => {
                let Record::Triangle(refs) = decode_triangle_cell(cell, self.published)? else {
                    unreachable!()
                };
                Ok(Stage::Queue(TriangleRef { vertices: refs }))
            }
            2 if cell == 2 && self.cells_used == self.useful_cells => Ok(Stage::Done),
            2 => Err(PortFault::Stream(StreamFault::TrailingData)),
            _ => Err(PortFault::Stream(StreamFault::BadTag)),
        }
    }

    pub fn tick(&mut self) -> Result<PortCycle, PortFault> {
        self.tick_with_setup_read(None)
    }

    /// Setup owns the independent result-store read port. Its registered data
    /// appears in this edge's trace and is held by ResultStore until another
    /// read. Only already published rows may be requested.
    pub fn tick_with_setup_read(
        &mut self,
        setup_read: Option<usize>,
    ) -> Result<PortCycle, PortFault> {
        if setup_read
            .is_some_and(|row| row >= RESULT_ROWS || row / ROWS_PER_VERTEX >= self.published)
        {
            return Err(PortFault::UnpublishedRead);
        }
        if self.done() {
            if let Some(row) = setup_read {
                self.edges += 1;
                self.results.tick(Some(row), None);
                return Ok(PortCycle {
                    setup_read: Some(row),
                    setup_output: Some(self.results.output()),
                    ..PortCycle::default()
                });
            }
            return Ok(PortCycle::default());
        }
        self.edges += 1;
        self.receive_read()?;
        let mut cycle = PortCycle {
            setup_read,
            ..PortCycle::default()
        };
        let mut result_write = None;
        let stage = std::mem::replace(&mut self.stage, Stage::Done);
        self.stage = match stage {
            Stage::Load => {
                if self.next_header < HEADER_READ_WORDS {
                    let slot = self.next_header;
                    let address = if slot < UNIFORM_READ_WORDS {
                        slot
                    } else {
                        slot + 5
                    };
                    cycle.core_read = Some(address);
                    self.pending_read = Some(PendingRead::Header(slot));
                    self.next_header += 1;
                    Stage::Load
                } else if self.header_captured == HEADER_READ_WORDS {
                    self.start_plan()?
                } else {
                    Stage::Load
                }
            }
            Stage::Prepare(left) if left > 0 => {
                cycle.prepare_issue = left > 3;
                Stage::Prepare(left - 1)
            }
            Stage::Prepare(_) => Stage::Fetch {
                first: None,
                second: None,
            },
            Stage::Fetch { first, second } => {
                if self.cells.len() <= 1
                    && self.pending_read.is_none()
                    && self.next_stream_word < self.stream_words
                {
                    cycle.core_read = Some(self.stream_start + self.next_stream_word);
                    self.pending_read = Some(PendingRead::Stream);
                    self.next_stream_word += 1;
                }
                if let Some(cell) = self.cells.pop_front() {
                    self.cells_used += 1;
                    cycle.stream_cell = Some(cell);
                    self.accept_cell(cell, first, second)?
                } else if self.next_stream_word == self.stream_words && self.pending_read.is_none()
                {
                    return Err(PortFault::Stream(StreamFault::Truncated));
                } else {
                    Stage::Fetch { first, second }
                }
            }
            Stage::Execute(mut unit) => {
                let (pair, completed) = unit.tick()?;
                cycle.pair = pair;
                match completed {
                    Some(vertex) => Stage::Write { vertex, row: 0 },
                    None => Stage::Execute(unit),
                }
            }
            Stage::Write { vertex, row } => {
                let address = self.published * ROWS_PER_VERTEX + row;
                result_write = Some((address, vertex.rows()[row]));
                cycle.result_write = Some(address);
                if row + 1 == ROWS_PER_VERTEX {
                    Stage::Publish
                } else {
                    Stage::Write {
                        vertex,
                        row: row + 1,
                    }
                }
            }
            Stage::Publish => {
                self.results.publish(self.published);
                cycle.publish = Some(self.published);
                self.published += 1;
                Stage::Fetch {
                    first: None,
                    second: None,
                }
            }
            Stage::Queue(reference) if self.queue.len() == self.queue_capacity => {
                cycle.credit_stall = true;
                Stage::Queue(reference)
            }
            Stage::Queue(reference) => {
                self.queue.push_back(reference);
                self.triangles += 1;
                cycle.queue_push = Some(reference);
                Stage::Fetch {
                    first: None,
                    second: None,
                }
            }
            Stage::Done => Stage::Done,
        };
        self.scratch.tick(cycle.core_read, None, None, None);
        self.results.tick(setup_read, result_write);
        if setup_read.is_some() {
            cycle.setup_output = Some(self.results.output());
        }
        Ok(cycle)
    }

    pub fn run_bounded(&mut self, max_edges: usize) -> Result<(), PortFault> {
        for _ in 0..max_edges {
            if self.done() {
                return Ok(());
            }
            self.tick()?;
        }
        if self.done() {
            Ok(())
        } else {
            Err(PortFault::Timeout)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::Q14;
    use crate::stream96::encode_stream;

    fn q16(raw: i32) -> Q16 {
        Q16::from_raw(i128::from(raw)).unwrap()
    }
    fn q14(raw: i32) -> Q14 {
        Q14::from_raw(i128::from(raw)).unwrap()
    }

    fn fixture(records: &[Record], useful_cells: usize) -> MvpPortTrial {
        let zero16 = q16(0);
        let zero14 = q14(0);
        let uniform = Uniform {
            mvp: std::array::from_fn(|row| {
                std::array::from_fn(|column| if row == column { q16(1 << 16) } else { zero16 })
            }),
            normal: std::array::from_fn(|row| {
                std::array::from_fn(|column| if row == column { q14(1 << 14) } else { zero14 })
            }),
        };
        let mut scratch = Scratchpad::new(0xa55a);
        for (slot, bytes) in uniform.encode().as_chunks::<8>().0.iter().enumerate() {
            scratch.tick(None, None, None, Some((slot, u64::from_le_bytes(*bytes))));
        }
        for axis in 0..3 {
            let lo = 0_u32;
            let hi = 1_u32 << 16;
            scratch.tick(
                None,
                None,
                None,
                Some((16 + axis, u64::from(lo) | (u64::from(hi) << 32))),
            );
        }
        let words = encode_stream(records).unwrap();
        for (slot, &word) in words.iter().enumerate() {
            scratch.tick(None, None, None, Some((32 + slot, word)));
        }
        MvpPortTrial::new(
            scratch,
            32,
            words.len(),
            useful_cells,
            CompactGrid {
                origin: [0; 3],
                base_step: 64,
                meshlet_base: [0; 3],
                level: 0,
            },
            PairProfile {
                pair_latency: 2,
                residual_digit_bits: 1,
            },
            1,
        )
        .unwrap()
    }

    fn vertex(x: u16, color565: u16) -> Record {
        Record::Vertex {
            xyz10: [x, x + 1, x + 2],
            normal: [q14(1 << 14), q14(0), q14(0)],
            uv12: [0xabc, 0x123],
            color565,
        }
    }

    #[test]
    fn one_core_port_orders_reads_writes_publish_and_queue_credit() {
        let records = [
            vertex(1, 0xf800),
            vertex(3, 0x07e0),
            vertex(5, 0x001f),
            Record::Triangle([0, 1, 2]),
            Record::Triangle([2, 1, 0]),
            Record::End,
        ];
        let mut trial = fixture(&records, 12);
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        let mut publish_edges = Vec::new();
        let mut push_edges = Vec::new();
        let mut stalled = false;
        for _ in 0..1000 {
            let cycle = trial.tick().unwrap();
            if let Some(address) = cycle.core_read {
                reads.push(address);
            }
            if let Some(row) = cycle.result_write {
                writes.push((trial.edges(), row));
            }
            if let Some(id) = cycle.publish {
                publish_edges.push((trial.edges(), id));
            }
            if cycle.queue_push.is_some() {
                push_edges.push(trial.edges());
            }
            if cycle.credit_stall {
                assert!(cycle.core_read.is_none());
                assert!(cycle.result_write.is_none());
                assert_eq!(trial.queue_len(), 1);
                stalled = true;
                assert_eq!(trial.pop_triangle().unwrap().vertices, [0, 1, 2]);
            }
            if trial.done() {
                break;
            }
        }
        assert!(trial.done());
        assert!(stalled);
        assert_eq!(trial.published_vertices(), 3);
        assert_eq!(trial.triangles_queued(), 2);
        assert_eq!(trial.pop_triangle().unwrap().vertices, [2, 1, 0]);
        assert_eq!(&reads[..11], &(0..11).collect::<Vec<_>>());
        assert_eq!(&reads[11..14], &[16, 17, 18]);
        assert_eq!(&reads[14..], &[32, 33, 34, 35, 36, 37]);
        assert_eq!(
            writes.iter().map(|&(_, row)| row).collect::<Vec<_>>(),
            (0..9).collect::<Vec<_>>()
        );
        for (id, &(edge, _)) in publish_edges.iter().enumerate() {
            assert!(edge > writes[id * 3 + 2].0);
            assert_eq!(
                trial.results().vertex(id).unwrap().clip[0].raw(),
                i64::from([1, 3, 5][id]) * 64
            );
        }
        assert!(push_edges[0] > publish_edges[2].0);
        assert!(push_edges[1] > push_edges[0]);
        assert_eq!(trial.results().vertex(0).unwrap().rgba, [255, 0, 0, 255]);
        assert_eq!(trial.results().vertex(1).unwrap().rgba, [0, 255, 0, 255]);
        assert_eq!(trial.results().vertex(2).unwrap().rgba, [0, 0, 255, 255]);
    }

    #[test]
    fn forward_reference_faults_and_unconsumed_queue_has_bounded_timeout() {
        let records = [vertex(1, 0xffff), Record::Triangle([0, 1, 0]), Record::End];
        let mut trial = fixture(&records, 5);
        let mut fault = None;
        for _ in 0..300 {
            if let Err(error) = trial.tick() {
                fault = Some(error);
                break;
            }
        }
        assert_eq!(
            fault,
            Some(PortFault::Stream(StreamFault::ForwardReference))
        );

        let records = [
            vertex(1, 0xffff),
            Record::Triangle([0, 0, 0]),
            Record::Triangle([0, 0, 0]),
            Record::End,
        ];
        let mut trial = fixture(&records, 6);
        assert_eq!(trial.run_bounded(300), Err(PortFault::Timeout));
        assert_eq!(trial.queue_len(), 1);
        trial.pop_triangle();
        trial.run_bounded(300).unwrap();
    }

    #[test]
    fn setup_reads_published_rows_during_next_vertex_work() {
        let records = [
            vertex(1, 0xf800),
            vertex(3, 0x07e0),
            vertex(5, 0x001f),
            Record::Triangle([0, 1, 2]),
            vertex(7, 0xffff),
            Record::End,
        ];
        let mut trial = fixture(&records, 14);
        assert_eq!(
            trial.tick_with_setup_read(Some(0)),
            Err(PortFault::UnpublishedRead)
        );
        let mut next_read = None;
        let mut overlapping_work = false;
        let mut read_rows = 0_usize;
        for _ in 0..1000 {
            let cycle = trial.tick_with_setup_read(next_read).unwrap();
            if let Some(row) = next_read {
                assert_eq!(
                    cycle.setup_output,
                    Some(ResultRead::Data(trial.results().inspect_row(row)))
                );
                overlapping_work |= cycle.core_read.is_some()
                    || cycle.pair.issue.is_some()
                    || cycle.result_write.is_some();
                read_rows += 1;
                next_read = if row < 2 { Some(row + 1) } else { None };
            }
            if cycle.queue_push.is_some() {
                assert_eq!(trial.pop_triangle().unwrap().vertices, [0, 1, 2]);
                next_read = Some(0);
            }
            if trial.done() {
                break;
            }
        }
        assert!(trial.done());
        assert_eq!(trial.published_vertices(), 4);
        assert_eq!(read_rows, 3);
        assert!(overlapping_work);
        let final_read = trial.tick_with_setup_read(Some(9)).unwrap();
        assert_eq!(
            final_read.setup_output,
            Some(ResultRead::Data(trial.results().inspect_row(9)))
        );
    }
}
