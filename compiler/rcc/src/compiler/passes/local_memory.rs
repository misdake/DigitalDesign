//! Remove overwritten local writes and unread, unescaped local storage.
use super::{frame_addresses, has_observable_effect, mutable_values};
use crate::{Instr, IrFunc};
use std::collections::{HashMap, HashSet};

pub(super) fn eliminate(function: &mut IrFunc) -> bool {
    let addresses = frame_addresses(function, &mutable_values(function));
    let local = |base, offset: i16| {
        addresses
            .get(&base)
            .map(|slot| slot.wrapping_add(offset as u16))
            .filter(|slot| *slot < u16::from(function.local_slots))
            .map(|slot| slot as u8)
    };
    let mut reads = HashSet::new();
    let mut escaped = false;
    for block in &function.blocks {
        for phi in &block.phis {
            escaped |= phi.args.iter().any(|(_, v)| addresses.contains_key(v));
        }
        for inst in &block.insts {
            // An opaque observer may name frame memory through a raw address,
            // even when no canonical AddrOfLocal value appears in its arguments.
            if has_observable_effect(inst)
                && !matches!(
                    inst,
                    Instr::StoreLocal { .. }
                        | Instr::StoreMem { .. }
                        | Instr::StoreStatic { .. }
                        | Instr::StoreSp { .. }
                        | Instr::FpuStore { .. }
                        | Instr::FpuVecStore { .. }
                )
            {
                escaped = true;
            }
            match *inst {
                Instr::LoadLocal { slot, .. } => {
                    reads.insert(slot);
                }
                Instr::LoadMem { base, offset, .. } => {
                    if let Some(slot) = local(base, offset) {
                        reads.insert(slot);
                    } else {
                        escaped = true;
                    }
                }
                _ => {}
            }
            let uses = super::super::regalloc::inst_uses(inst);
            let safe = match *inst {
                Instr::LoadMem { base, offset, .. } => local(base, offset).is_some(),
                Instr::StoreMem { base, offset, src } => {
                    local(base, offset).is_some() && !addresses.contains_key(&src)
                }
                _ => super::super::regalloc::inst_defs(inst)
                    .iter()
                    .any(|v| addresses.contains_key(v)),
            };
            if !safe && uses.iter().any(|v| addresses.contains_key(v)) {
                escaped = true;
            }
            // Wider FPU accesses and unknown pointers are conservative aliases.
            if matches!(inst, Instr::FpuLoad { .. } | Instr::FpuVecLoad { .. }) {
                escaped = true;
            }
        }
        if block.term.as_ref().is_some_and(|term| match term {
            crate::Terminator::Ret { values } => values.iter().any(|v| addresses.contains_key(v)),
            _ => false,
        }) {
            escaped = true;
        }
    }
    let mut changed = false;
    for block in &mut function.blocks {
        let mut overwritten: HashMap<u8, usize> = HashMap::new();
        let mut remove = HashSet::new();
        for (index, inst) in block.insts.iter().enumerate().rev() {
            let store = match *inst {
                Instr::StoreLocal { slot, .. } => Some(slot),
                Instr::StoreMem { base, offset, .. } => local(base, offset),
                _ => None,
            };
            if let Some(slot) = store {
                if overwritten.insert(slot, index).is_some() || (!escaped && !reads.contains(&slot))
                {
                    remove.insert(index);
                }
                continue;
            }
            match *inst {
                Instr::LoadLocal { slot, .. } => {
                    overwritten.remove(&slot);
                }
                Instr::LoadMem { base, offset, .. } => {
                    if let Some(slot) = local(base, offset) {
                        overwritten.remove(&slot);
                    } else {
                        overwritten.clear();
                    }
                }
                _ if has_observable_effect(inst)
                    || matches!(inst, Instr::FpuLoad { .. } | Instr::FpuVecLoad { .. }) =>
                {
                    overwritten.clear()
                }
                _ => {}
            }
        }
        if !remove.is_empty() {
            changed = true;
            let insts = std::mem::take(&mut block.insts);
            let lines = std::mem::take(&mut block.lines);
            for (index, (inst, line)) in insts.into_iter().zip(lines).enumerate() {
                if !remove.contains(&index) {
                    block.insts.push(inst);
                    block.lines.push(line);
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FuncBuilder;

    #[test]
    fn removes_overwritten_and_unread_private_words() {
        let (mut b, args) = FuncBuilder::new("locals", 1, 1);
        let input = b.get(args[0]);
        let slot = b.alloc_local_slots(2);
        let one = b.load_imm(1);
        b.store_local(slot, one);
        b.store_local(slot, input);
        b.store_local(slot + 1, one);
        let output = b.load_local(slot);
        b.ret(&[output]);
        let mut function = b.finish();
        assert!(eliminate(&mut function));
        let stores = function.blocks[0]
            .insts
            .iter()
            .filter(|i| matches!(i, Instr::StoreLocal { .. }))
            .count();
        assert_eq!(stores, 1);
    }

    #[test]
    fn escaped_addresses_and_calls_preserve_observable_writes() {
        let (mut b, args) = FuncBuilder::new("escape", 1, 0);
        let input = b.get(args[0]);
        let slot = b.alloc_local_slots(1);
        let address = b.addr_of_local(slot);
        b.store_local(slot, input);
        b.call("observer", &[address], 0);
        b.store_local(slot, input);
        b.ret(&[]);
        let mut function = b.finish();
        assert!(!eliminate(&mut function));
    }

    #[test]
    fn opaque_observers_without_explicit_addresses_still_see_frame_memory() {
        let (mut b, args) = FuncBuilder::new("opaque", 1, 0);
        let value = b.get(args[0]);
        let slot = b.alloc_local_slots(1);
        b.store_local(slot, value);
        b.call("raw_frame_observer", &[], 0);
        b.ret(&[]);
        assert!(!eliminate(&mut b.finish()));
    }
}
