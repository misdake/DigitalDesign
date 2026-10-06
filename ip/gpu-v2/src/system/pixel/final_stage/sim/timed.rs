//! Finite-resource timing for the final-color leaf.
//!
//! Two independent certificates are produced here:
//!
//! * A **fixed stage register calendar** (`StageCalendar`) that binds every
//!   audited operation to the exact pipeline stage and dedicated lane the
//!   emulator and the emitted RTL implement. It is derived from the audited
//!   dependency graph, then checked against the hand-declared pattern
//!   (`tint*texture` and `specular*h` at stage 1, `base*g` at 4, `sum` at 5,
//!   `+127` at 6, `+bit` at 7, `compare` at 8, `select` at 9). A generic
//!   modulo schedule is still run as an II=1 feasibility gate, but it never
//!   supplies the plan's latency: the implementation calendar is fixed.
//! * A **credit-aware finite completion** (`credit::CreditCalendar`). The
//!   arithmetic span `latency + n - 1` is only the pipelining lower bound; with
//!   only four result credits and nine stages the engine also stalls on
//!   credit exhaustion. `Plan::arithmetic_lower_bound` reports the former and
//!   `Plan::completion` the latter. Neither claims one pixel per clock
//!   indefinitely for this finite leaf.
//!
//! Inputs, slices and zero-extends are wires: they add no register stage. Adds
//! are classified as literal (`+128`, `+127`), step/increment (`+1` host
//! addressing) or full variable adds (`t + (t >> 8)`, `base*g + spec*h`,
//! `+bit`). `Dsp18` is a *logical* 18-bit-class multiply: the model counts a
//! combinational multiply followed by an output register. It is not a placed
//! or timing-proven Gowin DSP macro; no PnR, area or fmax claim is made.

use super::super::{math, Input, PIPELINE_LATENCY, PIPELINE_STAGES, RESULT_CAPACITY};
use super::{counted, credit};
use audited::physical::bound_dependencies;
use audited::{Event, FrameReport, Operation, Resource};
use resource_scheduler::{
    check_modulo, modulo_schedule_bounded, Graph, Limits, ModuloGraph, Node, Resource as Lane,
    SearchConfig,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hardware {
    /// Logical 18-bit-class multiply units (combinational multiply plus output
    /// register). Not placed Gowin DSP macros.
    pub mul18_lanes: usize,
    /// 16-bit add units (per channel: `t = p + 128`, `b = t + (t >> 8)`).
    pub add16_lanes: usize,
    /// 18-bit add units (`sum`, `+127`, `+bit`).
    pub add18_lanes: usize,
    pub compare_lanes: usize,
    pub select_lanes: usize,
    pub max_nodes: usize,
    pub max_cycles: u64,
    pub max_candidates: usize,
}
impl Default for Hardware {
    fn default() -> Self {
        // One dedicated lane per (stage, operand): stage 1 has six multiplies,
        // stage 4 three; stages 2/3 three 16-bit adds each; stages 5/6/7 three
        // 18-bit adds each; stage 8 three compares; stage 9 three selects.
        Self {
            mul18_lanes: 9,
            add16_lanes: 6,
            add18_lanes: 9,
            compare_lanes: 3,
            select_lanes: 3,
            max_nodes: 1024,
            max_cycles: 1_000_000,
            max_candidates: 16,
        }
    }
}
impl Hardware {
    fn validate(self) -> Result<(), String> {
        if self.mul18_lanes == 0
            || self.add16_lanes == 0
            || self.add18_lanes == 0
            || self.compare_lanes == 0
            || self.select_lanes == 0
            || self.max_nodes == 0
            || self.max_cycles == 0
            || !(1..=64).contains(&self.max_candidates)
        {
            return Err("final timed hardware bounds".into());
        }
        Ok(())
    }
    fn lanes(self, kind: LaneKind) -> usize {
        match kind {
            LaneKind::Multiply18 => self.mul18_lanes,
            LaneKind::Add16 => self.add16_lanes,
            LaneKind::Add18 => self.add18_lanes,
            LaneKind::Compare => self.compare_lanes,
            LaneKind::Select => self.select_lanes,
        }
    }
}

/// Physical lane class. `Multiply18` is the logical Dsp18 class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LaneKind {
    Multiply18,
    Add16,
    Add18,
    Compare,
    Select,
}

/// Numerical class of a registered operation. Literal adds and `+1` steps are
/// deliberately separated from full variable adds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpClass {
    Multiply,
    LiteralAdd,
    Increment,
    VariableAdd,
    Compare,
    Select,
}

/// Per-event calendar entry. Wiring events (inputs, literals, slices, resizes
/// and product mappings) have `lane_kind == None`, `registered == false` and
/// never consume a lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageBinding {
    pub event: usize,
    pub operation: String,
    /// 1-based register stage this event is produced into; for wiring it is the
    /// stage whose register the value is combinationally derived from (0 at the
    /// frame input).
    pub stage: u8,
    pub registered: bool,
    pub class: Option<OpClass>,
    pub lane_kind: Option<LaneKind>,
    /// Operand/output width used for accounting.
    pub width: u32,
    pub lane: Option<usize>,
}

/// Fixed stage register calendar and dedicated lane binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageCalendar {
    /// Number of register stages (pipeline latency in enabled edges).
    pub latency: u64,
    pub bindings: Vec<StageBinding>,
    /// Lanes consumed per class; each stage owns a disjoint lane range.
    pub lanes_used: BTreeMap<LaneKind, usize>,
}
impl StageCalendar {
    /// The hand-declared implementation calendar. The audited DAG must
    /// reproduce this per-stage class census exactly; the emulator state
    /// machine and the emitted RTL implement the same assignment.
    pub fn fixed_pattern() -> BTreeMap<(u8, OpClass), usize> {
        [
            ((1, OpClass::Multiply), 6),
            ((2, OpClass::LiteralAdd), 3),
            ((3, OpClass::VariableAdd), 3),
            ((4, OpClass::Multiply), 3),
            ((5, OpClass::VariableAdd), 3),
            ((6, OpClass::LiteralAdd), 3),
            ((7, OpClass::VariableAdd), 3),
            ((8, OpClass::Compare), 3),
            ((9, OpClass::Select), 3),
        ]
        .into_iter()
        .collect()
    }

    /// Longest registered path; wiring adds no stage.
    fn classify(
        frame: &FrameReport,
        event: &Event,
    ) -> Result<Option<(OpClass, LaneKind, u32)>, String> {
        let literal_input = |v: usize| {
            matches!(
                frame.events[frame.values[v].producer].operation,
                Operation::Literal
            )
        };
        Ok(match (&event.operation, event.resource) {
            (Operation::Multiply, Some(Resource::Dsp18)) => {
                Some((OpClass::Multiply, LaneKind::Multiply18, 18))
            }
            (Operation::Add, Some(Resource::Adder(width))) => {
                let literals: Vec<i128> = event
                    .inputs
                    .iter()
                    .filter(|&&v| literal_input(v))
                    .map(|&v| frame.values[v].raw)
                    .collect();
                let class = match literals.as_slice() {
                    [1] => OpClass::Increment,
                    [] => OpClass::VariableAdd,
                    _ => OpClass::LiteralAdd,
                };
                let kind = if width <= 16 {
                    LaneKind::Add16
                } else {
                    LaneKind::Add18
                };
                Some((class, kind, width))
            }
            (Operation::Less, Some(Resource::Compare(width))) => {
                Some((OpClass::Compare, LaneKind::Compare, width))
            }
            (Operation::Select, Some(Resource::Select(width))) => {
                Some((OpClass::Select, LaneKind::Select, width))
            }
            // Input reads and every resource-free operation (literals, slices,
            // resizes, product mappings) are combinational wiring.
            (_, None) | (_, Some(Resource::Read(_))) => None,
            (operation, resource) => {
                return Err(format!(
                    "unexpected final calendar operation {operation:?} on {resource:?}"
                ))
            }
        })
    }

    /// Derive the fixed calendar from the audited DAG and bind dedicated lanes.
    pub fn build(frame: &FrameReport, hardware: Hardware) -> Result<Self, String> {
        let dependencies = bound_dependencies(frame, &[]).map_err(|e| format!("{e:?}"))?;
        let mut depth = vec![0u8; frame.events.len()];
        let mut bindings = Vec::with_capacity(frame.events.len());
        for (id, event) in frame.events.iter().enumerate() {
            let mut predecessor_depth = 0u8;
            for &p in &dependencies[id] {
                predecessor_depth = predecessor_depth.max(depth[p]);
            }
            let classified = Self::classify(frame, event)?;
            let registered = classified.is_some();
            let stage = if registered {
                predecessor_depth + 1
            } else {
                predecessor_depth
            };
            if stage > 63 {
                return Err("final calendar stage depth".into());
            }
            depth[id] = stage;
            let (class, lane_kind, width) = match classified {
                Some((class, kind, width)) => (Some(class), Some(kind), width),
                None => (None, None, 0),
            };
            bindings.push(StageBinding {
                event: id,
                operation: format!("{:?}", event.operation),
                stage,
                registered,
                class,
                lane_kind,
                width,
                lane: None,
            });
        }
        let latency = depth.iter().copied().max().unwrap_or(0) as u64;
        // Dedicated per-stage lane ranges: all nine stages execute concurrently
        // at II=1, so no physical lane may serve two stages.
        let mut lanes_used: BTreeMap<LaneKind, usize> = BTreeMap::new();
        for stage in 1..=latency {
            for binding in bindings.iter_mut() {
                if binding.stage as u64 != stage {
                    continue;
                }
                if let Some(kind) = binding.lane_kind {
                    let lane = lanes_used.entry(kind).or_default();
                    binding.lane = Some(*lane);
                    *lane += 1;
                }
            }
        }
        for (&kind, &used) in &lanes_used {
            if used > hardware.lanes(kind) {
                return Err(format!("final calendar starves {kind:?} ({used})"));
            }
        }
        Ok(Self {
            latency,
            bindings,
            lanes_used,
        })
    }

    /// Registered operations per `(stage, class)`.
    pub fn class_census(&self) -> BTreeMap<(u8, OpClass), usize> {
        let mut census = BTreeMap::new();
        for binding in &self.bindings {
            if let (Some(class), true) = (binding.class, binding.registered) {
                *census.entry((binding.stage, class)).or_default() += 1;
            }
        }
        census
    }

    /// Independent replay: rebuild from the frame and check the fixed pattern,
    /// the dependency order and the dedicated lane ranges.
    pub fn audit(&self, frame: &FrameReport, hardware: Hardware) -> Result<(), String> {
        let rebuilt = Self::build(frame, hardware)?;
        if format!("{rebuilt:?}") != format!("{self:?}") {
            return Err("forged final stage calendar".into());
        }
        if self.latency != PIPELINE_STAGES as u64 {
            return Err("final calendar latency".into());
        }
        if self.class_census() != Self::fixed_pattern() {
            return Err("final fixed stage pattern".into());
        }
        let dependencies = bound_dependencies(frame, &[]).map_err(|e| format!("{e:?}"))?;
        for binding in &self.bindings {
            for &p in &dependencies[binding.event] {
                if self.bindings[p].stage > binding.stage {
                    return Err("final calendar dependency order".into());
                }
                if binding.registered
                    && self.bindings[p].registered
                    && self.bindings[p].stage >= binding.stage
                {
                    return Err("final calendar same-stage registered dependency".into());
                }
            }
        }
        let mut seen: BTreeMap<LaneKind, usize> = BTreeMap::new();
        for binding in &self.bindings {
            if let Some(kind) = binding.lane_kind {
                let lane = binding
                    .lane
                    .ok_or_else(|| "final calendar missing lane".to_string())?;
                if lane >= hardware.lanes(kind) {
                    return Err("final calendar lane bound".into());
                }
                *seen.entry(kind).or_default() += 1;
            } else if binding.lane.is_some() {
                return Err("final calendar wiring lane".into());
            }
        }
        if seen != self.lanes_used {
            return Err("final lane census".into());
        }
        Ok(())
    }
}

pub struct Plan {
    /// Closed numerical batch, independently re-checked against `math`.
    pub counted: counted::Report,
    /// Single-pixel template frame (no cross-pixel index chain).
    pub template: FrameReport,
    /// Single-pixel template DAG.
    pub graph: Graph,
    /// Fixed stage register calendar; the authoritative implementation latency.
    pub calendar: StageCalendar,
    pub hardware: Hardware,
    /// Original inputs, retained for an independent numerical re-check.
    pub pixels: Vec<Input>,
    /// Fixed implementation latency in enabled edges (`calendar.latency`).
    pub latency: u64,
    pub initiation_interval: u64,
    /// Arithmetic-only lower bound `latency + n - 1`: no credit stall, one
    /// pixel accepted and published per enabled edge. Not finite completion.
    pub arithmetic_lower_bound: u64,
    /// Credit-aware finite completion under continuous request and drain.
    pub completion: credit::CreditCompletion,
}

fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

/// Lower a counted frame into the finite-resource DAG. Input reads, literals,
/// slices, resizes and product mappings are zero-latency wiring.
fn graph(frame: &FrameReport, hardware: Hardware) -> Result<Graph, String> {
    let dependencies = bound_dependencies(frame, &[]).map_err(err)?;
    let mut resources: Vec<Lane> = Vec::new();
    let mut mapping: BTreeMap<String, usize> = BTreeMap::new();
    let mut nodes = Vec::with_capacity(frame.events.len());
    for (id, event) in frame.events.iter().enumerate() {
        let spec = match event.resource {
            Some(Resource::Dsp18) => Some(("mul18", hardware.mul18_lanes)),
            Some(Resource::Adder(w)) if w <= 16 => Some(("add16", hardware.add16_lanes)),
            Some(Resource::Adder(_)) => Some(("add18", hardware.add18_lanes)),
            Some(Resource::Compare(_)) => Some(("cmp10", hardware.compare_lanes)),
            Some(Resource::Select(_)) => Some(("sel10", hardware.select_lanes)),
            Some(Resource::Read(_)) | None => None,
            Some(other) => return Err(format!("unexpected final resource {other:?}")),
        };
        let resource = spec.map(|(name, lanes)| {
            *mapping.entry(name.to_string()).or_insert_with(|| {
                let id = resources.len();
                resources.push(Lane {
                    name: name.to_string(),
                    lanes,
                    latency: 1,
                    initiation_interval: 1,
                });
                id
            })
        });
        let mut predecessors = dependencies[id].clone();
        predecessors.sort_unstable();
        predecessors.dedup();
        nodes.push(Node {
            name: format!("event.{id}"),
            predecessors,
            earliest: 0,
            resource,
        });
    }
    Ok(Graph { nodes, resources })
}

/// Build and independently audit the finite-resource pipeline plan.
pub fn run(pixels: &[Input], hardware: Hardware) -> Result<Plan, String> {
    hardware.validate()?;
    let counted = counted::run(pixels)?;
    let template = counted::run(&pixels[..1])?;
    let graph = graph(&template.frame, hardware)?;
    let calendar = StageCalendar::build(&template.frame, hardware)?;
    calendar.audit(&template.frame, hardware)?;
    let latency = calendar.latency;
    if latency != PIPELINE_LATENCY as u64 {
        return Err("final fixed latency".into());
    }
    // Generic II=1 modulo schedule: a feasibility floor, never the plan's
    // latency or calendar. The audited DAG must be modulo-schedulable with the
    // declared lanes, and that generic schedule must fit the fixed latency.
    let limits = Limits::new(
        hardware.max_nodes,
        hardware.max_cycles,
        hardware.max_candidates,
    );
    let modulo_graph = ModuloGraph::from_graph(&graph).map_err(err)?;
    let modulo = modulo_schedule_bounded(&modulo_graph, 1, &limits, &SearchConfig::default())
        .map_err(|e| format!("final II=1 infeasible: {e:?}"))?;
    let report = check_modulo(&modulo_graph, &modulo);
    if !report.is_ok() {
        return Err(format!("final modulo certificate: {report:?}"));
    }
    if modulo.span > latency {
        return Err("final fixed calendar slower than generic schedule".into());
    }
    let arithmetic_lower_bound = latency + pixels.len() as u64 - 1;
    if arithmetic_lower_bound > hardware.max_cycles {
        return Err("final timed deadline".into());
    }
    let completion = credit::CreditCalendar::leaf()
        .completion(pixels.len(), hardware.max_cycles)
        .map_err(|e| format!("final credit completion: {e}"))?;
    let plan = Plan {
        counted,
        template: template.frame,
        graph,
        calendar,
        hardware,
        pixels: pixels.to_vec(),
        latency,
        initiation_interval: 1,
        arithmetic_lower_bound,
        completion,
    };
    plan.audit()?;
    Ok(plan)
}

impl Plan {
    /// Independent replay of the graph, the fixed calendar, the credit
    /// completion and the numerical payload; no search state is trusted.
    pub fn audit(&self) -> Result<(), String> {
        self.counted.frame.audit().map_err(err)?;
        if self.counted.outputs.len() != self.pixels.len() {
            return Err("final timed batch shape".into());
        }
        for (pixel, output) in self.pixels.iter().zip(&self.counted.outputs) {
            if output.rgb != math::reference_rgb(pixel) || output.key != pixel.key {
                return Err("final timed numerical payload".into());
            }
        }
        let rebuilt = graph(&self.template, self.hardware)?;
        if format!("{rebuilt:?}") != format!("{:?}", self.graph) {
            return Err("forged final dependency/resource graph".into());
        }
        self.calendar.audit(&self.template, self.hardware)?;
        if self.latency != self.calendar.latency || self.initiation_interval != 1 {
            return Err("final latency/II certificate".into());
        }
        if self.arithmetic_lower_bound != self.latency + self.counted.outputs.len() as u64 - 1 {
            return Err("final arithmetic lower-bound formula".into());
        }
        let completion = credit::CreditCalendar::leaf()
            .completion(self.pixels.len(), self.hardware.max_cycles)
            .map_err(|e| format!("final credit completion: {e}"))?;
        if completion != self.completion {
            return Err("forged final credit completion".into());
        }
        self.completion_shape()?;
        Ok(())
    }

    fn completion_shape(&self) -> Result<(), String> {
        let n = self.pixels.len();
        let c = &self.completion;
        if c.accepted != n
            || c.retired != n
            || c.first_accept_edge.is_none()
            || c.first_publish_edge.is_none()
            || c.first_retire_edge.is_none()
            || c.enabled_edges < self.arithmetic_lower_bound
            || c.max_in_flight > RESULT_CAPACITY
            || c.max_queued > RESULT_CAPACITY
        {
            return Err("final credit completion shape".into());
        }
        Ok(())
    }
}
