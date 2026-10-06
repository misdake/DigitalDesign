//! Static hardware program. Values in the template are used only for literals;
//! pixel/context reads are runtime sources, never golden numerical results.
use super::{
    format::*,
    ports::*,
    sim::{binding::BoundDag, counted},
    LightingProfile,
};
use audited::{FrameReport, Operation};
use resource_scheduler::{
    Graph, Limits, ModuloGraph, ModuloSchedule, Node, Resource, SearchConfig,
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod compensation_review {
    use super::*;
    use crate::lighting::{sim::oracle, LightingRetiming};

    #[test]
    #[ignore = "requires the frozen numerical experiment dataset"]
    fn compensated_frozen_dataset_replays_actual_dag() {
        let path = std::env::var_os("LIGHTING_COMPENSATION_DATASET").expect("dataset path");
        let data = std::fs::read_to_string(path).unwrap();
        assert_eq!(data.lines().count(), 32975);
        {
            let profile = LightingProfile::Fast;
            let kernel = counted::Config::compensated_resource_profile(profile);
            let program = Program::with_retiming(
                profile,
                true,
                false,
                kernel,
                true,
                0,
                LightingRetiming::steered_resource_candidate(profile),
            )
            .unwrap();
            let mut stats = [[0_i128; 8]; 5];
            for line in data.lines() {
                let v: Vec<_> = line.split(',').collect();
                let i = |n: usize| v[n].parse::<i32>().unwrap();
                let pixel = PixelInput {
                    normal: [i(1) as i16, i(2) as i16, i(3) as i16],
                    ndc: [i(4), i(5)],
                };
                let material = Material {
                    shininess_code: i(11) as u8,
                    unlit: i(12) != 0,
                    specular_color: if i(13) == 0 { [0; 3] } else { [255; 3] },
                };
                let light = Light {
                    direction: [i(6) as i16, i(7) as i16, i(8) as i16],
                    ambient: i(9) as u16,
                    directional: i(10) as u16,
                };
                let projection = Projection {
                    k: i(14) as i16,
                    ray_scale: [i(15) as i16, i(16) as i16],
                };
                let ctx = LightingContext {
                    material,
                    light,
                    projection,
                    epoch: 0,
                };
                let mut values = vec![None; program.frame.values.len()];
                for &j in &program.order {
                    for (value, raw) in program
                        .execute(&program.instructions[j], &values, pixel, ctx)
                        .unwrap()
                    {
                        values[value] = Some(raw);
                    }
                }
                let actual = program.output_values.map(|v| values[v].unwrap());
                let golden = oracle::evaluate(
                    pixel,
                    material,
                    light,
                    projection,
                    oracle::Config::from_counted(kernel),
                )
                .unwrap();
                assert_eq!(actual, [golden.g, golden.h], "{profile:?}: {line}");
                let s = &mut stats[i(0) as usize];
                s[0] += 1;
                s[1] += i128::from(actual != [i(17) as i128, i(18) as i128]);
                for k in 0..2 {
                    let e = actual[k] - i(17 + k) as i128;
                    s[2 + k] += e;
                    s[4 + k] += e.abs();
                    s[6 + k] = s[6 + k].max(e.abs());
                }
            }
            for (group, s) in stats.into_iter().enumerate() {
                println!("NUMERIC {profile:?} group={group} n={} changed={} signed={}/{} abs={}/{} max={}/{}",s[0],s[1],s[2],s[3],s[4],s[5],s[6],s[7]);
            }
        }
    }
}

fn ancestors(frame: &FrameReport, names: &[&str]) -> Vec<bool> {
    let mut result = vec![false; frame.events.len()];
    let mut pending: Vec<_> = frame
        .outputs
        .iter()
        .filter(|o| names.contains(&o.name.as_str()))
        .map(|o| frame.values[o.value].producer)
        .collect();
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut result[id], true) {
            continue;
        }
        pending.extend(
            frame.events[id]
                .inputs
                .iter()
                .map(|&v| frame.values[v].producer),
        );
    }
    result
}

#[derive(Clone)]
pub(crate) struct Instruction {
    pub root: usize,
    pub members: Vec<usize>,
    pub inputs: Vec<usize>,
    pub issue: usize,
    pub ready: usize,
}

pub(crate) struct Program {
    pub frame: FrameReport,
    pub graph: Graph,
    pub binding: BoundDag,
    pub schedule: ModuloSchedule,
    pub instructions: Vec<Instruction>,
    pub latency: usize,
    pub output_values: [usize; 2],
    pub order: Vec<usize>,
    pub stable: Vec<bool>,
    pub ii: usize,
    pub full: bool,
}
impl Program {
    pub fn with_kernel_depth(
        profile: LightingProfile,
        full: bool,
        dedicated: bool,
        config: counted::Config,
        roles: bool,
        logic_depth: usize,
    ) -> Result<Self, String> {
        Self::with_retiming(
            profile,
            full,
            dedicated,
            config,
            roles,
            logic_depth,
            Default::default(),
        )
    }
    pub fn with_retiming(
        profile: LightingProfile,
        full: bool,
        dedicated: bool,
        config: counted::Config,
        roles: bool,
        logic_depth: usize,
        retiming: super::LightingRetiming,
    ) -> Result<Self, String> {
        if retiming.extra_large_multiply > 3
            || retiming.extra_small_multiply > 4
            || retiming.extra_normalize_reads > 2
        {
            return Err("retiming resource delta outside bounded experiment".into());
        }
        if retiming
            .multiply_latency
            .is_some_and(|n| !(1..=3).contains(&n))
        {
            return Err("ordinary DSP latency must be 1..=3 advancing edges".into());
        }
        if logic_depth != 0 && !(2..=8).contains(&logic_depth) {
            return Err("logic boundary depth must be 2..=8".into());
        }
        if retiming.sum_address && !retiming.measured_functions {
            return Err("sum-address contraction requires typed function bindings".into());
        }
        if config.compact_normal
            || !config.dataflow
            || !config.signed_square
            || !config.power_floor
            || config.prepared_ray
            || config.shared_half
            || config.flat_normal
        {
            return Err("cycle hardware requires the closed architecture pixel ports".into());
        }
        let scalar = config.scalar_norm;
        if config.scalar_normal && !scalar {
            return Err("scalar N requires scalar H".into());
        }
        let frame = if config == counted::Config::architecture() {
            counted::hardware_template(full)
        } else {
            counted::hardware_template_with_config(full, config)
        }
        .map_err(|e| format!("template: {e:?}"))?
        .frame;
        frame
            .audit()
            .map_err(|e| format!("template audit: {e:?}"))?;
        let mut hardware = profile.hardware();
        hardware.kernel = config;
        if logic_depth != 0 {
            hardware.cone_depth = logic_depth;
        }
        let full_ii = profile.ii(true);
        let scalar_squares: usize = if config.direct_square { 3 } else { 0 };
        let large_work: usize = if scalar { 12 + scalar_squares } else { 14 };
        let mut role_capacity = [
            1,
            3_usize.div_ceil(full_ii),
            (large_work - 5).div_ceil(full_ii),
        ];
        let mut system_cone_work = BTreeMap::<super::sim::timed::LaneKind, usize>::new();
        if !profile.system() && config.scalar_normal {
            // The factor kernel must budget the actual full AND diffuse rates.
            // In particular Fast diffuse II1 cannot borrow an II2 full budget.
            let mut small_capacity = 0;
            let mut read_capacity = 0;
            role_capacity = [0; 3];
            for mode in [true, false] {
                let f = counted::hardware_template_with_config(mode, config)
                    .map_err(|e| format!("factor workload: {e:?}"))?
                    .frame;
                let b = BoundDag::new(&f, hardware)?;
                let ray = ancestors(&f, &["ray.0", "ray.1"]);
                let view = ancestors(
                    &f,
                    &[
                        "v.0",
                        "v.1",
                        "v.2",
                        "weighted.light.0",
                        "weighted.light.1",
                        "weighted.light.2",
                    ],
                );
                let mut large = [0_usize; 3];
                let mut small = 0_usize;
                let mut reads = 0_usize;
                let mut cones = BTreeMap::new();
                for (id, kind) in b.kinds.iter().enumerate() {
                    match kind {
                        Some(k @ super::sim::timed::LaneKind::LogicCone { .. }) => {
                            *cones.entry(k.clone()).or_insert(0_usize) += 1
                        }
                        Some(super::sim::timed::LaneKind::LargeMultiply) => {
                            large[if ray[id] {
                                0
                            } else if view[id] {
                                1
                            } else {
                                2
                            }] += 1
                        }
                        Some(super::sim::timed::LaneKind::SmallMultiply) => small += 1,
                        Some(super::sim::timed::LaneKind::NormalizeRead) => reads += 1,
                        _ => (),
                    }
                }
                let ii = profile.ii(mode);
                for (capacity, work) in role_capacity.iter_mut().zip(large) {
                    *capacity = (*capacity).max(work.div_ceil(ii));
                }
                small_capacity = small_capacity.max(small.div_ceil(ii));
                read_capacity = read_capacity.max(reads.div_ceil(ii));
                for (k, work) in cones {
                    // Existing graph builder stores effective work in full-II units.
                    let effective_work = work.div_ceil(ii) * full_ii;
                    system_cone_work
                        .entry(k)
                        .and_modify(|n| *n = (*n).max(effective_work))
                        .or_insert(effective_work);
                }
            }
            hardware.large_multiply = role_capacity.iter().sum();
            hardware.small_multiply = small_capacity;
            hardware.normalize_reads = read_capacity;
        } else if profile.system() {
            // Budget both modes from the complete full body, not one sample
            // path or the smaller diffuse body. The binding stays audited.
            let full_frame = counted::hardware_template_with_config(true, config)
                .map_err(|e| format!("full workload: {e:?}"))?
                .frame;
            let full_binding = BoundDag::new(&full_frame, hardware)?;
            let rays = ancestors(&full_frame, &["ray.0", "ray.1"]);
            let views = ancestors(
                &full_frame,
                &[
                    "v.0",
                    "v.1",
                    "v.2",
                    "weighted.light.0",
                    "weighted.light.1",
                    "weighted.light.2",
                ],
            );
            let mut large = [0_usize; 3];
            let mut small = 0_usize;
            let mut reads = 0_usize;
            for (id, kind) in full_binding.kinds.iter().enumerate() {
                if let Some(k @ super::sim::timed::LaneKind::LogicCone { .. }) = kind {
                    *system_cone_work.entry(k.clone()).or_insert(0_usize) += 1;
                }
                match kind {
                    Some(super::sim::timed::LaneKind::LargeMultiply) => {
                        large[if rays[id] {
                            0
                        } else if views[id] {
                            1
                        } else {
                            2
                        }] += 1;
                    }
                    Some(super::sim::timed::LaneKind::SmallMultiply) => small += 1,
                    Some(super::sim::timed::LaneKind::NormalizeRead) => reads += 1,
                    _ => {}
                }
            }
            role_capacity = large.map(|work| work.div_ceil(full_ii));
            hardware.large_multiply = if roles {
                role_capacity.iter().sum()
            } else {
                large.iter().sum::<usize>().div_ceil(full_ii)
            };
            hardware.small_multiply = small.div_ceil(full_ii);
            hardware.normalize_reads = reads.div_ceil(full_ii);
        } else if scalar {
            hardware.large_multiply = if roles {
                1 + 3_usize.div_ceil(full_ii) + (large_work - 5).div_ceil(full_ii)
            } else {
                large_work.div_ceil(full_ii)
            };
            hardware.small_multiply = (14 - scalar_squares).div_ceil(full_ii);
            hardware.normalize_reads = (12 - scalar_squares).div_ceil(full_ii);
        }
        if config.dedicated_dots {
            hardware.paired_macros = 2;
        }
        if dedicated {
            hardware.small_multiply = 32;
            hardware.large_multiply = 32;
        }
        hardware.measured_functions = retiming.measured_functions;
        hardware.combine_sum_address = retiming.sum_address;
        if let Some(latency) = retiming.paired_latency {
            if !matches!(latency, 1 | 4) {
                return Err("paired DSP latency must be one or four edges".into());
            }
            hardware.paired_latency = latency;
        }
        if let Some(latency) = retiming.cone_latency {
            if !(1..=2).contains(&latency) {
                return Err("generic cone latency must be one or two edges".into());
            }
            hardware.cone_latency = latency;
        }
        if let Some(latency) = retiming.multiply_latency {
            hardware.multiply_latency = latency;
        }
        hardware.large_multiply += retiming.extra_large_multiply;
        hardware.small_multiply += retiming.extra_small_multiply;
        hardware.normalize_reads += retiming.extra_normalize_reads;
        role_capacity[2] += retiming.extra_large_multiply;
        // A real kind-separated Gowin placement, not fractional multiplier sums.
        if !dedicated {
            let macros = hardware.small_multiply.div_ceil(4)
                + hardware.large_multiply.div_ceil(2)
                + hardware.paired_macros;
            hardware.dsp_tiles = hardware.dsp_tiles.max(macros.div_ceil(2));
            hardware.dsp_inventory()?;
        }
        let ii = profile.ii(full);
        let binding = BoundDag::new(&frame, hardware)?;
        if profile.system() {
            let mut current = BTreeMap::new();
            for k in binding.kinds.iter().flatten() {
                if matches!(k, super::sim::timed::LaneKind::LogicCone { .. }) {
                    *current.entry(k.clone()).or_insert(0_usize) += 1;
                }
            }
            for (k, count) in current {
                system_cone_work
                    .entry(k)
                    .and_modify(|n| *n = (*n).max(count))
                    .or_insert(count);
            }
        }
        binding.audit_logic_depth(&frame, hardware)?;
        let mut graph = Graph::default();
        if config.direct_square && !scalar {
            return Err("direct square study requires scalar architecture".into());
        }
        if roles && (!scalar || dedicated) {
            return Err("role schedule requires shared scalar architecture".into());
        }
        let ray = ancestors(&frame, &["ray.0", "ray.1"]);
        let view = ancestors(
            &frame,
            &[
                "v.0",
                "v.1",
                "v.2",
                "weighted.light.0",
                "weighted.light.1",
                "weighted.light.2",
            ],
        );
        let mut resources = BTreeMap::new();
        let mut physical_resources = Vec::new();
        for (id, kind) in binding.kinds.iter().enumerate() {
            let resource = kind.as_ref().map(|k| {
                let role = if roles && matches!(k, super::sim::timed::LaneKind::LargeMultiply) {
                    if ray[id] {
                        1
                    } else if view[id] {
                        2
                    } else {
                        3
                    }
                } else {
                    0
                };
                *resources.entry((k.clone(), role)).or_insert_with(|| {
                    let (mut lanes, latency) = hardware.unit(k);
                    if let Some(&work) = system_cone_work.get(k) {
                        lanes = work.div_ceil(full_ii);
                    }
                    let base = match role {
                        0 | 1 => 0,
                        2 => role_capacity[0],
                        3 => role_capacity[0] + role_capacity[1],
                        _ => unreachable!(),
                    };
                    if role != 0 {
                        lanes = role_capacity[role - 1];
                    }
                    let id = graph.resources.len();
                    graph.resources.push(Resource {
                        name: if role == 0 {
                            format!("{k:?}")
                        } else {
                            format!("{k:?}/role{role}")
                        },
                        lanes,
                        latency,
                        initiation_interval: 1,
                    });
                    physical_resources.push((k.clone(), base, lanes));
                    id
                })
            });
            graph.nodes.push(Node {
                name: format!("event{id}"),
                predecessors: binding.dependencies[id].clone(),
                earliest: 0,
                resource,
            });
        }
        let output_values =
            ["g", "h"].map(|name| frame.outputs.iter().find(|o| o.name == name).unwrap().value);
        graph.resources.push(Resource {
            name: "commit".into(),
            lanes: 1,
            latency: 1,
            initiation_interval: 1,
        });
        graph.nodes.push(Node {
            name: "commit".into(),
            predecessors: output_values
                .iter()
                .map(|&v| frame.values[v].producer)
                .collect(),
            earliest: 0,
            resource: Some(graph.resources.len() - 1),
        });
        let modulo = ModuloGraph::from_graph(&graph).map_err(|e| format!("graph: {e:?}"))?;
        if profile.system() {
            let mut work = vec![0_usize; graph.resources.len()];
            for node in &graph.nodes {
                if let Some(r) = node.resource {
                    work[r] += 1;
                }
            }
            let pressure: Vec<_> = graph
                .resources
                .iter()
                .zip(work)
                .filter(|(r, n)| n.div_ceil(r.lanes) > ii)
                .map(|(r, n)| format!("{}: work {n}/lanes {}", r.name, r.lanes))
                .collect();
            if !pressure.is_empty() {
                return Err(format!("II{ii} resource pressure: {}", pressure.join(", ")));
            }
        }
        let mut schedule = resource_scheduler::modulo_schedule_bounded(
            &modulo,
            ii as u64,
            &Limits::new(4096, 20000, 32),
            &SearchConfig::default(),
        )
        .map_err(|e| format!("II{ii}: {e:?}"))?;
        if dedicated {
            let mut ordinals = BTreeMap::new();
            for (id, kind) in binding.kinds.iter().enumerate() {
                if let Some(kind) = kind {
                    if matches!(
                        kind,
                        super::sim::timed::LaneKind::SmallMultiply
                            | super::sim::timed::LaneKind::LargeMultiply
                    ) {
                        let lane = ordinals.entry(kind.clone()).or_insert(0);
                        schedule.nodes[id].lane = Some(*lane);
                        *lane += 1;
                    }
                }
            }
        }
        if config.dedicated_dots {
            let mut lane = 0;
            for (id, kind) in binding.kinds.iter().enumerate() {
                if matches!(kind, Some(super::sim::timed::LaneKind::PairMultiplyAdd)) {
                    schedule.nodes[id].lane = Some(lane);
                    lane += 1;
                }
            }
            if lane > 2 {
                return Err("dedicated dot inventory".into());
            }
        }
        let checked = resource_scheduler::check_modulo(&modulo, &schedule);
        if !checked.is_ok() {
            return Err(format!("calendar: {checked:?}"));
        }
        if roles {
            // Role resources describe compile-time restrictions only. Merge
            // back to the physical pool and independently check its calendar.
            let mut physical = Graph {
                resources: Vec::new(),
                nodes: graph.nodes.clone(),
            };
            let mut canonical = BTreeMap::new();
            let mut relocation = Vec::new();
            for (kind, base, restricted_lanes) in &physical_resources {
                let id = *canonical.entry(kind.clone()).or_insert_with(|| {
                    let id = physical.resources.len();
                    let (mut lanes, latency) = hardware.unit(kind);
                    if !matches!(kind, super::sim::timed::LaneKind::LargeMultiply) {
                        lanes = *restricted_lanes;
                    }
                    physical.resources.push(Resource {
                        name: format!("{kind:?}"),
                        lanes,
                        latency,
                        initiation_interval: 1,
                    });
                    id
                });
                relocation.push((id, *base));
            }
            let commit = physical.resources.len();
            physical
                .resources
                .push(graph.resources.last().unwrap().clone());
            relocation.push((commit, 0));
            for (node, slot) in physical.nodes.iter_mut().zip(&mut schedule.nodes) {
                if let Some(r) = node.resource {
                    node.resource = Some(relocation[r].0);
                    slot.lane = slot.lane.map(|lane| lane + relocation[r].1);
                }
            }
            let checked = resource_scheduler::check_modulo(
                &ModuloGraph::from_graph(&physical)
                    .map_err(|e| format!("physical roles: {e:?}"))?,
                &schedule,
            );
            if !checked.is_ok() {
                return Err(format!("physical role calendar: {checked:?}"));
            }
            graph = physical;
        }
        let latency = schedule.nodes.last().unwrap().issue as usize + 1;
        let absorbed: BTreeSet<_> = binding
            .groups
            .iter()
            .flat_map(|g| &g.absorbed_events)
            .chain(binding.cones.iter().flat_map(|g| &g.absorbed_events))
            .copied()
            .collect();
        let mut instructions = Vec::new();
        for e in &frame.events {
            if e.output.is_none() || absorbed.contains(&e.id) {
                continue;
            }
            let (mut members, inputs) =
                if let Some(g) = binding.groups.iter().find(|g| g.result_event == e.id) {
                    (g.absorbed_events.clone(), g.operands.clone())
                } else if let Some(g) = binding.cones.iter().find(|g| g.result_event == e.id) {
                    (g.absorbed_events.clone(), g.operands.clone())
                } else {
                    (Vec::new(), e.inputs.clone())
                };
            members.push(e.id);
            members.sort_unstable();
            let issue = schedule.nodes[e.id].issue as usize;
            let ready = issue
                + binding.kinds[e.id]
                    .as_ref()
                    .map_or(0, |k| hardware.unit(k).1) as usize;
            instructions.push(Instruction {
                root: e.id,
                members,
                inputs,
                issue,
                ready,
            });
        }
        // At each age, producer-before-consumer wiring must settle before issue.
        let mut order = Vec::new();
        let mut visited = BTreeSet::new();
        while order.len() < instructions.len() {
            let before = order.len();
            for (id, ins) in instructions.iter().enumerate() {
                if visited.contains(&id) {
                    continue;
                }
                if ins.inputs.iter().all(|&v| {
                    let producer = frame.values[v].producer;
                    matches!(frame.events[producer].operation, Operation::Literal)
                        || instructions
                            .iter()
                            .position(|p| p.members.contains(&producer))
                            .is_some_and(|p| visited.contains(&p))
                }) {
                    visited.insert(id);
                    order.push(id);
                }
            }
            if order.len() == before {
                return Err("hardware instruction cycle".into());
            }
        }
        let mut stable = vec![false; frame.values.len()];
        for &id in &order {
            let ins = &instructions[id];
            let e = &frame.events[ins.root];
            let invariant = match e.operation {
                Operation::Literal => true,
                Operation::Read { memory, .. } => {
                    frame.memories[memory].name.starts_with("context.")
                }
                _ => binding.kinds[ins.root].is_none() && ins.inputs.iter().all(|&v| stable[v]),
            };
            stable[e.output.unwrap()] = invariant;
        }
        if retiming.compact_lifetimes {
            let modulo =
                ModuloGraph::from_graph(&graph).map_err(|e| format!("compact graph: {e:?}"))?;
            let target = resource_scheduler::compact_modulo(&modulo, &schedule)
                .map_err(|e| format!("compact calendar: {e:?}"))?;
            let node_latency = |id: usize| {
                graph.nodes[id]
                    .resource
                    .map_or(0, |r| graph.resources[r].latency)
            };
            let mut owner: Vec<_> = (0..frame.events.len()).collect();
            for ins in &instructions {
                for &id in &ins.members {
                    owner[id] = ins.root;
                }
            }
            // Score only real instruction boundaries, never internal cone values.
            // This is a payload proxy; the emitter's storage CSV remains authoritative.
            let cost = |candidate: &ModuloSchedule| {
                let mut last = BTreeMap::<usize, u64>::new();
                for ins in &instructions {
                    for &v in &ins.inputs {
                        if !stable[v] {
                            let age = candidate.nodes[ins.root].issue;
                            last.entry(v)
                                .and_modify(|t| *t = (*t).max(age))
                                .or_insert(age);
                        }
                    }
                }
                for &v in &output_values {
                    last.insert(v, candidate.span);
                }
                last.into_iter()
                    .map(|(v, end)| {
                        let root = owner[frame.values[v].producer];
                        let ready = candidate.nodes[root].issue + node_latency(root);
                        end.saturating_sub(ready).div_ceil(ii as u64)
                            * u64::from(frame.values[v].format.bits)
                    })
                    .sum::<u64>()
            };
            if cost(&target) < cost(&schedule) {
                schedule = target.clone();
            }
            let mut topo = Vec::new();
            let mut seen = BTreeSet::new();
            while topo.len() < graph.nodes.len() {
                let before = topo.len();
                for (id, node) in graph.nodes.iter().enumerate() {
                    if !seen.contains(&id) && node.predecessors.iter().all(|p| seen.contains(p)) {
                        seen.insert(id);
                        topo.push(id);
                    }
                }
                if topo.len() == before {
                    return Err("compact dependency cycle".into());
                }
            }
            // Two bounded local passes preserve every physical lane and modulo phase.
            // A checked move can delay an early branch without delaying all its inputs.
            for _ in 0..2 {
                let mut tried = 0;
                for &id in topo.iter().rev() {
                    if schedule.nodes[id].lane.is_none()
                        || target.nodes[id].issue <= schedule.nodes[id].issue
                    {
                        continue;
                    }
                    if tried == 64 {
                        break;
                    }
                    tried += 1;
                    let mut trial = schedule.clone();
                    trial.nodes[id].issue = target.nodes[id].issue;
                    for &wire in &topo {
                        if graph.nodes[wire].resource.is_none() {
                            trial.nodes[wire].issue = graph.nodes[wire]
                                .predecessors
                                .iter()
                                .map(|&d| trial.nodes[d].issue + node_latency(d))
                                .max()
                                .unwrap_or(0);
                        }
                    }
                    if resource_scheduler::check_modulo(&modulo, &trial).is_ok()
                        && cost(&trial) < cost(&schedule)
                    {
                        schedule = trial;
                    }
                }
            }
            for ins in &mut instructions {
                ins.issue = schedule.nodes[ins.root].issue as usize;
                ins.ready = ins.issue + node_latency(ins.root) as usize;
            }
        }
        Ok(Self {
            frame,
            graph,
            binding,
            schedule,
            instructions,
            latency,
            output_values,
            order,
            stable,
            ii,
            full,
        })
    }

    pub fn source(
        &self,
        memory: usize,
        row: usize,
        p: PixelInput,
        c: LightingContext,
    ) -> Result<i128, String> {
        let m = &self.frame.memories[memory];
        let value = match m.name.as_str() {
            "pixel.normal-lsb" => Some(i128::from(
                (p.normal[0] as u16 & 7)
                    | ((p.normal[1] as u16 & 7) << 3)
                    | ((p.normal[2] as u16 & 7) << 6),
            )),
            "pixel.rows" => PixelRows::encode(p)
                .map_err(|e| format!("pixel: {e:?}"))?
                .0
                .get(row)
                .copied()
                .map(i128::from),
            "context.light" => c.light.direction.get(row).copied().map(i128::from),
            "context.projection" => [
                c.projection.ray_scale[0],
                c.projection.ray_scale[1],
                c.projection.k,
            ]
            .get(row)
            .copied()
            .map(i128::from),
            "context.intensity" => [c.light.ambient, c.light.directional]
                .get(row)
                .copied()
                .map(i128::from),
            "context.mode" => Some(if self.full { 3 } else { 2 }),
            "context.shininess" => Some(i128::from(c.material.shininess_code)),
            "context.power" => CONTEXT_RAW
                .get(c.material.shininess_code as usize)
                .copied()
                .map(i128::from),
            "SQ" => SQUARE_SIGNED_RAW.get(row).copied().map(i128::from),
            "RSQRT" => RSQRT_RAW.get(row).copied().map(i128::from),
            "SQRT" => SQRT_RAW.get(row).copied().map(i128::from),
            "POWER" => POWER_RAW.get(row).copied().map(i128::from),
            "POWER_MIDPOINT_Q15" => POWER_MIDPOINT_RAW.get(row).copied().map(i128::from),
            _ => None,
        };
        value.ok_or_else(|| format!("source {}[{row}]", m.name))
    }

    pub fn execute(
        &self,
        ins: &Instruction,
        values: &[Option<i128>],
        p: PixelInput,
        c: LightingContext,
    ) -> Result<Vec<(usize, i128)>, String> {
        let mut local = BTreeMap::new();
        for &id in &ins.members {
            let e = &self.frame.events[id];
            let v = e.output.ok_or("missing instruction output")?;
            let get = |index: usize| -> Result<i128, String> {
                let value = e.inputs[index];
                if let Some(raw) = local.get(&value) {
                    return Ok(*raw);
                }
                let producer = &self.frame.events[self.frame.values[value].producer];
                if producer.operation == Operation::Literal {
                    return Ok(self.frame.values[value].raw);
                }
                values[value].ok_or_else(|| format!("event{id} consumed unready value{value}"))
            };
            let f = self.frame.values[v].format;
            let raw = match e.operation {
                Operation::Literal => self.frame.values[v].raw,
                Operation::Add => get(0)? + get(1)?,
                Operation::Sub => get(0)? - get(1)?,
                Operation::Multiply => get(0)? * get(1)?,
                Operation::Resize | Operation::BinaryScale => get(0)?,
                Operation::ShiftLeft(n) => get(0)? << n,
                Operation::Shift => {
                    let a = get(0)?;
                    let n = get(1)?;
                    if !(-126..=126).contains(&n) {
                        return Err("shift range".into());
                    }
                    if n < 0 {
                        a >> -n
                    } else {
                        a << n
                    }
                }
                Operation::LeadingZeros => {
                    let a = get(0)? as u128;
                    let bits = self.frame.values[e.inputs[0]].format.bits;
                    i128::from(a.leading_zeros() - (128 - bits))
                }
                Operation::Slice(n) => {
                    let a = (get(0)? >> n) & ((1_i128 << f.bits) - 1);
                    if f.signed && a & (1_i128 << (f.bits - 1)) != 0 {
                        a - (1_i128 << f.bits)
                    } else {
                        a
                    }
                }
                Operation::RescaleFloor(n) => get(0)? >> n,
                Operation::Less => i128::from(get(0)? < get(1)?),
                Operation::Select => {
                    if get(0)? != 0 {
                        get(1)?
                    } else {
                        get(2)?
                    }
                }
                Operation::RoundIncrement(n) => {
                    let a = get(0)?;
                    let r = a & ((1_i128 << n) - 1);
                    let half = 1_i128 << (n - 1);
                    i128::from(r > half || r == half && (a >> n) & 1 != 0)
                }
                Operation::Read { memory, row } => {
                    let row = if e.inputs.is_empty() {
                        row
                    } else {
                        usize::try_from(get(0)?).map_err(|_| "negative ROM address")?
                    };
                    self.source(memory, row, p, c)?
                }
                _ => return Err(format!("unsupported hardware operation {:?}", e.operation)),
            };
            let (lo, hi) = if f.signed {
                (-(1_i128 << (f.bits - 1)), (1_i128 << (f.bits - 1)) - 1)
            } else {
                (0, (1_i128 << f.bits) - 1)
            };
            if raw < lo || raw > hi {
                return Err(format!("event{id} width overflow {raw} {f:?}"));
            }
            local.insert(v, raw);
        }
        Ok(local.into_iter().collect())
    }
}
