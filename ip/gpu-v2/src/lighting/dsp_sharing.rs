//! Reviewed DSP sharing constructions, always derived from the frozen H9 reference.
use super::{
    calendars::UnifiedCalendar,
    datapath::Program,
    rtl::LightingRtlOptions,
    sim::{timed::LaneKind, workbench::SchedulePlan},
    LightingProfile, LightingQuantization,
};

/// One shared dual-product MAC, with both existing issue dates and separate histories.
pub fn shared_dots() -> Result<(LightingRtlOptions, [SchedulePlan; 2]), String> {
    let q = LightingQuantization::CompensatedFloor;
    let options = UnifiedCalendar::SmoothH9Reference.options(q);
    let mut plans = UnifiedCalendar::SmoothH9Reference.plans(q)?;
    let mut p = Program::with_retiming(
        LightingProfile::Fast,
        true,
        options.dedicated_dsp,
        options.kernel(),
        options.role_schedule,
        options.logic_depth,
        options.retiming,
    )?;
    p.apply_plan(&plans[0])?;
    let mut count = 0;
    for slot in &mut plans[0].slots {
        if p.binding.kinds.get(slot.id) == Some(&Some(LaneKind::PairMultiplyAdd)) {
            plans[0].capacities[p.graph.nodes[slot.id].resource.unwrap()] = 1;
            slot.lane = 0;
            count += 1;
        }
    }
    if count != 2 {
        return Err("source-matched experiment requires exactly two dot operations".into());
    }
    plans[1] = plans[0].clone();
    p.apply_plan(&plans[0])?;
    if p.latency != 34 || p.ii != 2 {
        return Err("shared dots changed the temporal contract".into());
    }
    Ok((options, plans))
}

/// Four-edge admission with one shared dot MAC and an explicit wide-multiply pool.
/// Issue dates, numerical operations and the 34-edge latency stay unchanged.
pub fn four_cycle(wide_pool: usize) -> Result<(LightingRtlOptions, [SchedulePlan; 2]), String> {
    if !(7..=11).contains(&wide_pool) {
        return Err("II4 wide pool must contain seven to eleven multipliers".into());
    }
    let (mut options, mut plans) = shared_scalar_pairs(false, false)?;
    options.id_ring = false;
    options.free_slots = true;
    for plan in &mut plans {
        let mut program = Program::with_retiming(
            LightingProfile::Fast,
            true,
            options.dedicated_dsp,
            options.kernel(),
            options.role_schedule,
            options.logic_depth,
            options.retiming,
        )?;
        program.apply_plan(plan)?;
        plan.ii = 4;
        for _ in 7..wide_pool {
            let mut groups = std::collections::BTreeMap::<(usize, usize), Vec<usize>>::new();
            for slot in &plan.slots {
                if program.binding.kinds.get(slot.id) == Some(&Some(LaneKind::LargeMultiply)) {
                    groups
                        .entry((program.graph.nodes[slot.id].resource.unwrap(), slot.lane))
                        .or_default()
                        .push(slot.id);
                }
            }
            let ((resource, _), ids) = groups
                .into_iter()
                .filter(|(_, ids)| ids.len() > 1)
                .max_by_key(|(_, ids)| {
                    let different = program.frame.events[ids[0]]
                        .inputs
                        .iter()
                        .zip(&program.frame.events[*ids.last().unwrap()].inputs)
                        .filter(|(a, b)| a != b)
                        .count();
                    (different, ids.len(), ids[0])
                })
                .ok_or("II4 pool has no remaining shared wide lane")?;
            let moved = *ids.last().unwrap();
            let lane = plan.capacities[resource];
            plan.capacities[resource] += 1;
            plan.slots.iter_mut().find(|s| s.id == moved).unwrap().lane = lane;
        }
        program.apply_plan(plan)?;
        if program.ii != 4 || program.latency != 34 {
            return Err("II4 schedule changed its checked temporal contract".into());
        }
    }
    Ok((options, plans))
}

/// Share normalization read ports across modulo-four phases without retiming.
pub fn four_cycle_shared_roms(
    wide_pool: usize,
) -> Result<(LightingRtlOptions, [SchedulePlan; 2]), String> {
    let (options, mut plans) = four_cycle(wide_pool)?;
    for plan in &mut plans {
        let mut program = Program::with_retiming(
            LightingProfile::Fast,
            true,
            options.dedicated_dsp,
            options.kernel(),
            options.role_schedule,
            options.logic_depth,
            options.retiming,
        )?;
        program.apply_plan(plan)?;
        let mut counts = vec![[0usize; 4]; program.graph.resources.len()];
        for slot in &mut plan.slots {
            if program.binding.kinds.get(slot.id) == Some(&Some(LaneKind::NormalizeRead)) {
                let resource = program.graph.nodes[slot.id].resource.unwrap();
                let phase = (slot.issue % 4) as usize;
                slot.lane = counts[resource][phase];
                counts[resource][phase] += 1;
            }
        }
        for (resource, phases) in counts.iter().enumerate() {
            let ports = *phases.iter().max().unwrap();
            if ports != 0 {
                plan.capacities[resource] = ports;
            }
        }
        program.apply_plan(plan)?;
    }
    Ok((options, plans))
}

/// Checked opposite-parity pairs with a global physical pool, preserving SQ,
/// RSQRT, per-pixel H/V and the special combinational H9 tail.
pub fn shared_scalar_pairs(
    small: bool,
    six: bool,
) -> Result<(LightingRtlOptions, [SchedulePlan; 2]), String> {
    let q = LightingQuantization::CompensatedFloor;
    let original_options = UnifiedCalendar::SmoothH9Reference.options(q);
    let original = UnifiedCalendar::SmoothH9Reference.plans(q)?;
    let calendar = super::rtl::physical_calendar_with_schedule_plans(
        LightingProfile::Fast,
        original_options,
        &original,
    )?;
    let mut plans =
        super::rtl::canonical_schedule_plan(LightingProfile::Fast, original_options, &original)?;
    let mut options = original_options;
    // Explicit assignments are already post-Local. Only legal commutations may
    // orient operands; a later lane exchange must not undo a reviewed pair.
    options.dsp_steering = super::rtl::DspSteering::Orient;
    let mut p = Program::with_retiming(
        LightingProfile::Fast,
        true,
        options.dedicated_dsp,
        options.kernel(),
        options.role_schedule,
        options.logic_depth,
        options.retiming,
    )?;
    p.apply_plan(&plans[0])?;
    let groups = if six {
        vec![
            vec![29],
            vec![30],
            vec![31, 34],
            vec![32, 33],
            vec![35, 37],
            vec![36],
        ]
    } else {
        vec![
            vec![29],
            vec![30],
            vec![31, 32],
            vec![33],
            vec![34],
            vec![35, 37],
            vec![36],
        ]
    };
    let mut assignments = std::collections::BTreeMap::new();
    for (lane, group) in groups.iter().enumerate() {
        for &physical in group {
            for instruction in calendar
                .iter()
                .filter(|i| i.kind == "LargeMultiply" && i.lane == Some(physical))
            {
                assignments.insert(instruction.event, lane);
            }
        }
    }
    if assignments.len() != 11 {
        return Err("source-matched ordinary18 inventory changed".into());
    }
    let mut small_groups = std::collections::BTreeMap::new();
    if small {
        for (lane, physical_group) in [
            vec![21],
            vec![22],
            vec![23],
            vec![24, 28],
            vec![25],
            vec![26],
            vec![27],
        ]
        .iter()
        .enumerate()
        {
            for &physical in physical_group {
                for instruction in calendar
                    .iter()
                    .filter(|i| i.kind == "SmallMultiply" && i.lane == Some(physical))
                {
                    small_groups.insert(instruction.event, lane);
                }
            }
        }
        if small_groups.len() != 14 {
            return Err("source-matched ordinary9 inventory changed".into());
        }
    }
    for slot in &mut plans[0].slots {
        let Some(kind) = p.binding.kinds.get(slot.id) else {
            continue;
        };
        let Some(resource) = p.graph.nodes[slot.id].resource else {
            return Err("missing candidate resource".into());
        };
        if kind == &Some(LaneKind::PairMultiplyAdd) {
            slot.lane = 0;
            plans[0].capacities[resource] = 1;
        } else if let Some(&lane) = assignments.get(&slot.id) {
            slot.lane = lane;
            plans[0].capacities[resource] = groups.len();
        } else if let Some(&lane) = small_groups.get(&slot.id) {
            slot.lane = lane;
            plans[0].capacities[resource] = 7;
        }
        if six {
            // Normal X factor is available at 10; rounding at13 produces n.0
            // at14, the unchanged NL MAC issue. apply_plan audits every edge.
            if slot.id == 236 {
                if slot.issue != 11 {
                    return Err("normal-X issue changed".into());
                }
                slot.issue = 12;
            }
            if slot.id == 249 {
                if slot.issue != 12 {
                    return Err("normal-X round changed".into());
                }
                slot.issue = 13;
            }
        }
    }
    plans[1] = plans[0].clone();
    p.apply_plan(&plans[0])?;
    if p.latency != 34 || p.ii != 2 {
        return Err("candidate changed temporal output contract".into());
    }
    Ok((options, plans))
}

/// Resource and predecessor receipt from the actual arithmetic program.
pub fn graph_receipt(
    options: LightingRtlOptions,
    plans: &[SchedulePlan; 2],
) -> Result<String, String> {
    let mut p = Program::with_retiming(
        LightingProfile::Fast,
        true,
        options.dedicated_dsp,
        options.kernel(),
        options.role_schedule,
        options.logic_depth,
        options.retiming,
    )?;
    p.apply_plan(&plans[0])?;
    use std::fmt::Write;
    let mut out = String::from("node,resource,lanes,issue,ready,parents\n");
    for (id, node) in p.graph.nodes.iter().enumerate() {
        if let Some(r) = node.resource {
            writeln!(
                out,
                "{},{},{},{},{},{}",
                id,
                p.graph.resources[r].name.replace(',', ";"),
                p.graph.resources[r].lanes,
                p.schedule.nodes[id].issue,
                p.schedule.nodes[id].issue + p.graph.resources[r].latency,
                node.predecessors
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(";")
            )
            .unwrap();
        }
    }
    Ok(out)
}
