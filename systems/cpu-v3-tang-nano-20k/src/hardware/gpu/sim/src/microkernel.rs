//! First complete GPU v2 microkernel cmodel: two DMA commands and one DRAW.
//! The ROM uses coarse operations with bounded local sequencers. Event IDs
//! select fixed handler PCs at idle/WAIT only; arithmetic never preempts.

use std::collections::VecDeque;

use crate::dma::{DmaDesc, DmaEngine, DmaError};
use crate::dualwide::DualWideUnit;
use crate::events::{
    Events, EVENT_CACHE_DONE, EVENT_COMMAND, EVENT_FAULT, EVENT_OUTPUT_CREDIT, HANDLER_PC,
};
use crate::format::{
    normal_rom_words, CompactGrid, FormatError, Header, InputVertex, MeshletBounds, Uniform,
    UNIFORM_BYTES,
};
use crate::result_store::{ResultStore, TransformedVertex, TriangleRef};
use crate::scratchpad::{Scratchpad, SCRATCH_BYTES, SCRATCH_WORDS};
use crate::timing::{MemoryTiming, RamRead, SyncRam64};
use crate::transform::{TransformError, TransformUnit};

pub const PC_IDLE: u8 = 0;
pub const PC_WAIT_DMA: u8 = 1;
pub const PC_LOAD_UNIFORM: u8 = 2;
pub const PC_READ_HEADER: u8 = 3;
pub const PC_READ_PAYLOAD: u8 = 4;
pub const PC_TRANSFORM: u8 = 5;
pub const PC_WRITE_VERTEX: u8 = 6;
pub const PC_QUEUE_TRIANGLE: u8 = 7;
pub const PC_END: u8 = 8;
pub const PC_DONE: u8 = 9;
pub const PC_UNPACK_VERTEX: u8 = 10;
pub const PC_LOAD_BOUNDS: u8 = 11;

/// Coarse ROM operations. A long operation holds PC while its local sequencer
/// advances each edge; the ROM is not fetched once per arithmetic product.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MicroOp {
    WaitEvent,
    WaitDma,
    LoadUniform,
    ReadHeader,
    ReadPayload,
    UnpackCompact,
    LoadBounds,
    TransformVertex,
    WriteVertex,
    QueueTriangle,
    End,
    Halt,
    Reserved,
    HandleEvent(u8),
}

pub const ROM: [MicroOp; 24] = [
    MicroOp::WaitEvent,
    MicroOp::WaitDma,
    MicroOp::LoadUniform,
    MicroOp::ReadHeader,
    MicroOp::ReadPayload,
    MicroOp::TransformVertex,
    MicroOp::WriteVertex,
    MicroOp::QueueTriangle,
    MicroOp::End,
    MicroOp::Halt,
    MicroOp::UnpackCompact,
    MicroOp::LoadBounds,
    MicroOp::Reserved,
    MicroOp::Reserved,
    MicroOp::Reserved,
    MicroOp::Reserved,
    MicroOp::HandleEvent(0),
    MicroOp::HandleEvent(1),
    MicroOp::HandleEvent(2),
    MicroOp::HandleEvent(3),
    MicroOp::HandleEvent(4),
    MicroOp::HandleEvent(5),
    MicroOp::HandleEvent(6),
    MicroOp::HandleEvent(7),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrawDesc {
    pub uniform_token: u8,
    pub vertex_token: u8,
    pub uniform_addr: usize,
    pub stream_addr: usize,
    pub stream_bytes: usize,
    pub vertex_count: u8,
    pub triangle_count: u16,
    pub compact_grid: Option<CompactGrid>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MvpMode {
    Staged,
    Streaming,
    DualWide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Dma(DmaDesc),
    Draw(DrawDesc),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Owner {
    Filling,
    Ready,
    InUse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Region {
    start: usize,
    end: usize,
    owner: Owner,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MicroFault {
    BadCommand,
    Dma(DmaError),
    BadEvent(u8),
    BadStream,
    BadOwnership,
    Format(FormatError),
    Transform(TransformError),
    ScratchCollision,
    CycleLimit(u64),
}

impl From<DmaError> for MicroFault {
    fn from(value: DmaError) -> Self {
        Self::Dma(value)
    }
}
impl From<FormatError> for MicroFault {
    fn from(value: FormatError) -> Self {
        Self::Format(value)
    }
}
impl From<TransformError> for MicroFault {
    fn from(value: TransformError) -> Self {
        Self::Transform(value)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MicroStep {
    pub edge: u64,
    pub pc: u8,
    pub dispatched_event: Option<u8>,
    pub scratch_read: Option<usize>,
    pub normal_rom_read: Option<usize>,
    pub scratch_write: Option<usize>,
    pub result_write: Option<usize>,
    pub wide_issue: Option<u8>,
    pub wide_issue_second: Option<u8>,
    pub small_issue: Option<u8>,
    pub wide_retire: Option<u8>,
    pub wide_retire_second: Option<u8>,
    pub small_retire: Option<u8>,
    pub dma_completed: Option<u8>,
    pub vertex_published: Option<u8>,
    pub triangle_pushed: bool,
}

#[derive(Clone, Debug)]
struct Loader {
    base: usize,
    count: usize,
    requested: usize,
    data: Vec<u64>,
}

#[derive(Clone, Debug)]
enum VertexEngine {
    Mixed(TransformUnit),
    DualWide(DualWideUnit),
}

impl VertexEngine {
    fn tick(
        &mut self,
        matrix_output: RamRead,
        step: &mut MicroStep,
    ) -> Result<Option<TransformedVertex>, TransformError> {
        match self {
            Self::Mixed(unit) => {
                let (cycle, output) = unit.tick_with_ram(matrix_output)?;
                step.scratch_read = cycle.matrix_read;
                step.wide_issue = cycle.wide_issue;
                step.small_issue = cycle.small_issue;
                step.wide_retire = cycle.wide_retire;
                step.small_retire = cycle.small_retire;
                Ok(output)
            }
            Self::DualWide(unit) => {
                let (cycle, output) = unit.tick(matrix_output)?;
                step.scratch_read = cycle.matrix_read;
                [step.wide_issue, step.wide_issue_second] = cycle.issue;
                [step.wide_retire, step.wide_retire_second] = cycle.retire;
                Ok(output)
            }
        }
    }
}

impl Loader {
    fn new(base: usize, count: usize) -> Self {
        assert!(base + count <= SCRATCH_WORDS);
        Self {
            base,
            count,
            requested: 0,
            data: Vec::with_capacity(count),
        }
    }

    fn tick(&mut self, previous: RamRead) -> Result<(Option<usize>, bool), MicroFault> {
        if self.data.len() < self.requested {
            self.data.push(match previous {
                RamRead::Data(word) => word,
                RamRead::Collision => return Err(MicroFault::ScratchCollision),
            });
        }
        let address = if self.requested < self.count {
            let address = self.base + self.requested;
            self.requested += 1;
            Some(address)
        } else {
            None
        };
        Ok((address, self.data.len() == self.count))
    }
}

/// One draw publishes ordered transformed vertices and setup queue references.
/// Setup itself does not run.
#[derive(Clone, Debug)]
pub struct Microkernel {
    pub scratchpad: Scratchpad,
    normal_rom: SyncRam64,
    pub results: ResultStore,
    pub meshlet_bounds: Option<MeshletBounds>,
    pub setup_queue: VecDeque<TriangleRef>,
    pub dma: DmaEngine,
    pub events: Events,
    pub trace: Vec<MicroStep>,
    mode: MvpMode,
    commands: VecDeque<Command>,
    regions: [Option<Region>; 4],
    draw: Option<DrawDesc>,
    normal_matrix: Option<[[crate::fixed::Q14; 3]; 3]>,
    uniform_staged: Option<Uniform>,
    input: Option<InputVertex>,
    packed_vertex: Option<u64>,
    normal_rom_pending: bool,
    output: Option<TransformedVertex>,
    vertex_engine: Option<VertexEngine>,
    loader: Option<Loader>,
    pending_triangle: Option<TriangleRef>,
    cursor: usize,
    vertex_count: u8,
    triangle_count: u16,
    store_row: u8,
    pc: u8,
    resume_pc: u8,
    edge: u64,
    fault: Option<MicroFault>,
}

impl Microkernel {
    pub fn new(memory_words: Vec<u64>, timing: MemoryTiming, seed: u64) -> Self {
        Self::new_with_mode(memory_words, timing, seed, MvpMode::Streaming)
    }

    pub fn new_with_mode(
        memory_words: Vec<u64>,
        timing: MemoryTiming,
        seed: u64,
        mode: MvpMode,
    ) -> Self {
        let mut events = Events::default();
        // These identities are reserved in ROM, but this v0 transaction has
        // no output-credit or cache-control producer to consume them.
        events.set_mask((1 << EVENT_OUTPUT_CREDIT) | (1 << EVENT_CACHE_DONE));
        Self {
            scratchpad: Scratchpad::new(0xa55a),
            normal_rom: SyncRam64::new(normal_rom_words()),
            results: ResultStore::new(0x005a_5a5a_5a5a_5a5a_5a5a_u128),
            meshlet_bounds: None,
            setup_queue: VecDeque::new(),
            dma: DmaEngine::new(memory_words, timing, seed),
            events,
            trace: Vec::new(),
            mode,
            commands: VecDeque::new(),
            regions: [None; 4],
            draw: None,
            normal_matrix: None,
            uniform_staged: None,
            input: None,
            packed_vertex: None,
            normal_rom_pending: false,
            output: None,
            vertex_engine: None,
            loader: None,
            pending_triangle: None,
            cursor: 0,
            vertex_count: 0,
            triangle_count: 0,
            store_row: 0,
            pc: PC_IDLE,
            resume_pc: PC_IDLE,
            edge: 0,
            fault: None,
        }
    }

    pub fn submit(&mut self, command: Command) -> Result<(), MicroFault> {
        if self.commands.len() == 8
            || self.pc == PC_DONE
            || self.fault.is_some()
            || self.draw.is_some()
            || self
                .commands
                .back()
                .is_some_and(|command| matches!(command, Command::Draw(_)))
        {
            return Err(MicroFault::BadCommand);
        }
        self.commands.push_back(command);
        self.events.edge(0, 1 << EVENT_COMMAND);
        Ok(())
    }

    pub fn pc(&self) -> u8 {
        self.pc
    }
    pub fn edge(&self) -> u64 {
        self.edge
    }
    pub fn fault(&self) -> Option<MicroFault> {
        self.fault
    }
    pub fn completed(&self) -> bool {
        self.pc == PC_DONE
    }

    fn region_for(&self, token: u8, start: usize, bytes: usize, owner: Owner) -> bool {
        let Some(end) = start.checked_add(bytes) else {
            return false;
        };
        self.regions
            .get(usize::from(token))
            .and_then(|&region| region)
            .is_some_and(|region| {
                region.start <= start && region.end >= end && region.owner == owner
            })
    }

    fn region_available(&self, token: u8, start: usize, bytes: usize) -> bool {
        self.region_for(token, start, bytes, Owner::Filling)
            || self.region_for(token, start, bytes, Owner::Ready)
    }

    fn command_handler(&mut self, set_events: &mut u8) -> Result<(), MicroFault> {
        let command = self
            .commands
            .pop_front()
            .ok_or(MicroFault::BadEvent(EVENT_COMMAND))?;
        if !self.commands.is_empty() {
            *set_events |= 1 << EVENT_COMMAND;
        }
        match command {
            Command::Dma(desc) => {
                let start = desc.scratchpad_addr;
                let end = start
                    .checked_add(desc.byte_count)
                    .ok_or(MicroFault::BadOwnership)?;
                if desc.completion_token > 3
                    || end > SCRATCH_BYTES
                    || self.regions[usize::from(desc.completion_token)].is_some()
                    || self
                        .regions
                        .iter()
                        .flatten()
                        .any(|other| start < other.end && other.start < end)
                {
                    return Err(MicroFault::BadOwnership);
                }
                self.dma.enqueue(desc)?;
                self.regions[usize::from(desc.completion_token)] = Some(Region {
                    start,
                    end,
                    owner: Owner::Filling,
                });
            }
            Command::Draw(desc) => {
                let vertex_bytes = if desc.compact_grid.is_some() { 16 } else { 48 };
                let records_bytes = usize::from(desc.vertex_count) * vertex_bytes
                    + usize::from(desc.triangle_count) * 8
                    + 8;
                let shape_bytes_valid = desc.stream_bytes == records_bytes
                    || (desc.compact_grid.is_some()
                        && desc.stream_bytes == records_bytes + MeshletBounds::BYTES);
                let shape_valid = self.draw.is_none()
                    && (3..=64).contains(&desc.vertex_count)
                    && (1..=128).contains(&desc.triangle_count)
                    && shape_bytes_valid
                    && desc.uniform_addr.is_multiple_of(8)
                    && desc.stream_addr.is_multiple_of(8)
                    && desc.uniform_token != desc.vertex_token;
                let shape_valid = shape_valid && desc.compact_grid.is_none_or(CompactGrid::valid);
                if !shape_valid
                    || desc.uniform_token > 3
                    || desc.vertex_token > 3
                    || !self.region_available(desc.uniform_token, desc.uniform_addr, UNIFORM_BYTES)
                    || !self.region_available(
                        desc.vertex_token,
                        desc.stream_addr,
                        desc.stream_bytes,
                    )
                {
                    return Err(MicroFault::BadCommand);
                }
                self.draw = Some(desc);
                self.resume_pc = PC_WAIT_DMA;
            }
        }
        Ok(())
    }

    fn handle_event(
        &mut self,
        id: u8,
        acknowledge: &mut u8,
        set_events: &mut u8,
    ) -> Result<(), MicroFault> {
        if id < 4 {
            let region = self.regions[usize::from(id)]
                .as_mut()
                .ok_or(MicroFault::BadEvent(id))?;
            if region.owner != Owner::Filling {
                return Err(MicroFault::BadEvent(id));
            }
            region.owner = Owner::Ready;
            *acknowledge |= 1 << id;
        } else if id == EVENT_COMMAND {
            self.command_handler(set_events)?;
            *acknowledge |= 1 << EVENT_COMMAND;
        } else if id == EVENT_FAULT {
            return Err(self.fault.unwrap_or(MicroFault::BadEvent(id)));
        } else {
            return Err(MicroFault::BadEvent(id));
        }
        self.pc = self.resume_pc;
        Ok(())
    }

    fn load(
        &mut self,
        base: usize,
        count: usize,
    ) -> Result<(Option<usize>, Option<Vec<u64>>), MicroFault> {
        if self.loader.is_none() {
            self.loader = Some(Loader::new(base, count));
        }
        let loader = self.loader.as_mut().unwrap();
        if loader.base != base || loader.count != count {
            return Err(MicroFault::BadStream);
        }
        let (read, done) = loader.tick(self.scratchpad.core_output())?;
        let data = done.then(|| self.loader.take().unwrap().data);
        Ok((read, data))
    }

    fn advance_core(
        &mut self,
        step: &mut MicroStep,
        acknowledge: &mut u8,
        set_events: &mut u8,
        result_write: &mut Option<(usize, u128)>,
    ) -> Result<(), MicroFault> {
        let op = ROM
            .get(usize::from(self.pc))
            .copied()
            .ok_or(MicroFault::BadCommand)?;
        if matches!(op, MicroOp::WaitEvent | MicroOp::WaitDma) {
            if let Some(id) = self.events.select() {
                step.dispatched_event = Some(id);
                self.events.dispatched(id);
                self.resume_pc = self.pc;
                self.pc = HANDLER_PC[usize::from(id)];
            } else if op == MicroOp::WaitDma {
                let draw = self.draw.ok_or(MicroFault::BadCommand)?;
                if self.region_for(
                    draw.uniform_token,
                    draw.uniform_addr,
                    UNIFORM_BYTES,
                    Owner::Ready,
                ) && self.region_for(
                    draw.vertex_token,
                    draw.stream_addr,
                    draw.stream_bytes,
                    Owner::Ready,
                ) {
                    self.regions[usize::from(draw.uniform_token)]
                        .as_mut()
                        .unwrap()
                        .owner = Owner::InUse;
                    self.regions[usize::from(draw.vertex_token)]
                        .as_mut()
                        .unwrap()
                        .owner = Owner::InUse;
                    self.pc = PC_LOAD_UNIFORM;
                }
            }
            return Ok(());
        }
        if let MicroOp::HandleEvent(id) = op {
            return self.handle_event(id, acknowledge, set_events);
        }
        let draw = self.draw.ok_or(MicroFault::BadCommand)?;
        match op {
            MicroOp::LoadUniform => {
                let (base, count) = match self.mode {
                    MvpMode::Staged => (draw.uniform_addr / 8, 16),
                    MvpMode::Streaming | MvpMode::DualWide => (draw.uniform_addr / 8 + 8, 3),
                };
                let (read, data) = self.load(base, count)?;
                step.scratch_read = read;
                if let Some(data) = data {
                    match self.mode {
                        MvpMode::Staged => {
                            self.uniform_staged = Some(Uniform::decode(data.try_into().unwrap())?)
                        }
                        MvpMode::Streaming | MvpMode::DualWide => {
                            self.normal_matrix =
                                Some(Uniform::decode_normal_words(data.try_into().unwrap())?)
                        }
                    }
                    let records_bytes = usize::from(draw.vertex_count)
                        * (if draw.compact_grid.is_some() { 16 } else { 48 })
                        + usize::from(draw.triangle_count) * 8
                        + 8;
                    self.pc = if draw.stream_bytes == records_bytes + MeshletBounds::BYTES {
                        PC_LOAD_BOUNDS
                    } else {
                        PC_READ_HEADER
                    };
                }
            }
            MicroOp::LoadBounds => {
                let (read, data) = self.load(draw.stream_addr / 8, 3)?;
                step.scratch_read = read;
                if let Some(data) = data {
                    self.meshlet_bounds = Some(MeshletBounds::decode(data.try_into().unwrap())?);
                    self.cursor = MeshletBounds::BYTES;
                    self.pc = PC_READ_HEADER;
                }
            }
            MicroOp::ReadHeader => {
                if self.cursor + 8 > draw.stream_bytes {
                    return Err(MicroFault::BadStream);
                }
                let (read, data) = self.load((draw.stream_addr + self.cursor) / 8, 1)?;
                step.scratch_read = read;
                if let Some(data) = data {
                    self.cursor += 8;
                    match Header::decode(data[0])? {
                        Header::Vertex { id }
                            if id == self.vertex_count
                                && self.vertex_count < draw.vertex_count
                                && draw.compact_grid.is_none()
                                && self.pending_triangle.is_none() =>
                        {
                            self.pc = PC_READ_PAYLOAD;
                        }
                        Header::CompactVertex { id }
                            if id == self.vertex_count
                                && self.vertex_count < draw.vertex_count
                                && draw.compact_grid.is_some()
                                && self.pending_triangle.is_none() =>
                        {
                            self.pc = PC_READ_PAYLOAD;
                        }
                        Header::Triangle { refs }
                            if self.pending_triangle.is_none()
                                && self.triangle_count < draw.triangle_count
                                && refs.iter().all(|&reference| reference < self.vertex_count) =>
                        {
                            self.pending_triangle = Some(TriangleRef { vertices: refs });
                            self.pc = PC_QUEUE_TRIANGLE;
                        }
                        Header::End
                            if self.vertex_count == draw.vertex_count
                                && self.triangle_count == draw.triangle_count
                                && self.cursor == draw.stream_bytes =>
                        {
                            self.pc = PC_END
                        }
                        _ => return Err(MicroFault::BadStream),
                    }
                }
            }
            MicroOp::ReadPayload => {
                let payload_bytes = if draw.compact_grid.is_some() { 8 } else { 40 };
                if self.cursor + payload_bytes > draw.stream_bytes {
                    return Err(MicroFault::BadStream);
                }
                let (read, data) =
                    self.load((draw.stream_addr + self.cursor) / 8, payload_bytes / 8)?;
                step.scratch_read = read;
                if let Some(data) = data {
                    self.cursor += payload_bytes;
                    if draw.compact_grid.is_some() {
                        self.packed_vertex = Some(data[0]);
                        self.pc = PC_UNPACK_VERTEX;
                    } else {
                        self.input = Some(InputVertex::decode_payload(data.try_into().unwrap())?);
                        self.pc = PC_TRANSFORM;
                    }
                }
            }
            MicroOp::UnpackCompact => {
                let packed = self.packed_vertex.ok_or(MicroFault::BadStream)?;
                let grid = draw.compact_grid.ok_or(MicroFault::BadStream)?;
                if !self.normal_rom_pending {
                    step.normal_rom_read = Some(((packed >> 30) & 511) as usize);
                    self.normal_rom_pending = true;
                } else {
                    let magnitude = match self.normal_rom.output() {
                        RamRead::Data(word) => word,
                        RamRead::Collision => return Err(MicroFault::BadStream),
                    };
                    let vertex =
                        InputVertex::decode_compact_with_normal_word(packed, grid, magnitude)?;
                    if self
                        .meshlet_bounds
                        .is_some_and(|bounds| !bounds.contains(vertex.position))
                    {
                        return Err(MicroFault::BadStream);
                    }
                    self.input = Some(vertex);
                    self.packed_vertex = None;
                    self.normal_rom_pending = false;
                    self.pc = PC_TRANSFORM;
                }
            }
            MicroOp::TransformVertex => {
                let transform = self.vertex_engine.get_or_insert_with(|| match self.mode {
                    MvpMode::Staged => VertexEngine::Mixed(TransformUnit::new(
                        self.uniform_staged.unwrap(),
                        self.input.unwrap(),
                    )),
                    MvpMode::Streaming => VertexEngine::Mixed(TransformUnit::new_streaming(
                        self.normal_matrix.unwrap(),
                        self.input.unwrap(),
                        draw.uniform_addr / 8,
                    )),
                    MvpMode::DualWide => VertexEngine::DualWide(DualWideUnit::new(
                        self.normal_matrix.unwrap(),
                        self.input.unwrap(),
                        draw.uniform_addr / 8,
                    )),
                });
                let output = transform.tick(self.scratchpad.core_output(), step)?;
                if let Some(output) = output {
                    self.output = Some(output);
                    self.vertex_engine = None;
                    self.store_row = 0;
                    self.pc = PC_WRITE_VERTEX;
                }
            }
            MicroOp::WriteVertex => {
                if self.store_row < 3 {
                    let row = usize::from(self.vertex_count) * 3 + usize::from(self.store_row);
                    *result_write = Some((
                        row,
                        self.output.unwrap().rows()[usize::from(self.store_row)],
                    ));
                    step.result_write = Some(row);
                    self.store_row += 1;
                } else {
                    self.results.publish(usize::from(self.vertex_count));
                    step.vertex_published = Some(self.vertex_count);
                    self.vertex_count += 1;
                    self.output = None;
                    self.input = None;
                    self.pc = PC_READ_HEADER;
                }
            }
            MicroOp::QueueTriangle => {
                if self.setup_queue.len() >= 128 || self.pending_triangle.is_none() {
                    return Err(MicroFault::BadStream);
                }
                self.setup_queue
                    .push_back(self.pending_triangle.take().unwrap());
                step.triangle_pushed = true;
                self.triangle_count += 1;
                self.pc = PC_READ_HEADER;
            }
            MicroOp::End => {
                if !self.dma.is_idle()
                    || self.vertex_count != draw.vertex_count
                    || self.triangle_count != draw.triangle_count
                    || self.setup_queue.len() != usize::from(draw.triangle_count)
                {
                    return Err(MicroFault::BadStream);
                }
                self.pc = PC_DONE;
            }
            _ => return Err(MicroFault::BadCommand),
        }
        Ok(())
    }

    pub fn tick(&mut self) -> MicroStep {
        assert!(
            !self.completed() && self.fault.is_none(),
            "microkernel already stopped"
        );
        self.edge += 1;
        let dma_cycle = self.dma.tick();
        let mut step = MicroStep {
            edge: self.edge,
            pc: self.pc,
            scratch_write: dma_cycle.scratch_write.map(|(address, _)| address),
            dma_completed: dma_cycle.completed_token,
            ..MicroStep::default()
        };
        let mut acknowledge = 0;
        let mut set_events = dma_cycle.completed_token.map_or(0, |token| 1 << token);
        let mut result_write = None;
        let outcome = self.advance_core(
            &mut step,
            &mut acknowledge,
            &mut set_events,
            &mut result_write,
        );
        self.scratchpad
            .tick(step.scratch_read, None, None, dma_cycle.scratch_write);
        self.normal_rom.tick(step.normal_rom_read, None);
        self.results.tick(None, result_write);
        if let Err(fault) = outcome {
            self.fault = Some(fault);
            self.pc = HANDLER_PC[usize::from(EVENT_FAULT)];
            set_events |= 1 << EVENT_FAULT;
        }
        self.events.edge(acknowledge, set_events);
        self.trace.push(step);
        step
    }

    pub fn run(&mut self, max_edges: u64) -> Result<u64, MicroFault> {
        for _ in 0..max_edges {
            self.tick();
            if let Some(fault) = self.fault {
                return Err(fault);
            }
            if self.completed() {
                return Ok(self.edge);
            }
        }
        Err(MicroFault::CycleLimit(max_edges))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::{Q14, Q16};
    use crate::format::{encode_stream, Header, STREAM_BYTES};
    use crate::micro_audit::audit_v0_trace;

    fn q16(raw: i32) -> Q16 {
        Q16::from_raw(i128::from(raw)).unwrap()
    }
    fn q14(raw: i16) -> Q14 {
        Q14::from_raw(i128::from(raw)).unwrap()
    }

    fn sample() -> (Uniform, [InputVertex; 3]) {
        let mut mvp = [[q16(0); 4]; 4];
        for (row, coefficients) in mvp.iter_mut().enumerate() {
            coefficients[row] = q16(0x10000);
        }
        mvp[0][1] = q16(0x8000);
        mvp[1][0] = q16(-0x4000);
        mvp[2][3] = q16(0x20000);
        let mut normal = [[q14(0); 3]; 3];
        normal[0][1] = q14(-0x4000);
        normal[1][0] = q14(0x4000);
        normal[2][2] = q14(0x4000);
        let vertices = [
            InputVertex {
                position: [q16(0x18000), q16(-0x28000), q16(0x08000), q16(0x10000)],
                normal: [q14(0x2000), q14(0x1000), q14(-0x0800)],
                rgba: [1, 2, 3, 4],
            },
            InputVertex {
                position: [q16(-0x08000), q16(0x04000), q16(-0x18000), q16(0x10000)],
                normal: [q14(-0x1000), q14(0x3000), q14(0x0400)],
                rgba: [5, 6, 7, 8],
            },
            InputVertex {
                position: [q16(0x30000), q16(0x10000), q16(0x20000), q16(0x10000)],
                normal: [q14(0), q14(-0x2000), q14(0x1800)],
                rgba: [9, 10, 11, 12],
            },
        ];
        (Uniform { mvp, normal }, vertices)
    }

    fn machine(
        uniform: Uniform,
        stream: [u8; STREAM_BYTES],
        timing: MemoryTiming,
        seed: u64,
    ) -> Microkernel {
        machine_with_mode(uniform, stream, timing, seed, MvpMode::Streaming)
    }

    fn machine_with_mode(
        uniform: Uniform,
        stream: [u8; STREAM_BYTES],
        timing: MemoryTiming,
        seed: u64,
        mode: MvpMode,
    ) -> Microkernel {
        let mut bytes = vec![0x3d_u8; 512];
        bytes[..128].copy_from_slice(&uniform.encode());
        bytes[256..416].copy_from_slice(&stream);
        let words = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| u64::from_le_bytes(*chunk))
            .collect();
        let mut machine = Microkernel::new_with_mode(words, timing, seed, mode);
        machine
            .submit(Command::Dma(DmaDesc {
                physical_addr: 0,
                scratchpad_addr: 0,
                byte_count: 128,
                completion_token: 0,
            }))
            .unwrap();
        machine
            .submit(Command::Dma(DmaDesc {
                physical_addr: 256,
                scratchpad_addr: 256,
                byte_count: 160,
                completion_token: 1,
            }))
            .unwrap();
        machine
            .submit(Command::Draw(DrawDesc {
                uniform_token: 0,
                vertex_token: 1,
                uniform_addr: 0,
                stream_addr: 256,
                stream_bytes: 160,
                vertex_count: 3,
                triangle_count: 1,
                compact_grid: None,
            }))
            .unwrap();
        machine
    }

    fn independent_round(raw: i128, shift: u32) -> i128 {
        let unit = 1_i128 << shift;
        let base = raw.div_euclid(unit);
        let remainder = raw.rem_euclid(unit);
        base + i128::from(remainder * 2 > unit || (remainder * 2 == unit && base & 1 != 0))
    }

    fn independent_golden(uniform: Uniform, vertex: InputVertex) -> TransformedVertex {
        let clip = std::array::from_fn(|row| {
            let sum = (0..4)
                .map(|column| {
                    i128::from(uniform.mvp[row][column].raw())
                        * i128::from(vertex.position[column].raw())
                })
                .sum();
            Q16::from_raw(independent_round(sum, 16)).unwrap()
        });
        let normal = std::array::from_fn(|row| {
            let sum = (0..3)
                .map(|column| {
                    i128::from(uniform.normal[row][column].raw())
                        * i128::from(vertex.normal[column].raw())
                })
                .sum();
            Q14::from_raw(independent_round(sum, 14)).unwrap()
        });
        TransformedVertex {
            clip,
            normal,
            rgba: vertex.rgba,
        }
    }

    #[test]
    fn compact_vertices_use_registered_normal_rom_before_transform() {
        let (uniform, _) = sample();
        let grid = CompactGrid {
            origin: [-0x10000, 0x20000, -0x8000],
            base_step: 1,
            meshlet_base: [0, 0, 0],
            level: 3,
        };
        let payloads = [
            12_u64 | (23_u64 << 10) | (31_u64 << 20) | (7_u64 << 30) | (0x3fffff_u64 << 42),
            42_u64 | (3_u64 << 10) | (9_u64 << 20) | (0x612_u64 << 30) | (0x155555_u64 << 42),
            1_u64 | (50_u64 << 10) | (60_u64 << 20) | (0x8ff_u64 << 30) | (0x02a55a_u64 << 42),
        ];
        let decoded = payloads.map(|packed| InputVertex::decode_compact(packed, grid).unwrap());
        let mut stream = Vec::new();
        for axis in 0..3 {
            let minimum = decoded
                .iter()
                .map(|vertex| vertex.position[axis].raw() as i32)
                .min()
                .unwrap();
            let maximum = decoded
                .iter()
                .map(|vertex| vertex.position[axis].raw() as i32)
                .max()
                .unwrap();
            stream.extend_from_slice(&minimum.to_le_bytes());
            stream.extend_from_slice(&maximum.to_le_bytes());
        }
        let expected_bounds = MeshletBounds::decode(
            stream
                .as_chunks::<8>()
                .0
                .iter()
                .map(|word| u64::from_le_bytes(*word))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        )
        .unwrap();
        for (id, payload) in payloads.iter().enumerate() {
            stream.extend_from_slice(
                &Header::CompactVertex { id: id as u8 }
                    .encode()
                    .to_le_bytes(),
            );
            stream.extend_from_slice(&payload.to_le_bytes());
        }
        stream.extend_from_slice(&Header::Triangle { refs: [0, 1, 2] }.encode().to_le_bytes());
        stream.extend_from_slice(&Header::End.encode().to_le_bytes());
        assert_eq!(stream.len(), 88);
        let mut bytes = vec![0_u8; 512];
        bytes[..128].copy_from_slice(&uniform.encode());
        bytes[256..344].copy_from_slice(&stream);
        let words = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| u64::from_le_bytes(*chunk))
            .collect();
        let mut model = Microkernel::new(
            words,
            MemoryTiming {
                grant_wait: 2,
                first_beat: 3,
                beat_gap: 1,
                jitter: 2,
            },
            19,
        );
        for command in [
            Command::Dma(DmaDesc {
                physical_addr: 0,
                scratchpad_addr: 0,
                byte_count: 128,
                completion_token: 0,
            }),
            Command::Dma(DmaDesc {
                physical_addr: 256,
                scratchpad_addr: 256,
                byte_count: 96,
                completion_token: 1,
            }),
            Command::Draw(DrawDesc {
                uniform_token: 0,
                vertex_token: 1,
                uniform_addr: 0,
                stream_addr: 256,
                stream_bytes: 88,
                vertex_count: 3,
                triangle_count: 1,
                compact_grid: Some(grid),
            }),
        ] {
            model.submit(command).unwrap();
        }
        model.run(400).unwrap();
        assert_eq!(model.meshlet_bounds, Some(expected_bounds));
        assert_eq!(
            model
                .trace
                .iter()
                .filter(|step| step.pc == PC_LOAD_BOUNDS && step.scratch_read.is_some())
                .count(),
            3
        );
        for (id, packed) in payloads.into_iter().enumerate() {
            let vertex = InputVertex::decode_compact(packed, grid).unwrap();
            assert_eq!(
                model.results.vertex(id),
                Some(independent_golden(uniform, vertex))
            );
        }
        assert_eq!(
            model.setup_queue.iter().copied().collect::<Vec<_>>(),
            [TriangleRef {
                vertices: [0, 1, 2]
            }]
        );
        let rom_issues = model
            .trace
            .iter()
            .enumerate()
            .filter(|(_, step)| step.normal_rom_read.is_some())
            .collect::<Vec<_>>();
        assert_eq!(rom_issues.len(), 3);
        for (index, step) in rom_issues {
            assert_eq!(step.pc, PC_UNPACK_VERTEX);
            assert_eq!(model.trace[index + 1].pc, PC_UNPACK_VERTEX);
            assert!(model.trace[index + 1].normal_rom_read.is_none());
        }
        let mut bad = bytes;
        bad[256..260].copy_from_slice(&(expected_bounds.max[0].raw() as i32).to_le_bytes());
        let words = bad
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| u64::from_le_bytes(*chunk))
            .collect();
        let mut rejected = Microkernel::new(
            words,
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        for command in [
            Command::Dma(DmaDesc {
                physical_addr: 0,
                scratchpad_addr: 0,
                byte_count: 128,
                completion_token: 0,
            }),
            Command::Dma(DmaDesc {
                physical_addr: 256,
                scratchpad_addr: 256,
                byte_count: 96,
                completion_token: 1,
            }),
            Command::Draw(DrawDesc {
                uniform_token: 0,
                vertex_token: 1,
                uniform_addr: 0,
                stream_addr: 256,
                stream_bytes: 88,
                vertex_count: 3,
                triangle_count: 1,
                compact_grid: Some(grid),
            }),
        ] {
            rejected.submit(command).unwrap();
        }
        assert_eq!(rejected.run(400), Err(MicroFault::BadStream));
    }

    #[test]
    fn two_dmas_draw_three_ordered_vertices_one_triangle_and_stop() {
        let (uniform, vertices) = sample();
        let stream = encode_stream(vertices);
        for (seed, timing) in [
            (
                19,
                MemoryTiming {
                    grant_wait: 0,
                    first_beat: 1,
                    beat_gap: 0,
                    jitter: 0,
                },
            ),
            (
                0,
                MemoryTiming {
                    grant_wait: 3,
                    first_beat: 4,
                    beat_gap: 2,
                    jitter: 3,
                },
            ),
        ] {
            let mut machine = machine(uniform, stream, timing, seed);
            let edges = machine.run(500).unwrap();
            assert!(edges < 500);
            let audit = audit_v0_trace(&machine.trace, MvpMode::Streaming).unwrap();
            assert_eq!(
                (audit.edges, audit.dsp_products, audit.triangles),
                (edges, [48, 27], 1)
            );
            assert_eq!(
                machine.setup_queue.iter().copied().collect::<Vec<_>>(),
                [TriangleRef {
                    vertices: [0, 1, 2]
                }]
            );
            for (id, vertex) in vertices.into_iter().enumerate() {
                assert_eq!(
                    machine.results.vertex(id),
                    Some(independent_golden(uniform, vertex)),
                    "seed={seed} id={id}"
                );
            }
            assert_eq!(machine.results.vertex(3), None);
            assert_eq!(
                machine.results.inspect_row(9),
                0x005a_5a5a_5a5a_5a5a_5a5a_u128
            );
            assert_eq!(machine.scratchpad.inspect_word(16), 0xa55a_a55a_a55a_a55a);
            assert_eq!(machine.scratchpad.inspect_word(31), 0xa55a_a55a_a55a_a55a);
            assert_eq!(machine.scratchpad.inspect_word(52), 0xa55a_a55a_a55a_a55a);
            let published = machine
                .trace
                .iter()
                .filter_map(|step| step.vertex_published)
                .collect::<Vec<_>>();
            assert_eq!(published, [0, 1, 2]);
            assert_eq!(
                machine
                    .trace
                    .iter()
                    .filter(|step| step.triangle_pushed)
                    .count(),
                1
            );
            assert_eq!(
                machine
                    .trace
                    .iter()
                    .filter(|step| step.wide_issue.is_some())
                    .count(),
                48
            );
            assert_eq!(
                machine
                    .trace
                    .iter()
                    .filter(|step| step.small_issue.is_some())
                    .count(),
                27
            );
            let last_write = machine
                .trace
                .iter()
                .rposition(|step| step.scratch_write.is_some())
                .unwrap();
            let last_completion = machine
                .trace
                .iter()
                .rposition(|step| step.dma_completed.is_some())
                .unwrap();
            assert!(last_completion > last_write);
            let first_core_read = machine
                .trace
                .iter()
                .position(|step| step.scratch_read.is_some())
                .unwrap();
            assert!(first_core_read > last_completion);
            let events = machine
                .trace
                .iter()
                .filter_map(|step| step.dispatched_event)
                .collect::<Vec<_>>();
            assert!(events.contains(&0) && events.contains(&1));
            assert_eq!(
                events
                    .iter()
                    .filter(|&&event| event == EVENT_COMMAND)
                    .count(),
                3
            );
            assert!(machine
                .trace
                .iter()
                .all(|step| step.pc <= PC_END || (16..=23).contains(&step.pc)));
        }
    }

    #[test]
    fn forward_triangle_reference_never_enters_setup_queue() {
        let (uniform, vertices) = sample();
        let mut stream = encode_stream(vertices);
        stream[144..152]
            .copy_from_slice(&Header::Triangle { refs: [3, 1, 0] }.encode().to_le_bytes());
        let mut machine = machine(
            uniform,
            stream,
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        assert_eq!(machine.run(300), Err(MicroFault::BadStream));
        assert_eq!(machine.setup_queue.len(), 0);
        assert!(machine.results.vertex(2).is_some());
        assert_ne!(machine.events.pending() & (1 << EVENT_FAULT), 0);
    }

    #[test]
    fn transform_overflow_fault_does_not_publish_a_vertex() {
        let (mut uniform, mut vertices) = sample();
        uniform.mvp[0][0] = q16(i32::MAX);
        uniform.mvp[0][1] = q16(i32::MAX);
        vertices[0].position[0] = q16(i32::MAX);
        vertices[0].position[1] = q16(i32::MAX);
        let mut machine = machine(
            uniform,
            encode_stream(vertices),
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        assert_eq!(
            machine.run(300),
            Err(MicroFault::Transform(TransformError::ClipOverflow {
                row: 0
            }))
        );
        assert_eq!(machine.results.vertex(0), None);
        assert!(machine.setup_queue.is_empty());
    }

    #[test]
    fn bounded_run_and_duplicate_dma_region_are_explicit() {
        let (uniform, vertices) = sample();
        let mut machine = machine(
            uniform,
            encode_stream(vertices),
            MemoryTiming {
                grant_wait: 3,
                first_beat: 4,
                beat_gap: 2,
                jitter: 1,
            },
            7,
        );
        assert_eq!(machine.run(10), Err(MicroFault::CycleLimit(10)));
        assert!(machine.setup_queue.is_empty());
        assert_eq!(machine.run(500).unwrap(), machine.edge());

        let mut duplicate = Microkernel::new(
            vec![0; 64],
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        duplicate
            .submit(Command::Dma(DmaDesc {
                physical_addr: 0,
                scratchpad_addr: 0,
                byte_count: 128,
                completion_token: 0,
            }))
            .unwrap();
        duplicate
            .submit(Command::Dma(DmaDesc {
                physical_addr: 256,
                scratchpad_addr: 0,
                byte_count: 160,
                completion_token: 1,
            }))
            .unwrap();
        assert_eq!(duplicate.run(50), Err(MicroFault::BadOwnership));
    }

    #[test]
    fn three_matrix_prototypes_have_identical_output() {
        let (uniform, vertices) = sample();
        let timing = MemoryTiming {
            grant_wait: 2,
            first_beat: 3,
            beat_gap: 1,
            jitter: 2,
        };
        let mut staged = machine_with_mode(
            uniform,
            encode_stream(vertices),
            timing,
            19,
            MvpMode::Staged,
        );
        let mut streaming = machine_with_mode(
            uniform,
            encode_stream(vertices),
            timing,
            19,
            MvpMode::Streaming,
        );
        let mut dual = machine_with_mode(
            uniform,
            encode_stream(vertices),
            timing,
            19,
            MvpMode::DualWide,
        );
        assert_eq!(staged.run(500), Ok(232));
        assert_eq!(streaming.run(500), Ok(222));
        assert_eq!(dual.run(500), Ok(210));
        assert_eq!(
            audit_v0_trace(&staged.trace, MvpMode::Staged)
                .unwrap()
                .dsp_products,
            [48, 27]
        );
        assert_eq!(
            audit_v0_trace(&streaming.trace, MvpMode::Streaming)
                .unwrap()
                .dsp_products,
            [48, 27]
        );
        assert_eq!(
            audit_v0_trace(&dual.trace, MvpMode::DualWide)
                .unwrap()
                .dsp_products,
            [75, 0]
        );
        for id in 0..3 {
            assert_eq!(staged.results.vertex(id), streaming.results.vertex(id));
            assert_eq!(streaming.results.vertex(id), dual.results.vertex(id));
        }
        assert_eq!(staged.setup_queue, streaming.setup_queue);
        assert_eq!(streaming.setup_queue, dual.setup_queue);
        let staged_uniform_reads = staged
            .trace
            .iter()
            .filter(|step| step.pc == PC_LOAD_UNIFORM && step.scratch_read.is_some())
            .count();
        let streaming_uniform_reads = streaming
            .trace
            .iter()
            .filter(|step| step.pc == PC_LOAD_UNIFORM && step.scratch_read.is_some())
            .count();
        assert_eq!((staged_uniform_reads, streaming_uniform_reads), (16, 3));
        let staged_matrix_reads = staged
            .trace
            .iter()
            .filter(|step| step.pc == PC_TRANSFORM && step.scratch_read.is_some())
            .count();
        let streaming_matrix_reads = streaming
            .trace
            .iter()
            .filter(|step| step.pc == PC_TRANSFORM && step.scratch_read.is_some())
            .count();
        assert_eq!((staged_matrix_reads, streaming_matrix_reads), (0, 24));
    }

    #[test]
    fn interleaved_vertices_and_triangles_publish_in_stream_order() {
        let (uniform, first_three) = sample();
        let fourth = InputVertex {
            position: [q16(0x8000), q16(-0x10000), q16(0x4000), q16(0x10000)],
            normal: [q14(0x1000), q14(0x2000), q14(0)],
            rgba: [13, 14, 15, 16],
        };
        let vertices = [first_three[0], first_three[1], first_three[2], fourth];
        let mut stream = Vec::new();
        for (id, vertex) in vertices[..3].iter().enumerate() {
            stream.extend_from_slice(&Header::Vertex { id: id as u8 }.encode().to_le_bytes());
            stream.extend_from_slice(&vertex.encode_payload());
        }
        stream.extend_from_slice(&Header::Triangle { refs: [0, 1, 2] }.encode().to_le_bytes());
        stream.extend_from_slice(&Header::Vertex { id: 3 }.encode().to_le_bytes());
        stream.extend_from_slice(&fourth.encode_payload());
        stream.extend_from_slice(&Header::Triangle { refs: [0, 2, 3] }.encode().to_le_bytes());
        stream.extend_from_slice(&Header::End.encode().to_le_bytes());
        assert_eq!(stream.len(), 216);
        let mut memory = vec![0_u8; 512];
        memory[..128].copy_from_slice(&uniform.encode());
        memory[256..256 + stream.len()].copy_from_slice(&stream);
        let words = memory
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| u64::from_le_bytes(*chunk))
            .collect();
        let mut model = Microkernel::new(
            words,
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        for command in [
            Command::Dma(DmaDesc {
                physical_addr: 0,
                scratchpad_addr: 0,
                byte_count: 128,
                completion_token: 0,
            }),
            Command::Dma(DmaDesc {
                physical_addr: 256,
                scratchpad_addr: 256,
                byte_count: 224,
                completion_token: 1,
            }),
            Command::Draw(DrawDesc {
                uniform_token: 0,
                vertex_token: 1,
                uniform_addr: 0,
                stream_addr: 256,
                stream_bytes: stream.len(),
                vertex_count: 4,
                triangle_count: 2,
                compact_grid: None,
            }),
        ] {
            model.submit(command).unwrap();
        }
        assert!(model.run(600).is_ok());
        for (id, vertex) in vertices.into_iter().enumerate() {
            assert_eq!(
                model.results.vertex(id),
                Some(independent_golden(uniform, vertex))
            );
        }
        assert_eq!(
            model.setup_queue.iter().copied().collect::<Vec<_>>(),
            [
                TriangleRef {
                    vertices: [0, 1, 2]
                },
                TriangleRef {
                    vertices: [0, 2, 3]
                },
            ]
        );
        let pushed = model
            .trace
            .iter()
            .filter(|step| step.triangle_pushed)
            .map(|step| step.edge)
            .collect::<Vec<_>>();
        let vertex3 = model
            .trace
            .iter()
            .find(|step| step.vertex_published == Some(3))
            .unwrap()
            .edge;
        assert!(pushed[0] < vertex3 && pushed[1] > vertex3);
        assert_eq!(
            model.results.inspect_row(12),
            0x005a_5a5a_5a5a_5a5a_5a5a_u128
        );
    }

    #[test]
    fn varied_signed_corpus_matches_independent_golden_in_all_modes() {
        let mut state = 0x8e21_f91d_35ba_746c_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 32) as u32
        };
        for case in 0..16 {
            let mvp = std::array::from_fn(|_| {
                std::array::from_fn(|_| q16((next() % 0x20001) as i32 - 0x10000))
            });
            let shift = case % 3;
            let mut normal = [[q14(0); 3]; 3];
            for (row, coefficients) in normal.iter_mut().enumerate() {
                coefficients[(row + shift) % 3] = q14(if (case + row) & 1 == 0 {
                    0x4000
                } else {
                    -0x4000
                });
            }
            let uniform = Uniform { mvp, normal };
            let vertices = std::array::from_fn(|_| InputVertex {
                position: std::array::from_fn(|_| q16((next() % 0x40001) as i32 - 0x20000)),
                normal: std::array::from_fn(|_| q14((next() % 0x4001) as i16 - 0x2000)),
                rgba: next().to_le_bytes(),
            });
            let timing = MemoryTiming {
                grant_wait: 1,
                first_beat: 2,
                beat_gap: (case % 2) as u32,
                jitter: 3,
            };
            for mode in [MvpMode::Staged, MvpMode::Streaming, MvpMode::DualWide] {
                let mut model = machine_with_mode(
                    uniform,
                    encode_stream(vertices),
                    timing,
                    (case + 1) as u64,
                    mode,
                );
                assert!(
                    model.run(600).is_ok(),
                    "case={case} mode={mode:?} fault={:?}",
                    model.fault()
                );
                for (id, vertex) in vertices.into_iter().enumerate() {
                    assert_eq!(
                        model.results.vertex(id),
                        Some(independent_golden(uniform, vertex)),
                        "case={case} mode={mode:?} id={id}"
                    );
                }
                assert_eq!(model.setup_queue.len(), 1);
                assert!(audit_v0_trace(&model.trace, mode).is_ok());
            }
        }
    }

    #[test]
    fn trace_audit_detects_retirement_completion_and_publish_corruption() {
        let (uniform, vertices) = sample();
        let mut machine = machine(
            uniform,
            encode_stream(vertices),
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        machine.run(300).unwrap();
        assert!(audit_v0_trace(&machine.trace, MvpMode::Streaming).is_ok());

        let mut wrong_retire = machine.trace.clone();
        let issue = wrong_retire
            .iter()
            .position(|step| step.wide_retire == Some(0))
            .unwrap();
        wrong_retire[issue].wide_retire = Some(1);
        assert_eq!(
            audit_v0_trace(&wrong_retire, MvpMode::Streaming)
                .unwrap_err()
                .reason,
            "wide retirement latency"
        );

        let mut no_completion = machine.trace.clone();
        let completion = no_completion
            .iter()
            .position(|step| step.dma_completed == Some(1))
            .unwrap();
        no_completion[completion].dma_completed = None;
        assert_eq!(
            audit_v0_trace(&no_completion, MvpMode::Streaming)
                .unwrap_err()
                .reason,
            "completion before last scratch write"
        );

        let mut early_publish = machine.trace.clone();
        let publish = early_publish
            .iter()
            .position(|step| step.vertex_published == Some(0))
            .unwrap();
        early_publish[publish].vertex_published = None;
        early_publish[publish - 1].vertex_published = Some(0);
        assert_eq!(
            audit_v0_trace(&early_publish, MvpMode::Streaming)
                .unwrap_err()
                .reason,
            "vertex published before row commit"
        );
    }

    #[test]
    fn dual_wide_audit_checks_the_second_retirement_lane() {
        let (uniform, vertices) = sample();
        let mut machine = machine_with_mode(
            uniform,
            encode_stream(vertices),
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
            MvpMode::DualWide,
        );
        machine.run(300).unwrap();
        assert!(audit_v0_trace(&machine.trace, MvpMode::DualWide).is_ok());
        let mut corrupted = machine.trace.clone();
        let edge = corrupted
            .iter()
            .position(|step| step.wide_retire_second == Some(17))
            .unwrap();
        corrupted[edge].wide_retire_second = Some(18);
        assert_eq!(
            audit_v0_trace(&corrupted, MvpMode::DualWide)
                .unwrap_err()
                .reason,
            "dual-wide retirement latency"
        );
    }

    #[test]
    fn unsupported_level_events_remain_masked_without_empty_ack_spin() {
        let (uniform, vertices) = sample();
        let mut machine = machine(
            uniform,
            encode_stream(vertices),
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        machine
            .events
            .edge(0, (1 << EVENT_OUTPUT_CREDIT) | (1 << EVENT_CACHE_DONE));
        machine.run(300).unwrap();
        assert_eq!(
            machine.events.pending() & ((1 << EVENT_OUTPUT_CREDIT) | (1 << EVENT_CACHE_DONE)),
            (1 << EVENT_OUTPUT_CREDIT) | (1 << EVENT_CACHE_DONE)
        );
        assert!(machine.trace.iter().all(|step| !matches!(
            step.dispatched_event,
            Some(EVENT_OUTPUT_CREDIT | EVENT_CACHE_DONE)
        )));
        assert!(machine.setup_queue.len() == 1);
    }

    #[test]
    fn overflowing_draw_address_faults_instead_of_panicking() {
        let mut machine = Microkernel::new(
            vec![0; 64],
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        machine
            .submit(Command::Dma(DmaDesc {
                physical_addr: 0,
                scratchpad_addr: 0,
                byte_count: 128,
                completion_token: 0,
            }))
            .unwrap();
        machine
            .submit(Command::Dma(DmaDesc {
                physical_addr: 256,
                scratchpad_addr: 256,
                byte_count: 160,
                completion_token: 1,
            }))
            .unwrap();
        machine
            .submit(Command::Draw(DrawDesc {
                uniform_token: 0,
                vertex_token: 1,
                uniform_addr: usize::MAX - 7,
                stream_addr: 256,
                stream_bytes: 160,
                vertex_count: 3,
                triangle_count: 1,
                compact_grid: None,
            }))
            .unwrap();
        assert_eq!(machine.run(30), Err(MicroFault::BadCommand));
        assert!(machine.setup_queue.is_empty());
    }

    #[test]
    fn single_draw_submission_rejects_commands_that_would_be_lost() {
        let (uniform, vertices) = sample();
        let mut machine = machine(
            uniform,
            encode_stream(vertices),
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        let extra = Command::Dma(DmaDesc {
            physical_addr: 0,
            scratchpad_addr: 512,
            byte_count: 32,
            completion_token: 2,
        });
        assert_eq!(machine.submit(extra), Err(MicroFault::BadCommand));
        for _ in 0..7 {
            machine.tick();
        }
        assert!(machine.draw.is_some());
        assert_eq!(machine.submit(extra), Err(MicroFault::BadCommand));
        machine.run(300).unwrap();
        assert_eq!(machine.setup_queue.len(), 1);
    }
}
