//! Bounded offline static reservations for a batch with one uniform context.
//! Uses the counted full-path DAG as a conservative template, including power
//! endpoint slots. This is a compile-time planning experiment, not an RTL arbiter.
use super::super::ports::*;
use super::binding::BoundDag;
pub use super::binding::FusedGroup;
use super::{counted, oracle};
use audited::{FrameReport, MemoryKind, Operation, Resource};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

#[path = "periodic.rs"]
mod periodic;
pub use periodic::PeriodicSchedule;
#[path = "physical.rs"]
mod physical;
pub use physical::PhysicalReport;
#[path = "adder.rs"]
mod adder;
pub use adder::AdderInventory;
#[path = "stream.rs"]
mod stream;
pub use stream::{stream, StreamContext, StreamResult, StreamRun};

#[derive(Clone, Copy, Debug)]
pub struct Hardware {
    pub dsp_tiles: usize,
    /// Maximum serial nonwiring logic levels per explicitly certified cone; zero disables.
    pub cone_depth: usize,
    pub cone_latency: u64,
    pub cone_lanes_per_shape: usize,
    pub kernel: counted::Config,
    pub binding: Binding,
    pub narrow_adders: usize,
    pub incrementers_per_width: usize,
    pub negators_per_width: usize,
    pub paired_macros: usize,
    pub paired_latency: u64,
    pub small_multiply: usize,
    pub large_multiply: usize,
    pub adders_per_width: usize,
    pub compares_per_width: usize,
    pub selects_per_width: usize,
    pub shifts_per_width: usize,
    pub rounders_per_width: usize,
    pub leading_zeros_per_width: usize,
    pub normalize_reads: usize,
    pub multiply_latency: u64,
    pub logic_latency: u64,
    pub rom_latency: u64,
    pub max_cycles: u64,
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            dsp_tiles: 12,
            cone_depth: 0,
            cone_latency: 1,
            cone_lanes_per_shape: 3,
            kernel: counted::Config::default(),
            binding: Binding::Generic,
            narrow_adders: 2,
            incrementers_per_width: 2,
            negators_per_width: 6,
            paired_macros: 0,
            paired_latency: 4,
            small_multiply: 7,
            large_multiply: 9,
            adders_per_width: 2,
            compares_per_width: 2,
            selects_per_width: 3,
            shifts_per_width: 2,
            rounders_per_width: 2,
            leading_zeros_per_width: 1,
            normalize_reads: 6,
            multiply_latency: 3,
            logic_latency: 1,
            rom_latency: 1,
            max_cycles: 20000,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Generic,
    LightingDsp,
}
#[derive(Clone, Copy, Debug)]
pub enum Storage {
    /// Precaptured registers. Reports payload sizes but imposes no input bus.
    Registers,
    /// Candidate 36-bit rows: normal XY; normal Z + NDC X; NDC Y.
    Rows { read_lanes: usize, latency: u64 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    Serial,
    Interleaved,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LaneKind {
    LogicCone { shape: String, width: u32 },
    SmallMultiply,
    LargeMultiply,
    PairMultiplyAdd,
    Increment(u32),
    Negate(u32),
    Add(u32),
    Compare(u32),
    Select(u32),
    Shift(u32),
    Round(u32),
    LeadingZeros(u32),
    NormalizeRead,
    PowerRead,
    ContextRead,
}
#[derive(Clone, Debug)]
pub struct Reservation {
    pub pixel: usize,
    pub event: usize,
    pub kind: Option<LaneKind>,
    pub lane: Option<usize>,
    pub issue: u64,
    pub ready: u64,
}
#[derive(Clone, Debug)]
pub struct ResultWrite {
    pub pixel: usize,
    pub arithmetic_ready: u64,
    pub issue: u64,
    pub ready: u64,
}
#[derive(Clone, Debug)]
pub struct SourceRead {
    pub pixel: Option<usize>,
    pub row: usize,
    pub lane: usize,
    pub issue: u64,
    pub ready: u64,
}
pub struct Plan {
    pub template: FrameReport,
    pub events: Vec<Reservation>,
    pub outputs: Vec<LightingOutput>,
    pub writes: Vec<ResultWrite>,
    pub hardware: Hardware,
    pub storage: Storage,
    pub cycles: u64,
    pub source_reads: usize,
    pub reads: Vec<SourceRead>,
    pub pixel_payload_bits: usize,
    pub uniform_payload_bits: usize,
    gates: Vec<u64>,
    binding: BoundDag,
    kernel: counted::Config,
}
fn width(w: u32) -> u32 {
    if w <= 18 {
        18
    } else if w <= 36 {
        36
    } else {
        54
    }
}
pub(super) fn kind(report: &FrameReport, event: usize) -> Result<Option<LaneKind>, String> {
    let e = &report.events[event];
    Ok(match e.resource {
        None => None,
        Some(Resource::Dsp18) => Some(
            if e.inputs.iter().all(|&v| report.values[v].format.bits <= 9) {
                LaneKind::SmallMultiply
            } else {
                LaneKind::LargeMultiply
            },
        ),
        Some(Resource::Adder(w)) => Some(LaneKind::Add(width(w))),
        Some(Resource::Compare(w)) => Some(LaneKind::Compare(width(w))),
        Some(Resource::Select(w)) => Some(LaneKind::Select(width(w))),
        Some(Resource::Shift(w)) => Some(LaneKind::Shift(width(w))),
        Some(Resource::RoundControl(w)) => Some(LaneKind::Round(width(w))),
        Some(Resource::LeadingZeros(w)) => Some(LaneKind::LeadingZeros(width(w))),
        Some(Resource::Read(m)) => {
            if report.memories[m].kind == MemoryKind::Input {
                None
            } else {
                Some(match report.memories[m].name.as_str() {
                    "SQ" | "RSQRT" => LaneKind::NormalizeRead,
                    "POWER" => LaneKind::PowerRead,
                    "POWER_CONTEXT" => LaneKind::ContextRead,
                    _ => return Err("unknown lighting store".into()),
                })
            }
        }
        _ => return Err("unmapped resource".into()),
    })
}
impl Hardware {
    pub fn dsp_inventory(self) -> Result<audited::physical::DspInventory, String> {
        use audited::physical::{DspInventory, DspMode};
        let paired = if self.binding == Binding::LightingDsp {
            self.paired_macros
        } else {
            0
        };
        DspInventory::pack(
            self.dsp_tiles,
            &[
                (
                    DspMode::Multiply9,
                    self.small_multiply,
                    self.multiply_latency,
                    1,
                ),
                (
                    DspMode::Multiply18,
                    self.large_multiply,
                    self.multiply_latency,
                    1,
                ),
                (DspMode::PairMultiplyAdd, paired, self.paired_latency, 1),
            ],
        )
        .map_err(|e| format!("DSP placement: {e:?}"))
    }
    /// Twice the 18x18-equivalent multiplier budget, avoiding fractional counts.
    pub fn multiplier_half_slots(self) -> usize {
        self.small_multiply
            + 2 * self.large_multiply
            + if self.binding == Binding::LightingDsp {
                4 * self.paired_macros
            } else {
                0
            }
    }
    /// Kind-separated macro budget; excludes fabric logic, routing and registers.
    pub fn multiplier_macros(self) -> usize {
        self.small_multiply.div_ceil(4)
            + self.large_multiply.div_ceil(2)
            + if self.binding == Binding::LightingDsp {
                self.paired_macros
            } else {
                0
            }
    }
    /// Same 9 large multiplier slots: 5 standalone, 2 dedicated pair+ALU macros.
    /// Pipeline latency assumptions are planning inputs, not fitted timing.
    pub fn lighting_dsp() -> Self {
        Self {
            binding: Binding::LightingDsp,
            large_multiply: 5,
            paired_macros: 2,
            narrow_adders: 8,
            incrementers_per_width: 8,
            rounders_per_width: 8,
            ..Self::default()
        }
    }
    /// Capacity target for the current full-path graph at II=2.
    /// One pair+ALU macro is enough for the two dot groups per pixel; returning
    /// the other macro to standalone multiplication preserves the DSP budget.
    /// Logic counts are ledger-based planning capacities, not fitted area.
    pub fn lighting_ii2() -> Self {
        Self {
            large_multiply: 7,
            paired_macros: 1,
            narrow_adders: 21,
            adders_per_width: 8,
            incrementers_per_width: 17,
            compares_per_width: 27,
            selects_per_width: 35,
            shifts_per_width: 8,
            rounders_per_width: 13,
            leading_zeros_per_width: 2,
            ..Self::lighting_dsp()
        }
    }
    pub fn lighting_optimized_ii2() -> Self {
        Self {
            kernel: counted::Config::optimized(),
            narrow_adders: 17,
            compares_per_width: 23,
            selects_per_width: 31,
            ..Self::lighting_ii2()
        }
    }
    /// Registered bounded-logic alternative; conservative two-cycle cone contract.
    pub fn lighting_architecture_ii2() -> Self {
        Self {
            kernel: counted::Config::architecture(),
            cone_depth: 4,
            cone_latency: 2,
            ..Self::lighting_optimized_ii2()
        }
    }
    /// Exploratory one-cycle cones; requires later synthesis/timing evidence.
    pub fn lighting_experimental_ii2() -> Self {
        Self {
            cone_latency: 1,
            ..Self::lighting_architecture_ii2()
        }
    }
    pub(crate) fn unit(self, k: &LaneKind) -> (usize, u64) {
        match k {
            LaneKind::LogicCone { .. } => (self.cone_lanes_per_shape, self.cone_latency),
            LaneKind::SmallMultiply => (self.small_multiply, self.multiply_latency),
            LaneKind::LargeMultiply => (self.large_multiply, self.multiply_latency),
            LaneKind::PairMultiplyAdd => (self.paired_macros, self.paired_latency),
            LaneKind::Negate(_) => (self.negators_per_width, self.logic_latency),
            LaneKind::Increment(_) => (self.incrementers_per_width, self.logic_latency),
            LaneKind::Add(18) if self.binding == Binding::LightingDsp => {
                (self.narrow_adders, self.logic_latency)
            }
            LaneKind::Add(_) => (self.adders_per_width, self.logic_latency),
            LaneKind::Compare(_) => (self.compares_per_width, self.logic_latency),
            LaneKind::Select(_) => (self.selects_per_width, self.logic_latency),
            LaneKind::Shift(_) => (self.shifts_per_width, self.logic_latency),
            LaneKind::Round(_) => (self.rounders_per_width, self.logic_latency),
            LaneKind::LeadingZeros(_) => (self.leading_zeros_per_width, self.logic_latency),
            LaneKind::NormalizeRead => (self.normalize_reads, self.rom_latency),
            LaneKind::PowerRead | LaneKind::ContextRead => (1, self.rom_latency),
        }
    }
}
pub(super) fn dependencies(template: &FrameReport, event: usize) -> Vec<usize> {
    let e = &template.events[event];
    let mut deps: Vec<_> = e
        .inputs
        .iter()
        .map(|&v| template.values[v].producer)
        .collect();
    deps.extend(e.control);
    deps.sort_unstable();
    deps.dedup();
    deps
}

pub fn plan(
    pixels: &[PixelInput],
    material: Material,
    light: Light,
    projection: Projection,
    hardware: Hardware,
    storage: Storage,
    strategy: Strategy,
) -> Result<Plan, String> {
    if pixels.is_empty() || pixels.len() > 64 {
        return Err("batch must contain 1..64 pixels".into());
    }
    if (hardware.kernel.shared_half || hardware.kernel.dataflow || hardware.kernel.flat_normal)
        && !matches!(storage, Storage::Registers)
    {
        return Err("architecture context currently requires explicit register inputs".into());
    }
    if hardware.max_cycles == 0 || hardware.max_cycles > 1_000_000 {
        return Err("max_cycles must be 1..1000000".into());
    }
    if hardware.kernel.flat_normal && pixels.iter().any(|p| p.normal != pixels[0].normal) {
        return Err("flat-normal promise requires identical raw normals".into());
    }
    let template_pixel = PixelInput {
        normal: [0; 3],
        ndc: [0; 2],
    };
    let template = counted::evaluate_with_config(
        template_pixel,
        material,
        light,
        projection,
        2048,
        hardware.kernel,
    )
    .map_err(|e| format!("template: {e:?}"))?
    .frame;
    let mut outputs = Vec::new();
    let binding = BoundDag::new(&template, hardware)?;
    // Physical context rows are latched before logical counted context reads.
    let _context_rows =
        UniformRows::encode(material, light, projection).map_err(|e| format!("context: {e:?}"))?;
    for &p in pixels {
        let report =
            counted::evaluate_with_config(p, material, light, projection, 2048, hardware.kernel)
                .map_err(|e| format!("pixel: {e:?}"))?;
        BoundDag::new(&report.frame, hardware)?;
        // The only data-dependent skipped arithmetic is the power endpoint.
        for (resource, count) in &report.frame.counts.resources {
            if *count
                > template
                    .counts
                    .resources
                    .get(resource)
                    .copied()
                    .unwrap_or(0)
            {
                return Err("template no longer bounds this kernel; rebuild reservations".into());
            }
        }
        outputs.push(report.output);
    }
    let serial_span = if strategy == Strategy::Serial && pixels.len() > 1 {
        plan(
            &pixels[..1],
            material,
            light,
            projection,
            hardware,
            storage,
            Strategy::Interleaved,
        )?
        .cycles
    } else {
        0
    };
    let mut units = BTreeMap::<LaneKind, Vec<u64>>::new();
    for e in 0..template.events.len() {
        if let Some(k) = binding.kinds[e].clone() {
            let (lanes, latency) = hardware.unit(&k);
            if lanes == 0 || lanes > 64 || latency == 0 || latency > hardware.max_cycles {
                return Err("invalid hardware".into());
            }
            units.entry(k).or_insert_with(|| vec![0; lanes]);
        }
    }
    let n = pixels.len();
    let stride = template.events.len();
    let total = n * stride;
    let mut source_reads = 0;
    let mut reads = Vec::new();
    let (uniform_ready, pixel_ready) = match storage {
        Storage::Registers => (0, vec![[0; 3]; n]),
        Storage::Rows {
            read_lanes,
            latency,
        } => {
            if read_lanes == 0 || read_lanes > 64 || latency == 0 || latency > hardware.max_cycles {
                return Err("invalid read interface".into());
            }
            let mut slots = vec![0; read_lanes];
            let mut read = |earliest: u64, pixel: Option<usize>, row: usize| {
                let (lane, free) = slots
                    .iter()
                    .copied()
                    .enumerate()
                    .min_by_key(|(_, t)| *t)
                    .unwrap();
                let issue = free.max(earliest);
                slots[lane] = issue + 1;
                source_reads += 1;
                reads.push(SourceRead {
                    pixel,
                    row,
                    lane,
                    issue,
                    ready: issue + latency,
                });
                issue + latency
            };
            let uniform = (0..4).map(|row| read(0, None, row)).max().unwrap();
            let pixel = (0..n)
                .map(|pixel| std::array::from_fn(|row| read(uniform, Some(pixel), row)))
                .collect();
            (uniform, pixel)
        }
    };
    let mut deps_left = vec![0; total];
    let mut children = vec![Vec::new(); total];
    let mut earliest = vec![0; total];
    let mut gates = vec![0; total];
    for pixel in 0..n {
        let release = pixel as u64 * serial_span;
        for event in 0..stride {
            let id = pixel * stride + event;
            gates[id] = release;
            if let Operation::Read { memory, row } = template.events[event].operation {
                if template.memories[memory].kind == MemoryKind::Input {
                    gates[id] = gates[id].max(match template.memories[memory].name.as_str() {
                        "pixel.rows" => pixel_ready[pixel][row],
                        _ => uniform_ready,
                    });
                }
            }
            earliest[id] = gates[id];
            let deps = &binding.dependencies[event];
            deps_left[id] = deps.len();
            for &dep in deps {
                children[pixel * stride + dep].push(id);
            }
        }
    }
    let mut heap = BinaryHeap::new();
    for id in 0..total {
        if deps_left[id] == 0 {
            heap.push(Reverse((earliest[id], id)));
        }
    }
    let mut events: Vec<Option<Reservation>> = vec![None; total];
    while let Some(Reverse((start, id))) = heap.pop() {
        let pixel = id / stride;
        let event = id % stride;
        let k = binding.kinds[event].clone();
        let (issue, ready, lane) = if let Some(k) = &k {
            let slots = units.get_mut(k).unwrap();
            let (lane, free) = slots
                .iter()
                .copied()
                .enumerate()
                .min_by_key(|(_, t)| *t)
                .unwrap();
            let issue = start.max(free);
            slots[lane] = issue + 1;
            (issue, issue + hardware.unit(k).1, Some(lane))
        } else {
            (start, start, None)
        };
        if ready > hardware.max_cycles {
            return Err("cycle limit".into());
        }
        events[id] = Some(Reservation {
            pixel,
            event,
            kind: k,
            lane,
            issue,
            ready,
        });
        for &child in &children[id] {
            earliest[child] = earliest[child].max(ready);
            deps_left[child] -= 1;
            if deps_left[child] == 0 {
                heap.push(Reverse((earliest[child], child)));
            }
        }
    }
    let events: Vec<_> = events
        .into_iter()
        .map(|v| v.ok_or_else(|| "dependency cycle".to_owned()))
        .collect::<Result<_, _>>()?;
    let result_events: Vec<_> = template
        .events
        .iter()
        .filter(|e| matches!(&e.operation,Operation::Publish(name) if name=="g" || name=="h"))
        .map(|e| e.id)
        .collect();
    let mut writes = Vec::new();
    let mut output_free = 0;
    for pixel in 0..n {
        let arithmetic_ready = result_events
            .iter()
            .map(|&e| events[pixel * stride + e].ready)
            .max()
            .unwrap();
        let issue = arithmetic_ready.max(output_free);
        output_free = issue + 1;
        writes.push(ResultWrite {
            pixel,
            arithmetic_ready,
            issue,
            ready: output_free,
        });
    }
    if output_free > hardware.max_cycles {
        return Err("output cycle limit".into());
    }
    let plan = Plan {
        template,
        events,
        outputs,
        writes,
        hardware,
        storage,
        cycles: output_free,
        source_reads,
        reads,
        pixel_payload_bits: (if hardware.kernel.shared_half || hardware.kernel.prepared_ray {
            96
        } else {
            84
        }) * n,
        uniform_payload_bits: 121
            + if hardware.kernel.dataflow { 43 } else { 0 }
            + if hardware.kernel.flat_normal { 100 } else { 0 },
        gates,
        binding,
        kernel: hardware.kernel,
    };
    plan.audit()?;
    Ok(plan)
}

impl Plan {
    /// Search the same arithmetic graph, capacities, input gates and ordered output port.
    /// Keep the original plan if none of the bounded candidates improves its completion.
    pub fn optimize(
        mut self,
        candidates: usize,
    ) -> Result<(Self, resource_scheduler::SearchOutcome), String> {
        use resource_scheduler::{Graph, Limits, Node, Resource as Unit, SearchConfig};
        self.audit()?;
        if candidates == 0 || candidates > 64 {
            return Err("candidate budget must be 1..64".into());
        }
        let mut graph = Graph::default();
        let mut resource_ids = BTreeMap::new();
        let stride = self.template.events.len();
        for (id, r) in self.events.iter().enumerate() {
            let resource = r.kind.as_ref().map(|k| {
                *resource_ids.entry(k.clone()).or_insert_with(|| {
                    let (lanes, latency) = self.hardware.unit(k);
                    let index = graph.resources.len();
                    graph.resources.push(Unit {
                        name: format!("{k:?}"),
                        lanes,
                        latency,
                        initiation_interval: 1,
                    });
                    index
                })
            });
            graph.nodes.push(Node {
                name: format!("pixel{}.event{}", r.pixel, r.event),
                predecessors: self.binding.dependencies[r.event]
                    .iter()
                    .map(|e| r.pixel * stride + e)
                    .collect(),
                earliest: self.gates[id],
                resource,
            });
        }
        let output_resource = graph.resources.len();
        graph.resources.push(Unit {
            name: "ordered-result-row".into(),
            lanes: 1,
            latency: 1,
            initiation_interval: 1,
        });
        let offset = graph.nodes.len();
        for pixel in 0..self.outputs.len() {
            let mut predecessors: Vec<_> = self
                .template
                .events
                .iter()
                .filter(
                    |e| matches!(&e.operation,Operation::Publish(name) if name=="g" || name=="h"),
                )
                .map(|e| pixel * stride + e.id)
                .collect();
            if pixel > 0 {
                predecessors.push(offset + pixel - 1);
            }
            graph.nodes.push(Node {
                name: format!("pixel{pixel}.write"),
                predecessors,
                earliest: 0,
                resource: Some(output_resource),
            });
        }
        let limits = Limits::new(150_000, self.hardware.max_cycles, candidates);
        let outcome = resource_scheduler::plan(&graph, &limits, &SearchConfig::default())
            .map_err(|e| format!("search: {e:?}"))?;
        // Recheck every feasible candidate rather than trusting search counters.
        for c in &outcome.candidates {
            if c.within_deadline && !resource_scheduler::check(&graph, &limits, &c.schedule).is_ok()
            {
                return Err("candidate audit".into());
            }
        }
        let best = outcome.best_candidate();
        if best.makespan <= self.cycles {
            for (r, s) in self.events.iter_mut().zip(&best.schedule.nodes) {
                r.issue = s.issue;
                r.ready = s.ready;
                r.lane = s.lane;
            }
            for (pixel, w) in self.writes.iter_mut().enumerate() {
                w.arithmetic_ready=self.template.events.iter().filter(|e|matches!(&e.operation,Operation::Publish(name) if name=="g" || name=="h")).map(|e|self.events[pixel*stride+e.id].ready).max().ok_or("missing g/h")?;
                w.issue = best.schedule.nodes[offset + pixel].issue;
                w.ready = best.schedule.nodes[offset + pixel].ready;
            }
            self.cycles = self.writes.last().ok_or("missing writes")?.ready;
            self.audit()?;
        }
        Ok((self, outcome))
    }
    /// Verify every static slot, input gate, dependency and ordered result write.
    pub fn audit(&self) -> Result<(), String> {
        self.hardware
            .dsp_inventory()?
            .audit()
            .map_err(|e| format!("DSP inventory: {e:?}"))?;
        if self.kernel != self.hardware.kernel {
            return Err("kernel config certificate".into());
        }
        self.template
            .audit()
            .map_err(|e| format!("template: {e:?}"))?;
        self.binding
            .audit_logic_depth(&self.template, self.hardware)?;
        let expected = BoundDag::new(&self.template, self.hardware)?;
        if expected != self.binding {
            return Err("binding certificate".into());
        }
        let stride = self.template.events.len();
        if stride == 0
            || self.outputs.is_empty()
            || self.outputs.len() > 64
            || self.events.len() != stride * self.outputs.len()
            || self.gates.len() != self.events.len()
            || self.writes.len() != self.outputs.len()
            || self.hardware.max_cycles == 0
            || self.hardware.max_cycles > 1_000_000
            || self.events.iter().enumerate().any(|(id, r)| {
                r.pixel != id / stride
                    || r.event != id % stride
                    || r.ready > self.hardware.max_cycles
                    || r.issue > r.ready
            })
        {
            return Err("plan shape or identity".into());
        }
        if self.source_reads != self.reads.len() {
            return Err("input read total".into());
        }
        match self.storage {
            Storage::Registers => {
                if !self.reads.is_empty() {
                    return Err("register input reads".into());
                }
            }
            Storage::Rows {
                read_lanes,
                latency,
            } => {
                if read_lanes == 0
                    || read_lanes > 64
                    || latency == 0
                    || latency > self.hardware.max_cycles
                    || self.reads.len() != 4 + 3 * self.outputs.len()
                {
                    return Err("input row coverage".into());
                }
                let mut seen = std::collections::BTreeSet::new();
                let mut calendar = BTreeMap::<usize, Vec<u64>>::new();
                for r in &self.reads {
                    if r.lane >= read_lanes
                        || r.issue.checked_add(latency) != Some(r.ready)
                        || r.ready > self.hardware.max_cycles
                        || r.row >= if r.pixel.is_some() { 3 } else { 4 }
                        || r.pixel.is_some_and(|p| p >= self.outputs.len())
                        || !seen.insert((r.pixel, r.row))
                    {
                        return Err("input read slot or identity".into());
                    }
                    calendar.entry(r.lane).or_default().push(r.issue);
                }
                for times in calendar.values_mut() {
                    times.sort_unstable();
                    if times.windows(2).any(|v| v[1] <= v[0]) {
                        return Err("input port collision".into());
                    }
                }
                let uniform_ready = self
                    .reads
                    .iter()
                    .filter(|r| r.pixel.is_none())
                    .map(|r| r.ready)
                    .max()
                    .unwrap();
                if self
                    .reads
                    .iter()
                    .any(|r| r.pixel.is_some() && r.issue < uniform_ready)
                {
                    return Err("context not loaded".into());
                }
                for r in &self.events {
                    if let Operation::Read { memory, row } = self.template.events[r.event].operation
                    {
                        if self.template.memories[memory].kind == MemoryKind::Input {
                            let ready = if self.template.memories[memory].name == "pixel.rows" {
                                self.reads
                                    .iter()
                                    .find(|s| s.pixel == Some(r.pixel) && s.row == row)
                                    .unwrap()
                                    .ready
                            } else {
                                uniform_ready
                            };
                            if r.issue < ready {
                                return Err("physical input not ready".into());
                            }
                        }
                    }
                }
            }
        }
        if self.events.len() != stride * self.outputs.len()
            || self.gates.len() != self.events.len()
            || self.writes.len() != self.outputs.len()
            || self.cycles > self.hardware.max_cycles
        {
            return Err("plan shape or deadline".into());
        }
        let mut issues = BTreeMap::<(LaneKind, usize), Vec<u64>>::new();
        for (id, r) in self.events.iter().enumerate() {
            if r.pixel != id / stride
                || r.event != id % stride
                || r.kind != self.binding.kinds[r.event]
                || r.issue < self.gates[id]
            {
                return Err("slot identity/resource/gate".into());
            }
            if let Some(k) = &r.kind {
                let (lanes, latency) = self.hardware.unit(k);
                let lane = r.lane.ok_or("missing lane")?;
                if lanes == 0
                    || lanes > 64
                    || latency == 0
                    || lane >= lanes
                    || r.issue.checked_add(latency) != Some(r.ready)
                {
                    return Err("lane or latency".into());
                }
                issues.entry((k.clone(), lane)).or_default().push(r.issue);
            } else if r.lane.is_some() || r.issue != r.ready {
                return Err("wiring timing".into());
            }
            for &d in &self.binding.dependencies[r.event] {
                if self.events[r.pixel * stride + d].ready > r.issue {
                    return Err("operand/control not ready".into());
                }
            }
        }
        for times in issues.values_mut() {
            times.sort_unstable();
            if times.windows(2).any(|pair| pair[1] < pair[0] + 1) {
                return Err("lane collision".into());
            }
        }
        let mut previous = 0;
        for (pixel, w) in self.writes.iter().enumerate() {
            let actual = self
                .template
                .events
                .iter()
                .filter(
                    |e| matches!(&e.operation,Operation::Publish(name) if name=="g" || name=="h"),
                )
                .map(|e| self.events[pixel * stride + e.id].ready)
                .max()
                .ok_or("missing g/h")?;
            if w.pixel != pixel
                || w.arithmetic_ready != actual
                || w.issue < actual
                || w.issue < previous
                || w.issue.checked_add(1) != Some(w.ready)
                || w.ready > self.hardware.max_cycles
            {
                return Err("ordered result write".into());
            }
            previous = w.ready;
        }
        if self.cycles != previous {
            return Err("completion total".into());
        }
        Ok(())
    }
    pub fn work(&self) -> BTreeMap<LaneKind, usize> {
        let mut counts = BTreeMap::new();
        for r in &self.events {
            if let Some(k) = &r.kind {
                *counts.entry(k.clone()).or_default() += 1;
            }
        }
        counts
    }
    pub fn logic_cones(&self) -> &[audited::physical::LogicCone] {
        &self.binding.cones
    }
    pub fn fused_groups(&self) -> &[FusedGroup] {
        &self.binding.groups
    }
    pub fn compare_oracle(
        &self,
        pixels: &[PixelInput],
        material: Material,
        light: Light,
        projection: Projection,
    ) -> Result<(), String> {
        if pixels.len() != self.outputs.len() {
            return Err("pixel count".into());
        }
        for (&pixel, output) in pixels.iter().zip(&self.outputs) {
            let golden = oracle::evaluate(
                pixel,
                material,
                light,
                projection,
                oracle::Config {
                    rounding: oracle::RoundingPolicy {
                        power: if self.kernel.power_floor {
                            oracle::Rounding::Floor
                        } else {
                            oracle::Rounding::NearestEven
                        },
                        ..oracle::RoundingPolicy::default()
                    },
                    ..oracle::Config::default()
                },
            )
            .map_err(|e| format!("{e:?}"))?;
            if i128::from(output.g) != golden.g || i128::from(output.h) != golden.h {
                return Err("numerical mismatch".into());
            }
        }
        Ok(())
    }
}
