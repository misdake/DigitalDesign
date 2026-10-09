//! Local physical steering, with no arithmetic or temporal rescheduling.
use super::{LaneKind, LoweredProgram, Operation};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DspSteering {
    #[default]
    None,
    /// Commute a certified scalar multiply to align operand representations.
    Orient,
    /// Also exchange same-phase lane assignments within each scalar DSP kind.
    Local,
    /// Joint lane/operand exchanges can cross a coordinate-descent local minimum.
    Joint,
}

fn scalar(kind: &Option<LaneKind>) -> bool {
    matches!(
        kind,
        Some(LaneKind::SmallMultiply | LaneKind::LargeMultiply)
    )
}

// Intern semantic expressions; runtime template sample values are never keys.
// This estimates opportunities for shared sources, not fitted LUTs or routing.
pub(super) fn sources(p: &LoweredProgram) -> Vec<usize> {
    fn visit(
        p: &LoweredProgram,
        v: usize,
        memo: &mut [Option<usize>],
        intern: &mut BTreeMap<String, usize>,
    ) -> usize {
        if let Some(key) = memo[v] {
            return key;
        }
        let value = &p.frame.values[v];
        let event = &p.frame.events[value.producer];
        let inputs: Vec<_> = event
            .inputs
            .iter()
            .map(|&v| visit(p, v, memo, intern))
            .collect();
        let op = match &event.operation {
            Operation::Literal => format!("literal:{}", value.raw),
            Operation::Read { memory, row } => format!(
                "read:{}:{}",
                p.frame.memories[*memory].name,
                if inputs.is_empty() { *row } else { 0 }
            ),
            op => format!("{op:?}"),
        };
        let text = format!("{op}:{:?}:{inputs:?}", value.format);
        let next = intern.len() + 2; // 0 and 1 are constant bit symbols below.
        let key = *intern.entry(text).or_insert(next);
        memo[v] = Some(key);
        key
    }
    let mut memo = vec![None; p.frame.values.len()];
    let mut intern = BTreeMap::new();
    (0..memo.len())
        .map(|v| visit(p, v, &mut memo, &mut intern))
        .collect()
}

fn score(p: &LoweredProgram, keys: &[usize]) -> usize {
    let mut ports = BTreeMap::<(LaneKind, usize, usize, u32), BTreeSet<(usize, usize, u32)>>::new();
    for ins in &p.instructions {
        if !scalar(&p.binding.kinds[ins.root]) {
            continue;
        }
        let kind = p.binding.kinds[ins.root].as_ref().unwrap();
        let width = if *kind == LaneKind::SmallMultiply {
            10
        } else {
            19
        };
        let lane = p.schedule.nodes[ins.root].lane.unwrap();
        for (port, &v) in ins.inputs.iter().enumerate() {
            let value = &p.frame.values[v];
            let f = value.format;
            let ready = p
                .instructions
                .iter()
                .find(|i| i.members.contains(&value.producer))
                .map_or(0, |i| i.ready);
            let delay = if p.stable[v] {
                0
            } else {
                ins.issue.saturating_sub(ready).div_ceil(p.ii[ins.root])
            };
            for bit in 0..width {
                let symbol = if p.frame.events[value.producer].operation == Operation::Literal {
                    (usize::from((value.raw >> bit) & 1 != 0), 0, 0)
                } else if bit >= f.bits && !f.signed {
                    (0, 0, 0)
                } else {
                    (keys[v], delay, bit.min(f.bits - 1))
                };
                ports
                    .entry((kind.clone(), lane, port, bit))
                    .or_default()
                    .insert(symbol);
            }
        }
    }
    ports.values().map(|s| s.len().saturating_sub(1)).sum()
}

pub(super) fn arrange(p: &mut LoweredProgram, mode: DspSteering) -> Result<(), String> {
    if mode == DspSteering::None {
        return Ok(());
    }
    let original: Vec<_> = p
        .instructions
        .iter()
        .map(|i| {
            let mut operands = i.inputs.clone();
            operands.sort_unstable();
            (i.root, i.issue, i.ready, operands)
        })
        .collect();
    let keys = sources(p);
    let ids: Vec<_> = p
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, i)| scalar(&p.binding.kinds[i.root]) && i.inputs.len() == 2)
        .map(|(id, _)| id)
        .collect();
    let mut current = score(p, &keys);
    // Two deterministic coordinate-descent rounds, <=256 trials in each round.
    // A lane exchange preserves both operation phases and the full lane inventory.
    for _ in 0..2 {
        let mut trials = 0;
        for &id in &ids {
            if trials == 256 {
                break;
            }
            trials += 1;
            p.instructions[id].inputs.swap(0, 1);
            let cost = score(p, &keys);
            if cost < current {
                current = cost;
            } else {
                p.instructions[id].inputs.swap(0, 1);
            }
        }
        if matches!(mode, DspSteering::Orient) {
            continue;
        }
        for (n, &a) in ids.iter().enumerate() {
            for &b in &ids[n + 1..] {
                if trials == 256 {
                    break;
                }
                let ia = &p.instructions[a];
                let ib = &p.instructions[b];
                if p.full[ia.root] != p.full[ib.root]
                    || p.binding.kinds[ia.root] != p.binding.kinds[ib.root]
                    || ia.issue % p.ii[ia.root] != ib.issue % p.ii[ib.root]
                {
                    continue;
                }
                let ra = ia.root;
                let rb = ib.root;
                let la = p.schedule.nodes[ra].lane;
                let lb = p.schedule.nodes[rb].lane;
                if la == lb {
                    continue;
                }
                let mut best = None;
                let mut best_cost = current;
                let variants = if mode == DspSteering::Joint { 4 } else { 1 };
                for mask in 0..variants {
                    if trials == 256 {
                        break;
                    }
                    trials += 1;
                    p.schedule.nodes[ra].lane = lb;
                    p.schedule.nodes[rb].lane = la;
                    if mask & 1 != 0 {
                        p.instructions[a].inputs.swap(0, 1);
                    }
                    if mask & 2 != 0 {
                        p.instructions[b].inputs.swap(0, 1);
                    }
                    let cost = score(p, &keys);
                    if cost < best_cost {
                        best = Some(mask);
                        best_cost = cost;
                    }
                    if mask & 1 != 0 {
                        p.instructions[a].inputs.swap(0, 1);
                    }
                    if mask & 2 != 0 {
                        p.instructions[b].inputs.swap(0, 1);
                    }
                    p.schedule.nodes[ra].lane = la;
                    p.schedule.nodes[rb].lane = lb;
                }
                if let Some(mask) = best {
                    p.schedule.nodes[ra].lane = lb;
                    p.schedule.nodes[rb].lane = la;
                    if mask & 1 != 0 {
                        p.instructions[a].inputs.swap(0, 1);
                    }
                    if mask & 2 != 0 {
                        p.instructions[b].inputs.swap(0, 1);
                    }
                    current = best_cost;
                }
            }
        }
    }
    let mut occupied = BTreeSet::new();
    for (i, before) in p.instructions.iter().zip(original) {
        let mut operands = i.inputs.clone();
        operands.sort_unstable();
        if before != (i.root, i.issue, i.ready, operands) {
            return Err("DSP steering changed operands or temporal schedule".into());
        }
        if let Some(kind) = &p.binding.kinds[i.root] {
            if !occupied.insert((
                p.full[i.root],
                kind.clone(),
                p.schedule.nodes[i.root].lane,
                i.issue % p.ii[i.root],
            )) {
                return Err("DSP steering phase collision".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lighting::{rtl::LightingRtlOptions, LightingProfile};

    #[test]
    fn steering_is_checked_nonincreasing_and_preserves_every_temporal_edge() {
        let mut changed = false;
        {
            let profile = LightingProfile::Fast;
            let options = LightingRtlOptions::retimed_resource_profile(profile);
            let mut p = LoweredProgram::new(profile, options).unwrap();
            let keys = sources(&p);
            let before = score(&p, &keys);
            let edges: Vec<_> = p
                .instructions
                .iter()
                .map(|i| (i.root, i.issue, i.ready))
                .collect();
            arrange(&mut p, DspSteering::Local).unwrap();
            let after = score(&p, &keys);
            println!("{profile:?}: bit-source proxy {before} -> {after}");
            assert!(after <= before);
            changed |= after < before;
            assert_eq!(
                edges,
                p.instructions
                    .iter()
                    .map(|i| (i.root, i.issue, i.ready))
                    .collect::<Vec<_>>()
            );
        }
        assert!(
            changed,
            "candidate must actually improve its structural proxy"
        );
    }
}
