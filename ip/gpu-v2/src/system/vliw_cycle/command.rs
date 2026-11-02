//! Production command-control subset. Render and RAM remain explicit peers.
use super::{
    command_dma::Cache, require, ContractError, Input as TransportInput, Output as TransportOutput,
    Physical, Transport,
};

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
    Waiting = 7,
    Flushing = 8,
    Ending = 9,
    Failed = 10,
    Latch = 11,
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
    pub flush_complete: bool,
    pub cache_ready: bool,
    pub cache_done: bool,
    pub cache_done_bank: bool,
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
}
impl Processor {
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
        ProcessorOutput {
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
        }
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
        if i.reset {
            *self = Self::default();
            return Ok(());
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
                op => return Err(Error::UnsupportedOpcode(op as u8)),
            }
        }
        let mut n = self.clone();
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
                    4 => n.phase = Phase::Waiting,
                    7 => n.phase = Phase::Flushing,
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
                _ => {}
            }
        }
        if self.busy && i.abort {
            n.failed = true;
            n.phase = Phase::Failed;
            n.fetch_left = 0;
        }
        *self = n;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
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
    fn connect(&self, i: Input) -> (ProcessorInput, TransportInput) {
        let mut p = ProcessorInput {
            reset: i.reset,
            ce: i.ce,
            abort: i.abort,
            bus: i.bus,
            render_idle: i.render_idle,
            render_fault: i.render_fault,
            ram_ready: i.ram_ready,
            ram_q: i.ram_q,
            flush_complete: i.flush_complete,
            ..Default::default()
        };
        let o = self.processor.outputs(p);
        let t = TransportInput {
            reset: i.reset,
            ce: i.ce,
            stop: o.stop || i.render_fault,
            cache: o.cache,
            physical: i.physical,
            ..Default::default()
        };
        let q = self.transport.outputs(t);
        p.memory_busy = q.router.busy;
        p.work_busy = q.dma.work_busy;
        p.dma_idle = q.dma.idle;
        p.dma_fault = q.dma.fault;
        p.cache_ready = q.dma.cache_ready;
        p.cache_done = q.dma.cache_done;
        p.cache_done_bank = q.dma.cache_done_bank;
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
        let (p, t) = self.connect(i);
        let mut n = self.clone();
        n.processor.clock(p)?;
        n.transport.clock(t)?;
        n.device.clock(i.reset, i.physical);
        *self = n;
        Ok(())
    }
}
