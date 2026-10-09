//! Bounded two-stage cuts. Delay scores are heuristics; full PnR decides timing.
use super::{ranges::RawRange, LoweredFrame};
use crate::lighting::datapath::Instruction;
use audited::Operation;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Cut {
    pub at: usize,
    pub early: Vec<usize>,
    pub late: Vec<usize>,
    pub crossing: Vec<usize>,
    pub bits: usize,
    pub effective_bits: usize,
    pub align_bits: usize,
    pub levels: [usize; 2],
    pub delay: [usize; 2],
}
fn logic(op: &Operation) -> bool {
    !matches!(
        op,
        Operation::Literal
            | Operation::Resize
            | Operation::BinaryScale
            | Operation::Slice(_)
            | Operation::ShiftLeft(_)
    )
}
fn effective(r: RawRange, bits: u32) -> usize {
    if r.lo == r.hi {
        return 0;
    }
    let mask = (1_i128 << bits) - 1;
    (128 - ((r.lo ^ r.hi) & mask).leading_zeros()) as usize
}
fn weight(frame: &LoweredFrame, id: usize, range: &impl Fn(usize) -> RawRange) -> usize {
    let e = &frame.events[id];
    let bits = frame.values[e.output.unwrap()].format.bits as usize;
    let input_bits = e
        .inputs
        .iter()
        .map(|&v| frame.values[v].format.bits as usize)
        .max()
        .unwrap_or(1);
    match e.operation {
        Operation::Literal
        | Operation::Resize
        | Operation::BinaryScale
        | Operation::Slice(_)
        | Operation::ShiftLeft(_)
        | Operation::RescaleFloor(_) => 0,
        Operation::Add | Operation::Sub => bits.div_ceil(18),
        Operation::Less => input_bits.div_ceil(18),
        Operation::Select => 1,
        Operation::RoundIncrement(n) => {
            if n <= 1 {
                1
            } else {
                1 + (n as usize).ilog2() as usize
            }
        }
        Operation::LeadingZeros => input_bits.next_power_of_two().ilog2() as usize,
        Operation::Shift => {
            let r = range(e.inputs[1]);
            let choices = (r.hi - r.lo + 1).clamp(1, (input_bits * 2 + 1) as i128) as usize;
            choices.next_power_of_two().ilog2() as usize
        }
        _ => 1,
    }
}
pub(super) fn candidates(
    frame: &LoweredFrame,
    ins: &Instruction,
    ranges: &[RawRange],
    union: &BTreeMap<usize, RawRange>,
) -> Vec<Cut> {
    let range = |v| union.get(&v).copied().unwrap_or(ranges[v]);
    let members: BTreeSet<_> = ins.members.iter().copied().collect();
    let mut depths = BTreeMap::<usize, usize>::new();
    for &id in &ins.members {
        let e = &frame.events[id];
        let depth = e
            .inputs
            .iter()
            .filter_map(|&v| depths.get(&frame.values[v].producer))
            .copied()
            .max()
            .unwrap_or(0);
        depths.insert(id, depth + usize::from(logic(&e.operation)));
    }
    let maximum = depths[&ins.root];
    (1..maximum)
        .map(|at| {
            let early: Vec<_> = ins
                .members
                .iter()
                .copied()
                .filter(|i| depths[i] <= at)
                .collect();
            let late: Vec<_> = ins
                .members
                .iter()
                .copied()
                .filter(|i| depths[i] > at)
                .collect();
            let mut crossing = BTreeSet::new();
            for &id in &late {
                for &v in &frame.events[id].inputs {
                    let p = frame.values[v].producer;
                    if !matches!(frame.events[p].operation, Operation::Literal)
                        && (!members.contains(&p) || depths[&p] <= at)
                    {
                        crossing.insert(v);
                    }
                }
            }
            let bits = crossing
                .iter()
                .map(|&v| frame.values[v].format.bits as usize)
                .sum();
            let effective_bits = crossing
                .iter()
                .map(|&v| effective(range(v), frame.values[v].format.bits))
                .sum();
            let align_bits = crossing
                .iter()
                .filter(|&&v| !members.contains(&frame.values[v].producer))
                .map(|&v| effective(range(v), frame.values[v].format.bits))
                .sum();
            let mut levels = [0; 2];
            let mut delay = [0; 2];
            for (side, ids) in [&early, &late].into_iter().enumerate() {
                let mut l = BTreeMap::<usize, usize>::new();
                let mut d = BTreeMap::<usize, usize>::new();
                for &id in ids {
                    let e = &frame.events[id];
                    let pred = |m: &BTreeMap<usize, usize>| {
                        e.inputs
                            .iter()
                            .filter_map(|&v| m.get(&frame.values[v].producer))
                            .copied()
                            .max()
                            .unwrap_or(0)
                    };
                    let level = pred(&l) + usize::from(logic(&e.operation));
                    let cost = pred(&d) + weight(frame, id, &range);
                    l.insert(id, level);
                    d.insert(id, cost);
                    levels[side] = levels[side].max(level);
                    delay[side] = delay[side].max(cost);
                }
            }
            Cut {
                at,
                early,
                late,
                crossing: crossing.into_iter().collect(),
                bits,
                effective_bits,
                align_bits,
                levels,
                delay,
            }
        })
        .collect()
}
pub(super) fn choose(cuts: &[Cut], baseline: usize) -> usize {
    let old = cuts.iter().find(|c| c.at == baseline).unwrap();
    let limit = *old.delay.iter().max().unwrap();
    cuts.iter()
        .filter(|c| c.levels.iter().all(|&n| n <= 4) && c.delay.iter().all(|&n| n <= limit))
        .min_by_key(|c| {
            (
                c.effective_bits + c.align_bits,
                c.effective_bits,
                c.bits,
                *c.delay.iter().max().unwrap(),
                c.at.abs_diff(baseline),
            )
        })
        .map_or(baseline, |c| c.at)
}
