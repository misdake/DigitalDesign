//! Bounded micro-operation sequencer; DMA advances even during WAIT and core CE=0.
use super::super::ports::*;
use crate::{
    command_processor::{
        ports::*,
        sim::{
            counted as command_counted, oracle as command_oracle,
            timed::{self as event_audit, EventTick},
        },
    },
    scratchpad::{
        ports::*,
        sim::{
            counted::{self as sp_counted, Transaction},
            oracle::Scratchpad,
            timed::{self as sp_timed, Transfer},
        },
    },
    vertex::{ports::*, sim::timed as vertex_timed},
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub hardware: vertex_timed::Hardware,
    pub max_cycles: u64,
    pub dma_first_latency: u64,
    pub dma_gap: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            hardware: vertex_timed::Hardware::default(),
            max_cycles: 20000,
            dma_first_latency: 5,
            dma_gap: 1,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Submit {
        descriptor: DmaDescriptor,
        lease: Lease,
    },
    DmaWrite {
        lease: Lease,
        address: usize,
        data: u64,
    },
    DmaComplete {
        lease: Lease,
        token: u8,
    },
    DmaFault {
        lease: Lease,
    },
    DmaDrain {
        lease: Lease,
        beat: usize,
    },
    DmaFaultComplete {
        lease: Lease,
    },
    CoreRead {
        lease: Lease,
        address: usize,
        data: u64,
    },
    DrawStart {
        lease: Lease,
        slot: usize,
        epoch: u32,
    },
    Compute {
        slot: usize,
        vertex: usize,
        plan: usize,
        body_start: u64,
        setup: bool,
    },
    VertexWrite {
        slot: usize,
        epoch: u32,
        row: usize,
        data: u64,
    },
    Publish {
        slot: usize,
        epoch: u32,
        vertex: usize,
    },
    DrawDone {
        lease: Lease,
        slot: usize,
    },
    Release {
        slot: usize,
        epoch: u32,
    },
    WaitAck(u8),
    Fence,
    Fault(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub cycle: u64,
    pub core_cycle: u64,
    pub action: Action,
}
pub struct Report {
    pub input: Input,
    pub config: Config,
    pub core_stalls: Vec<u64>,
    pub cycles: u64,
    pub core_cycles: u64,
    pub outputs: Vec<DrawOutput>,
    pub slots: [Slot; 2],
    pub scratchpad: Scratchpad,
    pub records: Vec<Record>,
    pub events: Vec<EventTick>,
    pub plans: Vec<vertex_timed::Plan>,
    pub commands: audited::FrameReport,
    pub scratchpad_counted: Option<sp_counted::Report>,
    pub transfers: Vec<Transfer>,
    pub fault: Option<String>,
    pub fence: bool,
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
    plan: usize,
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
    cached: Vec<(usize, u64)>,
    pending: Option<(usize, u64, u64)>,
    execution: Option<Execution>,
    result: Vec<Transformed>,
    loaded: bool,
}
type ReplayedDraw = (
    Lease,
    usize,
    u32,
    usize,
    usize,
    Context,
    usize,
    Vec<Transformed>,
);
pub fn run(input: &Input, config: Config, core_stalls: &[u64]) -> Result<Report, String> {
    command_oracle::validate(&input.commands, 64)?;
    let commands = command_counted::run(&input.commands, 64)?;
    if config.max_cycles == 0
        || config.max_cycles > 1_000_000
        || config.dma_first_latency == 0
        || config.dma_first_latency > 1000
        || config.dma_gap == 0
        || config.dma_gap > 1000
        || input.memory.len() > 1_048_576
        || core_stalls.len() > 100000
        || core_stalls.iter().any(|&c| c >= config.max_cycles)
        || core_stalls.iter().copied().collect::<BTreeSet<_>>().len() != core_stalls.len()
    {
        return Err("frontend cycle/memory/service bounds".into());
    }
    let mut sp = Scratchpad::default();
    let mut leases = [None; 2];
    let mut tokens = [false; 4];
    let mut reserved = [false; 4];
    let stall_set = core_stalls.iter().copied().collect::<BTreeSet<_>>();
    let mut slots: [Slot; 2] = std::array::from_fn(|_| Slot::default());
    let mut queue = VecDeque::new();
    let mut active: Option<Dma> = None;
    let mut draw: Option<Draw> = None;
    let mut events = EventState::default();
    let mut event_rows = Vec::new();
    let mut records = Vec::new();
    let mut plans: Vec<vertex_timed::Plan> = Vec::new();
    let mut outputs = Vec::new();
    let mut transactions = Vec::new();
    let mut transfers = Vec::new();
    let mut pc = 0;
    let mut core_cycle = 0;
    let mut fault = None;
    let mut fence = false;
    for cycle in 0..config.max_cycles {
        let ce = !stall_set.contains(&cycle);
        let mut set = 0_u8;
        let mut ack = 0_u8;
        let mut emit = |action| {
            records.push(Record {
                cycle,
                core_cycle,
                action,
            })
        };
        if active.is_none() {
            if let Some((descriptor, lease)) = queue.pop_front() {
                active = Some(Dma {
                    descriptor,
                    lease,
                    beat: 0,
                    next: cycle + config.dma_first_latency,
                    last_write: None,
                    failed: false,
                });
            }
        }
        if let Some(dma) = active.as_mut() {
            if dma.beat == dma.descriptor.byte_count / 8 {
                if dma.last_write.is_some_and(|c| cycle > c) {
                    if !dma.failed {
                        sp.complete(dma.lease)?;
                        tokens[usize::from(dma.descriptor.completion_token)] = true;
                        set |= 1 << dma.descriptor.completion_token;
                        emit(Action::DmaComplete {
                            lease: dma.lease,
                            token: dma.descriptor.completion_token,
                        });
                    } else {
                        emit(Action::DmaFaultComplete { lease: dma.lease });
                    }
                    active = None;
                }
            } else if cycle >= dma.next {
                if !dma.failed {
                    match input.read_beat(dma.descriptor.physical_addr + dma.beat as u64 * 8) {
                        Ok(data) => {
                            let address = sp.dma_beat(dma.lease, data)?;
                            emit(Action::DmaWrite {
                                lease: dma.lease,
                                address,
                                data,
                            });
                            let transaction = transactions.len();
                            transactions.push(Transaction::DmaWrite { address, data });
                            transfers.push(Transfer {
                                transaction,
                                issue: cycle,
                                ready: cycle + 1,
                            });
                        }
                        Err(error) => {
                            sp.fault(dma.lease)?;
                            dma.failed = true;
                            fault = Some(error.clone());
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
        if pc < input.commands.len() && fault.is_none() {
            set |= 1 << 4;
        }
        if fault.is_some() {
            set |= 1 << 7;
        }
        let boundary = ce && draw.is_none();
        events.update(set, 0);
        let handler = events.take(boundary);
        if let Some(id) = handler {
            if id < 4 {
                ack |= 1 << id;
            } else if id == 4 && pc >= input.commands.len() {
                ack |= 1 << 4;
            }
        }
        if ce && fault.is_none() {
            if let Some(d) = draw.as_mut() {
                if let Some((address, data, ready)) = d.pending {
                    if cycle >= ready {
                        if d.cached.len() == 2 {
                            d.cached.remove(0);
                        }
                        d.cached.push((address, data));
                        d.pending = None;
                    }
                }
                if let Some(execution) = &d.execution {
                    let plan = &plans[execution.plan];
                    if core_cycle >= execution.body_start {
                        let relative = core_cycle - execution.body_start + plan.setup_cycles;
                        for e in &plan.counted.frame.events {
                            if let audited::Operation::Write { memory, row } = e.operation {
                                if plan.counted.frame.memories[memory].name == "TRANSFORMED"
                                    && plan.schedule.nodes[e.id].issue == relative
                                {
                                    let data = plan.counted.outputs[0].rows()[row];
                                    let destination = d.vertex * 7 + row;
                                    slots[d.slot].rows[destination] = data;
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
                            slots[d.slot].ready[d.vertex] = true;
                            d.result.push(plan.counted.outputs[0].clone());
                            emit(Action::Publish {
                                slot: d.slot,
                                epoch: d.epoch,
                                vertex: d.vertex,
                            });
                            d.vertex += 1;
                            d.execution = None;
                            if d.vertex == d.count {
                                slots[d.slot].finish_production(d.epoch)?;
                                sp.release(d.lease)?;
                                emit(Action::DrawDone {
                                    lease: d.lease,
                                    slot: d.slot,
                                });
                                outputs.push(DrawOutput {
                                    slot: d.slot,
                                    epoch: d.epoch,
                                    vertices: d.result.clone(),
                                });
                                draw = None;
                            }
                        }
                    }
                } else if d.pending.is_none() {
                    let start = d.region * REGION_BYTES + d.offset + d.vertex * 12;
                    let addresses = [start & !7, (start + 8) & !7];
                    if let Some(&address) = addresses
                        .iter()
                        .find(|a| !d.cached.iter().any(|(known, _)| known == *a))
                    {
                        match sp.read64(d.lease, address) {
                            Ok(data) => {
                                d.pending = Some((address, data, cycle + 1));
                                emit(Action::CoreRead {
                                    lease: d.lease,
                                    address,
                                    data,
                                });
                                let transaction = transactions.len();
                                transactions.push(Transaction::CoreRead { address });
                                transfers.push(Transfer {
                                    transaction,
                                    issue: cycle,
                                    ready: cycle + 1,
                                });
                            }
                            Err(error) => {
                                fault = Some(error.clone());
                                emit(Action::Fault(error));
                            }
                        }
                    } else {
                        let packed = PackedVertex(std::array::from_fn(|k| {
                            let addr = start + k * 4;
                            let data = d
                                .cached
                                .iter()
                                .find(|(a, _)| *a == addr & !7)
                                .expect("two-word vertex latch")
                                .1;
                            (data >> ((addr & 7) * 8)) as u32
                        }));
                        match vertex_timed::run(&d.context, &[packed], config.hardware) {
                            Ok(plan) => {
                                let index = plans.len();
                                let setup = !d.loaded;
                                let body_start =
                                    core_cycle + if setup { plan.setup_cycles } else { 0 };
                                emit(Action::Compute {
                                    slot: d.slot,
                                    vertex: d.vertex,
                                    plan: index,
                                    body_start,
                                    setup,
                                });
                                d.loaded = true;
                                plans.push(plan);
                                d.execution = Some(Execution {
                                    plan: index,
                                    body_start,
                                });
                            }
                            Err(error) => {
                                fault = Some(error.clone());
                                emit(Action::Fault(error));
                            }
                        }
                    }
                }
            } else if pc < input.commands.len()
                && (handler == Some(4)
                    || matches!(input.commands[pc], Command::Wait(_) | Command::Fence))
            {
                let mut accepted = false;
                match &input.commands[pc] {
                    Command::Dma(descriptor) => {
                        let token = usize::from(descriptor.completion_token);
                        if reserved[token] {
                            fault = Some("DMA token still reserved/unacknowledged".into());
                        } else if queue.len() < 4 {
                            match sp.reserve(*descriptor) {
                                Ok(lease) => {
                                    leases[lease.region] = Some(lease);
                                    reserved[token] = true;
                                    queue.push_back((*descriptor, lease));
                                    emit(Action::Submit {
                                        descriptor: *descriptor,
                                        lease,
                                    });
                                    accepted = true;
                                }
                                Err(error) => fault = Some(error),
                            }
                        }
                    }
                    Command::Wait(token) => {
                        if tokens[usize::from(*token)] {
                            tokens[usize::from(*token)] = false;
                            reserved[usize::from(*token)] = false;
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
                        if let Some(lease) = leases[*region] {
                            if sp.regions[*region].owner == Owner::Ready {
                                if let Some(slot) = slots.iter().position(|s| !s.held) {
                                    let epoch = slots[slot].allocate()?;
                                    sp.acquire(lease)?;
                                    emit(Action::DrawStart { lease, slot, epoch });
                                    draw = Some(Draw {
                                        lease,
                                        slot,
                                        epoch,
                                        region: *region,
                                        offset: *byte_offset,
                                        count: *vertices,
                                        context: context.clone(),
                                        vertex: 0,
                                        cached: Vec::new(),
                                        pending: None,
                                        execution: None,
                                        result: Vec::new(),
                                        loaded: false,
                                    });
                                    accepted = true;
                                }
                            } else if matches!(
                                sp.regions[*region].owner,
                                Owner::Free | Owner::Faulted
                            ) {
                                fault = Some("DRAW region not READY or FILLING".into());
                            }
                        } else {
                            fault = Some("DRAW region has no producer".into());
                        }
                    }
                    Command::Release { slot, epoch } => match slots[*slot].release(*epoch) {
                        Ok(()) => {
                            emit(Action::Release {
                                slot: *slot,
                                epoch: *epoch,
                            });
                            accepted = true;
                        }
                        Err(error) => fault = Some(error),
                    },
                    Command::Fence => {
                        if active.is_none() && queue.is_empty() {
                            fence = true;
                            emit(Action::Fence);
                            accepted = true;
                        }
                    }
                    Command::Unsupported(_) => fault = Some("unsupported command".into()),
                }
                if accepted {
                    pc += 1;
                    ack |= 1 << 4;
                }
                if let Some(error) = &fault {
                    emit(Action::Fault(error.clone()));
                }
            }
        }
        // Set wins for same-edge handler acknowledgment and new event.
        events.update(set, ack);
        event_rows.push(EventTick {
            cycle,
            set,
            ack,
            boundary,
            handler,
            pending: events.pending,
        });
        if ce {
            core_cycle += 1;
        }
        if (pc == input.commands.len() && draw.is_none() || fault.is_some())
            && active.is_none()
            && queue.is_empty()
        {
            let scratchpad_counted = if transactions.is_empty() {
                None
            } else {
                Some(sp_counted::run_with_vertices(
                    &transactions,
                    &packet_layouts(&commands, &records)?,
                )?)
            };
            let report = Report {
                input: input.clone(),
                config,
                core_stalls: core_stalls.to_vec(),
                cycles: cycle + 1,
                core_cycles: core_cycle,
                outputs,
                slots,
                scratchpad: sp,
                records,
                events: event_rows,
                plans,
                commands,
                scratchpad_counted,
                transfers,
                fault,
                fence,
            };
            report.audit()?;
            return Ok(report);
        }
    }
    Err("frontend maximum cycle count exceeded (WAIT/credit/service)".into())
}
impl Report {
    /// Replay ownership and payload effects without using the sequencer's state.
    pub fn audit(&self) -> Result<(), String> {
        if self.cycles == 0
            || self.cycles > self.config.max_cycles
            || self.events.len() as u64 != self.cycles
            || self
                .core_stalls
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != self.core_stalls.len()
            || self.core_cycles
                != self.cycles
                    - self
                        .core_stalls
                        .iter()
                        .filter(|&&c| c < self.cycles)
                        .count() as u64
        {
            return Err("frontend trace shape/bound".into());
        }
        self.commands.audit().map_err(|e| format!("{e:?}"))?;
        let expected_commands = command_counted::run(&self.input.commands, 64)?;
        if expected_commands.counts != self.commands.counts
            || expected_commands
                .values
                .iter()
                .map(|v| (v.format, v.raw))
                .collect::<Vec<_>>()
                != self
                    .commands
                    .values
                    .iter()
                    .map(|v| (v.format, v.raw))
                    .collect::<Vec<_>>()
        {
            return Err("command frame input certificate".into());
        }
        event_audit::audit_events(&self.events, self.config.max_cycles)?;
        let stall_set = self.core_stalls.iter().copied().collect::<BTreeSet<_>>();
        let mut clock = 0;
        let core_at = (0..self.cycles)
            .map(|cycle| {
                let before = clock;
                if !stall_set.contains(&cycle) {
                    clock += 1;
                }
                before
            })
            .collect::<Vec<_>>();
        let mut effects: BTreeMap<u64, Vec<&Record>> = BTreeMap::new();
        for r in &self.records {
            effects.entry(r.cycle).or_default().push(r);
        }
        let mut prior_pc = 0;
        let mut prior_fault = false;
        let mut active_draw = false;
        // Event sources and boundaries are reconstructed from accepted effects,
        // rather than treating arbitrary pending-bit inputs as a valid replay.
        for (cycle, row) in self.events.iter().enumerate() {
            if row.cycle != cycle as u64 {
                return Err("event clock coverage".into());
            }
            let accepted = |r: &Record| {
                matches!(
                    r.action,
                    Action::Submit { .. }
                        | Action::WaitAck(_)
                        | Action::DrawStart { .. }
                        | Action::Release { .. }
                        | Action::Fence
                )
            };
            let current_effects = effects.get(&row.cycle).map(Vec::as_slice).unwrap_or(&[]);
            let fault_before = prior_fault
                || current_effects
                    .iter()
                    .any(|r| matches!(r.action, Action::DmaFault { .. }));
            let mut set = 0;
            let mut ack = 0;
            for r in current_effects {
                match r.action {
                    Action::DmaComplete { token, .. } => set |= 1 << token,
                    Action::WaitAck(token) => ack |= 1 << token,
                    _ => {}
                }
                if accepted(r) {
                    ack |= 1 << 4;
                }
            }
            if prior_pc < self.input.commands.len() && !fault_before {
                set |= 1 << 4;
            }
            if fault_before {
                set |= 1 << 7;
            }
            if let Some(id) = row.handler {
                if id < 4 {
                    ack |= 1 << id;
                } else if id == 4 && prior_pc >= self.input.commands.len() {
                    ack |= 1 << 4;
                }
            }
            if row.set != set
                || row.ack != ack
                || row.boundary != (!stall_set.contains(&row.cycle) && !active_draw)
            {
                return Err("event source/ack/boundary certificate".into());
            }
            if current_effects.iter().any(|r| {
                matches!(
                    r.action,
                    Action::Submit { .. } | Action::DrawStart { .. } | Action::Release { .. }
                )
            }) && row.handler != Some(4)
            {
                return Err("command bypassed fixed ROM handler".into());
            }
            for r in current_effects {
                if accepted(r) {
                    prior_pc += 1;
                }
                match r.action {
                    Action::DrawStart { .. } => active_draw = true,
                    Action::DrawDone { .. } => active_draw = false,
                    Action::Fault(_) => prior_fault = true,
                    _ => {}
                }
            }
        }
        for plan in &self.plans {
            plan.audit()?;
        }
        let mut sp = Scratchpad::default();
        let mut slots: [Slot; 2] = std::array::from_fn(|_| Slot::default());
        let mut dmas: Vec<(DmaDescriptor, Lease, usize, u64, bool, u64)> = Vec::new();
        let mut tokens = [false; 4];
        let mut reserved = [false; 4];
        let mut last_dma_done = 0;
        let mut expected_fault: Option<String> = None;
        let mut pc = 0;
        let mut current: Option<ReplayedDraw> = None;
        let mut cached: Vec<(usize, u64, u64)> = Vec::new();
        let mut execution: Option<(usize, u64, usize)> = None;
        let mut writes = [false; 7];
        let mut outputs = Vec::new();
        let mut used_plans = 0;
        let mut transactions = Vec::new();
        let mut transfers = Vec::new();
        let mut fault = false;
        let mut fence = false;
        let mut last_cycle = 0;
        for record in &self.records {
            if record.cycle < last_cycle
                || record.cycle >= self.cycles
                || record.core_cycle != core_at[record.cycle as usize]
            {
                return Err("wall/core clock trace".into());
            }
            last_cycle = record.cycle;
            let independent = matches!(
                record.action,
                Action::DmaWrite { .. }
                    | Action::DmaComplete { .. }
                    | Action::DmaFault { .. }
                    | Action::DmaDrain { .. }
                    | Action::DmaFaultComplete { .. }
                    | Action::Fault(_)
            );
            if !independent && stall_set.contains(&record.cycle) {
                return Err("core operation during CE=0".into());
            }
            match &record.action {
                Action::Submit { descriptor, lease } => {
                    if fault
                        || !matches!(self.input.commands.get(pc),Some(Command::Dma(d)) if d==descriptor)
                        || reserved[usize::from(descriptor.completion_token)]
                    {
                        return Err("unexpected DMA submission/token".into());
                    }
                    let actual = sp.reserve(*descriptor)?;
                    if actual != *lease {
                        return Err("DMA lease".into());
                    }
                    reserved[usize::from(descriptor.completion_token)] = true;
                    dmas.push((*descriptor, *lease, 0, 0, false, record.cycle));
                    pc += 1;
                }
                Action::DmaWrite {
                    lease,
                    address,
                    data,
                } => {
                    if dmas.first().is_none_or(|d| d.1 != *lease) {
                        return Err("more than one active DMA descriptor".into());
                    }
                    let dma = dmas
                        .iter_mut()
                        .find(|d| d.1 == *lease)
                        .ok_or("DMA write without descriptor")?;
                    if dma.4
                        || dma.2 >= dma.0.byte_count / 8
                        || *address != dma.0.scratchpad_addr + dma.2 * 8
                        || self
                            .input
                            .read_beat(dma.0.physical_addr + dma.2 as u64 * 8)?
                            != *data
                    {
                        return Err("DMA beat provenance/order".into());
                    }
                    if record.cycle
                        != if dma.2 == 0 {
                            dma.5.max(last_dma_done) + 1 + self.config.dma_first_latency
                        } else {
                            dma.3 + self.config.dma_gap
                        }
                    {
                        return Err("DMA service latency/gap".into());
                    }
                    if sp.dma_beat(*lease, *data)? != *address {
                        return Err("DMA address".into());
                    }
                    dma.2 += 1;
                    dma.3 = record.cycle;
                    let transaction = transactions.len();
                    transactions.push(Transaction::DmaWrite {
                        address: *address,
                        data: *data,
                    });
                    transfers.push(Transfer {
                        transaction,
                        issue: record.cycle,
                        ready: record.cycle + 1,
                    });
                }
                Action::DmaComplete { lease, token } => {
                    let i = dmas
                        .iter()
                        .position(|d| d.1 == *lease)
                        .ok_or("completion descriptor")?;
                    let dma = dmas[i];
                    if i != 0
                        || dma.4
                        || *token != dma.0.completion_token
                        || dma.2 != dma.0.byte_count / 8
                        || record.cycle != dma.3 + 1
                    {
                        return Err("normal completion before final write ACK".into());
                    }
                    sp.complete(*lease)?;
                    tokens[usize::from(*token)] = true;
                    dmas.remove(i);
                    last_dma_done = record.cycle;
                }
                Action::DmaFault { lease } => {
                    if dmas.first().is_none_or(|d| d.1 != *lease) {
                        return Err("fault from inactive DMA descriptor".into());
                    }
                    let dma = dmas
                        .iter_mut()
                        .find(|d| d.1 == *lease)
                        .ok_or("fault descriptor")?;
                    if dma.4
                        || record.cycle
                            != if dma.2 == 0 {
                                dma.5.max(last_dma_done) + 1 + self.config.dma_first_latency
                            } else {
                                dma.3 + self.config.dma_gap
                            }
                    {
                        return Err("DMA fault timing".into());
                    }
                    expected_fault = Some(
                        self.input
                            .read_beat(dma.0.physical_addr + dma.2 as u64 * 8)
                            .err()
                            .ok_or("forged DMA source fault")?,
                    );
                    sp.fault(*lease)?;
                    dma.2 += 1;
                    dma.3 = record.cycle;
                    dma.4 = true;
                    fault = true;
                }
                Action::DmaDrain { lease, beat } => {
                    let dma = dmas
                        .iter_mut()
                        .find(|d| d.1 == *lease)
                        .ok_or("drain descriptor")?;
                    if !dma.4
                        || *beat != dma.2
                        || dma.2 >= dma.0.byte_count / 8
                        || record.cycle != dma.3 + self.config.dma_gap
                    {
                        return Err("fault drain order".into());
                    }
                    dma.2 += 1;
                    dma.3 = record.cycle;
                }
                Action::DmaFaultComplete { lease } => {
                    let i = dmas
                        .iter()
                        .position(|d| d.1 == *lease)
                        .ok_or("fault complete descriptor")?;
                    let dma = dmas[i];
                    if i != 0
                        || !dma.4
                        || dma.2 != dma.0.byte_count / 8
                        || record.cycle != dma.3 + 1
                    {
                        return Err("fault drain incomplete".into());
                    }
                    dmas.remove(i);
                    last_dma_done = record.cycle;
                }
                Action::WaitAck(token) => {
                    if !matches!(self.input.commands.get(pc),Some(Command::Wait(t)) if t==token)
                        || !tokens[usize::from(*token)]
                    {
                        return Err("WAIT completed without sticky token".into());
                    }
                    tokens[usize::from(*token)] = false;
                    reserved[usize::from(*token)] = false;
                    pc += 1;
                }
                Action::DrawStart { lease, slot, epoch } => {
                    let Some(Command::Draw {
                        region,
                        byte_offset,
                        vertices,
                        context,
                    }) = self.input.commands.get(pc)
                    else {
                        return Err("unexpected draw start".into());
                    };
                    if current.is_some()
                        || *region != lease.region
                        || slots[*slot].allocate()? != *epoch
                    {
                        return Err("draw ownership".into());
                    }
                    sp.acquire(*lease)?;
                    current = Some((
                        *lease,
                        *slot,
                        *epoch,
                        *byte_offset,
                        *vertices,
                        context.clone(),
                        0,
                        Vec::new(),
                    ));
                    cached.clear();
                    pc += 1;
                }
                Action::CoreRead {
                    lease,
                    address,
                    data,
                } => {
                    if current.as_ref().is_none_or(|d| d.0 != *lease)
                        || sp.read64(*lease, *address)? != *data
                    {
                        return Err("core read lease/data".into());
                    }
                    if cached.len() == 2 {
                        cached.remove(0);
                    }
                    cached.push((*address, *data, record.cycle + 1));
                    let transaction = transactions.len();
                    transactions.push(Transaction::CoreRead { address: *address });
                    transfers.push(Transfer {
                        transaction,
                        issue: record.cycle,
                        ready: record.cycle + 1,
                    });
                }
                Action::Compute {
                    slot,
                    vertex,
                    plan,
                    body_start,
                    setup,
                } => {
                    let d = current.as_ref().ok_or("compute outside draw")?;
                    if *slot != d.1
                        || *vertex != d.6
                        || execution.is_some()
                        || *plan != used_plans
                        || *setup != (*vertex == 0)
                    {
                        return Err("vertex compute order/shape".into());
                    }
                    let p = self.plans.get(*plan).ok_or("vertex plan index")?;
                    if *body_start != record.core_cycle + if *setup { p.setup_cycles } else { 0 } {
                        return Err("setup/body ROM entry timing".into());
                    }
                    let start = d.0.region * 4096 + d.3 + d.6 * 12;
                    let mut cells = [0; 3];
                    for (k, cell) in cells.iter_mut().enumerate() {
                        let addr = start + k * 4;
                        let source = cached
                            .iter()
                            .find(|(a, _, ready)| *a == addr & !7 && *ready <= record.cycle)
                            .ok_or("vertex consumed unavailable synchronous read")?;
                        *cell = (source.1 >> ((addr & 7) * 8)) as u32;
                    }
                    let expected = crate::vertex::sim::counted::run(&d.5, PackedVertex(cells))?;
                    if expected.output != p.counted.outputs[0]
                        || expected.frame.counts != p.counted.frame.counts
                        || expected
                            .frame
                            .values
                            .iter()
                            .map(|v| (v.format, v.raw))
                            .collect::<Vec<_>>()
                            != p.counted
                                .frame
                                .values
                                .iter()
                                .map(|v| (v.format, v.raw))
                                .collect::<Vec<_>>()
                    {
                        return Err("vertex plan input/payload certificate".into());
                    }
                    execution = Some((*plan, *body_start, *vertex));
                    writes = [false; 7];
                    used_plans += 1;
                }
                Action::VertexWrite {
                    slot,
                    epoch,
                    row,
                    data,
                } => {
                    let d = current.as_ref().ok_or("write outside draw")?;
                    let (index, start, vertex) = execution.ok_or("write without computation")?;
                    let p = &self.plans[index];
                    if *slot != d.1
                        || *epoch != d.2
                        || row / 7 != vertex
                        || writes[row % 7]
                        || p.counted.outputs[0].rows()[row % 7] != *data
                    {
                        return Err("transformed write payload/ownership".into());
                    }
                    let e=p.counted.frame.events.iter().find(|e|matches!(e.operation,audited::Operation::Write{memory,row:r} if p.counted.frame.memories[memory].name=="TRANSFORMED" && r==row%7)).ok_or("row ROM entry")?;
                    if record.core_cycle != start + p.schedule.nodes[e.id].issue - p.setup_cycles {
                        return Err("row write differs from certified static ROM".into());
                    }
                    slots[*slot].rows[*row] = *data;
                    writes[row % 7] = true;
                }
                Action::Publish {
                    slot,
                    epoch,
                    vertex,
                } => {
                    let d = current.as_mut().ok_or("publish outside draw")?;
                    let (index, start, v) = execution.ok_or("publish without computation")?;
                    let p = &self.plans[index];
                    if *slot != d.1
                        || *epoch != d.2
                        || *vertex != v
                        || !writes.iter().all(|b| *b)
                        || record.core_cycle != start + p.publication[0] - p.setup_cycles
                    {
                        return Err("publish before complete seven-row ACK".into());
                    }
                    slots[*slot].ready[*vertex] = true;
                    d.7.push(p.counted.outputs[0].clone());
                    d.6 += 1;
                    execution = None;
                }
                Action::DrawDone { lease, slot } => {
                    let d = current.take().ok_or("draw done without draw")?;
                    if d.0 != *lease || d.1 != *slot || d.6 != d.4 || execution.is_some() {
                        return Err("premature draw completion".into());
                    }
                    sp.release(*lease)?;
                    slots[*slot].finish_production(d.2)?;
                    outputs.push(DrawOutput {
                        slot: *slot,
                        epoch: d.2,
                        vertices: d.7,
                    });
                }
                Action::Release { slot, epoch } => {
                    if !matches!(self.input.commands.get(pc),Some(Command::Release{slot:s,epoch:e}) if s==slot && e==epoch)
                    {
                        return Err("release command order".into());
                    }
                    slots[*slot].release(*epoch)?;
                    pc += 1;
                }
                Action::Fence => {
                    if !matches!(self.input.commands.get(pc), Some(Command::Fence))
                        || !dmas.is_empty()
                        || current.is_some()
                        || fault
                    {
                        return Err("early/failed fence".into());
                    }
                    fence = true;
                    pc += 1;
                }
                Action::Fault(message) => {
                    if expected_fault.is_none() {
                        expected_fault = if let Some(d) = current.as_ref() {
                            let start = d.0.region * 4096 + d.3 + d.6 * 12;
                            let needed = [start & !7, (start + 8) & !7];
                            if let Some(&addr) = needed
                                .iter()
                                .find(|a| !cached.iter().any(|(known, _, _)| known == *a))
                            {
                                sp.read64(d.0, addr).err()
                            } else {
                                let packed = PackedVertex(std::array::from_fn(|k| {
                                    let addr = start + k * 4;
                                    let word = cached
                                        .iter()
                                        .find(|(a, _, _)| *a == addr & !7)
                                        .expect("cached fault input")
                                        .1;
                                    (word >> ((addr & 7) * 8)) as u32
                                }));
                                vertex_timed::run(&d.5, &[packed], self.config.hardware).err()
                            }
                        } else {
                            match self.input.commands.get(pc) {
                                Some(Command::Dma(d)) => {
                                    if reserved[usize::from(d.completion_token)] {
                                        Some("DMA token still reserved/unacknowledged".into())
                                    } else {
                                        sp.clone().reserve(*d).err()
                                    }
                                }
                                Some(Command::Release { slot, epoch }) => {
                                    slots[*slot].clone().release(*epoch).err()
                                }
                                Some(Command::Draw { region, .. }) => {
                                    if sp.regions[*region].epoch == 0 {
                                        Some("DRAW region has no producer".into())
                                    } else if matches!(
                                        sp.regions[*region].owner,
                                        Owner::Free | Owner::Faulted
                                    ) {
                                        Some("DRAW region not READY or FILLING".into())
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            }
                        };
                    }
                    if expected_fault.as_ref() != Some(message) {
                        return Err("unjustified sticky fault".into());
                    }
                    fault = true;
                }
            }
        }
        if !dmas.is_empty()
            || used_plans != self.plans.len()
            || outputs != self.outputs
            || slots != self.slots
            || sp.banks != self.scratchpad.banks
            || format!("{:?}", sp.regions) != format!("{:?}", self.scratchpad.regions)
            || expected_fault != self.fault
            || fault != self.fault.is_some()
            || fence != self.fence
            || !fault && pc != self.input.commands.len()
        {
            return Err("frontend final state certificate".into());
        }
        if transfers != self.transfers {
            return Err("scratchpad transaction timing bridge".into());
        }
        if let Some(counted) = &self.scratchpad_counted {
            if counted.transactions != transactions {
                return Err("scratchpad numerical transaction bridge".into());
            }
            if counted.vertex_reads != packet_layouts(&self.commands, &self.records)?
                || counted.vertices.len() != self.plans.len()
            {
                return Err("scratchpad vertex packet shape/addresses".into());
            }
            for (index, plan) in self.plans.iter().enumerate() {
                let frame = &plan.counted.frame;
                let source=frame.events.iter().find(|e|matches!(e.operation,audited::Operation::Read{memory,..} if frame.memories[memory].name=="v6")).ok_or("missing vertex input port")?;
                if frame.values[source.output.ok_or("vertex input value")?].raw as u128
                    != counted.vertices[index]
                {
                    return Err("vertex numerical payload bypassed scratchpad packet bridge".into());
                }
            }
            sp_timed::audit(counted, &transfers, self.config.max_cycles)?;
        } else if !transactions.is_empty() {
            return Err("missing scratchpad numerical frame".into());
        }
        if !fault {
            let expected = super::oracle::run(&self.input)?;
            if expected.outputs != self.outputs
                || expected.slots != self.slots
                || expected.scratchpad.banks != self.scratchpad.banks
                || expected.fence != self.fence
            {
                return Err("frontend oracle mismatch".into());
            }
        }
        Ok(())
    }
}
fn packet_layouts(
    commands: &audited::FrameReport,
    records: &[Record],
) -> Result<Vec<sp_counted::VertexRead>, String> {
    let get = |name: &str| {
        commands
            .outputs
            .iter()
            .find(|o| o.name == name)
            .map(|o| o.raw)
            .ok_or_else(|| format!("missing prepared command field {name}"))
    };
    let mut command = 0;
    let mut draw_command = None;
    let mut read_count = 0;
    let mut reads = BTreeMap::new();
    let mut layouts = Vec::new();
    for r in records {
        match r.action {
            Action::Submit { .. } | Action::WaitAck(_) | Action::Release { .. } | Action::Fence => {
                command += 1
            }
            Action::DrawStart { .. } => {
                draw_command = Some(command);
                command += 1;
            }
            Action::DrawDone { .. } => draw_command = None,
            Action::CoreRead { address, .. } => {
                reads.insert(address, read_count);
                read_count += 1;
            }
            Action::Compute { vertex, .. } => {
                let id = draw_command.ok_or("packet compute without DRAW")?;
                let prefix = format!("command.{id}.vertex.{vertex}");
                let first = get(&format!("{prefix}.first"))? as usize;
                let second = get(&format!("{prefix}.second"))? as usize;
                layouts.push(sp_counted::VertexRead {
                    first: *reads.get(&first).ok_or("first packet read unavailable")?,
                    second: *reads.get(&second).ok_or("second packet read unavailable")?,
                    high_half: get(&format!("{prefix}.high_half"))? != 0,
                });
            }
            _ => {}
        }
    }
    Ok(layouts)
}
