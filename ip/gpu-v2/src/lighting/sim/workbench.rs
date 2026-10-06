//! Host scheduling editor over the same bound DAG as LightingEmu. Edits are
//! planning artifacts: they do not silently replace the emulator's program.
use crate::lighting::{
    datapath::Program, rtl::LightingRtlOptions, LightingProfile, LightingQuantization,
};
use audited::physical::{DspInventory, DspMode, DspUsage};
use resource_scheduler::{
    check_modulo, modulo_schedule_bounded, Graph, Limits, ModuloGraph, ModuloSchedule,
    ModuloViolation, SearchConfig,
};
use std::collections::BTreeSet;

pub const MAX_ISSUE: u64 = 1024;
pub const MAX_II: u64 = 32;

#[derive(Clone, Debug)]
pub struct PortInfo {
    pub value: usize,
    pub label: String,
    pub description: String,
    /// Visible producers, after following hidden zero-latency wiring.
    pub sources: Vec<usize>,
    /// Literal or format-only wiring of a literal, never a fixture's sampled value.
    pub constant: bool,
}
#[derive(Clone, Debug)]
pub struct NodeInfo {
    pub id: usize,
    pub label: String,
    pub resource: usize,
    /// Exact post-steering RTL lane; commit is a cycle-program boundary.
    pub physical_lane: Option<usize>,
    pub physical_kind: String,
    /// Logical table identity, independent of its replicated physical ports.
    pub read_memory: Option<String>,
    /// Audited operand and product widths, including multiplications inside MACs.
    pub multipliers: Vec<[u32; 3]>,
    pub bits: u32,
    pub stable: bool,
    pub parents: Vec<usize>,
    pub members: Vec<String>,
    pub inputs: Vec<PortInfo>,
    pub outputs: Vec<PortInfo>,
    pub program_lines: Vec<usize>,
    pub explanation: String,
    /// Complete lowered recipe owned by this physical instruction, in DAG order.
    pub recipe: Vec<String>,
    /// Arithmetic stages, including comparisons hidden behind a final select.
    pub steps: Vec<String>,
    /// Final select's predicate and alternatives; predicate may come from a
    /// separate block, explicitly marked rather than silently claiming fusion.
    pub choice: Option<[String; 4]>,
}
/// A readable expansion of the actual audited kernel, with an exact operator
/// range and the originating Rust call site. Fusion may own several lines.
#[derive(Clone, Debug)]
pub struct ProgramLine {
    pub line: usize,
    pub event: usize,
    pub before: String,
    pub operator: String,
    pub after: String,
    pub source_file: String,
    pub source_line: u32,
    pub source_column: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub id: usize,
    pub issue: u64,
    pub lane: usize,
}
/// Dependency-only earliest starts for one pixel, without resource sharing.
/// Issues include hidden zero-latency wiring; no physical lanes are allocated.
pub struct SinglePixel {
    pub issues: Vec<u64>,
    pub span: u64,
    pub bypassed: Vec<usize>,
}
#[derive(Clone, Debug)]
pub struct Conflict {
    pub nodes: Vec<usize>,
    pub message: String,
}
#[derive(Clone, Debug)]
pub struct Inspection {
    pub schedule: ModuloSchedule,
    pub conflicts: Vec<Conflict>,
    pub resource_lower_bound: u64,
    pub critical_path: u64,
    /// Nonstable bound-operation results only; excludes ports/context/FIFOs,
    /// absorbed cone internals and control. Never reported as total flip-flops.
    pub live_bits_by_phase: Option<Vec<u64>>,
    pub dsp: DspUsage,
}
pub struct Workbench {
    pub quantization: LightingQuantization,
    /// Accept-to-valid latency of the reviewed cycle program, in advancing edges.
    pub latency: usize,
    pub graph: Graph,
    pub nodes: Vec<NodeInfo>,
    pub baseline: ModuloSchedule,
    pub program: Vec<ProgramLine>,
    order: Vec<usize>,
}
impl Workbench {
    pub fn new(profile: LightingProfile, full: bool) -> Result<Self, String> {
        Self::with_quantization(profile, full, LightingQuantization::CompensatedFloor)
    }
    pub fn with_quantization(
        profile: LightingProfile,
        full: bool,
        quantization: LightingQuantization,
    ) -> Result<Self, String> {
        if profile != LightingProfile::Fast {
            return Err("workbench supports Fast".into());
        }
        let options = LightingRtlOptions::lit_queue_resource_profile(profile, quantization);
        let p = Program::with_retiming(
            profile,
            full,
            options.dedicated_dsp,
            options.kernel(),
            options.role_schedule,
            options.logic_depth,
            options.retiming,
        )?;
        let physical = crate::lighting::rtl::physical_calendar_with_options(profile, options)?;
        let physical: std::collections::BTreeMap<_, _> = physical
            .into_iter()
            .filter(|i| i.full == full)
            .map(|i| (i.event, i))
            .collect();
        for i in &p.instructions {
            let rtl = physical
                .get(&i.root)
                .ok_or("missing physical instruction")?;
            if (i.issue, i.ready) != (rtl.issue, rtl.ready) {
                return Err("RTL and cycle-program operation ages disagree".into());
            }
        }
        let mut order = Vec::new();
        let mut visited = vec![false; p.graph.nodes.len()];
        while order.len() < p.graph.nodes.len() {
            let before = order.len();
            for (id, n) in p.graph.nodes.iter().enumerate() {
                if !visited[id] && n.predecessors.iter().all(|&v| visited[v]) {
                    visited[id] = true;
                    order.push(id);
                }
            }
            if before == order.len() {
                return Err("cyclic hardware graph".into());
            }
        }
        // Collapse only zero-latency wiring. The physical fusion certificates
        // stay atomic and retain their real external operand dependencies.
        let mut ancestors = vec![BTreeSet::new(); p.graph.nodes.len()];
        for &id in &order {
            for &parent in &p.graph.nodes[id].predecessors {
                if p.graph.nodes[parent].resource.is_some() {
                    ancestors[id].insert(parent);
                } else {
                    let inherited = ancestors[parent].clone();
                    ancestors[id].extend(inherited);
                }
            }
        }
        let mut aliases = vec![BTreeSet::new(); p.graph.nodes.len()];
        for observation in &p.frame.outputs {
            let mut producer = p.frame.values[observation.value].producer;
            for _ in 0..p.frame.events.len() {
                let owner = p
                    .instructions
                    .iter()
                    .find(|i| i.members.contains(&producer));
                if let Some(i) = owner {
                    if p.graph.nodes[i.root].resource.is_some() {
                        aliases[i.root].insert(observation.name.clone());
                        break;
                    }
                }
                let e = &p.frame.events[producer];
                if e.inputs.len() != 1 {
                    break;
                }
                producer = p.frame.values[e.inputs[0]].producer;
            }
        }
        let semantic = semantic_names(&p.frame);
        let value_names = compact_names(&p.frame, &semantic);
        let mut program = display_program(&p.frame, &value_names);
        for (id, node) in p.graph.nodes.iter().enumerate().skip(p.frame.events.len()) {
            if node.resource.is_some() {
                program.push(ProgramLine {
                    line: program.len() + 1,
                    event: id,
                    before: String::new(),
                    operator: "commit".into(),
                    after: format!(
                        "({}, {});",
                        value_names[p.output_values[0]], value_names[p.output_values[1]]
                    ),
                    source_file: "ip/gpu-v2/src/lighting/sim/counted.rs".into(),
                    source_line: 0,
                    source_column: 0,
                });
            }
        }
        let value_label = |v: usize| {
            let f = p.frame.values[v].format;
            format!(
                "{}: {}{}F{}",
                value_names[v],
                if f.signed { "S" } else { "U" },
                f.bits,
                f.fraction
            )
        };
        let operation = |id: usize| {
            if id >= p.frame.events.len() {
                return "commit g/h".to_string();
            }
            let e = &p.frame.events[id];
            let name = if let audited::Operation::Read { memory, .. } = e.operation {
                format!("read {}", p.frame.memories[memory].name)
            } else {
                format!("{:?}", e.operation)
            };
            format!(
                "{name}({}) -> {}",
                e.inputs
                    .iter()
                    .map(|&v| value_label(v))
                    .collect::<Vec<_>>()
                    .join(", "),
                e.output.map_or_else(|| "control".into(), value_label)
            )
        };
        // Operand wires carry data only. Hidden literals/format operations may
        // also have a branch gate in the scheduler; that gate must not become
        // a fictitious producer of every variable in the branch.
        let mut data_sources = vec![BTreeSet::new(); p.frame.values.len()];
        let mut constants = vec![false; p.frame.values.len()];
        for e in &p.frame.events {
            let Some(v) = e.output else { continue };
            constants[v] = e.operation == audited::Operation::Literal
                || matches!(
                    e.operation,
                    audited::Operation::Resize
                        | audited::Operation::BinaryScale
                        | audited::Operation::Slice(_)
                        | audited::Operation::ShiftLeft(_)
                        | audited::Operation::RescaleFloor(_)
                ) && e.inputs.iter().all(|&input| constants[input]);
            let root = p
                .instructions
                .iter()
                .find(|i| i.members.contains(&e.id))
                .map_or(e.id, |i| i.root);
            if p.graph.nodes[root].resource.is_some() {
                data_sources[v].insert(root);
            } else {
                for &input in &e.inputs {
                    let inherited = data_sources[input].clone();
                    data_sources[v].extend(inherited);
                }
            }
        }
        let port = |v: usize| {
            let producer = p.frame.values[v].producer;
            PortInfo {
                value: v,
                label: value_names[v].clone(),
                description: format!(
                    "{}; {}; {}",
                    semantic[v],
                    value_label(v),
                    operation(producer)
                ),
                sources: data_sources[v].iter().copied().collect(),
                constant: constants[v],
            }
        };
        let nodes = p
            .graph
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, n)| {
                let resource = n.resource?;
                let value = p.frame.events.get(id).and_then(|e| e.output);
                let bits = value.map_or(0, |v| p.frame.values[v].format.bits);
                let stable = value.is_some_and(|v| p.stable[v]);
                let mut labels: Vec<_> = aliases[id]
                    .iter()
                    .map(|name| semantic_alias(name))
                    .collect();
                labels.push(operation(id));
                // Use the physical instruction boundary, including all operands
                // of fused DSP/logic members. Commit consumes the observed g/h.
                let input_values = p
                    .instructions
                    .iter()
                    .find(|i| i.root == id)
                    .map_or_else(|| p.output_values.to_vec(), |i| i.inputs.clone());
                Some(NodeInfo {
                    id,
                    label: labels.join(" · "),
                    resource,
                    physical_lane: physical.get(&id).and_then(|i| i.lane),
                    physical_kind: physical
                        .get(&id)
                        .map_or_else(|| "commit".into(), |i| i.kind.clone()),
                    read_memory: p.frame.events.get(id).and_then(|e| match e.operation {
                        audited::Operation::Read { memory, .. } => {
                            Some(p.frame.memories[memory].name.clone())
                        }
                        _ => None,
                    }),
                    multipliers: p
                        .instructions
                        .iter()
                        .find(|i| i.root == id)
                        .into_iter()
                        .flat_map(|i| &i.members)
                        .filter_map(|&m| {
                            let e = &p.frame.events[m];
                            (e.operation == audited::Operation::Multiply).then(|| {
                                [
                                    p.frame.values[e.inputs[0]].format.bits,
                                    p.frame.values[e.inputs[1]].format.bits,
                                    p.frame.values[e.output.unwrap()].format.bits,
                                ]
                            })
                        })
                        .collect(),
                    bits,
                    stable,
                    parents: ancestors[id].iter().copied().collect(),
                    members: p
                        .instructions
                        .iter()
                        .find(|i| i.root == id)
                        .map_or_else(Vec::new, |i| {
                            i.members.iter().map(|&m| operation(m)).collect()
                        }),
                    inputs: input_values.iter().map(|&v| port(v)).collect(),
                    outputs: value.into_iter().map(port).collect(),
                    program_lines: program
                        .iter()
                        .filter(|line| {
                            p.instructions
                                .iter()
                                .find(|i| i.root == id)
                                .is_some_and(|i| i.members.contains(&line.event))
                                || id == line.event && id >= p.frame.events.len()
                        })
                        .map(|line| line.line)
                        .collect(),
                    explanation: value.map_or_else(
                        || "Publish diffuse and specular factors".into(),
                        |v| semantic[v].clone(),
                    ),
                    recipe: program
                        .iter()
                        .filter(|line| {
                            p.instructions
                                .iter()
                                .find(|i| i.root == id)
                                .is_some_and(|i| i.members.contains(&line.event))
                                || id == line.event && id >= p.frame.events.len()
                        })
                        .map(|line| format!("{}{}{}", line.before, line.operator, line.after))
                        .collect(),
                    steps: p
                        .instructions
                        .iter()
                        .find(|i| i.root == id)
                        .into_iter()
                        .flat_map(|i| &i.members)
                        .filter_map(|&m| {
                            use audited::Operation as O;
                            Some(
                                match p.frame.events[m].operation {
                                    O::Add => "add",
                                    O::Sub => "subtract",
                                    O::Multiply => "multiply",
                                    O::Less => "compare",
                                    O::Select => "select",
                                    O::Shift => "shift",
                                    O::LeadingZeros => "leading zeros",
                                    O::RoundIncrement(_) => "round control",
                                    O::Read { .. } => "read",
                                    _ => return None,
                                }
                                .to_string(),
                            )
                        })
                        .collect(),
                    choice: p.instructions.iter().find(|i| i.root == id).and_then(|i| {
                        let e = i
                            .members
                            .iter()
                            .rev()
                            .map(|&m| &p.frame.events[m])
                            .find(|e| e.operation == audited::Operation::Select)?;
                        let test = &p.frame.events[p.frame.values[e.inputs[0]].producer];
                        let condition = if test.operation == audited::Operation::Less {
                            format!(
                                "{} < {}",
                                value_names[test.inputs[0]], value_names[test.inputs[1]]
                            )
                        } else {
                            value_names[e.inputs[0]].clone()
                        };
                        Some([
                            condition,
                            value_names[e.inputs[1]].clone(),
                            value_names[e.inputs[2]].clone(),
                            if i.members.contains(&test.id) {
                                "internal"
                            } else {
                                "external"
                            }
                            .into(),
                        ])
                    }),
                })
            })
            .collect();
        Ok(Self {
            quantization,
            latency: p.latency,
            graph: p.graph,
            nodes,
            baseline: p.schedule,
            program,
            order,
        })
    }
    pub fn single_pixel(&self) -> SinglePixel {
        self.single_pixel_with_boundary(false)
    }
    /// Planning projection at the lit-queue entrance. Quad ownership supplies
    /// g=1/h=0 for unlit without issuing a lighting request. The standalone
    /// emulator/RTL compatibility program remains unchanged.
    pub fn lit_queue_single_pixel(&self) -> SinglePixel {
        self.single_pixel_with_boundary(true)
    }
    fn single_pixel_with_boundary(&self, lit_queue: bool) -> SinglePixel {
        let bypassed: Vec<_> = self
            .nodes
            .iter()
            .filter(|n| lit_queue && n.outputs.iter().any(|p| p.label == "mode.unlit"))
            .map(|n| n.id)
            .collect();
        let latency = |id: usize| {
            if bypassed.contains(&id) {
                0
            } else {
                self.graph.nodes[id]
                    .resource
                    .map_or(0, |r| self.graph.resources[r].latency)
            }
        };
        let mut issues = vec![0; self.graph.nodes.len()];
        let mut span = 0;
        for &id in &self.order {
            let n = &self.graph.nodes[id];
            issues[id] = n
                .predecessors
                .iter()
                .map(|&p| issues[p] + latency(p))
                .max()
                .unwrap_or(0)
                .max(n.earliest);
            span = span.max(issues[id] + latency(id));
        }
        if lit_queue {
            // Use only slack before the first visible consumer. Stagger table
            // reads when possible, without extending the dependency bound or
            // pretending this implies fewer ports in a periodic pipeline.
            let mut reads: Vec<_> = self
                .nodes
                .iter()
                .filter(|n| n.read_memory.is_some())
                .collect();
            reads.sort_by_key(|n| (issues[n.id], n.id));
            let mut occupied = std::collections::BTreeMap::<&str, BTreeSet<u64>>::new();
            for n in reads {
                let deadline = self
                    .nodes
                    .iter()
                    .filter(|m| m.parents.contains(&n.id))
                    .map(|m| issues[m.id])
                    .min()
                    .unwrap_or(span)
                    .saturating_sub(latency(n.id));
                let used = occupied
                    .entry(n.read_memory.as_deref().unwrap())
                    .or_default();
                if let Some(issue) = (issues[n.id]..=deadline).find(|t| !used.contains(t)) {
                    issues[n.id] = issue;
                }
                used.insert(issues[n.id]);
            }
            // Hidden format wires follow the moved read. The deadline above
            // ensures that every visible arithmetic consumer retains its age.
            for &id in &self.order {
                for &p in &self.graph.nodes[id].predecessors {
                    issues[id] = issues[id].max(issues[p] + latency(p));
                }
                debug_assert!(issues[id] + latency(id) <= span);
            }
        }
        SinglePixel {
            issues,
            span,
            bypassed,
        }
    }
    pub fn slots(&self, schedule: &ModuloSchedule) -> Vec<Slot> {
        self.nodes
            .iter()
            .map(|n| Slot {
                id: n.id,
                issue: schedule.nodes[n.id].issue,
                lane: schedule.nodes[n.id].lane.unwrap_or(0),
            })
            .collect()
    }
    pub fn graph_with_capacity(&self, capacities: &[usize]) -> Result<Graph, String> {
        if capacities.len() != self.graph.resources.len()
            || capacities.iter().any(|n| !(1..=64).contains(n))
        {
            return Err("one capacity in 1..=64 required per resource".into());
        }
        let mut g = self.graph.clone();
        for (r, &capacity) in g.resources.iter_mut().zip(capacities) {
            r.lanes = capacity;
        }
        Ok(g)
    }
    fn normalize(&self, graph: &Graph, schedule: &mut ModuloSchedule) {
        for &id in &self.order {
            if graph.nodes[id].resource.is_none() {
                schedule.nodes[id].issue = graph.nodes[id]
                    .predecessors
                    .iter()
                    .map(|&p| {
                        schedule.nodes[p].issue
                            + graph.nodes[p]
                                .resource
                                .map_or(0, |r| graph.resources[r].latency)
                    })
                    .max()
                    .unwrap_or(0)
                    .max(graph.nodes[id].earliest);
                schedule.nodes[id].lane = None;
            }
        }
        schedule.span = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(id, n)| {
                schedule.nodes[id].issue + n.resource.map_or(0, |r| graph.resources[r].latency)
            })
            .max()
            .unwrap_or(0);
    }
    pub fn inspect(
        &self,
        ii: u64,
        slots: &[Slot],
        capacities: &[usize],
    ) -> Result<Inspection, String> {
        if !(1..=MAX_II).contains(&ii) || slots.len() != self.nodes.len() {
            return Err("II or assignment count outside editor bounds".into());
        }
        let graph = self.graph_with_capacity(capacities)?;
        let mut schedule = self.baseline.clone();
        schedule.initiation_interval = ii;
        let mut seen = BTreeSet::new();
        for s in slots {
            if s.issue > MAX_ISSUE
                || s.lane >= 64
                || !seen.insert(s.id)
                || !self.nodes.iter().any(|n| n.id == s.id)
            {
                return Err("duplicate, unknown or out-of-bounds assignment".into());
            }
            schedule.nodes[s.id].issue = s.issue;
            schedule.nodes[s.id].lane = Some(s.lane);
        }
        // A read-only load must preserve the reviewed calendar, including its
        // deliberately delayed wiring. Recompute wiring only for actual edits.
        if schedule != self.baseline
            || graph
                .resources
                .iter()
                .zip(&self.graph.resources)
                .any(|(a, b)| a.lanes != b.lanes)
        {
            self.normalize(&graph, &mut schedule);
        }
        let mg = ModuloGraph::from_graph(&graph).map_err(|e| e.to_string())?;
        let checked = check_modulo(&mg, &schedule);
        let conflicts: Vec<_> = checked
            .violations
            .iter()
            .map(|v| {
                let nodes = match v {
                    ModuloViolation::Dependency {
                        node, predecessor, ..
                    } => {
                        let mut ids = vec![*node];
                        if graph.nodes[*predecessor].resource.is_some() {
                            ids.push(*predecessor);
                        } else if let Some(n) = self.nodes.iter().find(|n| n.id == *node) {
                            ids.extend(&n.parents);
                        }
                        ids
                    }
                    ModuloViolation::InitiationCollision {
                        resource,
                        lane,
                        first,
                        second,
                        ..
                    } => self
                        .nodes
                        .iter()
                        .filter(|n| {
                            n.resource == *resource
                                && schedule.nodes[n.id].lane == Some(*lane)
                                && [*first, *second].contains(&(schedule.nodes[n.id].issue % ii))
                        })
                        .map(|n| n.id)
                        .collect(),
                    ModuloViolation::ReleaseGate { node, .. }
                    | ModuloViolation::MissingLane { node }
                    | ModuloViolation::UnexpectedLane { node, .. }
                    | ModuloViolation::LaneOutOfRange { node, .. }
                    | ModuloViolation::WiringTiming { node }
                    | ModuloViolation::LatencyOverflow { node } => vec![*node],
                    _ => Vec::new(),
                };
                Conflict {
                    nodes,
                    message: v.to_string(),
                }
            })
            .collect();
        let live_bits_by_phase = if conflicts.is_empty() {
            let lifetimes: Vec<_> = self
                .nodes
                .iter()
                .filter(|n| !n.stable && n.bits != 0)
                .map(|n| {
                    let start = schedule.nodes[n.id].issue + graph.resources[n.resource].latency;
                    let end = self
                        .nodes
                        .iter()
                        .filter(|c| c.parents.contains(&n.id))
                        .map(|c| schedule.nodes[c.id].issue)
                        .max()
                        .unwrap_or(start);
                    (start, end, n.bits)
                })
                .collect();
            Some(
                (0..ii)
                    .map(|phase| {
                        lifetimes
                            .iter()
                            .map(|&(start, end, bits)| {
                                periodic_live(start, end, phase, ii) * u64::from(bits)
                            })
                            .sum()
                    })
                    .collect(),
            )
        } else {
            None
        };
        let declarations: Vec<_> = graph
            .resources
            .iter()
            .filter_map(|r| {
                let mode = match r.name.as_str() {
                    "SmallMultiply" => DspMode::Multiply9,
                    "LargeMultiply" => DspMode::Multiply18,
                    "PairMultiplyAdd" => DspMode::PairMultiplyAdd,
                    _ => return None,
                };
                Some((mode, r.lanes, r.latency, r.initiation_interval))
            })
            .collect();
        let dsp = DspInventory::pack(128, &declarations)
            .and_then(|i| i.audit())
            .map_err(|e| format!("DSP packing: {e:?}"))?;
        Ok(Inspection {
            schedule,
            conflicts,
            live_bits_by_phase,
            dsp,
            critical_path: mg.critical_path(),
            resource_lower_bound: mg.resource_lower_bound(),
        })
    }
    pub fn search(&self, ii: u64, capacities: &[usize]) -> Result<Inspection, String> {
        if !(1..=MAX_II).contains(&ii) {
            return Err("II outside 1..=32".into());
        }
        let graph = self.graph_with_capacity(capacities)?;
        let mg = ModuloGraph::from_graph(&graph).map_err(|e| e.to_string())?;
        let schedule = modulo_schedule_bounded(
            &mg,
            ii,
            &Limits::new(4096, MAX_ISSUE, 16),
            &SearchConfig::default(),
        )
        .map_err(|e| e.to_string())?;
        self.inspect(ii, &self.slots(&schedule), capacities)
    }
}
fn semantic_alias(name: &str) -> String {
    let mut parts = name.split('.');
    let head = parts.next().unwrap_or(name);
    let tail = parts.next();
    let prefix = match head {
        "n" => "normal",
        "v" => "view",
        "h" if tail.is_some() => "halfway",
        "ray" => "view_ray",
        _ => head,
    };
    match (prefix, tail) {
        (p, Some("0")) => format!("{p}.x"),
        (p, Some("1")) => format!("{p}.y"),
        (p, Some("2")) => format!("{p}.z"),
        (p, Some("q")) => format!("{p}.length_squared"),
        (p, Some("r")) => format!("{p}.inverse_length"),
        (p, Some("shift")) => format!("{p}.scale_shift"),
        (_, Some(_)) => name.into(),
        ("nl", None) => "normal_dot_light".into(),
        ("nh", None) => "normal_dot_halfway".into(),
        ("d", None) => "diffuse.weight".into(),
        ("x", None) => "specular.cosine".into(),
        ("power", None) => "specular.power".into(),
        ("p9", None) => "specular.weight".into(),
        ("g", None) => "diffuse_factor".into(),
        ("h", None) => "specular_factor".into(),
        _ => name.into(),
    }
}

fn semantic_names(frame: &audited::FrameReport) -> Vec<String> {
    let mut names: Vec<_> = frame
        .values
        .iter()
        .map(|v| v.name.clone().unwrap_or_default())
        .collect();
    for output in &frame.outputs {
        names[output.value] = semantic_alias(&output.name);
    }
    // A source variable often names a final resize/scale wire. Carry that
    // semantic name back to its physical producer rather than showing an
    // unnamed select/add expression for the same value at the block boundary.
    for value in 0..names.len() {
        if names[value].is_empty() {
            continue;
        }
        let mut current = value;
        for _ in 0..frame.events.len() {
            let e = &frame.events[frame.values[current].producer];
            if e.inputs.len() != 1
                || !matches!(
                    e.operation,
                    audited::Operation::Resize
                        | audited::Operation::BinaryScale
                        | audited::Operation::RescaleFloor(_)
                )
            {
                break;
            }
            let predecessor = e.inputs[0];
            if !names[predecessor].is_empty() {
                break;
            }
            names[predecessor] = names[value].clone();
            current = predecessor;
        }
    }
    for e in &frame.events {
        let Some(v) = e.output else { continue };
        if !names[v].is_empty() {
            continue;
        }
        let args: Vec<_> = e.inputs.iter().map(|&id| names[id].as_str()).collect();
        let arg = |i: usize| args.get(i).copied().unwrap_or("input");
        names[v] = match &e.operation {
            audited::Operation::Literal => {
                let value = &frame.values[v];
                let mut raw = value.raw;
                let mut fraction = value.format.fraction;
                while fraction > 0 && raw % 2 == 0 {
                    raw /= 2;
                    fraction -= 1;
                }
                if fraction == 0 {
                    raw.to_string()
                } else if fraction <= 8 {
                    (raw as f64 / 2_f64.powi(fraction as i32)).to_string()
                } else {
                    format!("{raw}/2^{fraction}")
                }
            }
            audited::Operation::Read { memory, row } => {
                let axis = ["x", "y", "z"].get(*row).copied().unwrap_or("data");
                match frame.memories[*memory].name.as_str() {
                    "context.mode" => "lighting_mode".into(),
                    "context.light" => format!("light.{axis}"),
                    "context.projection" => match row {
                        0 => "ray_scale.x",
                        1 => "ray_scale.y",
                        _ => "view_ray.z",
                    }
                    .into(),
                    "context.intensity" => if *row == 0 {
                        "light.ambient"
                    } else {
                        "light.directional"
                    }
                    .into(),
                    "material.code" | "context.code" => "material.shininess".into(),
                    memory => format!("{memory}[{}]", args.join(", ")),
                }
            }
            audited::Operation::Add => format!("({} + {})", arg(0), arg(1)),
            audited::Operation::Sub => format!("({} − {})", arg(0), arg(1)),
            audited::Operation::Multiply => format!("({} × {})", arg(0), arg(1)),
            audited::Operation::Less => format!("({} < {})", arg(0), arg(1)),
            audited::Operation::Select => format!("select({}, {}, {})", arg(0), arg(1), arg(2)),
            audited::Operation::Resize
            | audited::Operation::BinaryScale
            | audited::Operation::RescaleFloor(_) => arg(0).into(),
            audited::Operation::Slice(bit) => format!("{}.bits{bit}", arg(0)),
            audited::Operation::RoundIncrement(_) => format!("{}.round_up", arg(0)),
            audited::Operation::LeadingZeros => format!("{}.leading_zeros", arg(0)),
            audited::Operation::Shift => format!("shift({}, {})", arg(0), arg(1)),
            audited::Operation::ShiftLeft(bits) => format!("({} << {bits})", arg(0)),
            op => format!("{op:?}({})", args.join(", ")),
        };
    }
    names
}

fn compact_names(frame: &audited::FrameReport, semantic: &[String]) -> Vec<String> {
    use audited::Operation;
    let mut names = vec![String::new(); semantic.len()];
    let mut used = std::collections::BTreeMap::<String, usize>::new();
    for e in &frame.events {
        let Some(v) = e.output else { continue };
        if matches!(e.operation, Operation::Literal) {
            names[v] = semantic[v].clone();
            continue;
        }
        // Format-only wiring carries a variable; real arithmetic gets a short
        // named temporary rather than recursively expanding a whole expression.
        if matches!(
            e.operation,
            Operation::Resize | Operation::RescaleFloor(_) | Operation::BinaryScale
        ) && semantic[v] == semantic[e.inputs[0]]
        {
            names[v] = names[e.inputs[0]].clone();
            continue;
        }
        let mut short = semantic[v]
            .replace("halfway", "half")
            .replace("inverse_length", "invLen")
            .replace("length_squared", "lenSq")
            .replace("normalized_product", "normProd")
            .replace("scaled_magnitude_work", "scaleWork")
            .replace("prescale_shift", "preShift")
            .replace("normal_dot_halfway", "dotNH")
            .replace("normal_dot_light", "dotNL")
            .replace("correction_product", "corrProd")
            .replace("lut_base", "base")
            .replace("correction", "corr")
            .replace("rsqrt", "invSqrt")
            .replace("square", "sq")
            .replace("specular", "spec")
            .replace("directional", "direct")
            .replace("degenerate", "isZero")
            .replace("leading_zeros", "zeros")
            .replace("magnitude", "mag")
            .replace("length_exponent", "lenExp")
            .replace("interpolated", "interp")
            .replace("intensity", "strength");
        if short.len() > 24
            || short
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
        {
            let stage = e
                .inputs
                .iter()
                .map(|&i| names[i].split('.').next().unwrap_or("lighting"))
                .find(|s| ["normal", "view", "half", "spec", "diffuse", "view_ray"].contains(s))
                .unwrap_or("lighting");
            let op = match e.operation {
                Operation::Add => "sum",
                Operation::Sub => "difference",
                Operation::Multiply => "product",
                Operation::Less => "test",
                Operation::Select => "choice",
                Operation::Slice(_) => "bits",
                Operation::RoundIncrement(_) => "roundUp",
                Operation::LeadingZeros => "zeros",
                Operation::Shift => "shift",
                Operation::Read { .. } => "table",
                Operation::ProductMapping { .. } => "product",
                _ => "scaled",
            };
            short = format!("{stage}.{op}");
        }
        let count = used.entry(short.clone()).or_default();
        *count += 1;
        names[v] = if *count == 1 {
            short
        } else {
            format!("{short}{count}")
        };
    }
    names
}
fn display_program(frame: &audited::FrameReport, names: &[String]) -> Vec<ProgramLine> {
    use audited::Operation as O;
    let mut lines = Vec::new();
    for e in &frame.events {
        if matches!(e.operation, O::Literal) {
            continue;
        }
        let args: Vec<_> = e.inputs.iter().map(|&v| names[v].as_str()).collect();
        let arg = |i: usize| args.get(i).copied().unwrap_or("input");
        let lhs = e
            .output
            .map_or(String::new(), |v| format!("{} = ", names[v]));
        let (before, operator, after) = match &e.operation {
            O::Add | O::Sub | O::Multiply | O::Less => (
                format!("{lhs}{} ", arg(0)),
                match e.operation {
                    O::Add => "+",
                    O::Sub => "-",
                    O::Multiply => "*",
                    _ => "<",
                }
                .into(),
                format!(" {};", arg(1)),
            ),
            O::Read { memory, row } => (
                lhs,
                "read".into(),
                format!(
                    "({}[{}]);",
                    frame.memories[*memory].name,
                    if args.is_empty() {
                        row.to_string()
                    } else {
                        args.join(", ")
                    }
                ),
            ),
            O::Write { memory, row } => (
                String::new(),
                "write".into(),
                format!(
                    "({}[{}], {});",
                    frame.memories[*memory].name,
                    row,
                    args.join(", ")
                ),
            ),
            O::Publish(name) => (
                String::new(),
                "publish".into(),
                format!("({name}, {});", arg(0)),
            ),
            operation => {
                let (op, suffix) = match operation {
                    O::Select => ("select".into(), String::new()),
                    O::Resize => ("resize".into(), String::new()),
                    O::BinaryScale => ("binaryScale".into(), String::new()),
                    O::RescaleFloor(bits) => ("floor".into(), format!(", {bits}")),
                    O::Slice(bits) => ("slice".into(), format!(", {bits}")),
                    O::RoundIncrement(bits) => ("roundUp".into(), format!(", {bits}")),
                    O::Shift => ("shift".into(), String::new()),
                    O::ShiftLeft(bits) => ("shiftLeft".into(), format!(", {bits}")),
                    O::LeadingZeros => ("leadingZeros".into(), String::new()),
                    O::ProductMapping { .. } => ("productMapping".into(), String::new()),
                    O::Require(expected) => ("require".into(), format!(", {expected}")),
                    O::Branch(_) => ("branch".into(), String::new()),
                    _ => (format!("{operation:?}"), String::new()),
                };
                (lhs, op, format!("({}{suffix});", args.join(", ")))
            }
        };
        let file = e.source.file().replace('\\', "/");
        let file = file
            .find("ip/gpu-v2/")
            .map_or(file.clone(), |at| file[at..].into());
        lines.push(ProgramLine {
            line: lines.len() + 1,
            event: e.id,
            before,
            operator,
            after,
            source_file: file,
            source_line: e.source.line(),
            source_column: e.source.column(),
        });
    }
    lines
}
fn periodic_live(start: u64, end: u64, phase: u64, ii: u64) -> u64 {
    if end <= start {
        return 0;
    }
    // Count all integer instance indices, including preceding/following pixels.
    ((phase as i64 - start as i64).div_euclid(ii as i64)
        - (phase as i64 - end as i64).div_euclid(ii as i64)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_workbench_matches_cycle_rtl_and_has_explicit_quantization() {
        {
            let profile = LightingProfile::Fast;
            for quantization in [
                LightingQuantization::CompensatedFloor,
                LightingQuantization::NearestEven,
            ] {
                let options = LightingRtlOptions::lit_queue_resource_profile(profile, quantization);
                let rtl = crate::lighting::rtl::generate_with_options(profile, options).unwrap();
                for full in [false, true] {
                    let w = Workbench::with_quantization(profile, full, quantization).unwrap();
                    assert_eq!(
                        w.latency,
                        if full {
                            rtl.latency
                        } else {
                            rtl.diffuse_latency
                        }
                    );
                    assert_eq!(
                        w.baseline.initiation_interval as usize,
                        if full {
                            rtl.specular_ii
                        } else {
                            rtl.diffuse_ii
                        }
                    );
                    let p = Program::with_retiming(
                        profile,
                        full,
                        options.dedicated_dsp,
                        options.kernel(),
                        options.role_schedule,
                        options.logic_depth,
                        options.retiming,
                    )
                    .unwrap();
                    assert_eq!(w.baseline, p.schedule);
                    for n in &w.nodes {
                        assert_eq!(
                            w.baseline.nodes[n.id].issue as usize,
                            p.instructions
                                .iter()
                                .find(|i| i.root == n.id)
                                .map_or(p.latency - 1, |i| i.issue)
                        );
                    }
                    let biased = w
                        .nodes
                        .iter()
                        .any(|n| n.label.contains("POWER_MIDPOINT_Q15"));
                    assert_eq!(
                        biased,
                        full && quantization == LightingQuantization::CompensatedFloor
                    );
                }
            }
        }
    }
    #[test]
    fn ports_use_physical_fusion_operands_and_real_output_values() {
        for full in [false, true] {
            let w = Workbench::new(LightingProfile::Fast, full).unwrap();
            let options = LightingRtlOptions::lit_queue_resource_profile(
                LightingProfile::Fast,
                LightingQuantization::CompensatedFloor,
            );
            let p = Program::with_retiming(
                LightingProfile::Fast,
                full,
                options.dedicated_dsp,
                options.kernel(),
                options.role_schedule,
                options.logic_depth,
                options.retiming,
            )
            .unwrap();
            let mut fused = 0;
            for n in &w.nodes {
                if let Some(i) = p.instructions.iter().find(|i| i.root == n.id) {
                    assert_eq!(
                        n.inputs.iter().map(|p| p.value).collect::<Vec<_>>(),
                        i.inputs
                    );
                    assert_eq!(n.outputs[0].value, p.frame.events[n.id].output.unwrap());
                    if i.members.len() > 1 {
                        fused += 1;
                    }
                } else {
                    assert_eq!(
                        n.inputs.iter().map(|p| p.value).collect::<Vec<_>>(),
                        p.output_values
                    );
                    assert!(n.outputs.is_empty());
                }
                for input in &n.inputs {
                    assert!(!input.label.is_empty());
                    assert!(!input.label.starts_with(&format!("v{}", input.value)));
                    assert!(input.sources.iter().all(|s| n.parents.contains(s)));
                    if p.frame.events[p.frame.values[input.value].producer].operation
                        == audited::Operation::Literal
                    {
                        assert!(
                            input.sources.is_empty(),
                            "a branch gate is not a literal producer"
                        );
                    }
                }
            }
            assert!(fused > 0);
        }
    }
    #[test]
    fn block_details_preserve_constants_fusion_and_pipeline_contract() {
        {
            let profile = LightingProfile::Fast;
            for full in [false, true] {
                for quantization in [
                    LightingQuantization::CompensatedFloor,
                    LightingQuantization::NearestEven,
                ] {
                    let w = Workbench::with_quantization(profile, full, quantization).unwrap();
                    assert!(w.graph.resources.iter().all(|r| r.initiation_interval == 1));
                    let abs = w
                        .nodes
                        .iter()
                        .find(|n| n.recipe.iter().any(|line| line.contains("normal.abs.x")))
                        .unwrap();
                    for step in ["subtract", "compare", "select"] {
                        assert!(abs.steps.iter().any(|s| s == step));
                    }
                    assert!(abs.inputs.iter().any(|p| !p.constant));
                    assert!(abs.inputs.iter().any(|p| p.constant));
                    assert!(abs.recipe.iter().any(|line| line.contains(" < ")));
                    assert!(abs.recipe.iter().any(|line| line.contains("select(")));
                    // In a deeper contraction safe-magnitude wiring need not
                    // remain a separate block; every surviving select must
                    // still carry its complete choice and arithmetic recipe.
                    for n in &w.nodes {
                        assert_eq!(n.choice.is_some(), n.steps.iter().any(|s| s == "select"));
                        assert_eq!(
                            n.recipe,
                            n.program_lines
                                .iter()
                                .map(|&line| {
                                    let p = &w.program[line - 1];
                                    format!("{}{}{}", p.before, p.operator, p.after)
                                })
                                .collect::<Vec<_>>()
                        );
                        for p in n.inputs.iter().filter(|p| p.constant) {
                            assert!(p.sources.is_empty(), "a constant must have no runtime wire");
                        }
                        // Context/ROM outputs remain runtime operands even when
                        // the template used identical fixture values.
                        if n.read_memory.is_some() {
                            assert!(n.outputs.iter().all(|p| !p.constant));
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn readable_program_preserves_fusion_members_and_kernel_locations() {
        for full in [false, true] {
            let w = Workbench::new(LightingProfile::Fast, full).unwrap();
            let options = LightingRtlOptions::lit_queue_resource_profile(
                LightingProfile::Fast,
                LightingQuantization::CompensatedFloor,
            );
            let p = Program::with_retiming(
                LightingProfile::Fast,
                full,
                options.dedicated_dsp,
                options.kernel(),
                options.role_schedule,
                options.logic_depth,
                options.retiming,
            )
            .unwrap();
            for n in &w.nodes {
                assert!(!n.program_lines.is_empty(), "missing program for {}", n.id);
                for &line in &n.program_lines {
                    let info = &w.program[line - 1];
                    assert!(!info.operator.is_empty());
                    if let Some(i) = p.instructions.iter().find(|i| i.root == n.id) {
                        assert!(i.members.contains(&info.event));
                        let source = if info.source_file.ends_with("lighting/sim/counted.rs") {
                            include_str!("counted.rs")
                        } else if info.source_file.ends_with("lighting/sim/pipeline.rs") {
                            include_str!("pipeline.rs")
                        } else if info.source_file.ends_with("lighting/sim/quantization.rs") {
                            include_str!("quantization.rs")
                        } else {
                            panic!("unexpected kernel location: {}", info.source_file);
                        };
                        assert!(source.lines().nth(info.source_line as usize - 1).is_some());
                    }
                }
                for port in n.inputs.iter().chain(&n.outputs) {
                    assert!(port.label.len() <= 28, "{}", port.label);
                    assert!(!port.label.contains(['(', ')', '+', '×']));
                }
            }
            assert!(w.program.iter().any(|p| p.operator == "+"));
            assert!(w.program.iter().any(|p| p.operator == "*"));
        }
    }
    #[test]
    fn single_pixel_attains_independent_critical_path_bound() {
        {
            let profile = LightingProfile::Fast;
            for full in [false, true] {
                let w = Workbench::new(profile, full).unwrap();
                let single = w.single_pixel();
                let mg = ModuloGraph::from_graph(&w.graph).unwrap();
                assert_eq!(single.span, mg.critical_path());
                assert!(single.span <= w.baseline.span);
                assert!(single.span <= MAX_ISSUE);
                for n in &w.nodes {
                    for &parent in &n.parents {
                        let latency =
                            w.graph.resources[w.graph.nodes[parent].resource.unwrap()].latency;
                        assert!(single.issues[n.id] >= single.issues[parent] + latency);
                    }
                }
            }
        }
    }
    #[test]
    fn lit_queue_projection_bypasses_unlit_and_spreads_reads_only_in_slack() {
        for quantization in [
            LightingQuantization::CompensatedFloor,
            LightingQuantization::NearestEven,
        ] {
            {
                let profile = LightingProfile::Fast;
                for full in [false, true] {
                    let w = Workbench::with_quantization(profile, full, quantization).unwrap();
                    let old = w.single_pixel();
                    let single = w.lit_queue_single_pixel();
                    assert!(single.bypassed.is_empty());
                    let mut specialized = w.graph.clone();
                    for &id in &single.bypassed {
                        specialized.nodes[id].resource = None;
                    }
                    assert_eq!(
                        single.span,
                        ModuloGraph::from_graph(&specialized)
                            .unwrap()
                            .critical_path()
                    );
                    assert_eq!(single.span, old.span);
                    for (id, n) in specialized.nodes.iter().enumerate() {
                        for &parent in &n.predecessors {
                            let latency = specialized.nodes[parent]
                                .resource
                                .map_or(0, |r| specialized.resources[r].latency);
                            assert!(single.issues[id] >= single.issues[parent] + latency);
                        }
                        let latency = n.resource.map_or(0, |r| specialized.resources[r].latency);
                        assert!(single.issues[id] + latency <= single.span);
                    }
                    if !full {
                        let square: Vec<_> = w
                            .nodes
                            .iter()
                            .filter(|n| n.read_memory.as_deref() == Some("SQ"))
                            .map(|n| single.issues[n.id])
                            .collect();
                        assert_eq!(square.len(), 3);
                        // A fused multi-output sum/address block can consume
                        // all three reads immediately, leaving no stagger slack.
                        // Read movement must not postpone that consumer.
                        for n in w
                            .nodes
                            .iter()
                            .filter(|n| n.read_memory.as_deref() == Some("SQ"))
                        {
                            assert!(single.issues[n.id] >= old.issues[n.id]);
                            for consumer in w.nodes.iter().filter(|m| m.parents.contains(&n.id)) {
                                assert!(
                                    single.issues[n.id] + specialized.resources[n.resource].latency
                                        <= old.issues[consumer.id]
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn finite_lifetimes_count_all_overlapping_instances() {
        assert_eq!(periodic_live(3, 8, 0, 2), 2);
        assert_eq!(periodic_live(3, 8, 1, 2), 3);
        assert_eq!(periodic_live(4, 4, 0, 2), 0);
    }
    #[test]
    fn baselines_and_edits_use_independent_periodic_checker() {
        {
            let profile = LightingProfile::Fast;
            for full in [false, true] {
                let w = Workbench::new(profile, full).unwrap();
                let capacities: Vec<_> = w.graph.resources.iter().map(|r| r.lanes).collect();
                let mut slots = w.slots(&w.baseline);
                let ii = w.baseline.initiation_interval;
                let a = w.inspect(ii, &slots, &capacities).unwrap();
                assert!(a.conflicts.is_empty(), "{:?}", a.conflicts);
                assert_eq!(a.schedule, w.baseline);
                assert!(a.live_bits_by_phase.is_some());
                let consumer = w.nodes.iter().find(|n| !n.parents.is_empty()).unwrap();
                slots
                    .iter_mut()
                    .find(|s| s.id == consumer.id)
                    .unwrap()
                    .issue = 0;
                assert!(w
                    .inspect(ii, &slots, &capacities)
                    .unwrap()
                    .conflicts
                    .iter()
                    .any(|c| c.message.contains("predecessor")));
                let mut slots = w.slots(&w.baseline);
                let resource = w
                    .nodes
                    .iter()
                    .find(|n| w.nodes.iter().filter(|m| m.resource == n.resource).count() > 1)
                    .unwrap()
                    .resource;
                for s in &mut slots {
                    if w.graph.nodes[s.id].resource == Some(resource) {
                        s.lane = 0;
                        s.issue = 0;
                    }
                }
                assert!(w
                    .inspect(ii, &slots, &capacities)
                    .unwrap()
                    .conflicts
                    .iter()
                    .any(|c| c.message.contains("lane")));
                slots[0].id = usize::MAX;
                assert!(w.inspect(ii, &slots, &capacities).is_err());
                assert!(w.search(ii, &capacities).unwrap().conflicts.is_empty());
            }
        }
    }
}
