//! Bounded pure-logic cuts. Certificates retain every operation and reject
//! escaping internal results; ROMs, multipliers and control are hard boundaries.
//! These cuts describe actual combinational execution, not fitted timing.
use audited::{physical::LogicCone, FrameReport, Operation};
use std::collections::{BTreeMap, BTreeSet};

fn pure(op: &Operation) -> bool {
    matches!(
        op,
        Operation::Add
            | Operation::Sub
            | Operation::Less
            | Operation::Select
            | Operation::LeadingZeros
            | Operation::Shift
            | Operation::RoundIncrement(_)
            | Operation::Resize
            | Operation::Slice(_)
            | Operation::BinaryScale
            | Operation::RescaleFloor(_)
            | Operation::ShiftLeft(_)
    )
}

fn region(f: &FrameReport, root: usize, stops: &BTreeSet<usize>) -> Result<LogicCone, String> {
    region_many(f, &[root], stops)
}
fn region_many(
    f: &FrameReport,
    roots: &[usize],
    stops: &BTreeSet<usize>,
) -> Result<LogicCone, String> {
    let root = *roots.iter().max().ok_or("empty fabric region")?;
    let mut members = BTreeSet::new();
    let mut pending = roots.to_vec();
    while let Some(id) = pending.pop() {
        let e = &f.events[id];
        if stops.contains(&id) || !pure(&e.operation) || e.control.is_some() || !members.insert(id)
        {
            continue;
        }
        pending.extend(e.inputs.iter().map(|&v| f.values[v].producer));
    }
    // Shared wiring remains wiring at the external boundary. Absorbing an
    // input view would falsely delay another diff/abs lane behind this cone.
    loop {
        let escape: Vec<_> = members
            .iter()
            .copied()
            .filter(|&id| {
                id != root
                    && f.events[id].resource.is_none()
                    && f.events[id]
                        .inputs
                        .iter()
                        .all(|v| !members.contains(&f.values[*v].producer))
            })
            .filter(|&id| {
                let v = f.events[id].output.unwrap();
                f.outputs.iter().any(|o| o.value == v)
                    || f.events
                        .iter()
                        .any(|e| !members.contains(&e.id) && e.inputs.contains(&v))
            })
            .collect();
        if escape.is_empty() {
            break;
        }
        for id in escape {
            members.remove(&id);
        }
    }
    let operands: BTreeSet<_> = members
        .iter()
        .flat_map(|&id| f.events[id].inputs.iter().copied())
        .filter(|&v| !members.contains(&f.values[v].producer))
        .collect();
    let mut exported_events: BTreeSet<_> = members
        .iter()
        .copied()
        .filter(|&id| id != root)
        .filter(|&id| {
            let v = f.events[id].output.unwrap();
            f.outputs.iter().any(|o| o.value == v)
                || f.events.iter().any(|e| {
                    !members.contains(&e.id) && (e.inputs.contains(&v) || e.control == Some(id))
                })
        })
        .collect();
    // Exported views retain their charged source bits, not a second bank of
    // identical aliases. Keep that source visible to bit-granular allocation.
    let mut pending: Vec<_> = exported_events.iter().copied().collect();
    while let Some(id) = pending.pop() {
        let e = &f.events[id];
        if e.resource.is_none() && e.inputs.len() == 1 {
            let source = f.values[e.inputs[0]].producer;
            if source != root && members.contains(&source) && exported_events.insert(source) {
                pending.push(source);
            }
        }
    }
    let max_width = members
        .iter()
        .flat_map(|&id| {
            f.events[id]
                .inputs
                .iter()
                .copied()
                .chain(f.events[id].output)
        })
        .map(|v| f.values[v].format.bits)
        .max()
        .unwrap();
    let cone = LogicCone {
        result_event: root,
        absorbed_events: members.into_iter().filter(|&id| id != root).collect(),
        exported_events: exported_events.into_iter().collect(),
        operands: operands.into_iter().collect(),
        max_width,
        latency: 1,
    };
    cone.audit(f)
        .map_err(|e| format!("explicit fabric region: {e:?}"))?;
    Ok(cone)
}

fn derivative(f: &FrameReport, cones: &mut Vec<LogicCone>) -> Result<(), String> {
    let mut used = BTreeSet::new();
    let axes: Vec<_> = f
        .outputs
        .iter()
        .filter(|o| o.name.starts_with("axis"))
        .map(|o| f.values[o.value].producer)
        .collect();
    if axes.len() != 2 {
        return Err("D cross-extrema shape".into());
    }
    // One physical axis datapath handles U and V on separate enabled edges.
    // Each cut includes diagonal extrema, both signed19 differences and their
    // maximum. The final cut compares the retained two axis magnitudes.
    for axis in axes {
        let cone = region(f, axis, &used)?;
        used.extend(std::iter::once(cone.result_event).chain(cone.absorbed_events.iter().copied()));
        cones.push(cone);
    }
    let slope = f
        .outputs
        .iter()
        .find(|o| o.name == "slope")
        .ok_or("D slope output")?
        .value;
    cones.push(region(f, f.values[slope].producer, &used)?);
    Ok(())
}

// Follow zero-cost views to their charged producer, so a view never becomes
// an artificial register solely because it is a named diagnostic output.
fn origin(f: &FrameReport, mut id: usize) -> usize {
    while f.events[id].resource.is_none()
        && f.events[id].inputs.len() == 1
        && pure(&f.events[id].operation)
    {
        id = f.values[f.events[id].inputs[0]].producer;
    }
    id
}
fn add_region(
    f: &FrameReport,
    roots: &[usize],
    used: &mut BTreeSet<usize>,
    regions: &mut Vec<LogicCone>,
) -> Result<(), String> {
    let roots: Vec<_> = roots
        .iter()
        .copied()
        .map(|id| origin(f, id))
        .filter(|id| !used.contains(id) && pure(&f.events[*id].operation))
        .collect();
    if roots.is_empty() {
        return Ok(());
    }
    let c = region_many(f, &roots, used)?;
    used.extend(c.absorbed_events.iter().copied().chain([c.result_event]));
    regions.push(c);
    Ok(())
}
fn output(f: &FrameReport, name: &str) -> Result<usize, String> {
    f.outputs
        .iter()
        .find(|o| o.name == name)
        .map(|o| f.values[o.value].producer)
        .ok_or_else(|| format!("missing fusion output {name}"))
}
pub(super) fn extend(f: &FrameReport, cones: &mut Vec<LogicCone>) -> Result<(), String> {
    if f.name == "texture_derivatives" {
        return derivative(f, cones);
    }
    if !matches!(
        f.name.as_str(),
        "texture_lod_context" | "texture_coordinates"
    ) {
        return Ok(());
    }
    let mut regions = Vec::new();
    let mut used = BTreeSet::new();
    if f.name == "texture_coordinates" {
        // Two actual edges: signed fine/coarse coordinates, then fraction/wrap.
        for axis in 0..2 {
            add_region(
                f,
                &[output(f, &format!("q1.{axis}"))?],
                &mut used,
                &mut regions,
            )?;
        }
        for axis in 0..2 {
            for plane in 0..2 {
                add_region(
                    f,
                    &[
                        output(f, &format!("t{plane}.{axis}.0"))?,
                        output(f, &format!("t{plane}.{axis}.1"))?,
                    ],
                    &mut used,
                    &mut regions,
                )?;
            }
        }
    } else {
        // All pre-ROM pure logic has a single register boundary. Include
        // exponent/guards in this boundary, not a sequence of scalar ALU slots.
        let mut rom_dependent = vec![false; f.events.len()];
        for e in &f.events {
            rom_dependent[e.id] = matches!(e.operation,Operation::Read{memory,..} if f.memories[memory].kind==audited::MemoryKind::Rom)
                || e.inputs
                    .iter()
                    .any(|&v| rom_dependent[f.values[v].producer]);
        }
        let head: Vec<_> = f
            .events
            .iter()
            .filter(|e| pure(&e.operation) && !rom_dependent[e.id] && e.resource.is_some())
            .map(|e| e.id)
            .collect();
        add_region(f, &head, &mut used, &mut regions)?;
        add_region(f, &[output(f, "lod")?], &mut used, &mut regions)?;
        let levels: Vec<_> = f
            .outputs
            .iter()
            .filter(|o| !o.name.starts_with("prefix"))
            .map(|o| f.values[o.value].producer)
            .collect();
        add_region(f, &levels, &mut used, &mut regions)?;
    }
    // A fully absorbed equality recipe is executed inside its parent region.
    // Any partially overlapping primitive certificate would be ambiguous.
    for c in cones.iter() {
        let members: Vec<_> = c
            .absorbed_events
            .iter()
            .copied()
            .chain([c.result_event])
            .collect();
        if members.iter().any(|id| used.contains(id)) && !members.iter().all(|id| used.contains(id))
        {
            return Err("fabric region splits an equality recipe".into());
        }
    }
    cones.retain(|c| !used.contains(&c.result_event));
    cones.extend(regions);
    Ok(())
}

/// Shape key retains op/format/constant/topology; unlike width-only accounting,
/// unrelated functions cannot be charged to one supposedly shared scalar site.
pub(super) fn shape(f: &FrameReport, c: &LogicCone) -> String {
    let members: BTreeMap<_, _> = c
        .absorbed_events
        .iter()
        .copied()
        .chain([c.result_event])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
    let external: BTreeMap<_, _> = c
        .operands
        .iter()
        .enumerate()
        .map(|(i, &v)| (v, i))
        .collect();
    let mut s = String::new();
    for &id in members.keys() {
        let e = &f.events[id];
        s.push_str(&format!(
            "{:?}:{:?}:export{}:",
            e.operation,
            f.values[e.output.unwrap()].format,
            c.exported_events.contains(&id)
        ));
        for &v in &e.inputs {
            let x = &f.values[v];
            if let Some(i) = members.get(&x.producer) {
                s.push_str(&format!("i{i},"));
            } else if f.events[x.producer].operation == Operation::Literal {
                s.push_str(&format!("k{}:{:?},", x.raw, x.format));
            } else {
                s.push_str(&format!("e{}:{:?},", external[&v], x.format));
            }
        }
        s.push(';');
    }
    s
}
