//! Closed graph recognition and a staged resource model. It does not alter the ledger.
use crate::{prepare, Spec, SPECS};
use audited::{Event, FrameReport, Operation};
use resource_scheduler::{check, plan, Graph, Limits, Node, Schedule, SearchConfig};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Group {
    pub members: BTreeSet<usize>,
    pub output_event: usize,
    pub value: usize,
    pub exponent: usize,
    pub spec: Spec,
}

fn producer(f: &FrameReport, value: usize) -> &Event {
    &f.events[f.values[value].producer]
}

fn constant(f: &FrameReport, value: usize, raw: i128) -> bool {
    matches!(producer(f, value).operation, Operation::Literal) && f.values[value].raw == raw
}

fn recognize(f: &FrameReport, end: &Event) -> Option<Group> {
    if end.operation != Operation::Resize || end.inputs.len() != 1 {
        return None;
    }
    let add = producer(f, end.inputs[0]);
    if add.operation != Operation::Add {
        return None;
    }
    let floor = producer(f, add.inputs[0]);
    let round = producer(f, add.inputs[1]);
    let Operation::RescaleFloor(drop) = floor.operation else {
        return None;
    };
    if round.operation != Operation::RoundIncrement(drop) || floor.inputs != round.inputs {
        return None;
    }
    let select = producer(f, floor.inputs[0]);
    if select.operation != Operation::Select {
        return None;
    }
    let jam = producer(f, select.inputs[0]);
    let inc = producer(f, select.inputs[1]);
    let shift = producer(f, select.inputs[2]);
    if jam.operation != Operation::Select
        || inc.operation != Operation::Add
        || shift.operation != Operation::Shift
    {
        return None;
    }
    if inc.inputs[0] != select.inputs[2] || !constant(f, inc.inputs[1], 1) {
        return None;
    }
    let low = producer(f, jam.inputs[0]);
    let sticky = producer(f, jam.inputs[2]);
    if low.operation != Operation::Slice(0)
        || low.inputs != [select.inputs[2]]
        || !constant(f, jam.inputs[1], 0)
        || sticky.operation != Operation::Less
    {
        return None;
    }
    if !constant(f, sticky.inputs[0], 0) {
        return None;
    }
    let lost = producer(f, sticky.inputs[1]);
    if lost.operation != Operation::Sub || lost.inputs[0] != shift.inputs[0] {
        return None;
    }
    let recover = producer(f, lost.inputs[1]);
    let negate = producer(f, shift.inputs[1]);
    if recover.operation != Operation::Shift
        || recover.inputs[0] != select.inputs[2]
        || negate.operation != Operation::Sub
        || !constant(f, negate.inputs[0], 0)
        || negate.inputs[1] != recover.inputs[1]
    {
        return None;
    }
    let input = &f.values[shift.inputs[0]];
    let e = &f.values[recover.inputs[1]];
    let output = &f.values[end.output?];
    let spec = Spec {
        fraction: input.format.fraction,
        bits: output.format.bits,
        out_fraction: output.format.fraction,
    };
    if input.format.bits != 72
        || !input.format.signed
        || e.format.bits != 18
        || e.format.fraction != 0
        || !e.format.signed
        || !output.format.signed
        || !SPECS.contains(&spec)
        || drop != spec.fraction - spec.out_fraction
    {
        return None;
    }
    let guard = &f.events[shift.control?];
    if guard.operation != Operation::Require(true) {
        return None;
    }
    let compare = producer(f, guard.inputs[0]);
    if compare.operation != Operation::Less
        || compare.inputs[0] != recover.inputs[1]
        || !constant(f, compare.inputs[1], 32)
    {
        return None;
    }
    let intermediate = audited::Format {
        bits: spec.bits + u32::from(72 - drop > spec.bits),
        fraction: spec.out_fraction,
        signed: true,
    };
    if [shift, recover, lost, inc, select]
        .iter()
        .any(|event| event.output.map(|v| f.values[v].format) != Some(input.format))
        || [floor, add]
            .iter()
            .any(|event| event.output.map(|v| f.values[v].format) != Some(intermediate))
    {
        return None;
    }
    let members: BTreeSet<_> = [
        shift.id, recover.id, lost.id, sticky.id, low.id, jam.id, inc.id, select.id, floor.id,
        round.id, add.id, end.id,
    ]
    .into_iter()
    .collect();
    if members
        .iter()
        .any(|&id| f.events[id].control != Some(guard.id))
    {
        return None;
    }
    Some(Group {
        members,
        output_event: end.id,
        value: shift.inputs[0],
        exponent: recover.inputs[1],
        spec,
    })
}

/// Refuse observations, stored/external intermediate consumers, or overlapping groups.
/// Recognition depends on operations/formats, never sample exponent/value ranges.
pub fn groups(f: &FrameReport) -> Result<Vec<Group>, String> {
    f.audit().map_err(|e| format!("numerical audit: {e:?}"))?;
    // An internally consistent fault report is auditable, but is not a
    // successful expression trace and must never acquire a schedule.
    if !f.valid || !f.faults.is_empty() {
        return Err("failed numerical frame".into());
    }
    let mut groups = Vec::new();
    let mut occupied = BTreeSet::new();
    for end in &f.events {
        let Some(group) = recognize(f, end) else {
            continue;
        };
        for event in &f.events {
            for &value in &event.inputs {
                let p = f.values[value].producer;
                if group.members.contains(&p)
                    && p != group.output_event
                    && !group.members.contains(&event.id)
                {
                    return Err(format!("intermediate consumer at event {}", event.id));
                }
            }
            if let Some(control) = event.control {
                if group.members.contains(&control) && !group.members.contains(&event.id) {
                    return Err("intermediate control escape".into());
                }
            }
        }
        for o in &f.outputs {
            let p = f.values[o.value].producer;
            if group.members.contains(&p) && p != group.output_event {
                return Err("intermediate observation".into());
            }
        }
        if group.members.iter().any(|e| !occupied.insert(*e)) {
            return Err("overlapping groups".into());
        }
        let result = prepare(
            group.spec,
            f.values[group.value].raw,
            f.values[group.exponent].raw,
        )
        .and_then(|w| w.finish())
        .map_err(|e| format!("bound expression rejected trace: {e:?}"))?;
        if result != f.values[end.output.unwrap()].raw {
            return Err("bound expression disagrees with ledger".into());
        }
        groups.push(group);
    }
    Ok(groups)
}

#[derive(Clone, Debug)]
pub struct Metrics {
    pub cycles: u64,
    pub resource_issues: usize,
    pub peak_payload_bits: u64,
    pub payload_bit_cycles: u64,
    pub dependency_payload_bits: u64,
    pub issues_by_role: BTreeMap<String, usize>,
}

pub struct Comparison {
    pub baseline: Metrics,
    pub candidate: Metrics,
    pub expressions: usize,
    pub graph: Graph,
    pub schedule: Schedule,
}

fn metrics(
    graph: &Graph,
    schedule: &Schedule,
    widths: &[u32],
    data_inputs: &[Vec<usize>],
) -> Metrics {
    let mut changes = BTreeMap::<u64, i64>::new();
    let mut last: Vec<_> = schedule.nodes.iter().map(|n| n.ready).collect();
    let mut transport = 0;
    let mut issues = BTreeMap::new();
    for (id, node) in graph.nodes.iter().enumerate() {
        // Ordering/field barriers constrain issue, but do not consume payload.
        // Do not inflate register lifetimes by treating every control edge as data.
        for &p in &data_inputs[id] {
            last[p] = last[p].max(schedule.nodes[id].issue);
            transport += u64::from(widths[p]);
        }
        if let Some(r) = node.resource {
            *issues.entry(graph.resources[r].name.clone()).or_insert(0) += 1;
        }
    }
    let mut bit_cycles = 0;
    for (id, &width) in widths.iter().enumerate() {
        if width == 0 {
            continue;
        }
        let start = schedule.nodes[id].ready;
        bit_cycles += u64::from(width) * (last[id] - start + 1);
        *changes.entry(start).or_default() += i64::from(width);
        *changes.entry(last[id] + 1).or_default() -= i64::from(width);
    }
    let (mut live, mut peak) = (0_i64, 0_i64);
    for &delta in changes.values() {
        live += delta;
        peak = peak.max(live);
    }
    Metrics {
        cycles: schedule.cycles,
        resource_issues: issues.values().sum(),
        peak_payload_bits: peak as u64,
        payload_bit_cycles: bit_cycles,
        dependency_payload_bits: transport,
        issues_by_role: issues,
    }
}

fn schedule(graph: &Graph) -> Result<Schedule, String> {
    let limits = Limits::new(200_000, 1_000_000, 1);
    let schedule = plan(graph, &limits, &SearchConfig::default())
        .map_err(|e| format!("plan: {e:?}"))?
        .best_candidate()
        .schedule
        .clone();
    let audit = check(graph, &limits, &schedule);
    if !audit.is_ok() {
        return Err(format!("calendar: {audit:?}"));
    }
    Ok(schedule)
}

/// Stage assumptions: existing shift72 latency2, round72 latency1, then an
/// existing width-appropriate add lane. No lane/port/DSP inventory is increased.
/// Sticky/range logic inside shift72 is an unverified combinational timing cone.
pub fn compare(f: &FrameReport, baseline: &Graph) -> Result<Comparison, String> {
    if baseline.nodes.len() != f.events.len() {
        return Err("requires one node per event, without synthetic issue tokens".into());
    }
    let groups = groups(f)?;
    let role = |name: &str| {
        baseline
            .resources
            .iter()
            .position(|r| r.name == name)
            .ok_or_else(|| format!("missing {name}"))
    };
    let shift = role("shift72")?;
    let round = role("round72")?;
    let mut graph = Graph {
        resources: baseline.resources.clone(),
        nodes: Vec::new(),
    };
    let mut map = vec![0; f.events.len()];
    let mut owner = BTreeMap::new();
    for (g, group) in groups.iter().enumerate() {
        for &e in &group.members {
            owner.insert(e, g);
        }
    }
    let mut stages = Vec::new();
    let mut widths = Vec::new();
    let mut data_inputs = Vec::new();
    for event in &f.events {
        if let Some(&g) = owner.get(&event.id) {
            let group = &groups[g];
            if event.id != *group.members.first().unwrap() {
                continue;
            }
            let spec = group.spec;
            let mid = spec.bits + u32::from(72 - (spec.fraction - spec.out_fraction) > spec.bits);
            let adder = role(if mid <= 18 {
                "small-control"
            } else if mid <= 36 {
                "add36"
            } else {
                "add54"
            })?;
            let first = graph.nodes.len();
            // Include valid and format-code transport, rather than charging
            // only mantissa bits while silently carrying host Spec metadata.
            for (stage, r, width) in [
                ("window", shift, mid + 6),
                ("round", round, mid + 5),
                ("checked-add", adder, spec.bits + 1),
            ] {
                graph.nodes.push(Node {
                    name: format!("scale.{g}.{stage}"),
                    predecessors: vec![],
                    earliest: 0,
                    resource: Some(r),
                });
                widths.push(width);
                data_inputs.push(Vec::new());
            }
            for &e in &group.members {
                map[e] = first + 2;
            }
            stages.push((g, first));
        } else {
            map[event.id] = graph.nodes.len();
            graph.nodes.push(baseline.nodes[event.id].clone());
            widths.push(if event.operation == Operation::Literal {
                0
            } else {
                event.output.map_or(0, |v| f.values[v].format.bits)
            });
            data_inputs.push(Vec::new());
        }
    }
    for event in &f.events {
        if owner.contains_key(&event.id) {
            continue;
        }
        graph.nodes[map[event.id]].predecessors = baseline.nodes[event.id]
            .predecessors
            .iter()
            .map(|&p| map[p])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        data_inputs[map[event.id]] = event
            .inputs
            .iter()
            .map(|&v| map[f.values[v].producer])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    for (g, first) in stages {
        let group = &groups[g];
        let mut deps = BTreeSet::new();
        let mut data = BTreeSet::new();
        for &e in &group.members {
            for &p in &baseline.nodes[e].predecessors {
                if !group.members.contains(&p) {
                    deps.insert(map[p]);
                }
            }
            for &v in &f.events[e].inputs {
                let p = f.values[v].producer;
                if !group.members.contains(&p) {
                    data.insert(map[p]);
                }
            }
            graph.nodes[first].earliest =
                graph.nodes[first].earliest.max(baseline.nodes[e].earliest);
        }
        graph.nodes[first].predecessors = deps.into_iter().collect();
        graph.nodes[first + 1].predecessors = vec![first];
        graph.nodes[first + 2].predecessors = vec![first + 1];
        data_inputs[first] = data.into_iter().collect();
        data_inputs[first + 1] = vec![first];
        data_inputs[first + 2] = vec![first + 1];
    }
    let base_schedule = schedule(baseline)?;
    let new_schedule = schedule(&graph)?;
    let base_widths: Vec<_> = f
        .events
        .iter()
        .map(|e| {
            if e.operation == Operation::Literal {
                0
            } else {
                e.output.map_or(0, |v| f.values[v].format.bits)
            }
        })
        .collect();
    let base_data_inputs: Vec<Vec<_>> = f
        .events
        .iter()
        .map(|e| {
            e.inputs
                .iter()
                .map(|&v| f.values[v].producer)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .collect();
    Ok(Comparison {
        baseline: metrics(baseline, &base_schedule, &base_widths, &base_data_inputs),
        candidate: metrics(&graph, &new_schedule, &widths, &data_inputs),
        expressions: groups.len(),
        graph,
        schedule: new_schedule,
    })
}
