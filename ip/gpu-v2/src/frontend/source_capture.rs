//! Bounded source lifetime/control model. Does not execute vertex or setup math.
use super::ports::Slot;
use crate::{triangle, vertex::ports::Transformed};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ISSUER: AtomicU64 = AtomicU64::new(1);

struct Producer {
    slot: usize,
    epoch: u32,
    count: usize,
    next: usize,
    mask: u8,
    last_edge: u64,
}
struct OldSlot {
    epoch: u32,
    held: bool,
    producing: bool,
    ready: [bool; 64],
    sealed: bool,
}
/// Opaque, single-use pre-edge metadata. No source payload or future results.
pub struct ProducerPermit {
    issuer: u64,
    wall: u64,
    ce: bool,
    slots: [OldSlot; 2],
    credit: bool,
}
/// A borrowed post-clock write port, authorized exclusively by old metadata.
pub struct SourceProducerEdge<'a> {
    source: &'a mut Controller,
    permit: ProducerPermit,
    used: bool,
    admitted: Option<(u64, Task)>,
    task_used: bool,
}

pub const TASK_CREDITS: usize = 4;
pub const SOURCE_ROWS: usize = 21;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Task {
    pub triangle_id: u32,
    pub slot: usize,
    pub epoch: u32,
    pub vertices: [u8; 3],
    /// One immutable draw context. Arithmetic formats are not encoded here.
    pub context: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub ticket: u64,
    pub task: Task,
    rows: [[u64; 7]; 3],
}
impl Snapshot {
    /// Owned triangle input; clip/fan may use it after source slot release.
    pub fn input(&self) -> Result<triangle::ports::Input, String> {
        Ok(triangle::ports::Input {
            id: self.task.triangle_id,
            vertices: [
                Transformed::from_rows(self.rows[0])?,
                Transformed::from_rows(self.rows[1])?,
                Transformed::from_rows(self.rows[2])?,
            ],
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ReadIssue {
        ticket: u64,
        slot: usize,
        row: usize,
    },
    ReadReturn {
        ticket: u64,
        source_row: usize,
        data: u64,
    },
    SourceCaptured {
        ticket: u64,
        slot: usize,
        epoch: u32,
    },
    SourceReleased {
        slot: usize,
        epoch: u32,
    },
    TriangleConsumed {
        ticket: u64,
    },
    Cancelled,
}
struct Capture {
    snapshot: Snapshot,
    issued: usize,
    returned: usize,
    /// Single registered, CE-gated return; credit reserved at issue.
    pending: Option<(usize, u64)>,
}
pub struct Controller {
    slots: [Slot; 2],
    sealed: [bool; 2],
    references: [usize; 2],
    queue: VecDeque<(u64, Task)>,
    active: Option<Capture>,
    ready: Option<Snapshot>,
    context: u64,
    next_ticket: u64,
    cycles: u64,
    max_cycles: u64,
    max_tasks: u64,
    stopped: bool,
    cancelled: bool,
    producer: Option<Producer>,
    checked: [bool; 2],
    producer_bound: Option<u64>,
    issuer: u64,
    last_ce: bool,
}
impl Controller {
    pub fn new(
        slots: [Slot; 2],
        context: u64,
        max_cycles: u64,
        max_tasks: u64,
    ) -> Result<Self, String> {
        if max_cycles == 0
            || max_tasks == 0
            || slots
                .iter()
                .any(|s| s.rows.len() != 512 || s.ready.len() != 64 || (s.producing && !s.held))
        {
            return Err("source capture shape/bounds".into());
        }
        Ok(Self {
            slots,
            sealed: [false; 2],
            references: [0; 2],
            queue: VecDeque::new(),
            active: None,
            ready: None,
            context,
            next_ticket: 0,
            cycles: 0,
            max_cycles,
            max_tasks,
            stopped: false,
            cancelled: false,
            producer: None,
            checked: [false; 2],
            producer_bound: None,
            last_ce: false,
            issuer: NEXT_ISSUER
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| "source issuer exhausted")?,
        })
    }
    pub fn prepare_producer_edge(&self, ce: bool) -> ProducerPermit {
        ProducerPermit {
            issuer: self.issuer,
            wall: self.cycles,
            ce,
            slots: std::array::from_fn(|i| OldSlot {
                epoch: self.slots[i].epoch,
                held: self.slots[i].held,
                producing: self.slots[i].producing,
                ready: std::array::from_fn(|v| self.slots[i].ready[v]),
                sealed: self.sealed[i],
            }),
            credit: self.queue.len() < TASK_CREDITS,
        }
    }
    pub fn bind_producer_edge(
        &mut self,
        permit: ProducerPermit,
    ) -> Result<SourceProducerEdge<'_>, String> {
        if permit.issuer != self.issuer
            || permit.ce != self.last_ce
            || permit.wall.checked_add(1) != Some(self.cycles)
            || self.producer_bound == Some(self.cycles)
        {
            return Err("source producer missing, duplicate or foreign clock permit".into());
        }
        self.producer_bound = Some(self.cycles);
        Ok(SourceProducerEdge {
            source: self,
            permit,
            used: false,
            admitted: None,
            task_used: false,
        })
    }
    fn fixture_access(&self, slot: usize) -> Result<(), String> {
        if *self.checked.get(slot).ok_or("source slot index")? || self.producer.is_some() {
            return Err("checked producer cannot use atomic fixture API".into());
        }
        Ok(())
    }
    pub fn slots(&self) -> &[Slot; 2] {
        &self.slots
    }
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.ready.as_ref()
    }
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
    pub fn cycles(&self) -> u64 {
        self.cycles
    }
    /// Capture/snapshot work only; producers and downstream raster/render work
    /// must be drained separately before a context change or render fence.
    pub fn drained(&self) -> bool {
        self.queue.is_empty() && self.active.is_none() && self.ready.is_none()
    }
    fn slot(&self, slot: usize, epoch: u32) -> Result<&Slot, String> {
        let s = self.slots.get(slot).ok_or("source slot index")?;
        if !s.held || s.epoch != epoch {
            return Err("stale source slot epoch".into());
        }
        Ok(s)
    }
    pub fn allocate(&mut self, slot: usize) -> Result<u32, String> {
        self.fixture_access(slot)?;
        self.allocate_inner(slot)
    }
    fn allocate_inner(&mut self, slot: usize) -> Result<u32, String> {
        if self.stopped {
            return Err("source capture stopped".into());
        }
        let s = self.slots.get_mut(slot).ok_or("source slot index")?;
        let epoch = s.allocate()?;
        self.sealed[slot] = false;
        Ok(epoch)
    }
    /// Producer fixture boundary: call only after its seven row writes and
    /// publication edge. This method does not model or charge producer timing.
    pub fn publish_completed_vertex(
        &mut self,
        slot: usize,
        epoch: u32,
        vertex: usize,
        value: &Transformed,
    ) -> Result<(), String> {
        self.fixture_access(slot)?;
        let s = self.slot(slot, epoch)?;
        if self.stopped || !s.producing || vertex >= 64 || s.ready[vertex] {
            return Err("source vertex publication state".into());
        }
        value.validate()?;
        let rows = value.rows();
        Transformed::from_rows(rows)?;
        self.slots[slot].rows[vertex * 7..vertex * 7 + 7].copy_from_slice(&rows);
        self.slots[slot].ready[vertex] = true;
        Ok(())
    }
    pub fn finish_production(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
        self.fixture_access(slot)?;
        self.slot(slot, epoch)?;
        self.slots[slot].finish_production(epoch)
    }
    /// Fault-only producer acknowledgment, after its accepted row writes have
    /// drained. Unlike success completion this permits zero published vertices.
    pub fn abort_production(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
        self.fixture_access(slot)?;
        let s = self.slot(slot, epoch)?;
        if !self.stopped || !s.producing {
            return Err("source producer abort state".into());
        }
        self.slots[slot].producing = false;
        Ok(())
    }
    /// Seal the ordered triangle stream, independently of producer completion.
    pub fn seal(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
        self.fixture_access(slot)?;
        self.seal_inner(slot, epoch)
    }
    fn seal_inner(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
        self.slot(slot, epoch)?;
        if self.sealed[slot] {
            return Err("source stream already sealed".into());
        }
        self.sealed[slot] = true;
        Ok(())
    }
    /// Nonblocking admission: None means four pending descriptors already full.
    /// Active capture/snapshot occupies one additional geometry task position.
    pub fn submit(&mut self, task: Task) -> Result<Option<u64>, String> {
        self.fixture_access(task.slot)?;
        self.submit_inner(task)
    }
    fn submit_inner(&mut self, task: Task) -> Result<Option<u64>, String> {
        let s = self.slot(task.slot, task.epoch)?;
        if self.stopped
            || self.sealed[task.slot]
            || task.context != self.context
            || task
                .vertices
                .iter()
                .any(|&v| v >= 64 || !s.ready[v as usize])
        {
            return Err("source task context/stream/publication".into());
        }
        if self.queue.len() == TASK_CREDITS {
            return Ok(None);
        }
        if self.next_ticket >= self.max_tasks {
            return Err("source task bound".into());
        }
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.references[task.slot] += 1;
        self.queue.push_back((ticket, task));
        Ok(Some(ticket))
    }
    /// Global geometry cancellation: no new task/producer publication. Pending
    /// synchronous source return drains on an enabled edge; no success ACK.
    /// Producer must still finish, and streams must still be sealed for release.
    /// Already published raster/output work is outside this controller's scope.
    pub fn cancel(&mut self) {
        self.stopped = true;
    }
    /// One edge. `consumed` acknowledges the snapshot only after its final
    /// clip/fan use, not merely when the first fan record is published.
    pub fn step(&mut self, ce: bool, consumed: Option<u64>) -> Result<Vec<Event>, String> {
        if self.cycles == self.max_cycles {
            return Err("source capture watchdog".into());
        }
        self.cycles += 1;
        self.last_ce = ce;
        if !ce {
            return Ok(vec![]);
        }
        let mut events = Vec::new();
        if self.stopped {
            for (_, task) in self.queue.drain(..) {
                self.references[task.slot] -= 1;
            }
            if let Some(mut active) = self.active.take() {
                if let Some((row, data)) = active.pending.take() {
                    events.push(Event::ReadReturn {
                        ticket: active.snapshot.ticket,
                        source_row: row,
                        data,
                    });
                }
                self.references[active.snapshot.task.slot] -= 1;
            }
            self.ready = None;
            if !self.cancelled {
                events.push(Event::Cancelled);
                self.cancelled = true;
            }
        } else {
            if let Some(ticket) = consumed {
                if self.ready.as_ref().is_none_or(|s| s.ticket != ticket) {
                    return Err("stale or premature triangle consumed".into());
                }
                self.ready = None;
                events.push(Event::TriangleConsumed { ticket });
            }
            if self
                .active
                .as_ref()
                .is_some_and(|a| a.returned == SOURCE_ROWS)
            {
                // Validate while retaining ownership: a bad row must still be
                // cancellable/drainable rather than losing its reference count.
                self.active.as_ref().unwrap().snapshot.input()?;
                let active = self.active.take().unwrap();
                let snapshot = active.snapshot;
                self.references[snapshot.task.slot] -= 1;
                events.push(Event::SourceCaptured {
                    ticket: snapshot.ticket,
                    slot: snapshot.task.slot,
                    epoch: snapshot.task.epoch,
                });
                self.ready = Some(snapshot);
            }
            if self.active.is_none() && self.ready.is_none() {
                if let Some((ticket, task)) = self.queue.pop_front() {
                    self.active = Some(Capture {
                        snapshot: Snapshot {
                            ticket,
                            task,
                            rows: [[0; 7]; 3],
                        },
                        issued: 0,
                        returned: 0,
                        pending: None,
                    });
                }
            }
            if let Some(active) = &mut self.active {
                if let Some((row, data)) = active.pending.take() {
                    active.snapshot.rows[row / 7][row % 7] = data;
                    active.returned += 1;
                    events.push(Event::ReadReturn {
                        ticket: active.snapshot.ticket,
                        source_row: row,
                        data,
                    });
                }
                if active.issued < SOURCE_ROWS {
                    let at = active.issued;
                    let slot = active.snapshot.task.slot;
                    let row = active.snapshot.task.vertices[at / 7] as usize * 7 + at % 7;
                    active.pending = Some((at, self.slots[slot].rows[row]));
                    active.issued += 1;
                    events.push(Event::ReadIssue {
                        ticket: active.snapshot.ticket,
                        slot,
                        row,
                    });
                }
            }
        }
        for slot in 0..2 {
            let s = &mut self.slots[slot];
            if s.held && !s.producing && self.sealed[slot] && self.references[slot] == 0 {
                let epoch = s.epoch;
                s.release(epoch)?;
                self.checked[slot] = false;
                events.push(Event::SourceReleased { slot, epoch });
            }
        }
        Ok(events)
    }
}

impl SourceProducerEdge<'_> {
    fn effect(&self) -> Result<(), String> {
        if !self.permit.ce || self.used || self.source.stopped {
            return Err("source producer CE/action/stopped state".into());
        }
        Ok(())
    }
    fn owner(&self, slot: usize, epoch: u32) -> Result<&Producer, String> {
        self.effect()?;
        let p = self.source.producer.as_ref().ok_or("no checked producer")?;
        let old = self.permit.slots.get(slot).ok_or("source slot index")?;
        let s = self.source.slot(slot, epoch)?;
        if p.slot != slot
            || p.epoch != epoch
            || !old.held
            || !old.producing
            || old.epoch != epoch
            || !s.producing
            || !self.source.checked[slot]
        {
            return Err("checked producer owner/epoch".into());
        }
        Ok(p)
    }
    /// Task eligibility comes only from pre-clock publication and pending credit.
    pub fn submit_task(&mut self, task: Task) -> Result<Option<u64>, String> {
        if !self.permit.ce || self.source.stopped || self.task_used {
            return Err("source task CE/stopped/edge state".into());
        }
        let old = self
            .permit
            .slots
            .get(task.slot)
            .ok_or("source slot index")?;
        if !old.held
            || old.epoch != task.epoch
            || old.sealed
            || task.context != self.source.context
            || task
                .vertices
                .iter()
                .any(|&v| v >= 64 || !old.ready[usize::from(v)])
        {
            return Err("source task old publication/context/epoch".into());
        }
        self.task_used = true;
        if !self.permit.credit {
            return Ok(None);
        }
        let ticket = self.source.submit_inner(task)?;
        if let Some(ticket) = ticket {
            self.admitted = Some((ticket, task));
        }
        Ok(ticket)
    }
    pub fn seal_after_admission(
        &mut self,
        slot: usize,
        epoch: u32,
        ticket: u64,
    ) -> Result<(), String> {
        if !self.permit.ce
            || !self
                .admitted
                .is_some_and(|(t, task)| t == ticket && task.slot == slot && task.epoch == epoch)
        {
            return Err("seal requires actual same-edge task admission".into());
        }
        self.source.seal_inner(slot, epoch)?;
        self.admitted = None;
        Ok(())
    }
}
impl super::sim::runtime::ProducerPort for SourceProducerEdge<'_> {
    fn free_slot(&self) -> Option<usize> {
        if !self.permit.ce
            || self.source.stopped
            || self.source.producer.is_some()
            || self.source.slots.iter().any(|s| s.producing)
        {
            return None;
        }
        self.permit.slots.iter().position(|s| !s.held)
    }
    fn allocate(&mut self, slot: usize, count: usize) -> Result<u32, String> {
        self.effect()?;
        if !(1..=64).contains(&count)
            || self.source.producer.is_some()
            || self.source.slots.iter().any(|s| s.producing)
            || self.permit.slots.get(slot).is_none_or(|s| s.held)
        {
            return Err("checked allocation old slot/count/writer".into());
        }
        let epoch = self.source.allocate_inner(slot)?;
        self.source.checked[slot] = true;
        self.source.producer = Some(Producer {
            slot,
            epoch,
            count,
            next: 0,
            mask: 0,
            last_edge: self.source.cycles,
        });
        self.used = true;
        Ok(epoch)
    }
    fn write_row(
        &mut self,
        slot: usize,
        epoch: u32,
        vertex: usize,
        row: usize,
        data: u64,
    ) -> Result<(), String> {
        let p = self.owner(slot, epoch)?;
        if vertex != p.next
            || vertex >= p.count
            || row >= 7
            || p.mask & (1 << row) != 0
            || self.permit.slots[slot].ready[vertex]
            || self.source.slots[slot].ready[vertex]
            || data >> [32, 32, 32, 32, 36, 24, 16][row] != 0
        {
            return Err("checked row identity/duplicate/canonical width".into());
        }
        self.source.slots[slot].rows[vertex * 7 + row] = data;
        let p = self.source.producer.as_mut().unwrap();
        p.mask |= 1 << row;
        p.last_edge = self.source.cycles;
        self.used = true;
        Ok(())
    }
    fn publish(&mut self, slot: usize, epoch: u32, vertex: usize) -> Result<(), String> {
        let p = self.owner(slot, epoch)?;
        if vertex != p.next
            || vertex >= p.count
            || p.mask != 0x7f
            || p.last_edge >= self.source.cycles
        {
            return Err("checked publication missing rows/edge/vertex".into());
        }
        // Publication uses the same canonical transport contract as atomic input.
        let rows = std::array::from_fn(|row| self.source.slots[slot].rows[vertex * 7 + row]);
        Transformed::from_rows(rows)?.validate()?;
        self.source.slots[slot].ready[vertex] = true;
        let p = self.source.producer.as_mut().unwrap();
        p.next += 1;
        p.mask = 0;
        p.last_edge = self.source.cycles;
        self.used = true;
        Ok(())
    }
    fn finish(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
        let p = self.owner(slot, epoch)?;
        if p.next != p.count || p.mask != 0 || p.last_edge >= self.source.cycles {
            return Err("checked finish partial/edge".into());
        }
        self.source.slots[slot].finish_production(epoch)?;
        self.source.producer = None;
        self.used = true;
        Ok(())
    }
    fn release(&mut self, _slot: usize, _epoch: u32) -> Result<(), String> {
        Err("connected source release is owned by source clock".into())
    }
}
