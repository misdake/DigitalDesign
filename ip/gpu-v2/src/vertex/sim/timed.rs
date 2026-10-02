//! Bounded static issue ROMs with independent dependency, DSP and bank certificates.
use super::{super::ports::*, counted};
use audited::{
    lifecycle::{self, LifetimePolicy, LiveReport},
    physical::*,
    FrameReport, MemoryKind, Operation, Resource,
};
use resource_scheduler::{Graph, Limits, Node, Resource as LaneResource, Schedule, SearchConfig};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug)]
pub struct Hardware {
    pub wide: usize,
    pub narrow: usize,
    pub matrix_read_ports: usize,
    pub max_cycles: u64,
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            wide: 1,
            narrow: 1,
            matrix_read_ports: 1,
            max_cycles: 20000,
        }
    }
}
#[derive(Clone, Debug)]
pub struct RomIssue {
    pub cycle: u64,
    pub event: usize,
    pub resource: Option<usize>,
    pub lane: Option<usize>,
}
pub struct Plan {
    pub counted: counted::BatchReport,
    pub graph: Graph,
    pub schedule: Schedule,
    pub rom: Vec<RomIssue>,
    pub layout: MemoryLayout,
    pub accesses: Vec<MemoryAccess>,
    pub dsp: DspInventory,
    pub retained: LiveReport,
    pub publication: Vec<u64>,
    pub hardware: Hardware,
    pub setup_cycles: u64,
}
fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}
fn increment(frame: &FrameReport, event: usize) -> bool {
    let e = &frame.events[event];
    matches!(e.operation, Operation::Add)
        && e.inputs.iter().any(|&v| {
            let p = &frame.events[frame.values[v].producer];
            matches!(p.operation, Operation::RoundIncrement(_))
                || matches!(p.operation, Operation::Literal) && frame.values[v].raw == 1
        })
}
fn graph(frame: &FrameReport, h: Hardware) -> Result<Graph, String> {
    let mut resources = Vec::new();
    let mut mapping = BTreeMap::new();
    let dependencies = bound_dependencies(frame, &[]).map_err(err)?;
    let mut last_write = BTreeMap::new();
    let mut reads: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    let mut nodes = Vec::new();
    let setup_writes:Vec<_>=frame.events.iter().filter_map(|e|matches!(e.operation,Operation::Write{memory,..} if frame.memories[memory].name!="TRANSFORMED").then_some(e.id)).collect();
    for (index, e) in frame.events.iter().enumerate() {
        let mut predecessors = dependencies[index].clone();
        if matches!(e.operation,Operation::Read{memory,..} if frame.memories[memory].name=="v6") {
            predecessors.extend(&setup_writes);
        }
        match e.operation {
            Operation::Read { memory, row } => {
                if let Some(&p) = last_write.get(&(memory, row)) {
                    predecessors.push(p);
                }
                reads.entry((memory, row)).or_default().push(index);
            }
            Operation::Write { memory, row } => {
                if let Some(&p) = last_write.get(&(memory, row)) {
                    predecessors.push(p);
                }
                if let Some(prior) = reads.remove(&(memory, row)) {
                    predecessors.extend(prior);
                }
                last_write.insert((memory, row), index);
            }
            _ => {}
        }
        let (key, lanes, latency) = match e.resource {
            Some(Resource::Dsp36) => ("wide".into(), h.wide, 3),
            Some(Resource::Dsp18) => ("narrow".into(), h.narrow, 2),
            Some(Resource::Adder(w)) => {
                let width = if w <= 18 {
                    18
                } else if w <= 36 {
                    36
                } else {
                    66
                };
                (
                    format!(
                        "{}.{width}",
                        if increment(frame, index) {
                            "increment"
                        } else {
                            "add"
                        }
                    ),
                    2,
                    1,
                )
            }
            Some(Resource::Read(m)) if frame.memories[m].kind != MemoryKind::Input => (
                format!("read.{m}"),
                if frame.memories[m].name == "MVP" {
                    h.matrix_read_ports
                } else {
                    1
                },
                1,
            ),
            Some(Resource::Write(m)) => (format!("write.{m}"), 1, 1),
            Some(Resource::Read(_)) | None => (String::new(), 0, 0),
            Some(r) => (format!("{r:?}"), 2, 1),
        };
        let resource = if key.is_empty() {
            None
        } else {
            Some(*mapping.entry(key.clone()).or_insert_with(|| {
                let id = resources.len();
                resources.push(LaneResource {
                    name: key,
                    lanes,
                    latency,
                    initiation_interval: 1,
                });
                id
            }))
        };
        predecessors.sort_unstable();
        predecessors.dedup();
        nodes.push(Node {
            name: format!("event.{index}"),
            predecessors,
            earliest: 0,
            resource,
        });
    }
    Ok(Graph { nodes, resources })
}
fn layout(frame: &FrameReport, h: Hardware) -> Result<MemoryLayout, String> {
    let mut layout = MemoryLayout::default();
    for (m, store) in frame
        .memories
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind != MemoryKind::Input)
    {
        let copies = if store.name == "MVP" {
            h.matrix_read_ports
        } else {
            1
        };
        let mut placement = MemoryPlacement {
            memory: m,
            copies: Vec::new(),
        };
        for copy in 0..copies {
            let output = store.name == "TRANSFORMED";
            let index = layout.banks.len();
            layout.banks.push(MemoryBank {
                name: format!("{}.copy{copy}", store.name),
                kind: if output {
                    RamKind::Bsram
                } else {
                    RamKind::Ssram
                },
                width: if output { 36 } else { store.format.bits },
                depth: if output { 512 } else { 16 },
                collision: ReadDuringWrite::Forbidden,
                ports: vec![
                    MemoryPort {
                        read: true,
                        write: false,
                        read_latency: 1,
                        write_latency: 1,
                        initiation_interval: 1,
                    },
                    MemoryPort {
                        read: false,
                        write: true,
                        read_latency: 1,
                        write_latency: 1,
                        initiation_interval: 1,
                    },
                ],
            });
            placement.copies.push(MemoryCopy {
                slices: vec![MemorySlice {
                    bank: index,
                    base_row: 0,
                    bit_offset: 0,
                    source_low: 0,
                    width: store.format.bits,
                }],
            });
        }
        layout.placements.push(placement);
    }
    // The second transformed slot is reserved even for one active meshlet.
    let first = layout
        .banks
        .iter()
        .find(|b| b.kind == RamKind::Bsram)
        .ok_or("missing transformed bank")?
        .clone();
    layout.banks.push(MemoryBank {
        name: "TRANSFORMED.reserved-slot".into(),
        ..first
    });
    Ok(layout)
}
pub fn run(
    context: &Context,
    vertices: &[PackedVertex],
    hardware: Hardware,
) -> Result<Plan, String> {
    if hardware.wide == 0
        || hardware.narrow == 0
        || hardware.wide > 4
        || hardware.narrow > 4
        || !(1..=2).contains(&hardware.matrix_read_ports)
        || hardware.max_cycles == 0
    {
        return Err("vertex hardware bounds".into());
    }
    let counted = counted::run_batch(context, vertices)?;
    let graph = graph(&counted.frame, hardware)?;
    let limits = Limits::new(100000, hardware.max_cycles, 12);
    let search =
        resource_scheduler::plan(&graph, &limits, &SearchConfig::default()).map_err(err)?;
    let schedule = search.best_candidate().schedule.clone();
    let layout = layout(&counted.frame, hardware)?;
    let mut accesses = Vec::new();
    for (event, e) in counted.frame.events.iter().enumerate() {
        match e.operation {
            Operation::Read { memory, .. }
                if counted.frame.memories[memory].kind != MemoryKind::Input =>
            {
                accesses.push(MemoryAccess {
                    event,
                    copy: schedule.nodes[event].lane.unwrap_or(0),
                    ports: vec![0],
                })
            }
            Operation::Write { memory, .. } => {
                for copy in 0..layout
                    .placements
                    .iter()
                    .find(|p| p.memory == memory)
                    .ok_or("store placement")?
                    .copies
                    .len()
                {
                    accesses.push(MemoryAccess {
                        event,
                        copy,
                        ports: vec![1],
                    });
                }
            }
            _ => {}
        }
    }
    let dsp = DspInventory::pack(
        12,
        &[
            (DspMode::Multiply36, hardware.wide, 3, 1),
            (DspMode::Multiply18, hardware.narrow, 2, 1),
        ],
    )
    .map_err(err)?;
    let rom = schedule
        .nodes
        .iter()
        .enumerate()
        .map(|(event, n)| RomIssue {
            cycle: n.issue,
            event,
            resource: graph.nodes[event].resource,
            lane: n.lane,
        })
        .collect();
    let times: Vec<_> = schedule
        .nodes
        .iter()
        .map(|n| Timing {
            issue: n.issue,
            ready: n.ready,
        })
        .collect();
    let retained = lifecycle::analyze_bound_policy(
        &counted.frame,
        &times,
        &[],
        &LifetimePolicy {
            commit_cycle: schedule.cycles,
            period: None,
            max_cycle: hardware.max_cycles,
            invariant_inputs: Vec::new(),
            retained_outputs: Some(Vec::new()),
        },
    )
    .map_err(err)?;
    let publication = (0..vertices.len())
        .map(|vertex| {
            counted
                .frame
                .events
                .iter()
                .filter_map(|e| {
                    if let Operation::Write { memory, row } = e.operation {
                        (counted.frame.memories[memory].name == "TRANSFORMED" && row / 7 == vertex)
                            .then_some(schedule.nodes[e.id].ready)
                    } else {
                        None
                    }
                })
                .max()
                .unwrap()
                + 1
        })
        .collect();
    let first_vertex=counted.frame.events.iter().find(|e|matches!(e.operation,Operation::Read{memory,..} if counted.frame.memories[memory].name=="v6")).ok_or("missing vertex body")?.id;
    let setup_cycles = schedule.nodes[..first_vertex]
        .iter()
        .map(|n| n.ready)
        .max()
        .unwrap_or(0);
    let plan = Plan {
        counted,
        graph,
        schedule,
        rom,
        layout,
        accesses,
        dsp,
        retained,
        publication,
        hardware,
        setup_cycles,
    };
    plan.audit()?;
    Ok(plan)
}
impl Plan {
    /// Reusable body calendar. Only resource/dependency periodicity is certified;
    /// mutable output addresses/slot releases are proved by bounded frontend traces.
    pub fn periodic_body(&self) -> Result<(u64, u64), String> {
        if self.counted.outputs.len() != 1 {
            return Err("periodic body requires one vertex template".into());
        }
        let start=self.counted.frame.events.iter().find(|e|matches!(e.operation,Operation::Read{memory,..} if self.counted.frame.memories[memory].name=="v6")).ok_or("missing body")?.id;
        let body = Graph {
            resources: self.graph.resources.clone(),
            nodes: self.graph.nodes[start..]
                .iter()
                .map(|n| Node {
                    name: n.name.clone(),
                    predecessors: n
                        .predecessors
                        .iter()
                        .filter(|&&p| p >= start)
                        .map(|&p| p - start)
                        .collect(),
                    earliest: 0,
                    resource: n.resource,
                })
                .collect(),
        };
        let graph = resource_scheduler::ModuloGraph::from_graph(&body).map_err(err)?;
        let lower = graph.resource_lower_bound().max(1);
        for ii in lower..=lower + 32 {
            if let Ok(schedule) = resource_scheduler::modulo_schedule_bounded(
                &graph,
                ii,
                &Limits::new(4096, self.hardware.max_cycles, 16),
                &SearchConfig::default(),
            ) {
                let check = resource_scheduler::check_modulo(&graph, &schedule);
                if !check.is_ok() {
                    return Err(err(check));
                }
                return Ok((ii, schedule.span));
            }
        }
        Err("no bounded periodic vertex body candidate".into())
    }
    pub fn audit(&self) -> Result<(), String> {
        self.counted.frame.audit().map_err(err)?;
        for (vertex, output) in self.counted.outputs.iter().enumerate() {
            let get = |suffix: &str| {
                self.counted
                    .frame
                    .outputs
                    .iter()
                    .find(|o| o.name == format!("vertex.{vertex}.{suffix}"))
                    .map(|o| o.raw)
            };
            for (row, &raw) in output.clip.iter().enumerate() {
                if get(&format!("clip.{row}")) != Some(i128::from(raw)) {
                    return Err("clip port differs from audited output".into());
                }
            }
            for (row, &raw) in output.normal.iter().enumerate() {
                if get(&format!("normal.{row}")) != Some(i128::from(raw)) {
                    return Err("normal port differs from audited output".into());
                }
            }
            if get("u") != Some(i128::from(output.uv[0]))
                || get("v") != Some(i128::from(output.uv[1]))
                || get("rgb565") != Some(i128::from(output.rgb565))
            {
                return Err("attribute port differs from audited output".into());
            }
        }
        let check = resource_scheduler::check(
            &self.graph,
            &Limits::new(100000, self.hardware.max_cycles, 12),
            &self.schedule,
        );
        if !check.is_ok() {
            return Err(err(check));
        }
        let reconstructed = graph(&self.counted.frame, self.hardware)?;
        if format!("{reconstructed:?}") != format!("{:?}", self.graph) {
            return Err("forged vertex dependency/resource graph".into());
        }
        let expected_layout = layout(&self.counted.frame, self.hardware)?;
        if format!("{expected_layout:?}") != format!("{:?}", self.layout) {
            return Err("physical layout differs from hardware declaration".into());
        }
        let expected_dsp = DspInventory::pack(
            12,
            &[
                (DspMode::Multiply36, self.hardware.wide, 3, 1),
                (DspMode::Multiply18, self.hardware.narrow, 2, 1),
            ],
        )
        .map_err(err)?;
        if format!("{expected_dsp:?}") != format!("{:?}", self.dsp) {
            return Err("DSP inventory differs from hardware declaration".into());
        }
        if self.rom.len() != self.schedule.nodes.len()
            || self.rom.iter().enumerate().any(|(i, r)| {
                r.event != i
                    || r.cycle != self.schedule.nodes[i].issue
                    || r.resource != self.graph.nodes[i].resource
                    || r.lane != self.schedule.nodes[i].lane
            })
        {
            return Err("forged static issue ROM".into());
        }
        let times: Vec<_> = self
            .schedule
            .nodes
            .iter()
            .map(|n| Timing {
                issue: n.issue,
                ready: n.ready,
            })
            .collect();
        let expected_live = lifecycle::analyze_bound_policy(
            &self.counted.frame,
            &times,
            &[],
            &LifetimePolicy {
                commit_cycle: self.schedule.cycles,
                period: None,
                max_cycle: self.hardware.max_cycles,
                invariant_inputs: Vec::new(),
                retained_outputs: Some(Vec::new()),
            },
        )
        .map_err(err)?;
        if expected_live != self.retained {
            return Err("forged retained-value report".into());
        }
        let first_vertex=self.counted.frame.events.iter().find(|e|matches!(e.operation,Operation::Read{memory,..} if self.counted.frame.memories[memory].name=="v6")).ok_or("missing vertex body")?.id;
        if self.setup_cycles
            != self.schedule.nodes[..first_vertex]
                .iter()
                .map(|n| n.ready)
                .max()
                .unwrap_or(0)
        {
            return Err("context setup timing".into());
        }
        audit_dependencies(&self.counted.frame, &times, self.hardware.max_cycles).map_err(err)?;
        self.layout
            .audit_accesses(
                &self.counted.frame,
                &times,
                &self.accesses,
                self.hardware.max_cycles,
            )
            .map_err(err)?;
        self.layout
            .audit_gowin_budget(
                &self.counted.frame,
                GowinMemoryBudget {
                    bsram_blocks: 2,
                    ssram_cells: 80,
                },
            )
            .map_err(err)?;
        let mut issues = Vec::new();
        for (event, e) in self.counted.frame.events.iter().enumerate() {
            if matches!(e.resource, Some(Resource::Dsp18 | Resource::Dsp36)) {
                let mode = if e.resource == Some(Resource::Dsp36) {
                    DspMode::Multiply36
                } else {
                    DspMode::Multiply18
                };
                let lane = self.schedule.nodes[event].lane.ok_or("missing DSP lane")?;
                let instance = self
                    .dsp
                    .instances
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| i.mode == mode)
                    .nth(lane)
                    .ok_or("missing DSP instance")?
                    .0;
                issues.push(DspIssue {
                    instance,
                    issue: times[event].issue,
                    ready: times[event].ready,
                    work: DspWork::Multiply {
                        a_bits: self.counted.frame.values[e.inputs[0]].format.bits,
                        b_bits: self.counted.frame.values[e.inputs[1]].format.bits,
                    },
                });
            }
        }
        self.dsp
            .audit_issues(&issues, None, self.hardware.max_cycles)
            .map_err(err)?;
        for (vertex, &publish) in self.publication.iter().enumerate() {
            let writes:Vec<_>=self.counted.frame.events.iter().filter(|e| matches!(e.operation,Operation::Write{memory,row} if self.counted.frame.memories[memory].name=="TRANSFORMED" && row/7==vertex)).collect();
            if writes.len() != 7
                || publish
                    != writes
                        .iter()
                        .map(|e| times[e.id].ready)
                        .max()
                        .ok_or("no vertex rows")?
                        + 1
            {
                return Err("publication before complete seven-row vertex".into());
            }
        }
        if self.publication.len() != self.counted.outputs.len() {
            return Err("publication shape".into());
        }
        Ok(())
    }
    pub fn memory_usage(&self) -> Result<GowinMemoryUsage, String> {
        self.layout
            .audit_gowin_budget(
                &self.counted.frame,
                GowinMemoryBudget {
                    bsram_blocks: 2,
                    ssram_cells: 80,
                },
            )
            .map_err(err)
    }
    pub fn dsp_utilization(&self) -> (f64, f64) {
        let count = |r| {
            self.counted
                .frame
                .events
                .iter()
                .filter(|e| e.resource == Some(r))
                .count() as f64
        };
        (
            count(Resource::Dsp36) / (self.schedule.cycles as f64 * self.hardware.wide as f64),
            count(Resource::Dsp18) / (self.schedule.cycles as f64 * self.hardware.narrow as f64),
        )
    }
}
