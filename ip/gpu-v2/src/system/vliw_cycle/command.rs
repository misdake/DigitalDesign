//! Production command-control subset. Render and RAM remain explicit peers.
use super::{
    command_dma::{Cache, Work},
    require, ContractError, Input as TransportInput, Output as TransportOutput, Physical,
    Transport,
};
#[path = "command_aux.rs"]
mod auxiliary;
#[path = "command_draw.rs"]
mod draw;
pub use auxiliary::{Input as AuxiliaryInput, Output as AuxiliaryOutput};
pub use draw::{Bindings, Input as DrawInput};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceBus {
    pub index: u8,
    pub channel: u8,
    pub read: bool,
    pub write: bool,
    pub data: u16,
}
impl DeviceBus {
    fn count(self) -> bool {
        self.index & 7 == 4 && self.channel & 15 == 0 && self.write
    }
    fn submit(self) -> bool {
        self.index & 7 == 4 && self.channel & 15 == 1 && self.write
    }
}
/// Reset-only sticky from the actual raw physical error pins, not CP failure.
#[derive(Clone, Copy, Debug, Default)]
pub struct Device4 {
    transport_error: bool,
}
impl Device4 {
    pub fn read(&self, bus: DeviceBus, busy: bool) -> u16 {
        if bus.index & 7 == 4 && bus.channel & 15 == 2 && bus.read {
            u16::from(busy) | u16::from(self.transport_error) << 1
        } else {
            0
        }
    }
    pub fn clock(&mut self, reset: bool, physical: [Physical; 3]) {
        if reset {
            self.transport_error = false;
        } else {
            self.transport_error |= physical
                .iter()
                .any(|p| p.response.valid && p.response.error);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    #[default]
    Idle = 0,
    Fetch = 1,
    Exec = 2,
    DrawWait = 3,
    ApplyRead = 4,
    ApplyWrite = 5,
    Waiting = 7,
    Flushing = 8,
    Ending = 9,
    Failed = 10,
    Latch = 11,
    ClearRequest = 12,
    ClearWait = 13,
    ClearDone = 14,
    ColorRead = 15,
    ColorWrite = 16,
    DrawStart = 17,
    UiWait = 18,
    CopyAddress = 19,
    CopyRequest = 20,
    CopyWait = 21,
    CopyDone = 22,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessorInput {
    pub reset: bool,
    pub ce: bool,
    pub abort: bool,
    pub bus: DeviceBus,
    pub render_idle: bool,
    pub render_fault: bool,
    pub memory_busy: bool,
    pub work_busy: bool,
    pub dma_idle: bool,
    pub dma_fault: bool,
    pub ram_ready: bool,
    pub ram_q: u64,
    pub prepared_light: Option<(bool, u16)>,
    pub flush_complete: bool,
    pub cache_ready: bool,
    pub cache_done: bool,
    pub cache_done_bank: bool,
    pub draw: DrawInput,
    pub auxiliary: AuxiliaryInput,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessorOutput {
    pub submit_ready: bool,
    pub busy: bool,
    pub stop: bool,
    pub ram_read: bool,
    pub ram_address: u16,
    pub cache: Cache,
    pub flush_request: bool,
    pub work: Work,
    pub start: bool,
    pub draw_slot: bool,
    pub texture_invalidate: bool,
    pub bindings: Bindings,
    pub auxiliary: AuxiliaryOutput,
}
/// Facts from one successful physical edge, never from an outputs/settle read.
/// PCs are submission-relative byte offsets, not the wrapping 64-word RAM PC.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandEdge {
    pub submit: Option<(u32, u32)>,  // physical byte base, byte length
    pub latched: Option<(u32, u32)>, // PC, actual RAM word
    pub execute: Option<(u32, u32)>, // enabled Exec observation (may retry)
    pub issued: Option<(u32, u32)>,  // Exec consumed, not fetch/prefetch
    pub released: Option<u32>,       // PC; origin word belongs to the observed RAM latch
    pub drained: bool,
    pub aborted: bool,
    pub reset: bool,
    pub target: Option<(u16, u16, bool)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Contract(ContractError),
    UnsupportedOpcode(u8),
}
impl From<ContractError> for Error {
    fn from(e: ContractError) -> Self {
        Self::Contract(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract(e) => e.fmt(f),
            Self::UnsupportedOpcode(op) => write!(f, "unsupported command opcode {op:x}"),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug, Default)]
pub struct Processor {
    phase: Phase,
    busy: bool,
    failed: bool,
    all_fence: bool,
    stored: bool,
    pc: u8,
    fetch_line: u16,
    fetch_left: u16,
    fetch_bank: bool,
    cache_inflight: bool,
    cache_pages: u8,
    command: u32,
    diagnostic_pc: u32,
    limit: u32,
    count_seen: bool,
    draw: Option<draw::Draw>,
    auxiliary: Option<auxiliary::Auxiliary>,
}
impl Processor {
    /// Explicit production DRAW/config control. Render and source ports must
    /// still be connected by the caller; this constructor is not a whole GPU.
    pub fn with_draws() -> Self {
        Self {
            draw: Some(draw::Draw::default()),
            ..Self::default()
        }
    }
    /// All production command opcodes, with explicit physical auxiliary peers.
    pub fn with_all_commands() -> Self {
        Self {
            auxiliary: Some(Default::default()),
            ..Self::with_draws()
        }
    }
    /// Register observations: global/render/target validity, page/uniform masks,
    /// metadata uniform selectors, and the current object row.
    pub fn draw_status(&self) -> Option<(bool, bool, bool, u8, u8, u8, u8)> {
        self.draw.as_ref().map(|d| {
            (
                d.global_valid,
                d.render_valid,
                d.target_valid,
                d.pages,
                d.uniforms,
                d.page_uniform,
                d.object_index,
            )
        })
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn pc(&self) -> u8 {
        self.pc
    }
    pub fn cache_pages(&self) -> u8 {
        self.cache_pages
    }
    pub fn outputs(&self, i: ProcessorInput) -> ProcessorOutput {
        let stop = i.abort || self.failed;
        let mut o = ProcessorOutput {
            submit_ready: !self.busy && !self.failed && !i.abort,
            busy: self.busy,
            stop,
            ram_read: !stop
                && self.phase == Phase::Fetch
                && self.cache_pages & (1 << (self.pc >> 5)) != 0,
            ram_address: 448 | (u16::from(self.pc >> 5) << 4) | u16::from((self.pc & 31) >> 1),
            cache: Cache {
                valid: self.busy
                    && !stop
                    && self.phase != Phase::Ending
                    && self.fetch_left != 0
                    && !self.cache_inflight
                    && self.cache_pages & (1 << u8::from(self.fetch_bank)) == 0,
                source_halfword: u32::from(self.fetch_line) << 6,
                bank: self.fetch_bank,
            },
            flush_request: self.phase == Phase::Flushing && !stop,
            ..Default::default()
        };
        if let Some(d) = &self.draw {
            d.outputs(self, i, &mut o);
        }
        if let Some(a) = &self.auxiliary {
            o.cache.valid &= self.phase != Phase::UiWait
                && !(self.command >> 28 == 11
                    && matches!(
                        self.phase,
                        Phase::Exec | Phase::ApplyRead | Phase::ApplyWrite
                    ));
            o.auxiliary = a.outputs(self, i, o);
        }
        o
    }
    fn advance(&mut self, old: &Self) {
        if old.pc & 31 == 31 {
            self.cache_pages &= !(1 << (old.pc >> 5));
        }
        self.pc = (old.pc + 1) & 63;
        self.phase = Phase::Fetch;
        self.diagnostic_pc = (old.diagnostic_pc + 4) & 0x1ffff;
    }
    pub fn clock(&mut self, i: ProcessorInput) -> Result<(), Error> {
        self.clock_observing(i).map(|_| ())
    }
    /// Execute the existing transition once and return its committed facts.
    /// No observer state participates in command acceptance or scheduling.
    pub fn clock_observing(&mut self, i: ProcessorInput) -> Result<CommandEdge, Error> {
        if i.reset {
            let reset = self.busy || self.failed;
            let retained = self.auxiliary.as_ref().map(|a| a.copy_address);
            *self = if self.auxiliary.is_some() {
                Self::with_all_commands()
            } else if self.draw.is_some() {
                Self::with_draws()
            } else {
                Self::default()
            };
            if let Some(address) = retained {
                self.auxiliary.as_mut().unwrap().copy_address = address;
            }
            return Ok(CommandEdge {
                reset,
                ..Default::default()
            });
        }
        let o = self.outputs(i);
        let check = |ok, rule| require(ok, "command_processor", rule);
        check(
            !i.bus.count() || o.submit_ready && i.bus.data & 0xc000 == 0,
            "CP count SEND contract",
        )?;
        check(
            !i.bus.submit()
                || o.submit_ready
                    && self.count_seen
                    && u32::from(i.bus.data) + u32::from(self.fetch_left) <= 65536,
            "CP base SEND contract",
        )?;
        check(
            !(i.ce && !o.stop && self.phase == Phase::Fetch) || self.diagnostic_pc < self.limit,
            "CP missing END",
        )?;
        check(
            !(self.busy && !i.abort && !self.failed) || !i.render_fault && !i.dma_fault,
            "CP downstream contract",
        )?;
        if i.ce && !o.stop && self.phase == Phase::Exec {
            let payload = self.command & 0x0fff_ffff;
            match self.command >> 28 {
                0 => check(payload == 0, "CP NOP encoding")?,
                4 => check(payload & !3 == 0, "CP WAIT encoding")?,
                7 => check(
                    payload == 0 && self.all_fence && i.render_idle && !i.work_busy,
                    "CP drain contract",
                )?,
                15 => check(
                    payload == 0
                        && self.diagnostic_pc + 4 == self.limit
                        && self.stored
                        && self.all_fence
                        && !i.work_busy,
                    "CP END contract",
                )?,
                13 | 14 => check(false, "CP opcode contract")?,
                1 | 2 | 3 | 5 | 6 | 9 | 10 if self.draw.is_some() => {}
                8 | 11 | 12 if self.auxiliary.is_some() => {}
                op => return Err(Error::UnsupportedOpcode(op as u8)),
            }
        }
        if let Some(d) = &self.draw {
            d.check(self, i)?;
        }
        if let Some(a) = &self.auxiliary {
            a.check(self, i, o)?;
        }
        let mut n = self.clone();
        if let Some(d) = &mut n.draw {
            d.wall(i.draw);
            if i.ce && !o.stop {
                if let Some((directional, value)) = i.prepared_light {
                    if directional {
                        d.config.directional = value;
                    } else {
                        d.config.ambient = value;
                    }
                }
            }
        }
        if i.bus.count() && o.submit_ready {
            n.fetch_left = ((i.bus.data & 0x3fff) >> 5) + 1;
            n.limit = u32::from(i.bus.data & 0x3fff) * 4 + 4;
            n.count_seen = true;
        }
        if o.cache.valid && i.cache_ready {
            n.cache_inflight = true;
            n.fetch_line = self.fetch_line.wrapping_add(1);
            n.fetch_left = self.fetch_left.wrapping_sub(1) & 1023;
            n.fetch_bank = !self.fetch_bank;
        }
        if i.cache_done {
            n.cache_inflight = false;
            n.cache_pages |= 1 << u8::from(i.cache_done_bank);
        }
        if self.busy && self.phase == Phase::Ending && i.render_idle && i.dma_idle && !i.memory_busy
        {
            n.busy = false;
            n.phase = Phase::Idle;
        }
        if i.bus.submit() && o.submit_ready {
            n.busy = true;
            n.pc = 0;
            n.diagnostic_pc = 0;
            n.count_seen = false;
            n.fetch_line = i.bus.data;
            n.fetch_bank = false;
            n.cache_inflight = false;
            n.cache_pages = 0;
            n.all_fence = false;
            n.stored = false;
            if let Some(d) = &mut n.draw {
                d.new_submission();
            }
            n.phase = Phase::Fetch;
        } else if i.ce && !o.stop {
            match self.phase {
                Phase::Fetch => {
                    if self.cache_pages & (1 << (self.pc >> 5)) != 0 && i.ram_ready {
                        n.phase = Phase::Latch;
                    }
                }
                Phase::Latch => {
                    n.command = if self.pc & 1 != 0 {
                        (i.ram_q >> 32) as u32
                    } else {
                        i.ram_q as u32
                    };
                    n.phase = Phase::Exec;
                }
                Phase::Exec => match self.command >> 28 {
                    0 => n.advance(self),
                    1 | 2 => {
                        if i.draw.work_ready {
                            n.advance(self);
                        }
                    }
                    3 => {
                        if self.draw.as_ref().unwrap().inputs(self, i) {
                            n.phase = Phase::ColorRead;
                        }
                    }
                    4 => n.phase = Phase::Waiting,
                    5 | 6 | 10 => {
                        n.draw.as_mut().unwrap().object_index =
                            u8::from(self.command >> 28 == 6 && self.command & 128 != 0);
                        n.phase = Phase::ApplyRead;
                    }
                    7 => n.phase = Phase::Flushing,
                    8 => {
                        let a = n.auxiliary.as_mut().unwrap();
                        a.clear_flight = 0;
                        a.clear_offer = false;
                        n.phase = Phase::ClearRequest;
                    }
                    11 | 12 => {
                        if i.dma_idle {
                            n.draw.as_mut().unwrap().object_index = 0;
                            n.phase = Phase::ApplyRead;
                        }
                    }
                    9 => n.advance(self),
                    15 => {
                        n.fetch_left = 0;
                        n.phase = Phase::Ending;
                    }
                    _ => unreachable!(),
                },
                Phase::Waiting => {
                    if (self.command & 1 == 0 || !i.work_busy)
                        && (self.command & 2 == 0 || i.render_idle)
                    {
                        if self.command & 2 != 0 {
                            n.all_fence = true;
                        }
                        n.advance(self);
                    }
                }
                Phase::Flushing if i.flush_complete => {
                    n.stored = true;
                    n.advance(self);
                }
                Phase::ApplyRead | Phase::ColorRead if i.ram_ready => {
                    n.phase = if self.phase == Phase::ApplyRead {
                        Phase::ApplyWrite
                    } else {
                        Phase::ColorWrite
                    };
                }
                Phase::ApplyWrite => {
                    if self.command >> 28 == 12 {
                        n.phase = Phase::CopyAddress;
                    } else if self.command >> 28 == 11 {
                        n.draw.as_mut().unwrap().pages = 0;
                        n.phase = Phase::UiWait;
                    } else {
                        let index = self.draw.as_ref().unwrap().object_index;
                        if n.draw.as_mut().unwrap().apply(self.command, index, i.ram_q) {
                            if self.command >> 28 == 10 {
                                n.stored = false;
                            }
                            n.advance(self);
                        } else {
                            n.phase = Phase::ApplyRead;
                        }
                    }
                }
                Phase::ClearDone | Phase::CopyDone => n.advance(self),
                Phase::UiWait if i.auxiliary.ui_done => n.advance(self),
                Phase::ColorWrite => {
                    n.draw.as_mut().unwrap().config.mesh_color = i.ram_q as u32;
                    n.draw.as_mut().unwrap().config.world_transform = self.command & 2 != 0;
                    n.phase = Phase::DrawStart;
                }
                Phase::DrawStart if o.start && i.draw.meshlet_ready => {
                    if n.draw.as_ref().unwrap().config.world_transform {
                        n.draw.as_mut().unwrap().uniforms &= !(1 << 2);
                    }
                    n.all_fence = false;
                    n.stored = false;
                    n.phase = Phase::DrawWait;
                }
                Phase::DrawWait if i.draw.meshlet_done => n.advance(self),
                _ => {}
            }
        }
        if let Some(a) = &self.auxiliary {
            a.wall(self, &mut n, i, o);
        }
        if self.busy && i.abort {
            n.failed = true;
            if !o.auxiliary.clear_active && !o.auxiliary.copy_active {
                n.phase = Phase::Failed;
            }
            n.fetch_left = 0;
        }
        let command = (self.diagnostic_pc, self.command);
        let enabled = i.ce && !o.stop;
        let execute = enabled && self.phase == Phase::Exec;
        let advanced = self.busy && !i.bus.submit() && n.diagnostic_pc != self.diagnostic_pc;
        let edge = CommandEdge {
            submit: (i.bus.submit() && o.submit_ready)
                .then_some((u32::from(i.bus.data) * 128, self.limit)),
            latched: (enabled && self.phase == Phase::Latch)
                .then_some((self.diagnostic_pc, n.command)),
            execute: execute.then_some(command),
            issued: (execute && (advanced || n.phase != Phase::Exec)).then_some(command),
            released: (enabled && (advanced || execute && n.phase == Phase::Ending))
                .then_some(self.diagnostic_pc),
            drained: self.busy && !n.busy && !n.failed,
            aborted: !self.failed && n.failed,
            reset: false,
            target: (advanced && self.command >> 28 == 10).then(|| {
                let d = n.draw.as_ref().unwrap();
                (d.config.color_base, d.config.depth_base, d.target_valid)
            }),
        };
        *self = n;
        Ok(edge)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub prepared_light: Option<(bool, u16)>,
    pub reset: bool,
    pub ce: bool,
    pub abort: bool,
    pub bus: DeviceBus,
    pub ram_ready: bool,
    pub ram_q: u64,
    pub render_idle: bool,
    pub render_fault: bool,
    pub flush_complete: bool,
    pub physical: [Physical; 3],
    pub draw: DrawInput,
    pub auxiliary: AuxiliaryInput,
    pub texture: super::router::Texture,
    pub framebuffer: super::router::Framebuffer,
    /// Root UI owns the existing DMA work port while active.
    pub auxiliary_work: Option<super::command_dma::Work>,
    /// CLEAR owns physical FB write feedback; Device 4 still observes the pins.
    pub clear_active: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub processor: ProcessorOutput,
    pub device_read_data: u16,
    pub transport: TransportOutput,
}
/// Caller owns RAM and all physical clocks. No decoded command is injected here.
#[derive(Clone, Debug, Default)]
pub struct Control {
    pub processor: Processor,
    pub transport: Transport,
    pub device: Device4,
}
impl Control {
    pub(crate) fn observe(&self, i: Input, trace: &mut super::trace::Trace) {
        let (p, t) = self.connect(i);
        let q = self.processor.outputs(p);
        trace.control = Some([
            u64::from(self.processor.diagnostic_pc),
            self.processor.phase as u64,
            u64::from(p.render_idle),
            u64::from(p.work_busy),
            u64::from(p.dma_idle),
            u64::from(p.memory_busy),
            u64::from(p.flush_complete),
            u64::from(p.ce),
            u64::from(q.busy),
        ]);
        trace.control_word = Some(self.processor.command);
        self.transport.observe(t, trace);
    }
    pub fn with_all_commands() -> Self {
        Self {
            processor: Processor::with_all_commands(),
            ..Self::default()
        }
    }
    pub fn with_draws() -> Self {
        Self {
            processor: Processor::with_draws(),
            ..Self::default()
        }
    }
    fn connect(&self, i: Input) -> (ProcessorInput, TransportInput) {
        let mut p = ProcessorInput {
            prepared_light: i.prepared_light,
            reset: i.reset,
            ce: i.ce,
            abort: i.abort,
            bus: i.bus,
            render_idle: i.render_idle,
            render_fault: i.render_fault,
            ram_ready: i.ram_ready,
            ram_q: i.ram_q,
            flush_complete: i.flush_complete,
            draw: i.draw,
            auxiliary: i.auxiliary,
            ..Default::default()
        };
        let o = self.processor.outputs(p);
        let mut t = TransportInput {
            reset: i.reset,
            ce: i.ce,
            stop: o.stop || i.render_fault,
            cache: o.cache,
            work: i.auxiliary_work.unwrap_or(o.work),
            texture: i.texture,
            framebuffer: i.framebuffer,
            physical: i.physical,
        };
        if i.clear_active {
            t.physical[2].request_ready = false;
            t.physical[2].write_ready = false;
            t.physical[2].response.valid = false;
        }
        let q = self.transport.outputs(t);
        p.memory_busy = q.router.busy;
        p.work_busy = q.dma.work_busy;
        p.dma_idle = q.dma.idle;
        p.dma_fault = q.dma.fault;
        p.cache_ready = q.dma.cache_ready;
        p.cache_done = q.dma.cache_done;
        p.cache_done_bank = q.dma.cache_done_bank;
        p.draw.work_ready = q.dma.work_ready;
        p.draw.mesh_loading = q.dma.mesh_loading;
        p.draw.mesh_slot = q.dma.mesh_slot;
        p.draw.mesh_begin = q.dma.mesh_begin;
        p.draw.mesh_begin_slot = q.dma.mesh_begin_slot;
        p.draw.mesh_done = q.dma.mesh_done;
        p.draw.mesh_done_slot = q.dma.mesh_done_slot;
        p.draw.uniform_done = q.dma.uniform_done;
        p.draw.uniform_bank = q.dma.uniform_bank;
        p.draw.header_write =
            q.dma.ram_write && q.dma.ram_address >> 3 == 32 && q.dma.ram_address & 3 == 0;
        p.draw.header_slot = q.dma.ram_address & 4 != 0;
        p.draw.header_data = q.dma.ram_data;
        (p, t)
    }
    pub fn outputs(&self, i: Input) -> Output {
        let (p, t) = self.connect(i);
        let o = self.processor.outputs(p);
        Output {
            processor: o,
            device_read_data: self.device.read(i.bus, o.busy),
            transport: self.transport.outputs(t),
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), Error> {
        self.clock_observing(i).map(|_| ())
    }
    pub fn clock_observing(&mut self, i: Input) -> Result<CommandEdge, Error> {
        let (p, t) = self.connect(i);
        let mut n = self.clone();
        let edge = n.processor.clock_observing(p)?;
        n.transport.clock(t)?;
        n.device.clock(i.reset, i.physical);
        *self = n;
        Ok(edge)
    }
}
