//! Persistent bounded control sequencer. DMA service is synthetic and vertex
//! values are counted-derived. Neither is an independent arithmetic emulator.
use super::{
    super::ports::*,
    timed::{Action, Config, Record},
};
use crate::{
    command_processor::{
        ports::*,
        sim::{oracle as command_oracle, timed::EventTick},
    },
    scratchpad::{ports::*, sim::oracle::Scratchpad},
    vertex::{ports::*, sim::timed as vertex_timed},
};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    LegacyStandalone,
    ConnectedTriangles,
}
/// A borrowed store port. Runtime owns no transformed payload slots.
pub trait ProducerPort {
    fn free_slot(&self) -> Option<usize>;
    fn allocate(&mut self, slot: usize, count: usize) -> Result<u32, String>;
    fn write_row(
        &mut self,
        slot: usize,
        epoch: u32,
        vertex: usize,
        row: usize,
        data: u64,
    ) -> Result<(), String>;
    fn publish(&mut self, slot: usize, epoch: u32, vertex: usize) -> Result<(), String>;
    fn finish(&mut self, slot: usize, epoch: u32) -> Result<(), String>;
    fn release(&mut self, slot: usize, epoch: u32) -> Result<(), String>;
}
pub struct FrontendOut {
    pub records: Vec<Record>,
    pub event: EventTick,
    pub plan_created: bool,
}
struct Dma {
    descriptor: DmaDescriptor,
    lease: Lease,
    beat: usize,
    next: u64,
    last_write: Option<u64>,
    failed: bool,
}
struct Execution {
    plan: vertex_timed::Plan,
    body_start: u64,
}
struct Draw {
    lease: Lease,
    slot: usize,
    epoch: u32,
    region: usize,
    offset: usize,
    count: usize,
    context: Context,
    vertex: usize,
    cached: [Option<(usize, u64)>; 2],
    pending: Option<(usize, u64, u64)>,
    execution: Option<Execution>,
    loaded: bool,
}
impl Draw {
    fn capture_word(&mut self, address: usize, data: u64) {
        if self.cached[0].is_none() {
            self.cached[0] = Some((address, data));
        } else if self.cached[1].is_none() {
            self.cached[1] = Some((address, data));
        } else {
            self.cached[0] = self.cached[1];
            self.cached[1] = Some((address, data));
        }
    }
}
pub struct Sequencer<'a> {
    input: &'a Input,
    config: Config,
    profile: Profile,
    sp: Scratchpad,
    leases: [Option<Lease>; 2],
    tokens: [bool; 4],
    reserved: [bool; 4],
    queue: VecDeque<(DmaDescriptor, Lease)>,
    active: Option<Dma>,
    draw: Option<Draw>,
    events: EventState,
    pc: usize,
    core_cycle: u64,
    wall: u64,
    plan_serial: usize,
    fault: Option<String>,
    fence: bool,
    poisoned: bool,
}
impl<'a> Sequencer<'a> {
    pub fn new(input: &'a Input, config: Config, profile: Profile) -> Result<Self, String> {
        command_oracle::validate(&input.commands, 64)?;
        if config.max_cycles == 0
            || config.max_cycles > 1_000_000
            || config.dma_first_latency == 0
            || config.dma_first_latency > 1000
            || config.dma_gap == 0
            || config.dma_gap > 1000
            || input.memory.len() > 1_048_576
        {
            return Err("frontend cycle/memory/service bounds".into());
        }
        if profile == Profile::ConnectedTriangles
            && (input.commands.iter().any(|c| match c {
                Command::Release { .. } => true,
                Command::Draw { vertices, .. } => *vertices != 3,
                _ => false,
            }) || input
                .commands
                .iter()
                .filter(|c| matches!(c, Command::Draw { .. }))
                .count()
                > 6
                || config.max_cycles > 120_000
                || !(1..=4).contains(&config.hardware.wide)
                || !(1..=4).contains(&config.hardware.narrow)
                || !(1..=2).contains(&config.hardware.matrix_read_ports)
                || !(1..=20_000).contains(&config.hardware.max_cycles))
        {
            return Err(
                "connected frontend topology/hardware bounds: six three-vertex DRAWs, no RELEASE"
                    .into(),
            );
        }
        Ok(Self {
            input,
            config,
            profile,
            sp: Scratchpad::default(),
            leases: [None; 2],
            tokens: [false; 4],
            reserved: [false; 4],
            queue: VecDeque::new(),
            active: None,
            draw: None,
            events: EventState::default(),
            pc: 0,
            core_cycle: 0,
            wall: 0,
            plan_serial: 0,
            fault: None,
            fence: false,
            poisoned: false,
        })
    }
    pub fn wall(&self) -> u64 {
        self.wall
    }
    pub fn core_cycles(&self) -> u64 {
        self.core_cycle
    }
    pub fn pc(&self) -> usize {
        self.pc
    }
    pub fn fence(&self) -> bool {
        self.fence
    }
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    pub fn done(&self) -> bool {
        (self.pc == self.input.commands.len() && self.draw.is_none() || self.fault.is_some())
            && self.active.is_none()
            && self.queue.is_empty()
    }
    pub fn current_plan(&self) -> Option<&vertex_timed::Plan> {
        self.draw.as_ref()?.execution.as_ref().map(|e| &e.plan)
    }
    pub fn retained_plan_count(&self) -> usize {
        usize::from(self.current_plan().is_some())
    }
    pub fn scratchpad(&self) -> &Scratchpad {
        &self.sp
    }
    pub(crate) fn into_scratchpad(self) -> Scratchpad {
        self.sp
    }
    pub fn step(
        &mut self,
        ce: bool,
        draw_start_allowed: bool,
        producer: &mut impl ProducerPort,
    ) -> Result<FrontendOut, String> {
        if self.poisoned {
            return Err("frontend edge is poisoned; recreation required".into());
        }
        if self.wall == self.config.max_cycles {
            return Err("frontend maximum cycle count exceeded (WAIT/credit/service)".into());
        }
        self.poisoned = true;
        let out = self.step_inner(ce, draw_start_allowed, producer)?;
        self.poisoned = false;
        Ok(out)
    }
    fn step_inner(
        &mut self,
        ce: bool,
        draw_start_allowed: bool,
        producer: &mut impl ProducerPort,
    ) -> Result<FrontendOut, String> {
        let cycle = self.wall;
        let input = self.input;
        let config = self.config;
        let core_cycle = self.core_cycle;
        let mut records = Vec::new();
        let mut plan_created = false;
        let mut set = 0_u8;
        let mut ack = 0_u8;
        let mut emit = |action| {
            records.push(Record {
                cycle,
                core_cycle,
                action,
            })
        };
        if self.active.is_none() {
            if let Some((descriptor, lease)) = self.queue.pop_front() {
                self.active = Some(Dma {
                    descriptor,
                    lease,
                    beat: 0,
                    next: cycle + config.dma_first_latency,
                    last_write: None,
                    failed: false,
                });
            }
        }
        if let Some(dma) = self.active.as_mut() {
            if dma.beat == dma.descriptor.byte_count / 8 {
                if dma.last_write.is_some_and(|c| cycle > c) {
                    if !dma.failed {
                        self.sp.complete(dma.lease)?;
                        self.tokens[usize::from(dma.descriptor.completion_token)] = true;
                        set |= 1 << dma.descriptor.completion_token;
                        emit(Action::DmaComplete {
                            lease: dma.lease,
                            token: dma.descriptor.completion_token,
                        });
                    } else {
                        emit(Action::DmaFaultComplete { lease: dma.lease });
                    }
                    self.active = None;
                }
            } else if cycle >= dma.next {
                if !dma.failed {
                    match input.read_beat(dma.descriptor.physical_addr + dma.beat as u64 * 8) {
                        Ok(data) => {
                            let address = self.sp.dma_beat(dma.lease, data)?;
                            emit(Action::DmaWrite {
                                lease: dma.lease,
                                address,
                                data,
                            });
                        }
                        Err(error) => {
                            self.sp.fault(dma.lease)?;
                            dma.failed = true;
                            self.fault = Some(error.clone());
                            emit(Action::DmaFault { lease: dma.lease });
                            emit(Action::Fault(error));
                        }
                    }
                } else {
                    emit(Action::DmaDrain {
                        lease: dma.lease,
                        beat: dma.beat,
                    });
                }
                dma.beat += 1;
                dma.next = cycle + config.dma_gap;
                dma.last_write = Some(cycle);
            }
        }
        if self.pc < input.commands.len() && self.fault.is_none() {
            set |= 1 << 4;
        }
        if self.fault.is_some() {
            set |= 1 << 7;
        }
        let boundary = ce && self.draw.is_none();
        self.events.update(set, 0);
        let handler = self.events.take(boundary);
        if let Some(id) = handler {
            if id < 4 {
                ack |= 1 << id;
            } else if id == 4 && self.pc >= input.commands.len() {
                ack |= 1 << 4;
            }
        }
        if ce && self.fault.is_none() {
            if let Some(d) = self.draw.as_mut() {
                if d.vertex == d.count {
                    producer.finish(d.slot, d.epoch)?;
                    self.sp.release(d.lease)?;
                    emit(Action::DrawDone {
                        lease: d.lease,
                        slot: d.slot,
                    });
                    self.draw = None;
                } else {
                    if let Some((address, data, ready)) = d.pending {
                        if cycle >= ready {
                            d.capture_word(address, data);
                            d.pending = None;
                        }
                    }
                    if let Some(execution) = &d.execution {
                        let plan = &execution.plan;
                        if self.core_cycle >= execution.body_start {
                            let relative =
                                self.core_cycle - execution.body_start + plan.setup_cycles;
                            for e in &plan.counted.frame.events {
                                if let audited::Operation::Write { memory, row } = e.operation {
                                    if plan.counted.frame.memories[memory].name == "TRANSFORMED"
                                        && plan.schedule.nodes[e.id].issue == relative
                                    {
                                        let data = plan.counted.outputs[0].rows()[row];
                                        let destination = d.vertex * 7 + row;
                                        producer.write_row(d.slot, d.epoch, d.vertex, row, data)?;
                                        emit(Action::VertexWrite {
                                            slot: d.slot,
                                            epoch: d.epoch,
                                            row: destination,
                                            data,
                                        });
                                    }
                                }
                            }
                            if relative == plan.publication[0] {
                                producer.publish(d.slot, d.epoch, d.vertex)?;
                                emit(Action::Publish {
                                    slot: d.slot,
                                    epoch: d.epoch,
                                    vertex: d.vertex,
                                });
                                d.vertex += 1;
                                d.execution = None;
                                if d.vertex == d.count && self.profile == Profile::LegacyStandalone
                                {
                                    producer.finish(d.slot, d.epoch)?;
                                    self.sp.release(d.lease)?;
                                    emit(Action::DrawDone {
                                        lease: d.lease,
                                        slot: d.slot,
                                    });
                                    self.draw = None;
                                }
                            }
                        }
                    } else if d.pending.is_none() {
                        let start = d.region * REGION_BYTES + d.offset + d.vertex * 12;
                        let addresses = [start & !7, (start + 8) & !7];
                        if let Some(&address) = addresses
                            .iter()
                            .find(|a| !d.cached.iter().flatten().any(|(known, _)| known == *a))
                        {
                            match self.sp.read64(d.lease, address) {
                                Ok(data) => {
                                    d.pending = Some((address, data, cycle + 1));
                                    emit(Action::CoreRead {
                                        lease: d.lease,
                                        address,
                                        data,
                                    });
                                }
                                Err(error) => {
                                    self.fault = Some(error.clone());
                                    emit(Action::Fault(error));
                                }
                            }
                        } else {
                            let packed = PackedVertex(std::array::from_fn(|k| {
                                let addr = start + k * 4;
                                let data = d
                                    .cached
                                    .iter()
                                    .flatten()
                                    .find(|(a, _)| *a == addr & !7)
                                    .expect("two-word vertex latch")
                                    .1;
                                (data >> ((addr & 7) * 8)) as u32
                            }));
                            match vertex_timed::run(&d.context, &[packed], config.hardware) {
                                Ok(plan) => {
                                    let index = self.plan_serial;
                                    self.plan_serial += 1;
                                    let setup = !d.loaded;
                                    let body_start =
                                        self.core_cycle + if setup { plan.setup_cycles } else { 0 };
                                    emit(Action::Compute {
                                        slot: d.slot,
                                        vertex: d.vertex,
                                        plan: index,
                                        body_start,
                                        setup,
                                    });
                                    d.loaded = true;
                                    plan_created = true;
                                    d.execution = Some(Execution { plan, body_start });
                                }
                                Err(error) => {
                                    self.fault = Some(error.clone());
                                    emit(Action::Fault(error));
                                }
                            }
                        }
                    }
                }
            } else if self.pc < input.commands.len()
                && (handler == Some(4)
                    || matches!(input.commands[self.pc], Command::Wait(_) | Command::Fence))
            {
                let mut accepted = false;
                match &input.commands[self.pc] {
                    Command::Dma(descriptor) => {
                        let token = usize::from(descriptor.completion_token);
                        if self.reserved[token] {
                            self.fault = Some("DMA token still reserved/unacknowledged".into());
                        } else if self.queue.len() < 4 {
                            match self.sp.reserve(*descriptor) {
                                Ok(lease) => {
                                    self.leases[lease.region] = Some(lease);
                                    self.reserved[token] = true;
                                    self.queue.push_back((*descriptor, lease));
                                    emit(Action::Submit {
                                        descriptor: *descriptor,
                                        lease,
                                    });
                                    accepted = true;
                                }
                                Err(error) => self.fault = Some(error),
                            }
                        }
                    }
                    Command::Wait(token) => {
                        if self.tokens[usize::from(*token)] {
                            self.tokens[usize::from(*token)] = false;
                            self.reserved[usize::from(*token)] = false;
                            ack |= 1 << token;
                            emit(Action::WaitAck(*token));
                            accepted = true;
                        }
                    }
                    Command::Draw {
                        region,
                        byte_offset,
                        vertices,
                        context,
                    } => {
                        if let Some(lease) = self.leases[*region] {
                            if self.sp.regions[*region].owner == Owner::Ready {
                                if let Some(slot) =
                                    producer.free_slot().filter(|_| draw_start_allowed)
                                {
                                    let epoch = producer.allocate(slot, *vertices)?;
                                    self.sp.acquire(lease)?;
                                    emit(Action::DrawStart { lease, slot, epoch });
                                    self.draw = Some(Draw {
                                        lease,
                                        slot,
                                        epoch,
                                        region: *region,
                                        offset: *byte_offset,
                                        count: *vertices,
                                        context: context.clone(),
                                        vertex: 0,
                                        cached: [None; 2],
                                        pending: None,
                                        execution: None,
                                        loaded: false,
                                    });
                                    accepted = true;
                                }
                            } else if matches!(
                                self.sp.regions[*region].owner,
                                Owner::Free | Owner::Faulted
                            ) {
                                self.fault = Some("DRAW region not READY or FILLING".into());
                            }
                        } else {
                            self.fault = Some("DRAW region has no producer".into());
                        }
                    }
                    Command::Release { slot, epoch } => match producer.release(*slot, *epoch) {
                        Ok(()) => {
                            emit(Action::Release {
                                slot: *slot,
                                epoch: *epoch,
                            });
                            accepted = true;
                        }
                        Err(error) => self.fault = Some(error),
                    },
                    Command::Fence => {
                        if self.active.is_none() && self.queue.is_empty() {
                            self.fence = true;
                            emit(Action::Fence);
                            accepted = true;
                        }
                    }
                    Command::Unsupported(_) => self.fault = Some("unsupported command".into()),
                }
                if accepted {
                    self.pc += 1;
                    ack |= 1 << 4;
                }
                if let Some(error) = &self.fault {
                    emit(Action::Fault(error.clone()));
                }
            }
        }
        // Set wins for same-edge handler acknowledgment and new event.
        self.events.update(set, ack);
        let event = EventTick {
            cycle,
            set,
            ack,
            boundary,
            handler,
            pending: self.events.pending,
        };
        if ce {
            self.core_cycle += 1;
        }
        if records.len() > 16 {
            return Err("frontend edge effect bound".into());
        }
        self.wall += 1;
        Ok(FrontendOut {
            records,
            event,
            plan_created,
        })
    }
}
/// Owned only by the legacy standalone driver, never by Sequencer.
pub(crate) struct LegacySlots(pub [Slot; 2]);
impl Default for LegacySlots {
    fn default() -> Self {
        Self(std::array::from_fn(|_| Slot::default()))
    }
}
impl ProducerPort for LegacySlots {
    fn free_slot(&self) -> Option<usize> {
        self.0.iter().position(|s| !s.held)
    }
    fn allocate(&mut self, slot: usize, _count: usize) -> Result<u32, String> {
        self.0[slot].allocate()
    }
    fn write_row(
        &mut self,
        slot: usize,
        epoch: u32,
        vertex: usize,
        row: usize,
        data: u64,
    ) -> Result<(), String> {
        let s = &mut self.0[slot];
        if !s.held
            || !s.producing
            || s.epoch != epoch
            || vertex >= 64
            || row >= 7
            || data >> 36 != 0
        {
            return Err("legacy row ownership".into());
        }
        s.rows[vertex * 7 + row] = data;
        Ok(())
    }
    fn publish(&mut self, slot: usize, epoch: u32, vertex: usize) -> Result<(), String> {
        let s = &mut self.0[slot];
        if s.epoch != epoch {
            return Err("legacy publication epoch".into());
        }
        s.ready[vertex] = true;
        Ok(())
    }
    fn finish(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
        self.0[slot].finish_production(epoch)
    }
    fn release(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
        self.0[slot].release(epoch)
    }
}
