use super::*;
use crate::lighting::sim::workbench::{SchedulePlan, Slot};
use crate::lighting::{calendars::UnifiedCalendar, rtl::LightingRtlOptions, LightingQuantization};
fn keys(p: &Program) -> Vec<u64> {
    let mut keys = Vec::new();
    for v in &p.frame.values {
        let e = &p.frame.events[v.producer];
        let op = match e.operation {
            Operation::Read { memory, row } => {
                format!("Read:{}:{row}", p.frame.memories[memory].name)
            }
            Operation::Literal => format!("Literal:{}", v.raw),
            _ => format!("{:?}", e.operation),
        };
        let inputs = e.inputs.iter().map(|&v| keys[v]).collect::<Vec<_>>();
        let text = format!("{op}|{:?}|{inputs:?}", v.format);
        keys.push(text.bytes().fold(0xcbf29ce484222325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        }));
    }
    keys
}
#[test]
fn h9_rebuild_geometry_and_tail_calendar() {
    let q = LightingQuantization::CompensatedFloor;
    let opts = UnifiedCalendar::Free.options(q);
    let make = |opts: LightingRtlOptions| {
        Program::with_retiming(
            LightingProfile::Fast,
            true,
            opts.dedicated_dsp,
            opts.kernel(),
            opts.role_schedule,
            opts.logic_depth,
            opts.retiming,
        )
        .unwrap()
    };
    let mut old = make(opts);
    old.apply_plan(&UnifiedCalendar::Free.plans(q).unwrap()[0])
        .unwrap();
    let options = LightingRtlOptions {
        smooth_h9: true,
        h9_tail_dsp: true,
        power_context_masks: false,
        ..opts
    };
    let mut new = make(options);
    for e in &new.frame.events {
        if e.output.is_some_and(|v| {
            new.frame.values[v]
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with("mode."))
        }) {
            new.graph.nodes[e.id].resource = None;
            new.binding.kinds[e.id] = None;
        }
    }
    let oldkeys = keys(&old);
    let newkeys = keys(&new);
    let mut before = BTreeMap::new();
    for (id, node) in old.graph.nodes.iter().enumerate() {
        if let (Some(r), Some(v)) = (
            node.resource,
            old.frame.events.get(id).and_then(|e| e.output),
        ) {
            before
                .entry((oldkeys[v], old.graph.resources[r].name.clone()))
                .or_insert_with(Vec::new)
                .push(id);
        }
    }
    let mut slots = BTreeMap::new();
    let mut mapped = 0;
    for (id, node) in new.graph.nodes.iter().enumerate() {
        if let (Some(r), Some(v)) = (
            node.resource,
            new.frame.events.get(id).and_then(|e| e.output),
        ) {
            if let Some(ids) = before.get(&(newkeys[v], new.graph.resources[r].name.clone())) {
                if ids.len() == 1 {
                    let prior = old.schedule.nodes[ids[0]];
                    slots.insert(
                        id,
                        Slot {
                            id,
                            issue: prior.issue,
                            lane: prior.lane.unwrap(),
                        },
                    );
                    mapped += 1;
                }
            }
        }
    }
    let mut capacities = new
        .graph
        .resources
        .iter()
        .map(|r| r.lanes)
        .collect::<Vec<_>>();
    for (r, res) in new.graph.resources.iter().enumerate() {
        if let Some((oldr, _)) = old
            .graph
            .resources
            .iter()
            .enumerate()
            .find(|(_, v)| v.name == res.name)
        {
            capacities[r] = old.graph.resources[oldr].lanes;
        }
    }
    let mut occupied = BTreeSet::new();
    for (id, slot) in &slots {
        occupied.insert((
            new.graph.nodes[*id].resource.unwrap(),
            slot.lane,
            slot.issue % 2,
        ));
    }
    let mut ready = BTreeMap::new();
    let mut pending = (0..new.graph.nodes.len()).collect::<BTreeSet<_>>();
    for _ in 0..new.graph.nodes.len() + 1 {
        let available = pending
            .iter()
            .copied()
            .filter(|&id| {
                new.graph.nodes[id]
                    .predecessors
                    .iter()
                    .all(|id| ready.contains_key(id))
            })
            .collect::<Vec<_>>();
        if available.is_empty() {
            break;
        }
        for id in available {
            let node = &new.graph.nodes[id];
            let earliest = node
                .predecessors
                .iter()
                .map(|p| ready[p])
                .max()
                .unwrap_or(0);
            let age = if let Some(r) = node.resource {
                let latency = new.graph.resources[r].latency;
                if let Some(slot) = slots.get(&id) {
                    assert!(
                        slot.issue >= earliest,
                        "mapped dependency {id}: {}<{earliest}",
                        slot.issue
                    );
                    slot.issue + latency
                } else {
                    let (issue, lane) = (earliest..earliest + 100)
                        .find_map(|issue| {
                            (0..capacities[r])
                                .find(|&lane| !occupied.contains(&(r, lane, issue % 2)))
                                .map(|lane| (issue, lane))
                        })
                        .expect("no candidate slot");
                    occupied.insert((r, lane, issue % 2));
                    slots.insert(id, Slot { id, issue, lane });
                    println!(
                        "H9_NEW {id} {} issue={issue} lane={lane}",
                        new.graph.resources[r].name
                    );
                    issue + latency
                }
            } else {
                earliest
            };
            ready.insert(id, age);
            pending.remove(&id);
        }
    }
    assert!(pending.is_empty(), "candidate cycle");
    let plan = SchedulePlan {
        ii: 2,
        capacities,
        slots: slots.into_values().collect(),
        preconnected_modes: true,
    };
    new.apply_plan(&plan).unwrap();
    for ins in &new.instructions {
        if new.binding.kinds[ins.root] == Some(crate::lighting::sim::timed::LaneKind::H9Tail) {
            crate::lighting::sim::pipeline::audit_h9_tail_interface(
                &new.frame,
                ins.root,
                &ins.inputs,
            )
            .unwrap();
            let mut swapped = ins.inputs.clone();
            swapped.swap(0, 1);
            assert!(crate::lighting::sim::pipeline::audit_h9_tail_interface(
                &new.frame, ins.root, &swapped
            )
            .is_err());
        }
    }
    println!(
        "H9_PROGRAM mapped={mapped} latency={} II={} instructions={}",
        new.latency,
        new.ii,
        new.instructions.len()
    );
    let mut text = format!(
        "2\n{}\nid,issue,lane\n",
        plan.capacities
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    );
    use std::fmt::Write;
    for slot in &plan.slots {
        writeln!(text, "{},{},{}", slot.id, slot.issue, slot.lane).unwrap();
    }
    assert_eq!((new.latency, new.ii), (34, 2));
    if std::env::var_os("LIGHTING_REGENERATE_H9_PLAN").is_some() {
        std::fs::write(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/spec/lighting-calendars/free-h9.plan"
            ),
            text,
        )
        .unwrap();
    } else {
        assert_eq!(
            text,
            include_str!("../../spec/lighting-calendars/free-h9.plan").replace("\r\n", "\n")
        );
    }
}
