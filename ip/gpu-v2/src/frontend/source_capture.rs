//! Bounded source lifetime/control model. Does not execute vertex or setup math.
use super::ports::Slot;
use crate::{triangle, vertex::ports::Transformed};
use std::collections::VecDeque;

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
        })
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
        self.slot(slot, epoch)?;
        self.slots[slot].finish_production(epoch)
    }
    /// Fault-only producer acknowledgment, after its accepted row writes have
    /// drained. Unlike success completion this permits zero published vertices.
    pub fn abort_production(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
        let s = self.slot(slot, epoch)?;
        if !self.stopped || !s.producing {
            return Err("source producer abort state".into());
        }
        self.slots[slot].producing = false;
        Ok(())
    }
    /// Seal the ordered triangle stream, independently of producer completion.
    pub fn seal(&mut self, slot: usize, epoch: u32) -> Result<(), String> {
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
                events.push(Event::SourceReleased { slot, epoch });
            }
        }
        Ok(events)
    }
}
