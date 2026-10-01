use super::sealed::Sealed;
use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub type ValueId = usize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    Literal,
    Add,
    Sub,
    Multiply,
    Resize,
    ShiftLeft(u32),
    Shift,
    LeadingZeros,
    Slice(u32),
    RescaleFloor(u32),
    BinaryScale,
    Less,
    Select,
    RoundIncrement(u32),
    Read {
        memory: usize,
        row: usize,
    },
    Write {
        memory: usize,
        row: usize,
    },
    Publish(String),
    Require(bool),
    ProductMapping {
        a_bits: u32,
        b_bits: u32,
        route: ProductRoute,
    },
}
impl Operation {
    fn label(&self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Add => "add",
            Self::Sub => "subtract",
            Self::Multiply => "physical_multiply",
            Self::Resize => "resize",
            Self::ShiftLeft(_) => "constant_shift_wiring",
            Self::Shift => "variable_shift",
            Self::LeadingZeros => "leading_zeros",
            Self::Slice(_) => "slice_wiring",
            Self::RescaleFloor(_) => "rescale_wiring",
            Self::BinaryScale => "binary_scale_wiring",
            Self::Less => "compare",
            Self::Select => "select",
            Self::RoundIncrement(_) => "round_control",
            Self::Read { .. } => "read",
            Self::Write { .. } => "write",
            Self::Publish(_) => "publish",
            Self::Require(_) => "control_guard",
            Self::ProductMapping { .. } => "logical_multiply_mapping",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Event {
    pub id: usize,
    pub operation: Operation,
    pub resource: Option<Resource>,
    pub lane: Option<usize>,
    pub issue_cycle: u64,
    pub ready_cycle: u64,
    pub inputs: Vec<ValueId>,
    pub output: Option<ValueId>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueRecord {
    pub format: Format,
    pub raw: i128,
    pub ready_cycle: u64,
    pub producer: usize,
}
#[derive(Clone, Debug)]
pub struct Observation {
    pub name: String,
    pub value: ValueId,
    pub format: Format,
    pub raw: i128,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    pub operations: BTreeMap<&'static str, u64>,
    pub resources: BTreeMap<Resource, u64>,
    pub logical_products: BTreeMap<(u32, u32), u64>,
    pub read_bits: BTreeMap<usize, u64>,
    pub write_bits: BTreeMap<usize, u64>,
}
#[derive(Clone, Debug)]
pub struct MemoryReport {
    pub name: String,
    pub format: Format,
    pub rows: usize,
    pub ports: PortShape,
    pub read_only: bool,
}
#[derive(Clone, Debug)]
pub struct FrameReport {
    pub name: String,
    pub limits: Limits,
    pub cycles: u64,
    /// Provenance, width and resource audit status; not a numerical-error contract.
    pub valid: bool,
    pub faults: Vec<Fault>,
    pub hardware: Hardware,
    pub memories: Vec<MemoryReport>,
    pub counts: Counts,
    pub events: Vec<Event>,
    pub values: Vec<ValueRecord>,
    pub outputs: Vec<Observation>,
    pub issue_histogram: BTreeMap<(Resource, u64), usize>,
    initial: BTreeMap<(usize, usize), i128>,
}
impl FrameReport {
    /// Independent replay of value provenance, arithmetic, memory, timing and counters.
    /// A resource omission or forged output is an error, even when the number looks right.
    pub fn audit(&self) -> Result<(), Fault> {
        let bad = |s: &str| Fault::Audit(s.into());
        if self.cycles != self.events.iter().map(|e| e.ready_cycle).max().unwrap_or(0)
            || self.cycles > self.limits.max_cycle
            || self.events.len() > self.limits.max_events
            || self.valid != self.faults.is_empty()
        {
            return Err(bad("frame totals, limits or validity"));
        }
        let mut expected_counts = Counts::default();
        let mut histogram = BTreeMap::new();
        let mut last_issue = BTreeMap::new();
        let mut producers = vec![false; self.values.len()];
        let mut control_ready = 0;
        for (id, e) in self.events.iter().enumerate() {
            if e.id != id {
                return Err(bad("event identity"));
            }
            if e.issue_cycle < control_ready {
                return Err(bad("operation before control guard"));
            }
            let mut operands = Vec::new();
            for &v in &e.inputs {
                let Some(value) = self.values.get(v) else {
                    return Err(bad("missing operand"));
                };
                if !producers[v] || value.ready_cycle > e.issue_cycle {
                    return Err(bad("operand before return"));
                }
                operands.push(value);
            }
            let result = e
                .output
                .map(|v| self.values.get(v).ok_or_else(|| bad("missing output")))
                .transpose()?;
            if let (Some(v), Some(record)) = (e.output, result) {
                if producers[v]
                    || record.producer != id
                    || record.ready_cycle != e.ready_cycle
                    || !record.format.fits(record.raw)
                {
                    return Err(bad("output provenance or width"));
                }
                producers[v] = true;
            }
            let unit = e
                .resource
                .map(|r| {
                    self.hardware
                        .units
                        .get(&r)
                        .ok_or_else(|| bad("unconfigured resource"))
                })
                .transpose()?;
            if let (Some(r), Some(unit)) = (e.resource, unit) {
                let lane = e.lane.ok_or_else(|| bad("missing lane"))?;
                if lane >= unit.lanes || e.ready_cycle != e.issue_cycle + unit.latency {
                    return Err(bad("latency or lane"));
                }
                if let Some(previous) = last_issue.insert((r, lane), e.issue_cycle) {
                    if e.issue_cycle < previous + unit.initiation {
                        return Err(bad("initiation interval"));
                    }
                }
                *histogram.entry((r, e.issue_cycle)).or_default() += 1;
            } else if e.lane.is_some() || e.ready_cycle != e.issue_cycle {
                return Err(bad("unclocked operation shape"));
            }
            let out = || result.ok_or_else(|| bad("operation without result"));
            let required = match &e.operation {
                Operation::Literal => {
                    if !operands.is_empty() {
                        return Err(bad("literal with input"));
                    }
                    out()?;
                    None
                }
                Operation::Add | Operation::Sub => {
                    if operands.len() != 2 {
                        return Err(bad("adder inputs"));
                    }
                    let out = out()?;
                    if operands
                        .iter()
                        .any(|v| v.format.fraction != out.format.fraction)
                    {
                        return Err(bad("adder alignment"));
                    }
                    let raw = if e.operation == Operation::Add {
                        operands[0].raw.checked_add(operands[1].raw)
                    } else {
                        operands[0].raw.checked_sub(operands[1].raw)
                    };
                    if raw != Some(out.raw) {
                        return Err(bad("adder value"));
                    }
                    let width = operands
                        .iter()
                        .map(|v| v.format.bits)
                        .chain([out.format.bits])
                        .max()
                        .unwrap();
                    match e.resource {
                        Some(Resource::Adder(w)) if w >= width => e.resource,
                        _ => return Err(bad("unaccounted adder")),
                    }
                }
                Operation::Multiply => {
                    if operands.len() != 2 {
                        return Err(bad("DSP inputs"));
                    }
                    let out = out()?;
                    if operands[0].raw.checked_mul(operands[1].raw) != Some(out.raw)
                        || out.format.bits != operands[0].format.bits + operands[1].format.bits
                        || out.format.fraction
                            != operands[0].format.fraction + operands[1].format.fraction
                    {
                        return Err(bad("DSP product format/value"));
                    }
                    match e.resource {
                        Some(Resource::Dsp18) if operands.iter().all(|v| v.format.bits <= 18) => {
                            e.resource
                        }
                        Some(Resource::Dsp36) if operands.iter().all(|v| v.format.bits <= 36) => {
                            e.resource
                        }
                        _ => return Err(bad("unaccounted DSP")),
                    }
                }
                Operation::Resize => {
                    if operands.len() != 1
                        || out()?.raw != operands[0].raw
                        || out()?.format.fraction != operands[0].format.fraction
                    {
                        return Err(bad("resize"));
                    }
                    None
                }
                Operation::ShiftLeft(n) => {
                    if operands.len() != 1
                        || *n >= 127
                        || operands[0].raw.checked_mul(1_i128 << n) != Some(out()?.raw)
                        || out()?.format != operands[0].format
                    {
                        return Err(bad("shift wiring"));
                    }
                    None
                }
                Operation::Shift => {
                    if operands.len() != 2
                        || operands[1].format != Fixed::<18, 0, true>::FORMAT
                        || !(-126..=126).contains(&operands[1].raw)
                        || out()?.format != operands[0].format
                    {
                        return Err(bad("variable shift shape"));
                    }
                    let amount = operands[1].raw;
                    let raw = if amount >= 0 {
                        operands[0].raw.checked_mul(1_i128 << amount as u32)
                    } else {
                        Some(operands[0].raw >> (-amount) as u32)
                    };
                    if raw != Some(out()?.raw) {
                        return Err(bad("variable shift value"));
                    }
                    match e.resource {
                        Some(Resource::Shift(w)) if w >= operands[0].format.bits => e.resource,
                        _ => return Err(bad("unaccounted variable shift")),
                    }
                }
                Operation::LeadingZeros => {
                    if operands.len() != 1
                        || operands[0].format.signed
                        || out()?.format != Fixed::<18, 0, true>::FORMAT
                    {
                        return Err(bad("leading zeros shape"));
                    }
                    // Deliberately replay by a scan instead of using the executor's intrinsic.
                    let width = operands[0].format.bits;
                    let zeros = (0..width)
                        .rev()
                        .take_while(|bit| operands[0].raw & (1_i128 << bit) == 0)
                        .count();
                    if out()?.raw != zeros as i128 {
                        return Err(bad("leading zeros value"));
                    }
                    match e.resource {
                        Some(Resource::LeadingZeros(w)) if w >= width => e.resource,
                        _ => return Err(bad("unaccounted leading zeros")),
                    }
                }
                Operation::Slice(low) => {
                    if operands.len() != 1 {
                        return Err(bad("slice input"));
                    }
                    let out = out()?;
                    let bits = (operands[0].raw >> low) & ((1_i128 << out.format.bits) - 1);
                    let raw = if out.format.signed && bits & (1_i128 << (out.format.bits - 1)) != 0
                    {
                        bits - (1_i128 << out.format.bits)
                    } else {
                        bits
                    };
                    if raw != out.raw {
                        return Err(bad("slice value"));
                    }
                    None
                }
                Operation::RescaleFloor(shift) => {
                    if operands.len() != 1
                        || out()?.raw != operands[0].raw >> shift
                        || out()?.format.fraction + shift != operands[0].format.fraction
                    {
                        return Err(bad("rescale floor"));
                    }
                    None
                }
                Operation::BinaryScale => {
                    if operands.len() != 1
                        || out()?.raw != operands[0].raw
                        || out()?.format.bits != operands[0].format.bits
                        || out()?.format.signed != operands[0].format.signed
                    {
                        return Err(bad("binary scale wiring"));
                    }
                    None
                }
                Operation::Less => {
                    if operands.len() != 2
                        || operands[0].format.fraction != operands[1].format.fraction
                        || out()?.format != Fixed::<1, 0, false>::FORMAT
                        || out()?.raw != i128::from(operands[0].raw < operands[1].raw)
                    {
                        return Err(bad("comparison"));
                    }
                    match e.resource {
                        Some(Resource::Compare(w))
                            if operands.iter().all(|v| v.format.bits <= w) =>
                        {
                            e.resource
                        }
                        _ => return Err(bad("unaccounted compare")),
                    }
                }
                Operation::Select => {
                    if operands.len() != 3
                        || operands[0].format != Fixed::<1, 0, false>::FORMAT
                        || operands[1].format != out()?.format
                        || operands[2].format != out()?.format
                        || ![0, 1].contains(&operands[0].raw)
                        || out()?.raw != operands[if operands[0].raw != 0 { 1 } else { 2 }].raw
                    {
                        return Err(bad("select"));
                    }
                    match e.resource {
                        Some(Resource::Select(w)) if w >= out()?.format.bits => e.resource,
                        _ => return Err(bad("unaccounted select")),
                    }
                }
                Operation::RoundIncrement(shift) => {
                    if operands.len() != 1 || *shift == 0 {
                        return Err(bad("round inputs"));
                    }
                    let raw = operands[0].raw;
                    let base = raw >> shift;
                    let rem = raw & ((1_i128 << shift) - 1);
                    let half = 1_i128 << (shift - 1);
                    if out()?.raw != i128::from(rem > half || rem == half && base & 1 != 0) {
                        return Err(bad("round increment"));
                    }
                    match e.resource {
                        Some(Resource::RoundControl(w)) if w >= operands[0].format.bits => {
                            e.resource
                        }
                        _ => return Err(bad("unaccounted round control")),
                    }
                }
                Operation::Read { memory, row } => {
                    let m = self
                        .memories
                        .get(*memory)
                        .ok_or_else(|| bad("read memory"))?;
                    if *row >= m.rows
                        || out()?.format != m.format
                        || operands.len() > 1
                        || operands
                            .first()
                            .is_some_and(|a| a.format.fraction != 0 || a.raw != *row as i128)
                    {
                        return Err(bad("read shape/address"));
                    }
                    Some(Resource::Read(*memory))
                }
                Operation::Write { memory, row } => {
                    let m = self
                        .memories
                        .get(*memory)
                        .ok_or_else(|| bad("write memory"))?;
                    if *row >= m.rows
                        || m.read_only
                        || !(1..=2).contains(&operands.len())
                        || operands[0].format != m.format
                        || operands
                            .get(1)
                            .is_some_and(|a| a.format.fraction != 0 || a.raw != *row as i128)
                        || result.is_some()
                    {
                        return Err(bad("write shape"));
                    }
                    Some(Resource::Write(*memory))
                }
                Operation::Publish(_) => {
                    if operands.len() != 1 || result.is_some() {
                        return Err(bad("publish shape"));
                    }
                    None
                }
                Operation::Require(expected) => {
                    if operands.len() != 1
                        || operands[0].format != Fixed::<1, 0, false>::FORMAT
                        || (operands[0].raw != 0) != *expected
                        || result.is_some()
                    {
                        return Err(bad("control guard"));
                    }
                    control_ready = control_ready.max(e.ready_cycle);
                    None
                }
                Operation::ProductMapping { a_bits, b_bits, .. } => {
                    if operands.len() != 3
                        || operands[0].format.bits != *a_bits
                        || operands[1].format.bits != *b_bits
                        || operands[0].raw.checked_mul(operands[1].raw) != Some(operands[2].raw)
                        || result.is_some()
                    {
                        return Err(bad("logical/physical product mapping"));
                    }
                    None
                }
            };
            if required != e.resource {
                return Err(bad("unexpected or missing resource"));
            }
            count_event(&mut expected_counts, e, &self.memories);
        }
        if producers.iter().any(|&v| !v)
            || expected_counts != self.counts
            || histogram != self.issue_histogram
        {
            return Err(bad("counter or producer omission"));
        }
        for (&(r, _), &n) in &histogram {
            if n > self.hardware.units[&r].lanes {
                return Err(bad("per-cycle capacity"));
            }
        }
        for (i, m) in self.memories.iter().enumerate() {
            if self.hardware.units.get(&Resource::Read(i))
                != Some(&Unit::pipelined(m.ports.read_ports, m.ports.read_latency))
                || (!m.read_only
                    && self.hardware.units.get(&Resource::Write(i))
                        != Some(&Unit::pipelined(m.ports.write_ports, 1)))
            {
                return Err(bad("storage port configuration"));
            }
            if self
                .counts
                .resources
                .get(&Resource::Read(i))
                .copied()
                .unwrap_or(0)
                > m.ports.max_reads_per_frame
                || self
                    .counts
                    .resources
                    .get(&Resource::Write(i))
                    .copied()
                    .unwrap_or(0)
                    > m.ports.max_writes_per_frame
            {
                return Err(bad("frame port budget"));
            }
        }
        // Memory semantics are checked in physical cycle order, not API call order.
        let mut memory = self.initial.clone();
        let mut memory_events = self
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.operation,
                    Operation::Read { .. } | Operation::Write { .. }
                )
            })
            .collect::<Vec<_>>();
        memory_events.sort_by_key(|e| (e.issue_cycle, e.id));
        let mut last_write = BTreeMap::new();
        let mut last_read = BTreeMap::new();
        for e in memory_events {
            match e.operation {
                Operation::Write { memory: m, row } => {
                    if last_write.get(&(m, row)).copied().unwrap_or(0) > e.issue_cycle
                        || last_read.get(&(m, row)).copied().unwrap_or(0) > e.issue_cycle
                    {
                        return Err(bad("write before prior memory completion"));
                    }
                    memory.insert((m, row), self.values[e.inputs[0]].raw);
                    last_write.insert((m, row), e.ready_cycle);
                }
                Operation::Read { memory: m, row } => {
                    if last_write.get(&(m, row)).copied().unwrap_or(0) > e.issue_cycle {
                        return Err(bad("read before write completion"));
                    }
                    if memory.get(&(m, row)).copied() != Some(self.values[e.output.unwrap()].raw) {
                        return Err(bad("memory return value"));
                    }
                    last_read.insert((m, row), e.ready_cycle);
                }
                _ => unreachable!(),
            }
        }
        for o in &self.outputs {
            let v = &self.values[o.value];
            if o.raw != v.raw
                || o.format != v.format
                || !self.events.iter().any(|e| {
                    matches!(&e.operation,Operation::Publish(name) if name==&o.name)
                        && e.inputs == [o.value]
                })
            {
                return Err(bad("unpublished observation"));
            }
        }
        Ok(())
    }
}
fn count_event(counts: &mut Counts, e: &Event, memories: &[MemoryReport]) {
    *counts.operations.entry(e.operation.label()).or_default() += 1;
    if let Some(r) = e.resource {
        *counts.resources.entry(r).or_default() += 1;
    }
    match e.operation {
        Operation::Read { memory, .. } => {
            *counts.read_bits.entry(memory).or_default() += u64::from(memories[memory].format.bits)
        }
        Operation::Write { memory, .. } => {
            *counts.write_bits.entry(memory).or_default() += u64::from(memories[memory].format.bits)
        }
        Operation::ProductMapping { a_bits, b_bits, .. } => {
            *counts.logical_products.entry((a_bits, b_bits)).or_default() += 1
        }
        _ => {}
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Memory<const B: u32, const F: u32, const S: bool> {
    model: u64,
    id: usize,
}
#[derive(Clone, Copy)]
enum Row {
    Constant(usize),
    Indexed(Operand),
}
/// A typed location. Only a memory handle can create it; no host runtime row exists.
#[derive(Clone, Copy)]
pub struct Address<const B: u32, const F: u32, const S: bool> {
    memory: Memory<B, F, S>,
    row: Row,
}
impl<const B: u32, const F: u32, const S: bool> Memory<B, F, S> {
    /// A static location is wiring, not an untracked runtime address calculation.
    pub fn at<const ROW: usize>(&self) -> Address<B, F, S> {
        Address {
            memory: *self,
            row: Row::Constant(ROW),
        }
    }
    /// Keep the fixed-point address provenance until the port operation resolves it.
    pub fn indexed<const A: u32, const SIGNED: bool>(
        &self,
        row: Fixed<A, 0, SIGNED>,
    ) -> Address<B, F, S> {
        Address {
            memory: *self,
            row: Row::Indexed(row.operand()),
        }
    }
}
struct Store {
    description: MemoryReport,
    cells: Vec<Option<i128>>,
}
pub struct Model {
    id: u64,
    hardware: Hardware,
    stores: RefCell<Vec<Store>>,
}
impl Model {
    pub fn new(hardware: Hardware) -> Result<Self, Fault> {
        if hardware
            .units
            .values()
            .any(|u| u.lanes == 0 || u.latency == 0 || u.initiation == 0)
        {
            return Err(Fault::MissingResource);
        }
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            hardware,
            stores: RefCell::new(Vec::new()),
        })
    }
    pub fn ram<const B: u32, const F: u32, const S: bool>(
        &mut self,
        name: &str,
        rows: usize,
        ports: PortShape,
    ) -> Result<Memory<B, F, S>, Fault> {
        self.memory(name, rows, ports, None)
    }
    pub fn rom<const B: u32, const F: u32, const S: bool>(
        &mut self,
        name: &str,
        constants: &[Fixed<B, F, S>],
        ports: PortShape,
    ) -> Result<Memory<B, F, S>, Fault> {
        if constants.iter().any(|v| v.origin != Origin::Constant) {
            return Err(Fault::ForeignValue);
        }
        self.memory(
            name,
            constants.len(),
            ports,
            Some(constants.iter().map(|v| v.bits).collect()),
        )
    }
    fn memory<const B: u32, const F: u32, const S: bool>(
        &mut self,
        name: &str,
        rows: usize,
        ports: PortShape,
        constants: Option<Vec<i128>>,
    ) -> Result<Memory<B, F, S>, Fault> {
        if rows == 0
            || !Fixed::<B, F, S>::FORMAT.valid()
            || ports.read_ports == 0
            || ports.read_latency == 0
            || constants.is_none() && ports.write_ports == 0
        {
            return Err(Fault::Format);
        }
        let mut stores = self.stores.borrow_mut();
        let id = stores.len();
        self.hardware.units.insert(
            Resource::Read(id),
            Unit::pipelined(ports.read_ports, ports.read_latency),
        );
        if constants.is_none() {
            self.hardware
                .units
                .insert(Resource::Write(id), Unit::pipelined(ports.write_ports, 1));
        }
        let read_only = constants.is_some();
        stores.push(Store {
            description: MemoryReport {
                name: name.into(),
                format: Fixed::<B, F, S>::FORMAT,
                rows,
                ports,
                read_only,
            },
            cells: constants
                .map_or_else(|| vec![None; rows], |v| v.into_iter().map(Some).collect()),
        });
        Ok(Memory { model: self.id, id })
    }
    pub fn begin_frame(&mut self, name: &str, limits: Limits) -> Frame<'_> {
        let initial = self
            .stores
            .borrow()
            .iter()
            .enumerate()
            .flat_map(|(m, s)| {
                s.cells
                    .iter()
                    .enumerate()
                    .filter_map(move |(row, v)| v.map(|v| ((m, row), v)))
            })
            .collect();
        let availability = self
            .hardware
            .units
            .iter()
            .map(|(&r, u)| (r, vec![0; u.lanes]))
            .collect();
        Frame {
            model: self,
            state: RefCell::new(State {
                frame: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                name: name.into(),
                limits,
                initial,
                availability,
                events: Vec::new(),
                values: Vec::new(),
                counts: Counts::default(),
                histogram: BTreeMap::new(),
                faults: Vec::new(),
                last_write: BTreeMap::new(),
                last_read: BTreeMap::new(),
                outputs: Vec::new(),
                control_ready: 0,
            }),
        }
    }
}
struct State {
    frame: u64,
    name: String,
    limits: Limits,
    initial: BTreeMap<(usize, usize), i128>,
    availability: BTreeMap<Resource, Vec<u64>>,
    events: Vec<Event>,
    values: Vec<ValueRecord>,
    counts: Counts,
    histogram: BTreeMap<(Resource, u64), usize>,
    faults: Vec<Fault>,
    last_write: BTreeMap<(usize, usize), u64>,
    last_read: BTreeMap<(usize, usize), u64>,
    outputs: Vec<(String, ValueId)>,
    control_ready: u64,
}
pub struct Frame<'a> {
    model: &'a Model,
    state: RefCell<State>,
}
impl Frame<'_> {
    pub(super) fn fail<T>(&self, fault: Fault) -> Result<T, Fault> {
        self.state.borrow_mut().faults.push(fault.clone());
        Err(fault)
    }
    pub(super) fn resolve(&self, v: impl FixedValue) -> Result<(ValueId, Operand), Fault> {
        let v = v.operand();
        match v.origin {
            Origin::Constant => {
                let id = self
                    .emit(Operation::Literal, None, &[], Some((v.format, v.bits)), 0)?
                    .unwrap();
                let state = self.state.borrow();
                Ok((
                    id,
                    Operand {
                        origin: Origin::Event {
                            frame: state.frame,
                            value: id,
                            ready: state.values[id].ready_cycle,
                        },
                        ..v
                    },
                ))
            }
            Origin::Event {
                frame,
                value,
                ready,
            } => {
                let state = self.state.borrow();
                if frame != state.frame
                    || state.values.get(value).is_none_or(|r| {
                        r.raw != v.bits || r.format != v.format || r.ready_cycle != ready
                    })
                {
                    drop(state);
                    return self.fail(Fault::ForeignValue);
                }
                Ok((value, v))
            }
        }
    }
    pub(super) fn unit_for(
        &self,
        kind: fn(u32) -> Resource,
        width: u32,
    ) -> Result<Resource, Fault> {
        self.model
            .hardware
            .units
            .keys()
            .copied()
            .filter(|r| match r {
                Resource::Adder(w)
                | Resource::Compare(w)
                | Resource::RoundControl(w)
                | Resource::Select(w)
                | Resource::LeadingZeros(w)
                | Resource::Shift(w) => *w >= width && kind(*w) == *r,
                _ => false,
            })
            .min()
            .ok_or_else(|| {
                self.state.borrow_mut().faults.push(Fault::MissingResource);
                Fault::MissingResource
            })
    }
    pub(super) fn emit(
        &self,
        op: Operation,
        resource: Option<Resource>,
        inputs: &[ValueId],
        output: Option<(Format, i128)>,
        extra: u64,
    ) -> Result<Option<ValueId>, Fault> {
        if output.is_some_and(|(f, v)| !f.fits(v)) {
            return self.fail(Fault::Range);
        }
        let mut state = self.state.borrow_mut();
        if state.events.len() >= state.limits.max_events {
            drop(state);
            return self.fail(Fault::EventLimit);
        }
        let earliest = inputs
            .iter()
            .map(|&i| state.values[i].ready_cycle)
            .chain([extra, state.control_ready])
            .max()
            .unwrap();
        let (issue, ready, lane) = if let Some(r) = resource {
            let Some(unit) = self.model.hardware.units.get(&r) else {
                drop(state);
                return self.fail(Fault::MissingResource);
            };
            let slots = &state.availability[&r];
            let (lane, &free) = slots.iter().enumerate().min_by_key(|(_, v)| **v).unwrap();
            let cycle = earliest.max(free);
            (cycle, cycle + unit.latency, Some(lane))
        } else {
            (earliest, earliest, None)
        };
        if ready > state.limits.max_cycle {
            drop(state);
            return self.fail(Fault::Deadline);
        }
        if let Some(r) = resource {
            let cap = match r {
                Resource::Read(m) => Some(
                    self.model.stores.borrow()[m]
                        .description
                        .ports
                        .max_reads_per_frame,
                ),
                Resource::Write(m) => Some(
                    self.model.stores.borrow()[m]
                        .description
                        .ports
                        .max_writes_per_frame,
                ),
                _ => None,
            };
            if cap.is_some_and(|cap| state.counts.resources.get(&r).copied().unwrap_or(0) >= cap) {
                drop(state);
                return self.fail(Fault::PortFrameLimit);
            }
            state.availability.get_mut(&r).unwrap()[lane.unwrap()] =
                issue + self.model.hardware.units[&r].initiation;
            *state.histogram.entry((r, issue)).or_default() += 1;
        }
        let id = state.events.len();
        let value = output.map(|(format, raw)| {
            let value = state.values.len();
            state.values.push(ValueRecord {
                format,
                raw,
                ready_cycle: ready,
                producer: id,
            });
            value
        });
        let event = Event {
            id,
            operation: op,
            resource,
            lane,
            issue_cycle: issue,
            ready_cycle: ready,
            inputs: inputs.to_vec(),
            output: value,
        };
        let descriptions = self
            .model
            .stores
            .borrow()
            .iter()
            .map(|s| s.description.clone())
            .collect::<Vec<_>>();
        count_event(&mut state.counts, &event, &descriptions);
        state.events.push(event);
        Ok(value)
    }
    pub(super) fn value<const B: u32, const F: u32, const S: bool>(
        &self,
        op: Operation,
        r: Option<Resource>,
        inputs: &[ValueId],
        raw: i128,
        extra: u64,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let id = self
            .emit(op, r, inputs, Some((Fixed::<B, F, S>::FORMAT, raw)), extra)?
            .unwrap();
        let state = self.state.borrow();
        Ok(Fixed {
            bits: raw,
            origin: Origin::Event {
                frame: state.frame,
                value: id,
                ready: state.values[id].ready_cycle,
            },
        })
    }
    pub(super) fn dynamic(
        &self,
        op: Operation,
        r: Option<Resource>,
        args: &[Operand],
        format: Format,
        raw: i128,
    ) -> Result<Operand, Fault> {
        let inputs = args
            .iter()
            .map(|a| self.resolve(*a).map(|v| v.0))
            .collect::<Result<Vec<_>, _>>()?;
        let id = self.emit(op, r, &inputs, Some((format, raw)), 0)?.unwrap();
        let state = self.state.borrow();
        Ok(Operand {
            bits: raw,
            format,
            origin: Origin::Event {
                frame: state.frame,
                value: id,
                ready: state.values[id].ready_cycle,
            },
        })
    }
    fn check_memory<const B: u32, const F: u32, const S: bool>(
        &self,
        m: &Memory<B, F, S>,
        row: usize,
    ) -> Result<(), Fault> {
        if m.model != self.model.id {
            return self.fail(Fault::ForeignMemory);
        }
        if row >= self.model.stores.borrow()[m.id].description.rows {
            return self.fail(Fault::Address);
        }
        Ok(())
    }
    fn resolve_address<const B: u32, const F: u32, const S: bool>(
        &self,
        address: Address<B, F, S>,
    ) -> Result<(Memory<B, F, S>, usize, Vec<ValueId>), Fault> {
        match address.row {
            Row::Constant(row) => Ok((address.memory, row, Vec::new())),
            Row::Indexed(a) => {
                let (id, a) = self.resolve(a)?;
                if a.format.fraction != 0 || a.bits < 0 {
                    return self.fail(Fault::Address);
                }
                let row = match usize::try_from(a.bits) {
                    Ok(row) => row,
                    Err(_) => return self.fail(Fault::Address),
                };
                Ok((address.memory, row, vec![id]))
            }
        }
    }
    /// Data format, storage and address dependencies come from the typed location.
    pub fn read<const B: u32, const F: u32, const S: bool>(
        &self,
        address: Address<B, F, S>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let (m, row, inputs) = self.resolve_address(address)?;
        self.read_row(&m, row, &inputs)
    }
    fn read_row<const B: u32, const F: u32, const S: bool>(
        &self,
        m: &Memory<B, F, S>,
        row: usize,
        inputs: &[ValueId],
    ) -> Result<Fixed<B, F, S>, Fault> {
        self.check_memory(m, row)?;
        let Some(bits) = self.model.stores.borrow()[m.id].cells[row] else {
            return self.fail(Fault::Uninitialized);
        };
        let extra = self
            .state
            .borrow()
            .last_write
            .get(&(m.id, row))
            .copied()
            .unwrap_or(0);
        let value = self.value(
            Operation::Read { memory: m.id, row },
            Some(Resource::Read(m.id)),
            inputs,
            bits,
            extra,
        )?;
        let Origin::Event { ready, .. } = value.origin else {
            unreachable!()
        };
        self.state
            .borrow_mut()
            .last_read
            .entry((m.id, row))
            .and_modify(|last| *last = (*last).max(ready))
            .or_insert(ready);
        Ok(value)
    }
    pub fn write<const B: u32, const F: u32, const S: bool>(
        &self,
        address: Address<B, F, S>,
        value: Fixed<B, F, S>,
    ) -> Result<(), Fault> {
        let (m, row, mut inputs) = self.resolve_address(address)?;
        self.check_memory(&m, row)?;
        if self.model.stores.borrow()[m.id].description.read_only {
            return self.fail(Fault::ReadOnly);
        }
        let (id, v) = self.resolve(value)?;
        inputs.insert(0, id);
        let state = self.state.borrow();
        let extra = state
            .last_write
            .get(&(m.id, row))
            .copied()
            .unwrap_or(0)
            .max(state.last_read.get(&(m.id, row)).copied().unwrap_or(0));
        drop(state);
        self.emit(
            Operation::Write { memory: m.id, row },
            Some(Resource::Write(m.id)),
            &inputs,
            None,
            extra,
        )?;
        let ready = self.state.borrow().events.last().unwrap().ready_cycle;
        self.state
            .borrow_mut()
            .last_write
            .insert((m.id, row), ready);
        self.model.stores.borrow_mut()[m.id].cells[row] = Some(v.bits);
        Ok(())
    }
    pub fn publish(&self, name: &str, value: impl FixedValue) -> Result<(), Fault> {
        let (id, _) = self.resolve(value)?;
        self.emit(Operation::Publish(name.into()), None, &[id], None, 0)?;
        self.state.borrow_mut().outputs.push((name.into(), id));
        Ok(())
    }
    pub fn require<const EXPECTED: bool>(&self, p: Fixed<1, 0, false>) -> Result<(), Fault> {
        let (id, value) = self.resolve(p)?;
        if (value.bits != 0) != EXPECTED {
            return self.fail(Fault::Range);
        }
        self.emit(Operation::Require(EXPECTED), None, &[id], None, 0)?;
        let mut state = self.state.borrow_mut();
        state.control_ready = state.control_ready.max(state.values[id].ready_cycle);
        Ok(())
    }
    /// Consume the computation boundary before exposing host integers.
    pub fn finish(self) -> FrameReport {
        let state = self.state.into_inner();
        let memories = self
            .model
            .stores
            .borrow()
            .iter()
            .map(|s| s.description.clone())
            .collect();
        let outputs = state
            .outputs
            .iter()
            .map(|(name, id)| {
                let v = &state.values[*id];
                Observation {
                    name: name.clone(),
                    value: *id,
                    format: v.format,
                    raw: v.raw,
                }
            })
            .collect();
        let mut report = FrameReport {
            name: state.name,
            limits: state.limits,
            cycles: state
                .events
                .iter()
                .map(|e| e.ready_cycle)
                .max()
                .unwrap_or(0),
            valid: state.faults.is_empty(),
            faults: state.faults,
            hardware: self.model.hardware.clone(),
            memories,
            counts: state.counts,
            events: state.events,
            values: state.values,
            outputs,
            issue_histogram: state.histogram,
            initial: state.initial,
        };
        if let Err(fault) = report.audit() {
            report.valid = false;
            report.faults.push(fault);
        }
        report
    }
}
