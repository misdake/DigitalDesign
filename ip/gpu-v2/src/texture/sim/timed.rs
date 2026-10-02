//! Bounded cache/color cycle execution with offline preparation reservations.
//! The preparation calendar is a conservative primitive baseline, not II=2 RTL.
use super::super::ports::*;
use super::{counted, oracle::State};
use audited::physical::{DspInventory, DspMode};
use std::{collections::VecDeque, sync::Arc};
mod audit;
mod filter;
mod schedule;
pub use schedule::ArithmeticPlan;

#[derive(Debug)]
pub struct Error(pub String);
impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}
impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}
impl From<counted::Error> for Error {
    fn from(e: counted::Error) -> Self {
        Self(format!("counted preparation: {e:?}"))
    }
}

#[derive(Clone, Debug)]
pub struct Hardware {
    /// PreparedGroups isolates cache/color throughput; Reserved includes the
    /// conservative counted primitive calendar. Neither claims optimized RTL.
    pub preparation: PreparationMode,
    pub group_capacity: usize,
    pub quad_capacity: usize,
    pub hint_capacity: usize,
    pub descriptor_capacity: usize,
    pub result_capacity: usize,
    pub prefetch: bool,
    pub coefficient_lanes: usize,
    pub logic_lanes_per_width: usize,
    pub multiply_latency: u64,
    pub read_latency: u64,
    pub max_arithmetic_cycles: u64,
    /// Conservative value pool; even constants and wiring are counted here.
    pub preparation_register_bits: usize,
    pub max_cycles: u64,
    pub max_quads: usize,
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            preparation: PreparationMode::Reserved,
            group_capacity: 32,
            quad_capacity: 4,
            hint_capacity: 8,
            descriptor_capacity: 4,
            result_capacity: 16,
            prefetch: true,
            coefficient_lanes: 3,
            logic_lanes_per_width: 1,
            multiply_latency: 3,
            read_latency: 1,
            max_arithmetic_cycles: 20000,
            preparation_register_bits: 16384,
            max_cycles: 2_000_000,
            max_quads: 256,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparationMode {
    Reserved,
    PreparedGroups,
}
impl Hardware {
    pub fn validate(&self) -> Result<(), Error> {
        if ![16, 32].contains(&self.group_capacity)
            || !(1..=8).contains(&self.quad_capacity)
            || !(1..=16).contains(&self.hint_capacity)
            || !(1..=4).contains(&self.descriptor_capacity)
            || !(1..=16).contains(&self.result_capacity)
            || !(1..=4).contains(&self.coefficient_lanes)
            || !(1..=8).contains(&self.logic_lanes_per_width)
            || !(1..=8).contains(&self.multiply_latency)
            || !(1..=4).contains(&self.read_latency)
            || self.max_arithmetic_cycles == 0
            || self.max_arithmetic_cycles > 100000
            || self.preparation_register_bits == 0
            || self.preparation_register_bits > 65536
            || self.max_cycles == 0
            || self.max_cycles > 2_000_000
            || self.max_quads == 0
            || self.max_quads > 256
        {
            return Err("timed hardware/budget bounds".into());
        }
        Ok(())
    }
    pub fn inventory(&self) -> Result<DspInventory, String> {
        let lanes = 4 + 12; // Dedicated preparation macro; three color macros.
        DspInventory::pack(2, &[(DspMode::Multiply9, lanes, self.multiply_latency, 1)])
            .map_err(|e| format!("9x8 target packing: {e:?}"))
    }
    fn partial_at(&self) -> u64 {
        self.read_latency + self.multiply_latency + 2
    }
    fn accumulator_at(&self) -> u64 {
        self.partial_at() + 1
    }
    pub fn color_latency(&self) -> u64 {
        self.accumulator_at() + 2
    }
}

pub struct Program {
    input: QuadInput,
    preparation: counted::Preparation,
    arithmetic: ArithmeticPlan,
    slots: Vec<Slot>,
}
impl Program {
    pub fn input(&self) -> &QuadInput {
        &self.input
    }
    pub fn preparation(&self) -> &counted::Preparation {
        &self.preparation
    }
    pub fn arithmetic(&self) -> &ArithmeticPlan {
        &self.arithmetic
    }
    fn audit(&self, slots: &[Slot], hardware: &Hardware) -> Result<(), Error> {
        if self.slots != slots {
            return Err("program belongs to different slot bindings".into());
        }
        self.arithmetic.audit(&self.preparation.frame, hardware)?;
        Ok(())
    }
    pub fn compile(
        input: &QuadInput,
        slots: &[Slot],
        hardware: &Hardware,
    ) -> Result<Arc<Self>, Error> {
        hardware.validate()?;
        let preparation = counted::prepare(input, slots)?;
        let arithmetic = schedule::plan(&preparation, hardware)?;
        if arithmetic.group_ready.len() != preparation.groups.len() {
            return Err("Group4 calendar length".into());
        }
        Ok(Arc::new(Self {
            input: input.clone(),
            preparation,
            arithmetic,
            slots: slots.to_vec(),
        }))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Control {
    pub ce: bool,
    pub result_ready: bool,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            ce: true,
            result_ready: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelResult {
    pub quad_id: u8,
    pub lane: u8,
    pub rgb: [u8; 3],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Accepted {
        quad: u8,
        mask: u8,
    },
    Produced {
        group: Group4,
        payload: i128,
    },
    HintDropped {
        key: TileKey,
    },
    HintMerged {
        key: TileKey,
    },
    Allocate {
        key: TileKey,
        line: usize,
        address: u64,
        prefetch: bool,
    },
    Promote {
        key: TileKey,
    },
    Submitted {
        id: u64,
        key: TileKey,
        line: usize,
        address: u64,
    },
    Beat {
        line: usize,
        index: usize,
        data: u64,
    },
    Ready {
        key: TileKey,
        line: usize,
    },
    PrefetchHit {
        key: TileKey,
        line: usize,
    },
    Read {
        group: Group4,
        payload: i128,
        line: usize,
        refill_overlap: bool,
    },
    Captured {
        group: Group4,
        line: usize,
        words: [u16; 4],
    },
    Partial {
        group: Group4,
        value: [u32; 3],
    },
    Accumulate {
        group: Group4,
        value: [u32; 3],
    },
    Result {
        pixel: PixelResult,
    },
    Commit {
        pixel: PixelResult,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub wall_cycles: u64,
    pub enabled_cycles: u64,
    pub producer_cycles: u64,
    pub producer_stalls: u64,
    pub demand_wait_cycles: u64,
    pub result_credit_stalls: u64,
    pub input_stalls: u64,
    pub reads: u64,
    pub hits_during_refill: u64,
    pub allocations: u64,
    pub refills: u64,
    pub beats: u64,
    pub promotions: u64,
    pub hints_dropped: u64,
    pub hints_merged: u64,
    pub prefetch_hits: u64,
    pub committed: u64,
    pub peak_groups: usize,
    pub peak_hints: usize,
    pub peak_descriptors: usize,
    pub peak_results: usize,
    pub peak_pipeline: usize,
    pub peak_quads: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub groups: usize,
    pub hints: usize,
    pub descriptors: usize,
    pub quads: usize,
    pub pipeline: usize,
    pub results: usize,
    pub result_credits: usize,
    pub reservations: u64,
    pub live_quads: u16,
    pub producer_clock: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub cycle: u64,
    pub control: Control,
    pub offered: Option<usize>,
    pub accepted: bool,
    pub responses: Vec<RefillEvent>,
    pub events: Vec<Event>,
    pub snapshot: Snapshot,
}
#[derive(Clone, Copy)]
struct Line {
    state: State,
    key: Option<TileKey>,
}
#[derive(Clone)]
struct Descriptor {
    key: TileKey,
    line: usize,
    address: u64,
}
struct Active {
    descriptor: Descriptor,
    id: u64,
    started: bool,
    next: usize,
}
struct Producer {
    program: Arc<Program>,
    clock: u64,
    next: usize,
}
struct Token {
    group: Group4,
    payload: i128,
    line: usize,
    age: u64,
    words: Option<[u16; 4]>,
    partial: Option<[u32; 3]>,
    sum: Option<[u32; 3]>,
}
struct Packet {
    group: Group4,
    payload: i128,
}

pub struct Machine {
    slots: Vec<Slot>,
    hardware: Hardware,
    lines: [Line; 64],
    banks: [[u16; 1024]; 4],
    plru: [u8; 16],
    quads: VecDeque<Arc<Program>>,
    producer: Option<Producer>,
    groups: VecDeque<Packet>,
    hints: VecDeque<TileKey>,
    pending: VecDeque<Descriptor>,
    active: Option<Active>,
    tokens: VecDeque<Token>,
    results: VecDeque<PixelResult>,
    credits: usize,
    reservations: u64,
    live: u16,
    remaining: [u8; 16],
    produced: [bool; 16],
    accumulator: Option<((u8, u8), [u32; 3])>,
    faulted: bool,
    pub stats: Stats,
}
impl Machine {
    pub fn new(slots: Vec<Slot>, hardware: Hardware) -> Result<Self, Error> {
        hardware.validate()?;
        if slots.is_empty() || slots.len() > 16 {
            return Err("slot count".into());
        }
        for s in &slots {
            if s.valid {
                s.validate()?;
            }
        }
        Ok(Self {
            slots,
            hardware,
            lines: [Line {
                state: State::Invalid,
                key: None,
            }; 64],
            banks: [[0; 1024]; 4],
            plru: [0; 16],
            quads: VecDeque::new(),
            producer: None,
            groups: VecDeque::new(),
            hints: VecDeque::new(),
            pending: VecDeque::new(),
            active: None,
            tokens: VecDeque::new(),
            results: VecDeque::new(),
            credits: 0,
            reservations: 0,
            live: 0,
            remaining: [0; 16],
            produced: [false; 16],
            accumulator: None,
            faulted: false,
            stats: Stats::default(),
        })
    }
    pub fn idle(&self) -> bool {
        self.quads.is_empty()
            && self.producer.is_none()
            && self.groups.is_empty()
            && self.hints.is_empty()
            && self.pending.is_empty()
            && self.active.is_none()
            && self.tokens.is_empty()
            && self.results.is_empty()
            && self.live == 0
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    pub fn lookup(&self, key: TileKey) -> Option<(usize, State)> {
        (key.set() * 4..key.set() * 4 + 4)
            .find(|&i| self.lines[i].state != State::Invalid && self.lines[i].key == Some(key))
            .map(|i| (i, self.lines[i].state))
    }
    pub fn rebind(&mut self, slots: Vec<Slot>) -> Result<(), Error> {
        if self.faulted || !self.idle() {
            return Err("rebind requires a healthy drained sampler".into());
        }
        let next = Self::new(slots, self.hardware.clone())?;
        self.slots = next.slots;
        self.lines.fill(Line {
            state: State::Invalid,
            key: None,
        });
        Ok(())
    }
    fn touch(&mut self, line: usize) {
        let way = line % 4;
        let bits = &mut self.plru[line / 4];
        if way < 2 {
            *bits = (*bits & !3) | 1 | (((way ^ 1) as u8) << 1);
        } else {
            *bits = (*bits & !5) | (((way ^ 1) as u8 & 1) << 2);
        }
    }
    fn victim(&self, key: TileKey, protected: Option<TileKey>) -> Option<usize> {
        let set = key.set();
        let usable = |way: usize| {
            let i = set * 4 + way;
            self.reservations >> i & 1 == 0
                && self.lines[i].state != State::Filling
                && (protected.is_none() || self.lines[i].key != protected)
        };
        if let Some(w) =
            (0..4).find(|&w| self.lines[set * 4 + w].state == State::Invalid && usable(w))
        {
            return Some(set * 4 + w);
        }
        let b = self.plru[set];
        let root = usize::from(b & 1);
        let left = usize::from(b >> 1 & 1);
        let right = usize::from(b >> 2 & 1);
        let halves = [[left, left ^ 1], [2 + right, 2 + (right ^ 1)]];
        [
            halves[root][0],
            halves[root][1],
            halves[root ^ 1][0],
            halves[root ^ 1][1],
        ]
        .into_iter()
        .find(|&w| self.lines[set * 4 + w].state == State::Ready && usable(w))
        .map(|w| set * 4 + w)
    }
    fn descriptors(&self) -> usize {
        self.pending.len() + usize::from(self.active.is_some())
    }
    fn allocate(
        &mut self,
        key: TileKey,
        prefetch: bool,
        protected: Option<TileKey>,
        events: &mut Vec<Event>,
    ) -> Result<bool, Error> {
        if self.descriptors() >= self.hardware.descriptor_capacity {
            return Ok(false);
        }
        let Some(line) = self.victim(key, protected) else {
            return Ok(false);
        };
        let address = key.address(&self.slots)?;
        self.lines[line] = Line {
            state: State::Filling,
            key: Some(key),
        };
        self.pending.push_back(Descriptor { key, line, address });
        self.stats.allocations += 1;
        events.push(Event::Allocate {
            key,
            line,
            address,
            prefetch,
        });
        Ok(true)
    }
    fn release_quad(&mut self, id: u8) {
        let i = usize::from(id);
        if self.produced[i] && self.remaining[i] == 0 {
            self.live &= !(1 << i);
        }
    }
    fn responses(
        &mut self,
        responses: &[RefillEvent],
        events: &mut Vec<Event>,
    ) -> Result<Option<usize>, Error> {
        let mut written = None;
        for response in responses {
            let active = self.active.as_mut().ok_or("unsolicited refill event")?;
            match *response {
                RefillEvent::Started { id } => {
                    if id != active.id || active.started {
                        return Err("refill start identity/order".into());
                    }
                    active.started = true;
                }
                RefillEvent::Beat {
                    id,
                    index,
                    data,
                    last,
                } => {
                    if id != active.id
                        || !active.started
                        || index != active.next
                        || index >= 16
                        || last != (index == 15)
                        || written.is_some()
                    {
                        return Err("refill beat identity/order/port budget".into());
                    }
                    let line = active.descriptor.line;
                    for j in 0..4 {
                        let linear = index * 4 + j;
                        let x = linear % 8;
                        let y = linear / 8;
                        let bank = ((y & 1) ^ ((x >> 1) & 1)) * 2 + (x & 1);
                        let local = (y & 7) * 2 + ((x >> 2) & 1);
                        self.banks[bank][line * 16 + local] = (data >> (j * 16)) as u16;
                    }
                    active.next += 1;
                    self.stats.beats += 1;
                    written = Some(line);
                    events.push(Event::Beat { line, index, data });
                }
                RefillEvent::Complete { id } => {
                    if id != active.id || !active.started || active.next != 16 {
                        return Err("refill completion without sixteen beats".into());
                    }
                    let descriptor = active.descriptor.clone();
                    self.lines[descriptor.line].state = State::Ready;
                    self.active = None;
                    self.touch(descriptor.line);
                    self.stats.refills += 1;
                    events.push(Event::Ready {
                        key: descriptor.key,
                        line: descriptor.line,
                    });
                }
            }
        }
        Ok(written)
    }
    fn pipeline(&mut self, events: &mut Vec<Event>) -> Result<(), Error> {
        let mut next = VecDeque::new();
        while let Some(mut token) = self.tokens.pop_front() {
            token.age += 1;
            if token.age == self.hardware.read_latency {
                if self.lines[token.line].state != State::Ready
                    || self.lines[token.line].key != Some(token.group.key)
                {
                    return Err("reserved read replaced before capture".into());
                }
                let words = std::array::from_fn(|t| {
                    let x = usize::from(token.group.top_left_local[0]) + (t & 1);
                    let y = usize::from(token.group.top_left_local[1]) + (t >> 1);
                    let bank = ((y & 1) ^ ((x >> 1) & 1)) * 2 + (x & 1);
                    let local = (y & 7) * 2 + ((x >> 2) & 1);
                    self.banks[bank][token.line * 16 + local]
                });
                token.words = Some(words);
                self.reservations &= !(1 << token.line);
                events.push(Event::Captured {
                    group: token.group.clone(),
                    line: token.line,
                    words,
                });
            }
            if token.age == self.hardware.partial_at() {
                let value =
                    filter::partial(token.payload, token.words.ok_or("missing bank data")?)?;
                token.partial = Some(value);
                events.push(Event::Partial {
                    group: token.group.clone(),
                    value,
                });
            }
            if token.age == self.hardware.accumulator_at() {
                let identity = (token.group.quad_id, token.group.lane);
                let previous = if token.group.first {
                    if self.accumulator.is_some() {
                        return Err("first interrupted prior sample".into());
                    }
                    [0; 3]
                } else {
                    let (owner, value) = self.accumulator.ok_or("non-first without accumulator")?;
                    if owner != identity {
                        return Err("sample group ordering".into());
                    }
                    value
                };
                let value = filter::accumulate(
                    previous,
                    token.partial.ok_or("missing partial")?,
                    token.payload,
                )?;
                token.sum = Some(value);
                events.push(Event::Accumulate {
                    group: token.group.clone(),
                    value,
                });
                self.accumulator = if token.group.last {
                    None
                } else {
                    Some((identity, value))
                };
                if !token.group.last {
                    continue;
                }
            }
            if token.age == self.hardware.color_latency() {
                let pixel = PixelResult {
                    quad_id: token.group.quad_id,
                    lane: token.group.lane,
                    rgb: filter::normalize(token.sum.ok_or("missing final sum")?)?,
                };
                if self.results.len() >= self.hardware.result_capacity {
                    return Err("reserved output capacity violated".into());
                }
                self.results.push_back(pixel.clone());
                events.push(Event::Result { pixel });
            } else {
                next.push_back(token);
            }
        }
        self.tokens = next;
        Ok(())
    }
    fn produce(&mut self, events: &mut Vec<Event>) -> Result<(), Error> {
        if self.producer.is_none() {
            if let Some(program) = self.quads.pop_front() {
                self.producer = Some(Producer {
                    program,
                    clock: 0,
                    next: 0,
                });
            }
        }
        let Some(p) = self.producer.as_mut() else {
            return Ok(());
        };
        let prepared = self.hardware.preparation == PreparationMode::PreparedGroups;
        let due = p.next < p.program.preparation.groups.len()
            && p.clock
                >= if prepared {
                    p.next as u64
                } else {
                    p.program.arithmetic.group_ready[p.next]
                };
        if due && self.groups.len() == self.hardware.group_capacity {
            self.stats.producer_stalls += 1;
            return Ok(());
        }
        if due {
            let group = p.program.preparation.groups[p.next].clone();
            let payload = p.program.preparation.payload(p.next);
            p.next += 1;
            if self.hardware.prefetch {
                if self.hints.iter().rev().take(3).any(|&k| k == group.key) {
                    self.stats.hints_merged += 1;
                    events.push(Event::HintMerged { key: group.key });
                } else if self.hints.len() == self.hardware.hint_capacity {
                    self.stats.hints_dropped += 1;
                    events.push(Event::HintDropped { key: group.key });
                } else {
                    self.hints.push_back(group.key);
                }
            }
            self.groups.push_back(Packet {
                group: group.clone(),
                payload,
            });
            events.push(Event::Produced { group, payload });
        }
        p.clock += 1;
        self.stats.producer_cycles += 1;
        if p.next == p.program.preparation.groups.len()
            && (prepared || p.clock > p.program.arithmetic.schedule.cycles)
        {
            let id = p.program.input.quad_id;
            self.produced[usize::from(id)] = true;
            self.producer = None;
            self.release_quad(id);
        }
        Ok(())
    }
    fn tags(
        &mut self,
        written: Option<usize>,
        refill_overlap: bool,
        events: &mut Vec<Event>,
    ) -> Result<(), Error> {
        let head = self.groups.front().map(|p| p.group.clone());
        let mut allocated = false;
        if let Some(g) = &head {
            match self.lookup(g.key) {
                None => {
                    allocated = self.allocate(g.key, false, None, events)?;
                    self.stats.demand_wait_cycles += 1;
                }
                Some((_, State::Filling)) => {
                    self.stats.demand_wait_cycles += 1;
                    if let Some(i) = self.pending.iter().position(|d| d.key == g.key) {
                        if i != 0 {
                            let d = self.pending.remove(i).unwrap();
                            self.pending.push_front(d);
                            self.stats.promotions += 1;
                            events.push(Event::Promote { key: g.key });
                        }
                    }
                }
                _ => {}
            }
        }
        // Prefetch sees any demand allocation at the single directory commit point.
        if let Some(&key) = self.hints.front() {
            match self.lookup(key) {
                Some((line, State::Ready)) => {
                    self.hints.pop_front();
                    self.touch(line);
                    self.stats.prefetch_hits += 1;
                    events.push(Event::PrefetchHit { key, line });
                }
                Some((_, State::Filling)) => {
                    self.hints.pop_front();
                    self.stats.hints_merged += 1;
                    events.push(Event::HintMerged { key });
                }
                None if !allocated
                    && self.allocate(key, true, head.as_ref().map(|g| g.key), events)? =>
                {
                    self.hints.pop_front();
                }
                _ => {}
            }
        }
        if let Some(g) = head {
            if let Some((line, State::Ready)) = self.lookup(g.key) {
                if written == Some(line) {
                    self.stats.demand_wait_cycles += 1;
                    return Ok(());
                }
                if g.last && self.credits == self.hardware.result_capacity {
                    self.stats.result_credit_stalls += 1;
                    return Ok(());
                }
                if self.reservations >> line & 1 != 0 {
                    return Ok(());
                }
                let packet = self.groups.pop_front().unwrap();
                self.reservations |= 1 << line;
                if g.last {
                    self.credits += 1;
                }
                self.touch(line);
                self.stats.reads += 1;
                if refill_overlap {
                    self.stats.hits_during_refill += 1;
                }
                self.tokens.push_back(Token {
                    group: g.clone(),
                    payload: packet.payload,
                    line,
                    age: 0,
                    words: None,
                    partial: None,
                    sum: None,
                });
                events.push(Event::Read {
                    group: g,
                    payload: packet.payload,
                    line,
                    refill_overlap,
                });
            }
        }
        Ok(())
    }
    pub fn step<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        offered: Option<(usize, Arc<Program>)>,
        control: Control,
    ) -> Result<Step, Error> {
        if self.faulted {
            return Err(
                "terminal sampler fault: drain accepted memory work externally before recreation"
                    .into(),
            );
        }
        let result = self.step_inner(memory, offered, control);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn step_inner<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
        offered: Option<(usize, Arc<Program>)>,
        control: Control,
    ) -> Result<Step, Error> {
        if self.stats.wall_cycles >= self.hardware.max_cycles {
            return Err("texture wall-cycle watchdog".into());
        }
        self.stats.wall_cycles += 1;
        let mut events = Vec::new();
        let overlap = self.active.is_some();
        let responses = memory.step()?;
        let written = self.responses(&responses, &mut events)?;
        let mut accepted = false;
        let offer_index = offered.as_ref().map(|(i, _)| *i);
        if control.ce {
            self.stats.enabled_cycles += 1;
            if control.result_ready {
                if let Some(pixel) = self.results.pop_front() {
                    self.credits -= 1;
                    let id = usize::from(pixel.quad_id);
                    let bit = 1 << pixel.lane;
                    if self.remaining[id] & bit == 0 {
                        return Err("duplicate/unowned lane commit".into());
                    }
                    self.remaining[id] &= !bit;
                    self.release_quad(pixel.quad_id);
                    self.stats.committed += 1;
                    events.push(Event::Commit { pixel });
                }
            }
            self.pipeline(&mut events)?;
            self.tags(written, overlap || written.is_some(), &mut events)?;
            self.produce(&mut events)?;
            if let Some((_, program)) = offered {
                let id = program.input.quad_id;
                if self.quads.len() < self.hardware.quad_capacity && self.live >> id & 1 == 0 {
                    program.audit(&self.slots, &self.hardware)?;
                    self.live |= 1 << id;
                    self.remaining[usize::from(id)] = program.input.mask;
                    self.produced[usize::from(id)] = false;
                    events.push(Event::Accepted {
                        quad: id,
                        mask: program.input.mask,
                    });
                    self.quads.push_back(program);
                    accepted = true;
                } else {
                    self.stats.input_stalls += 1;
                }
            }
        }
        // Accepted response beats and directory commits continue under consumer CE=0.
        if self.active.is_none() {
            if let Some(descriptor) = self.pending.pop_front() {
                let id = memory.submit_read(descriptor.address, 128)?;
                events.push(Event::Submitted {
                    id,
                    key: descriptor.key,
                    line: descriptor.line,
                    address: descriptor.address,
                });
                self.active = Some(Active {
                    descriptor,
                    id,
                    started: false,
                    next: 0,
                });
            }
        }
        let snapshot = self.snapshot();
        self.stats.peak_groups = self.stats.peak_groups.max(snapshot.groups);
        self.stats.peak_hints = self.stats.peak_hints.max(snapshot.hints);
        self.stats.peak_descriptors = self.stats.peak_descriptors.max(snapshot.descriptors);
        self.stats.peak_results = self.stats.peak_results.max(snapshot.results);
        self.stats.peak_pipeline = self.stats.peak_pipeline.max(snapshot.pipeline);
        self.stats.peak_quads = self.stats.peak_quads.max(snapshot.quads);
        Ok(Step {
            cycle: self.stats.wall_cycles,
            control,
            offered: offer_index,
            accepted,
            responses,
            events,
            snapshot,
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            groups: self.groups.len(),
            hints: self.hints.len(),
            descriptors: self.descriptors(),
            quads: self.quads.len(),
            pipeline: self.tokens.len(),
            results: self.results.len(),
            result_credits: self.credits,
            reservations: self.reservations,
            live_quads: self.live,
            producer_clock: self.producer.as_ref().map(|p| p.clock),
        }
    }
}
pub struct Report {
    pub hardware: Hardware,
    pub slots: Vec<Slot>,
    pub programs: Vec<Arc<Program>>,
    pub steps: Vec<Step>,
    pub pixels: Vec<PixelResult>,
    pub stats: Stats,
}
impl Report {
    pub fn audit(&self) -> Result<(), Error> {
        audit::audit(self)
    }
}
pub fn run<M: RefillPort + ?Sized>(
    inputs: &[QuadInput],
    slots: &[Slot],
    memory: &mut M,
    hardware: Hardware,
    mut control: impl FnMut(u64) -> Control,
) -> Result<Report, Error> {
    hardware.validate()?;
    if inputs.len() > hardware.max_quads {
        return Err("quad budget".into());
    }
    let programs: Vec<_> = inputs
        .iter()
        .map(|q| Program::compile(q, slots, &hardware))
        .collect::<Result<_, _>>()?;
    let mut machine = Machine::new(slots.to_vec(), hardware.clone())?;
    let mut next = 0;
    let mut steps = Vec::new();
    let mut pixels = Vec::new();
    while next < programs.len() || !machine.idle() {
        let offered = programs.get(next).map(|p| (next, p.clone()));
        let step = machine.step(memory, offered, control(machine.stats.wall_cycles + 1))?;
        if step.accepted {
            next += 1;
        }
        for e in &step.events {
            if let Event::Commit { pixel } = e {
                pixels.push(pixel.clone());
            }
        }
        steps.push(step);
    }
    let report = Report {
        hardware,
        slots: slots.to_vec(),
        programs,
        steps,
        pixels,
        stats: machine.stats,
    };
    report.audit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_demand_promotion_preserves_active_burst_and_merges_dual_lookup() {
        let slot = Slot {
            base_address: 0x1000,
            max_size_log2: 5,
            has_full_mip: false,
            valid: true,
        };
        let mut machine = Machine::new(vec![slot], Hardware::default()).unwrap();
        let keys: Vec<_> = (0..4)
            .map(|x| TileKey {
                slot: 0,
                n: 5,
                x,
                y: 0,
            })
            .collect();
        let mut events = vec![];
        for &key in &keys {
            assert!(machine.allocate(key, true, None, &mut events).unwrap());
        }
        machine.active = Some(Active {
            descriptor: machine.pending.pop_front().unwrap(),
            id: 91,
            started: true,
            next: 3,
        });
        let q = QuadInput {
            quad_id: 0,
            slot: 0,
            material_size_log2: 5,
            filter: Filter::Nearest,
            uv: [[0.51, 0.01]; 4],
            lod_bias: 0.0,
            mask: 1,
        };
        let p = counted::prepare(&q, &[slot]).unwrap();
        assert_eq!(p.groups[0].key, keys[2]);
        machine.groups.push_back(Packet {
            group: p.groups[0].clone(),
            payload: p.payload(0),
        });
        machine.hints.push_back(keys[2]);
        events.clear();
        machine.tags(None, true, &mut events).unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Promote { key } if *key == keys[2])));
        assert_eq!(machine.pending.front().unwrap().key, keys[2]);
        assert_eq!(
            (
                machine.active.as_ref().unwrap().id,
                machine.active.as_ref().unwrap().next
            ),
            (91, 3)
        );
        assert_eq!(machine.stats.allocations, 4); // Demand/hint FILLING merge.
        let plru = machine.plru;
        events.clear();
        machine.tags(None, true, &mut events).unwrap();
        assert!(events.is_empty()); // A stalled FILLING lookup never repeats touch.
        assert_eq!(machine.plru, plru);
    }
}
