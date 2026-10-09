use super::sealed::Sealed;
use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub type ValueId = usize;

#[path = "scheduling.rs"]
mod scheduling;

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
    Branch(bool),
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
            Self::Branch(_) => "branch",
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
    /// The preceding branch/guard event, independent of optional cycle timing.
    pub control: Option<usize>,
    pub output: Option<ValueId>,
    /// The kernel call site, independent of numerical and scheduling semantics.
    pub source: &'static std::panic::Location<'static>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueRecord {
    pub format: Format,
    pub raw: i128,
    pub ready_cycle: u64,
    pub producer: usize,
    /// Optional source-level name; presentation metadata, never a new event.
    pub name: Option<String>,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryKind {
    Ram,
    Rom,
    Input,
}
#[derive(Clone, Debug)]
pub struct MemoryReport {
    pub name: String,
    pub format: Format,
    pub rows: usize,
    pub ports: PortShape,
    pub read_only: bool,
    pub kind: MemoryKind,
}
#[derive(Clone, Debug)]
pub struct FrameReport {
    pub name: String,
    pub mode: ExecutionMode,
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
    /// A numerical report deliberately has no completion-cycle estimate.
    pub fn scheduled_cycles(&self) -> Option<u64> {
        (self.mode == ExecutionMode::Scheduled).then_some(self.cycles)
    }
    /// Independent replay of value provenance, arithmetic, memory, timing and counters.
    /// A resource omission or forged output is an error, even when the number looks right.
    pub fn audit(&self) -> Result<(), Fault> {
        let bad = |s: &str| Fault::Audit(s.into());
        if self.cycles != self.events.iter().map(|e| e.ready_cycle).max().unwrap_or(0)
            || (self.mode == ExecutionMode::Scheduled && self.cycles > self.limits.max_cycle)
            || self.events.len() > self.limits.max_events
            || self.valid != self.faults.is_empty()
        {
            return Err(bad("frame totals, limits or validity"));
        }
        self.audit_timing()?;
        let mut expected_counts = Counts::default();
        let mut histogram = BTreeMap::new();
        let mut producers = vec![false; self.values.len()];
        let mut control_ready = 0;
        let mut control = None;
        for (id, e) in self.events.iter().enumerate() {
            if e.id != id {
                return Err(bad("event identity"));
            }
            if e.control != control {
                return Err(bad("control dependency omission"));
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
            if let Some(r) = e.resource.filter(|_| self.mode == ExecutionMode::Scheduled) {
                *histogram.entry((r, e.issue_cycle)).or_default() += 1;
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
                    control = Some(e.id);
                    None
                }
                Operation::Branch(taken) => {
                    if operands.len() != 1
                        || operands[0].format != Fixed::<1, 0, false>::FORMAT
                        || (operands[0].raw != 0) != *taken
                        || result.is_some()
                    {
                        return Err(bad("branch decision"));
                    }
                    control_ready = control_ready.max(e.ready_cycle);
                    control = Some(e.id);
                    None
                }
                Operation::ProductMapping {
                    a_bits,
                    b_bits,
                    route,
                } => {
                    if operands.len() != 3
                        || operands[0].format.bits != *a_bits
                        || operands[1].format.bits != *b_bits
                        || operands[0].raw.checked_mul(operands[1].raw) != Some(operands[2].raw)
                        || operands[2].format.bits != a_bits + b_bits
                        || operands[2].format.fraction
                            != operands[0].format.fraction + operands[1].format.fraction
                        || operands[2].format.signed
                            != (operands[0].format.signed || operands[1].format.signed)
                        || result.is_some()
                    {
                        return Err(bad("logical/physical product mapping"));
                    }
                    self.audit_product_mapping(e, *route)?;
                    None
                }
            };
            if required != e.resource {
                return Err(bad("unexpected or missing resource"));
            }
            count_event(&mut expected_counts, e, |memory| {
                self.memories[memory].format.bits
            });
        }
        if producers.iter().any(|&v| !v)
            || expected_counts != self.counts
            || histogram != self.issue_histogram
        {
            return Err(bad("counter or producer omission"));
        }
        for (i, m) in self.memories.iter().enumerate() {
            if !m.format.valid() || m.rows == 0 || m.read_only != (m.kind != MemoryKind::Ram) {
                return Err(bad("storage kind/format"));
            }
            if m.read_only
                && (0..m.rows).any(|row| {
                    self.initial
                        .get(&(i, row))
                        .is_none_or(|&raw| !m.format.fits(raw))
                })
            {
                return Err(bad("input/ROM initial data"));
            }
        }
        for (&(r, _), &n) in &histogram {
            if n > self.hardware.units[&r].lanes {
                return Err(bad("per-cycle capacity"));
            }
        }
        // Numerical mode uses event order; scheduled mode uses physical cycle order.
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
    fn audit_product_mapping(&self, mapping: &Event, route: ProductRoute) -> Result<(), Fault> {
        let bad = || Fault::Audit("product route/physical lowering".into());
        let a = mapping.inputs[0];
        let b = mapping.inputs[1];
        let widths = (self.values[a].format.bits, self.values[b].format.bits);
        let mut stack = vec![mapping.inputs[2]];
        let mut visited = std::collections::BTreeSet::new();
        let mut work = BTreeMap::new();
        while let Some(value) = stack.pop() {
            if !visited.insert(value) || value == a || value == b {
                continue;
            }
            let producer = &self.events[self.values[value].producer];
            if let Some(resource) = producer.resource {
                *work.entry(resource).or_insert(0_u64) += 1;
            }
            stack.extend(&producer.inputs);
        }
        let shape = match route {
            ProductRoute::Native18 => {
                widths.0 <= 18 && widths.1 <= 18 && work == BTreeMap::from([(Resource::Dsp18, 1)])
            }
            ProductRoute::Wide36 => {
                widths.0 <= 36 && widths.1 <= 36 && work == BTreeMap::from([(Resource::Dsp36, 1)])
            }
            ProductRoute::Native18Pair => {
                ((19..=36).contains(&widths.0) && widths.1 <= 18
                    || (19..=36).contains(&widths.1) && widths.0 <= 18)
                    && work.len() == 2
                    && work.get(&Resource::Dsp18) == Some(&2)
                    && work.iter().any(|(r, n)| {
                        matches!(r,Resource::Adder(w) if *w >= widths.0+widths.1) && *n == 1
                    })
            }
        };
        if !shape || !visited.contains(&a) || !visited.contains(&b) {
            return Err(bad());
        }
        Ok(())
    }
}
fn count_event(counts: &mut Counts, e: &Event, memory_width: impl Fn(usize) -> u32) {
    *counts.operations.entry(e.operation.label()).or_default() += 1;
    if let Some(r) = e.resource {
        *counts.resources.entry(r).or_default() += 1;
    }
    match e.operation {
        Operation::Read { memory, .. } => {
            *counts.read_bits.entry(memory).or_default() += u64::from(memory_width(memory))
        }
        Operation::Write { memory, .. } => {
            *counts.write_bits.entry(memory).or_default() += u64::from(memory_width(memory))
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
    mode: ExecutionMode,
    hardware: Hardware,
    stores: RefCell<Vec<Store>>,
}
impl Model {
    /// Step 1: sealed numerical data and exact work counts, without scheduling.
    pub fn numerical() -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            mode: ExecutionMode::Numerical,
            hardware: Hardware {
                units: BTreeMap::new(),
            },
            stores: RefCell::new(Vec::new()),
        }
    }
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
            mode: ExecutionMode::Scheduled,
            hardware,
            stores: RefCell::new(Vec::new()),
        })
    }
    /// External raw data is checked here, before any computation can begin.
    /// This returns a read-only storage handle, never a runtime Fixed value.
    pub fn input<const B: u32, const F: u32, const S: bool>(
        &mut self,
        name: &str,
        raw: &[i128],
    ) -> Result<Memory<B, F, S>, Fault> {
        if self.mode != ExecutionMode::Numerical {
            return Err(Fault::Format);
        }
        if raw.iter().any(|&v| !Fixed::<B, F, S>::FORMAT.fits(v)) {
            return Err(Fault::Range);
        }
        let memory = self.memory(name, raw.len(), Self::untimed_ports(), Some(raw.to_vec()))?;
        self.stores.borrow_mut()[memory.id].description.kind = MemoryKind::Input;
        Ok(memory)
    }
    pub fn scratch<const B: u32, const F: u32, const S: bool>(
        &mut self,
        name: &str,
        rows: usize,
    ) -> Result<Memory<B, F, S>, Fault> {
        if self.mode != ExecutionMode::Numerical {
            return Err(Fault::Format);
        }
        self.ram(name, rows, Self::untimed_ports())
    }
    pub fn table<const B: u32, const F: u32, const S: bool>(
        &mut self,
        name: &str,
        constants: &[Fixed<B, F, S>],
    ) -> Result<Memory<B, F, S>, Fault> {
        if self.mode != ExecutionMode::Numerical {
            return Err(Fault::Format);
        }
        self.rom(name, constants, Self::untimed_ports())
    }
    fn untimed_ports() -> PortShape {
        PortShape {
            read_ports: 0,
            write_ports: 0,
            read_latency: 0,
            max_reads_per_frame: 0,
            max_writes_per_frame: 0,
        }
    }
    pub fn compute(&mut self, name: &str, max_events: usize) -> Result<Frame<'_>, Fault> {
        if self.mode != ExecutionMode::Numerical {
            return Err(Fault::Format);
        }
        Ok(self.begin_frame(
            name,
            Limits {
                max_cycle: 0,
                max_events,
            },
        ))
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
            || (self.mode == ExecutionMode::Scheduled
                && (ports.read_ports == 0
                    || ports.read_latency == 0
                    || constants.is_none() && ports.write_ports == 0))
        {
            return Err(Fault::Format);
        }
        let mut stores = self.stores.borrow_mut();
        let id = stores.len();
        if self.mode == ExecutionMode::Scheduled {
            self.hardware.units.insert(
                Resource::Read(id),
                Unit::pipelined(ports.read_ports, ports.read_latency),
            );
            if constants.is_none() {
                self.hardware
                    .units
                    .insert(Resource::Write(id), Unit::pipelined(ports.write_ports, 1));
            }
        }
        let read_only = constants.is_some();
        stores.push(Store {
            description: MemoryReport {
                name: name.into(),
                format: Fixed::<B, F, S>::FORMAT,
                rows,
                ports,
                read_only,
                kind: if read_only {
                    MemoryKind::Rom
                } else {
                    MemoryKind::Ram
                },
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
                control: None,
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
    control: Option<usize>,
}
pub struct Frame<'a> {
    model: &'a Model,
    state: RefCell<State>,
}
impl Frame<'_> {
    #[track_caller]
    pub(super) fn fail<T>(&self, fault: Fault) -> Result<T, Fault> {
        self.state.borrow_mut().faults.push(fault.clone());
        Err(fault)
    }
    #[track_caller]
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
    #[track_caller]
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
        let (issue, ready, lane) = match self.schedule(&mut state, resource, inputs, extra) {
            Ok(timing) => timing,
            Err(fault) => {
                drop(state);
                return self.fail(fault);
            }
        };
        let id = state.events.len();
        let value = output.map(|(format, raw)| {
            let value = state.values.len();
            state.values.push(ValueRecord {
                format,
                raw,
                ready_cycle: ready,
                producer: id,
                name: None,
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
            control: state.control,
            output: value,
            source: std::panic::Location::caller(),
        };
        let stores = self.model.stores.borrow();
        count_event(&mut state.counts, &event, |memory| {
            stores[memory].description.format.bits
        });
        state.events.push(event);
        Ok(value)
    }
    #[track_caller]
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
    #[track_caller]
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
    #[track_caller]
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
    #[track_caller]
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
    #[track_caller]
    pub fn read<const B: u32, const F: u32, const S: bool>(
        &self,
        address: Address<B, F, S>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let (m, row, inputs) = self.resolve_address(address)?;
        self.read_row(&m, row, &inputs)
    }
    #[track_caller]
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
    #[track_caller]
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
    /// Attach a source variable name without emitting work or materializing literals.
    #[track_caller]
    pub fn name_value(&self, name: &str, value: impl FixedValue) -> Result<(), Fault> {
        if matches!(value.operand().origin, Origin::Constant) {
            return Ok(());
        }
        let (id, _) = self.resolve(value)?;
        self.state.borrow_mut().values[id].name = Some(name.into());
        Ok(())
    }
    #[track_caller]
    pub fn publish(&self, name: &str, value: impl FixedValue) -> Result<(), Fault> {
        let (id, _) = self.resolve(value)?;
        self.emit(Operation::Publish(name.into()), None, &[id], None, 0)?;
        self.state.borrow_mut().outputs.push((name.into(), id));
        Ok(())
    }
    #[track_caller]
    pub fn require<const EXPECTED: bool>(&self, p: Fixed<1, 0, false>) -> Result<(), Fault> {
        let (id, value) = self.resolve(p)?;
        if (value.bits != 0) != EXPECTED {
            return self.fail(Fault::Range);
        }
        self.emit(Operation::Require(EXPECTED), None, &[id], None, 0)?;
        let mut state = self.state.borrow_mut();
        state.control_ready = state.control_ready.max(state.values[id].ready_cycle);
        state.control = Some(state.events.last().unwrap().id);
        Ok(())
    }
    #[track_caller]
    fn branch_decision(&self, p: Fixed<1, 0, false>) -> Result<bool, Fault> {
        let (id, value) = self.resolve(p)?;
        let taken = value.bits != 0;
        self.emit(Operation::Branch(taken), None, &[id], None, 0)?;
        let mut state = self.state.borrow_mut();
        state.control_ready = state.control_ready.max(state.values[id].ready_cycle);
        state.control = Some(state.events.last().unwrap().id);
        Ok(taken)
    }
    /// Execute and count only the selected path; false is a normal algorithm result.
    /// No host predicate or raw data escapes the computation boundary.
    #[track_caller]
    pub fn branch(
        &self,
        p: Fixed<1, 0, false>,
        yes: impl FnOnce(&Self) -> Result<(), Fault>,
        no: impl FnOnce(&Self) -> Result<(), Fault>,
    ) -> Result<(), Fault> {
        let result = if self.branch_decision(p)? {
            yes(self)
        } else {
            no(self)
        };
        match result {
            Ok(()) => Ok(()),
            Err(fault) => self.fail(fault),
        }
    }
    #[track_caller]
    pub fn branch_value<const B: u32, const F: u32, const S: bool>(
        &self,
        p: Fixed<1, 0, false>,
        yes: impl FnOnce(&Self) -> Result<Fixed<B, F, S>, Fault>,
        no: impl FnOnce(&Self) -> Result<Fixed<B, F, S>, Fault>,
    ) -> Result<Fixed<B, F, S>, Fault> {
        let result = if self.branch_decision(p)? {
            yes(self)
        } else {
            no(self)
        };
        let value = match result {
            Ok(value) => value,
            Err(fault) => return self.fail(fault),
        };
        // Preserve branch/control provenance even when a path returns a literal.
        self.resize_exact(value)
    }
    /// Consume the computation boundary before exposing host integers.
    #[track_caller]
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
            mode: self.model.mode,
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
