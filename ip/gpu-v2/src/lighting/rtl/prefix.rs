//! Conservative sharing of the identical, temporally aligned diffuse prefix.
//! No recipe is split and no operation is moved across an issue/ready boundary.
use super::{steering, LoweredProgram, Operation};
use std::collections::BTreeMap;

pub(super) fn share(p: &mut LoweredProgram) -> Result<(), String> {
    let base = p.full.iter().position(|full| !full).unwrap();
    let keys = steering::sources(p);
    // The two context-mode reads denote different constants despite identical
    // read descriptors. Exclude their entire data-dependent closure explicitly.
    let mut mode_dependent = vec![false; p.frame.values.len()];
    for event in &p.frame.events {
        if let Some(value) = event.output {
            mode_dependent[value] = event.inputs.iter().any(|&v| mode_dependent[v])
                || matches!(event.operation, Operation::Read { memory, .. }
                    if p.frame.memories[memory].name == "context.mode");
        }
    }
    let full: BTreeMap<_, _> = p
        .instructions
        .iter()
        .filter(|i| p.full[i.root])
        .map(|i| (i.root, i))
        .collect();
    for diff in p.instructions.iter().filter(|i| !p.full[i.root]) {
        let Some(original) = full.get(&(diff.root - base)) else {
            continue;
        };
        if diff.issue != original.issue
            || diff.ready != original.ready
            || p.binding.kinds[diff.root] != p.binding.kinds[original.root]
            || p.schedule.nodes[diff.root].lane != p.schedule.nodes[original.root].lane
            || diff.members.len() != original.members.len()
            || diff.inputs.len() != original.inputs.len()
            || diff.inputs.iter().zip(&original.inputs).any(|(&a, &b)| {
                keys[a] != keys[b]
                    || (p.aliases.get(&a) != Some(&b)
                        && p.frame.events[p.frame.values[a].producer].operation
                            != Operation::Literal)
            })
        {
            continue;
        }
        let equivalent = diff.members.iter().zip(&original.members).all(|(&a, &b)| {
            // Delayed raw-input capture chains still have separate mode CEs.
            if diff.issue != 0 && matches!(p.frame.events[a].operation,
                Operation::Read { memory, .. } if p.frame.memories[memory].name.starts_with("pixel.")) {
                return false;
            }
            let a = p.frame.events[a].output.unwrap();
            let b = p.frame.events[b].output.unwrap();
            !mode_dependent[a]
                && !mode_dependent[b]
                && keys[a] == keys[b]
                && p.stable[a] == p.stable[b]
        });
        if !equivalent {
            continue;
        }
        for (&a, &b) in diff.members.iter().zip(&original.members) {
            p.aliases.insert(
                p.frame.events[a].output.unwrap(),
                p.frame.events[b].output.unwrap(),
            );
            p.shared_events.insert(b);
        }
        p.duplicate_roots.insert(diff.root);
    }
    if p.duplicate_roots
        .iter()
        .all(|&root| p.binding.kinds[root].is_none())
    {
        return Err("shared prefix found no aligned arithmetic operations".into());
    }
    Ok(())
}
