//! Two-slot opaque triangle-record transport. No geometry arithmetic or row format.
//! Source capture is upstream; the snapshot lease remains upstream until LastUse.

pub const SLOTS: usize = 2;
pub const ROWS: usize = 64;
pub const MAX_FANS: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceOwner {
    pub ticket: u64,
    pub context: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub slot: usize,
    pub generation: u64,
    pub source: SourceOwner,
    pub fan: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    Coverage,
    Attribute,
}

#[derive(Clone, Copy, Debug)]
pub struct Reserve {
    pub source: SourceOwner,
    pub fan: u8,
    pub rows: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Write {
    pub key: Key,
    pub row: usize,
    pub word: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Read {
    pub key: Key,
    pub row: usize,
    pub consumer: Consumer,
    /// External attribute adapter seals further reads at its final quad capture.
    /// This transport cannot determine the actual last quad by itself.
    pub last_attribute_capture: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct SourceEnd {
    pub source: SourceOwner,
    /// Number of fans, including ones not yet admitted because both slots are full.
    pub fans: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub ce: bool,
    /// Notification of an already owned, fully captured upstream snapshot.
    pub source_captured: Option<SourceOwner>,
    pub reserve: Option<Reserve>,
    pub write: Option<Write>,
    pub read: Option<Read>,
    pub return_ready: bool,
    pub source_end: Option<SourceEnd>,
    pub last_quad_ack: Option<Key>,
    /// Fault-only acknowledgment that the consumer has cancelled all references.
    pub abort_ack: Option<Key>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub key: Key,
    pub row: usize,
    pub word: u64,
    pub consumer: Consumer,
    pub last_attribute_capture: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    SnapshotAccepted(SourceOwner),
    Reserved(Key),
    RowWritten {
        key: Key,
        row: usize,
    },
    Published(Key),
    ReadIssued {
        key: Key,
        row: usize,
    },
    ReturnCaptured(Response),
    ConsumerCaptured(Response),
    /// Maps to source_capture::step(consumed=ticket), not a record release.
    SnapshotLastUseAck(SourceOwner),
    /// Only follows the caller's last_quad_ack and final attribute transfer.
    RecordReleased(Key),
    CancelStarted,
    SnapshotAborted(SourceOwner),
    PartialWriteDiscarded(Key),
    ReturnDiscarded(Response),
    AbortAccepted(Key),
    RecordAborted(Key),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Bound,
    Context,
    SourceOrder,
    Owner,
    State,
    Row,
    EarlyAck,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub wall_edges: u64,
    pub sources: u64,
    pub records: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Free,
    Writing,
    Published,
}

#[derive(Clone, Copy)]
struct Slot {
    phase: Phase,
    key: Option<Key>,
    rows: usize,
    read_sealed: bool,
    final_captured: bool,
    abort_acked: bool,
}
impl Default for Slot {
    fn default() -> Self {
        Self {
            phase: Phase::Free,
            key: None,
            rows: 0,
            read_sealed: false,
            final_captured: false,
            abort_acked: false,
        }
    }
}
struct Source {
    owner: SourceOwner,
    next_fan: u8,
    end: Option<u8>,
}
struct Writer {
    key: Key,
    written: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReturnPhase {
    Pending,
    Captured,
}
struct Return {
    phase: ReturnPhase,
    response: Response,
}

pub struct Controller {
    words: [[u64; ROWS]; SLOTS],
    slots: [Slot; SLOTS],
    source: Option<Source>,
    writer: Option<Writer>,
    /// One logical return credit, shared by both consumers and both slots.
    returned: Option<Return>,
    context: u64,
    limits: Limits,
    wall_edges: u64,
    sources: u64,
    records: u64,
    last_ticket: Option<u64>,
    stopped: bool,
    cancel_announced: bool,
}

impl Controller {
    pub fn new(context: u64, limits: Limits) -> Result<Self, Fault> {
        if limits.wall_edges == 0 || limits.sources == 0 || limits.records == 0 {
            return Err(Fault::Bound);
        }
        Ok(Self {
            words: [[0; ROWS]; SLOTS],
            slots: [Slot::default(); SLOTS],
            source: None,
            writer: None,
            returned: None,
            context,
            limits,
            wall_edges: 0,
            sources: 0,
            records: 0,
            last_ticket: None,
            stopped: false,
            cancel_announced: false,
        })
    }
    pub fn free_slots(&self) -> usize {
        self.slots.iter().filter(|s| s.phase == Phase::Free).count()
    }
    pub fn phase(&self, slot: usize) -> Option<Phase> {
        self.slots.get(slot).map(|s| s.phase)
    }
    pub fn response(&self) -> Option<Response> {
        self.returned
            .as_ref()
            .filter(|r| r.phase == ReturnPhase::Captured)
            .map(|r| r.response)
    }
    pub fn drained(&self) -> bool {
        self.source.is_none()
            && self.writer.is_none()
            && self.returned.is_none()
            && self.free_slots() == SLOTS
    }
    /// Sideband cancellation intent. In-flight state changes only on enabled edges.
    pub fn cancel(&mut self) {
        self.stopped = true;
    }
    fn slot(&self, key: Key) -> Result<&Slot, Fault> {
        if key.source.context != self.context {
            return Err(Fault::Context);
        }
        let slot = self.slots.get(key.slot).ok_or(Fault::Owner)?;
        if slot.key != Some(key) || slot.phase == Phase::Free {
            return Err(Fault::Owner);
        }
        Ok(slot)
    }
    fn source(&self, owner: SourceOwner) -> Result<&Source, Fault> {
        if owner.context != self.context {
            return Err(Fault::Context);
        }
        let source = self.source.as_ref().ok_or(Fault::State)?;
        if source.owner != owner {
            return Err(Fault::Owner);
        }
        Ok(source)
    }
    fn will_transfer_final(&self, input: &Input, key: Key) -> bool {
        input.return_ready
            && self.returned.as_ref().is_some_and(|r| {
                r.phase == ReturnPhase::Captured
                    && r.response.key == key
                    && r.response.last_attribute_capture
            })
    }
    fn validate(&self, input: &Input) -> Result<(), Fault> {
        if input.abort_ack.is_some() {
            return Err(Fault::State);
        }
        if let Some(owner) = input.source_captured {
            if owner.context != self.context {
                return Err(Fault::Context);
            }
            if self.sources == self.limits.sources {
                return Err(Fault::Bound);
            }
            if self.last_ticket.is_some_and(|t| owner.ticket <= t) {
                return Err(Fault::SourceOrder);
            }
        }
        if let Some(r) = input.reserve {
            let source = self.source(r.source)?;
            if r.rows == 0 || r.rows > ROWS {
                return Err(Fault::Row);
            }
            if r.fan != source.next_fan
                || r.fan >= MAX_FANS
                || source.end.is_some_and(|end| r.fan >= end)
            {
                return Err(Fault::SourceOrder);
            }
            if self.records == self.limits.records {
                return Err(Fault::Bound);
            }
        }
        if let Some(w) = input.write {
            let slot = self.slot(w.key)?;
            let writer = self.writer.as_ref().ok_or(Fault::State)?;
            if writer.key != w.key || slot.phase != Phase::Writing {
                return Err(Fault::State);
            }
            if w.word >> 36 != 0 || w.row != writer.written || w.row >= slot.rows {
                return Err(Fault::Row);
            }
        }
        if let Some(r) = input.read {
            let slot = self.slot(r.key)?;
            if slot.phase != Phase::Published || slot.read_sealed {
                return Err(Fault::State);
            }
            if r.row >= slot.rows || r.last_attribute_capture && r.consumer != Consumer::Attribute {
                return Err(Fault::Row);
            }
        }
        if let Some(end) = input.source_end {
            let source = self.source(end.source)?;
            let reserved = u8::from(self.writer.is_some() || input.reserve.is_some());
            if source.end.is_some() || end.fans > MAX_FANS || end.fans < source.next_fan + reserved
            {
                return Err(Fault::SourceOrder);
            }
        }
        if let Some(key) = input.last_quad_ack {
            let slot = self.slot(key)?;
            let transferring = self.will_transfer_final(input, key);
            let outstanding = self
                .returned
                .as_ref()
                .is_some_and(|r| r.response.key == key && !transferring);
            if slot.phase != Phase::Published
                || !slot.final_captured && !transferring
                || outstanding
            {
                return Err(Fault::EarlyAck);
            }
        }
        Ok(())
    }
    pub fn audit(&self) -> Result<(), Fault> {
        let mut writers = 0;
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.phase == Phase::Free {
                if slot.key.is_some()
                    || slot.rows != 0
                    || slot.read_sealed
                    || slot.final_captured
                    || slot.abort_acked
                {
                    return Err(Fault::State);
                }
                continue;
            }
            let key = slot.key.ok_or(Fault::State)?;
            if key.slot != index
                || key.source.context != self.context
                || key.generation == 0
                || key.generation > self.records
                || slot.rows == 0
                || slot.rows > ROWS
            {
                return Err(Fault::State);
            }
            if slot.phase == Phase::Writing {
                writers += 1;
                if self
                    .writer
                    .as_ref()
                    .is_none_or(|w| w.key != key || w.written > slot.rows)
                {
                    return Err(Fault::State);
                }
                let source = self.source.as_ref().ok_or(Fault::State)?;
                if source.owner != key.source
                    || source.next_fan != key.fan
                    || source.end.is_some_and(|end| key.fan >= end)
                {
                    return Err(Fault::State);
                }
            }
            if slot.final_captured && !slot.read_sealed {
                return Err(Fault::State);
            }
        }
        if writers != usize::from(self.writer.is_some()) || writers > 1 {
            return Err(Fault::State);
        }
        if let Some(source) = &self.source {
            if source.next_fan > MAX_FANS || source.end.is_some_and(|end| source.next_fan > end) {
                return Err(Fault::State);
            }
        }
        if let Some(ret) = &self.returned {
            let slot = self.slot(ret.response.key)?;
            if slot.phase != Phase::Published
                || ret.response.row >= slot.rows
                || ret.response.last_attribute_capture
                    && (!slot.read_sealed || ret.response.consumer != Consumer::Attribute)
            {
                return Err(Fault::State);
            }
        }
        Ok(())
    }
    pub fn step(&mut self, input: Input) -> Result<Vec<Event>, Fault> {
        if self.wall_edges == self.limits.wall_edges {
            return Err(Fault::Bound);
        }
        self.wall_edges += 1;
        if !input.ce {
            return Ok(Vec::new());
        }
        if self.stopped {
            return self.drain_cancel(input.abort_ack);
        }
        self.validate(&input)?;
        // Eligibility uses pre-edge state: ACK does not free a same-edge writer,
        // capture does not create a same-edge consumer, and publish is separate W.
        let free_before = self.slots.iter().position(|s| s.phase == Phase::Free);
        let can_reserve = self.writer.is_none();
        let can_accept_source = self.source.is_none();
        let can_issue = self.returned.is_none()
            || self
                .returned
                .as_ref()
                .is_some_and(|r| r.phase == ReturnPhase::Captured && input.return_ready);
        let publish = self
            .writer
            .as_ref()
            .filter(|w| w.written == self.slots[w.key.slot].rows)
            .map(|w| w.key);
        let mut events = Vec::new();
        if let Some(mut returned) = self.returned.take() {
            if returned.phase == ReturnPhase::Pending {
                returned.phase = ReturnPhase::Captured;
                events.push(Event::ReturnCaptured(returned.response));
                self.returned = Some(returned);
            } else if input.return_ready {
                if returned.response.last_attribute_capture {
                    self.slots[returned.response.key.slot].final_captured = true;
                }
                events.push(Event::ConsumerCaptured(returned.response));
            } else {
                self.returned = Some(returned);
            }
        }
        if let Some(key) = input.last_quad_ack {
            self.slots[key.slot] = Slot::default();
            events.push(Event::RecordReleased(key));
        }
        if let Some(key) = publish {
            self.slots[key.slot].phase = Phase::Published;
            self.writer = None;
            self.source.as_mut().unwrap().next_fan += 1;
            events.push(Event::Published(key));
        }
        if let Some(w) = input.write {
            self.words[w.key.slot][w.row] = w.word;
            self.writer.as_mut().unwrap().written += 1;
            events.push(Event::RowWritten {
                key: w.key,
                row: w.row,
            });
        }
        if can_reserve {
            if let (Some(r), Some(slot)) = (input.reserve, free_before) {
                self.records += 1;
                let key = Key {
                    slot,
                    generation: self.records,
                    source: r.source,
                    fan: r.fan,
                };
                self.slots[slot] = Slot {
                    phase: Phase::Writing,
                    key: Some(key),
                    rows: r.rows,
                    ..Slot::default()
                };
                self.writer = Some(Writer { key, written: 0 });
                events.push(Event::Reserved(key));
            }
        }
        if can_issue {
            if let Some(r) = input.read {
                let response = Response {
                    key: r.key,
                    row: r.row,
                    word: self.words[r.key.slot][r.row],
                    consumer: r.consumer,
                    last_attribute_capture: r.last_attribute_capture,
                };
                self.returned = Some(Return {
                    phase: ReturnPhase::Pending,
                    response,
                });
                if r.last_attribute_capture {
                    self.slots[r.key.slot].read_sealed = true;
                }
                events.push(Event::ReadIssued {
                    key: r.key,
                    row: r.row,
                });
            }
        }
        if can_accept_source {
            if let Some(owner) = input.source_captured {
                self.source = Some(Source {
                    owner,
                    next_fan: 0,
                    end: None,
                });
                self.sources += 1;
                self.last_ticket = Some(owner.ticket);
                events.push(Event::SnapshotAccepted(owner));
            }
        }
        if let Some(end) = input.source_end {
            self.source.as_mut().unwrap().end = Some(end.fans);
        }
        if self
            .source
            .as_ref()
            .is_some_and(|s| s.end == Some(s.next_fan))
            && self.writer.is_none()
        {
            events.push(Event::SnapshotLastUseAck(self.source.take().unwrap().owner));
        }
        self.audit()?;
        Ok(events)
    }
    fn drain_cancel(&mut self, ack: Option<Key>) -> Result<Vec<Event>, Fault> {
        if let Some(key) = ack {
            let slot = self.slot(key)?;
            if slot.phase != Phase::Published || slot.abort_acked {
                return Err(Fault::State);
            }
        }
        let mut events = Vec::new();
        if !self.cancel_announced {
            self.cancel_announced = true;
            events.push(Event::CancelStarted);
        }
        if let Some(writer) = self.writer.take() {
            self.slots[writer.key.slot] = Slot::default();
            events.push(Event::PartialWriteDiscarded(writer.key));
        }
        if let Some(source) = self.source.take() {
            events.push(Event::SnapshotAborted(source.owner));
        }
        if let Some(mut returned) = self.returned.take() {
            if returned.phase == ReturnPhase::Pending {
                returned.phase = ReturnPhase::Captured;
                events.push(Event::ReturnCaptured(returned.response));
                self.returned = Some(returned);
            } else {
                events.push(Event::ReturnDiscarded(returned.response));
            }
        }
        if let Some(key) = ack {
            self.slots[key.slot].abort_acked = true;
            events.push(Event::AbortAccepted(key));
        }
        for slot in 0..SLOTS {
            if self.slots[slot].phase == Phase::Published
                && self.slots[slot].abort_acked
                && self
                    .returned
                    .as_ref()
                    .is_none_or(|r| r.response.key.slot != slot)
            {
                let key = self.slots[slot].key.unwrap();
                self.slots[slot] = Slot::default();
                events.push(Event::RecordAborted(key));
            }
        }
        self.audit()?;
        Ok(events)
    }
}
