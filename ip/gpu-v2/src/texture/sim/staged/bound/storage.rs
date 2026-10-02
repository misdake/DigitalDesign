//! Deterministic periodic FF slice reuse. This is a concrete bit-addressed
//! allocation, not a live-bit estimate. FF bits have one write per edge and
//! continuous read fanout; no RAM port or spare cache port is assumed.
use super::FfBank;
use audited::{
    physical::{lowering, LogicCone, Timing},
    FrameReport, MemoryKind, Operation,
};
use resource_scheduler::Graph;
use std::collections::{BTreeMap, BTreeSet};
type BitSource = Option<(usize, u32)>;
/// Resolve pure wiring at bit granularity. A discarded product low byte or a
/// sliced-away UV high part does not inherit the surviving field's lifetime.
pub fn fields(
    f: &FrameReport,
    graph: &Graph,
    times: &[Timing],
    cones: &[LogicCone],
    lowering: &lowering::Plan,
    retained: &[String],
    span: u64,
) -> Result<Vec<FfBank>, String> {
    let absorbed: BTreeSet<_> = cones
        .iter()
        .flat_map(|p| &p.absorbed_events)
        .copied()
        .collect();
    let wiring: BTreeMap<_, _> = lowering
        .wiring_adds
        .iter()
        .map(|p| (p.result_event, p))
        .collect();
    let mut views = vec![Vec::<BitSource>::new(); f.values.len()];
    let mut inputs = BTreeMap::<(usize, usize), usize>::new();
    let mut births = BTreeMap::new();
    for e in &f.events {
        let Some(v) = e.output else { continue };
        let width = f.values[v].format.bits;
        if absorbed.contains(&e.id) || e.operation == Operation::Literal {
            views[v] = vec![None; width as usize];
            continue;
        }
        let alias = if let Some(p) = wiring.get(&e.id) {
            Some(
                (0..width)
                    .map(|bit| {
                        let i = if p.possible_ones[0] & (1_u128 << bit) != 0 {
                            Some(e.inputs[0])
                        } else if p.possible_ones[1] & (1_u128 << bit) != 0 {
                            Some(e.inputs[1])
                        } else {
                            None
                        };
                        i.and_then(|i| extended(&views[i], f.values[i].format.signed, bit))
                    })
                    .collect(),
            )
        } else if e.inputs.len() == 1
            && matches!(
                e.operation,
                Operation::Resize
                    | Operation::BinaryScale
                    | Operation::Slice(_)
                    | Operation::ShiftLeft(_)
                    | Operation::RescaleFloor(_)
            )
        {
            let i = e.inputs[0];
            Some(
                (0..width)
                    .map(|bit| {
                        let source_bit = match e.operation {
                            Operation::Slice(low) | Operation::RescaleFloor(low) => Some(bit + low),
                            Operation::ShiftLeft(n) => bit.checked_sub(n),
                            _ => Some(bit),
                        };
                        source_bit.and_then(|b| extended(&views[i], f.values[i].format.signed, b))
                    })
                    .collect(),
            )
        } else {
            None
        };
        if let Some(view) = alias {
            views[v] = view;
            continue;
        }
        let origin = match e.operation {
            Operation::Read { memory, row } if f.memories[memory].kind == MemoryKind::Input => {
                *inputs.entry((memory, row)).or_insert(v)
            }
            _ => v,
        };
        births.entry(origin).or_insert(
            if matches!(e.operation,
            Operation::Read { memory, .. } if f.memories[memory].kind == MemoryKind::Input)
            {
                0
            } else {
                times[e.id].ready
            },
        );
        views[v] = (0..width).map(|bit| Some((origin, bit))).collect();
    }
    // Check the declared wiring projection against each numerical frame. The
    // projection itself depends only on operation/format certificates, never
    // on these sampled values; constants retain their literal source wiring.
    for (v, view) in views.iter().enumerate() {
        for (bit, source) in view.iter().enumerate() {
            if let Some((origin, source_bit)) = source {
                if (f.values[v].raw as u128 >> bit) & 1
                    != (f.values[*origin].raw as u128 >> source_bit) & 1
                {
                    return Err("wiring bit projection disagrees with closed frame".into());
                }
            }
        }
    }
    let mut last = BTreeMap::<(usize, u32), BTreeSet<u64>>::new();
    for e in &f.events {
        if absorbed.contains(&e.id) {
            continue;
        }
        let keep = matches!(&e.operation, Operation::Publish(name) if retained.contains(name));
        if graph.nodes[e.id].resource.is_none()
            && !keep
            && !matches!(e.operation, Operation::Require(_))
        {
            continue;
        }
        let operands = cones
            .iter()
            .find(|p| p.result_event == e.id)
            .map_or(e.inputs.as_slice(), |p| p.operands.as_slice());
        let at = if keep { span } else { times[e.id].issue };
        let mut controls = Vec::new();
        for event in std::iter::once(e.id).chain(
            cones
                .iter()
                .find(|p| p.result_event == e.id)
                .into_iter()
                .flat_map(|p| p.absorbed_events.iter().copied()),
        ) {
            if let Some(c) = f.events[event].control {
                controls.extend(&f.events[c].inputs);
            }
        }
        for &v in operands.iter().chain(&controls) {
            for source in views[v].iter().flatten() {
                if births[&source.0] > at {
                    return Err("field read before producer".into());
                }
                last.entry(*source).or_default().insert(at);
            }
        }
    }
    let mut banks = Vec::<FfBank>::new();
    for ((value, bit), reads) in last {
        let read_times: Vec<_> = reads.into_iter().collect();
        let last_read = *read_times.last().unwrap();
        let birth = births[&value];
        if let Some(b) = banks.last_mut().filter(|b| {
            b.value == value
                && b.source_low + b.width == bit
                && b.birth == birth
                && b.last_read == last_read
                && b.read_times == read_times
        }) {
            b.width += 1;
        } else {
            banks.push(FfBank {
                value,
                source_low: bit,
                width: 1,
                slots: 0,
                birth,
                last_read,
                read_times,
            });
        }
    }
    Ok(banks)
}
fn extended(view: &[BitSource], signed: bool, bit: u32) -> BitSource {
    view.get(bit as usize).copied().unwrap_or_else(|| {
        if signed {
            *view.last().unwrap()
        } else {
            None
        }
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub value: usize,
    pub source_low: u32,
    pub iteration: u64,
    pub low: usize,
    pub width: u32,
    pub birth_phase: u64,
    pub live_phases: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub ii: u64,
    pub period: u64,
    pub ff_bits: usize,
    pub peak_live_bits: usize,
    pub placements: Vec<Placement>,
    pub write_selector_tree_bits: u64,
    pub read_selector_tree_bits: u64,
    pub valid_bits: u64,
    /// One-hot phase ring and an issue-valid shift register through last read.
    pub control_ff_bits: u64,
    /// Phase AND valid per placement, plus conservative per-bit write-enable OR.
    pub control_boolean_gates: u64,
}
fn phases(b: &FfBank, iteration: u64, ii: u64, period: u64) -> u64 {
    (b.birth..=b.last_read).fold(0, |m, t| m | (1 << ((t + iteration * ii) % period)))
}
impl Layout {
    pub fn build(banks: &[FfBank], ii: u64) -> Result<Self, String> {
        if banks.is_empty() || ii == 0 {
            return Err("empty periodic storage".into());
        }
        let max_len = banks
            .iter()
            .map(|b| b.last_read + 1 - b.birth)
            .max()
            .unwrap();
        let period = ii * max_len.div_ceil(ii).next_power_of_two();
        if period > 64 {
            return Err("storage period exceeds bounded bitmap".into());
        }
        let mut placements = vec![];
        for b in banks {
            for iteration in 0..period / ii {
                placements.push(Placement {
                    value: b.value,
                    source_low: b.source_low,
                    iteration,
                    low: 0,
                    width: b.width,
                    birth_phase: (b.birth + iteration * ii) % period,
                    live_phases: phases(b, iteration, ii, period),
                });
            }
        }
        placements.sort_by_key(|p| {
            (
                std::cmp::Reverse(p.width),
                std::cmp::Reverse(p.live_phases.count_ones()),
                p.value,
                p.iteration,
            )
        });
        let mut occupancy = Vec::<u64>::new();
        for p in &mut placements {
            let width = p.width as usize;
            let low = (0..=occupancy.len())
                .find(|&lo| {
                    (lo..lo + width).all(|bit| {
                        occupancy
                            .get(bit)
                            .is_none_or(|mask| mask & p.live_phases == 0)
                    })
                })
                .unwrap();
            occupancy.resize(occupancy.len().max(low + width), 0);
            for mask in &mut occupancy[low..low + width] {
                *mask |= p.live_phases;
            }
            p.low = low;
        }
        placements.sort_by_key(|p| (p.value, p.source_low, p.iteration));
        let peak_live_bits = (0..period)
            .map(|phase| {
                occupancy
                    .iter()
                    .filter(|mask| *mask & (1 << phase) != 0)
                    .count()
            })
            .max()
            .unwrap();
        let (write_selector_tree_bits, read_selector_tree_bits, control_boolean_gates) =
            selectors(&placements, banks);
        let result = Self {
            ii,
            period,
            ff_bits: occupancy.len(),
            peak_live_bits,
            placements,
            write_selector_tree_bits,
            read_selector_tree_bits,
            valid_bits: banks.iter().map(|b| b.last_read + 1).max().unwrap(),
            control_ff_bits: period + banks.iter().map(|b| b.last_read + 1).max().unwrap(),
            control_boolean_gates,
        };
        result.audit(banks, ii)?;
        Ok(result)
    }
    pub fn audit(&self, banks: &[FfBank], ii: u64) -> Result<(), String> {
        if self.ii != ii
            || ii == 0
            || self.period == 0
            || self.period > 64
            || !self.period.is_multiple_of(ii)
            || self.ff_bits > 65536
        {
            return Err("periodic layout geometry".into());
        }
        let by_value: BTreeMap<_, _> = banks.iter().map(|b| ((b.value, b.source_low), b)).collect();
        if self.placements.windows(2).any(|pair| {
            (pair[0].value, pair[0].source_low, pair[0].iteration)
                >= (pair[1].value, pair[1].source_low, pair[1].iteration)
        }) {
            return Err("periodic placement lookup order".into());
        }
        let mut seen = BTreeSet::new();
        let mut occupancy = vec![0_u64; self.ff_bits];
        for p in &self.placements {
            let b = by_value
                .get(&(p.value, p.source_low))
                .ok_or("layout origin missing")?;
            if p.iteration >= self.period / ii
                || !seen.insert((p.value, p.source_low, p.iteration))
                || p.width != b.width
                || p.low + p.width as usize > self.ff_bits
                || b.last_read + 1 - b.birth > self.period
                || p.birth_phase != (b.birth + p.iteration * ii) % self.period
                || p.live_phases != phases(b, p.iteration, ii, self.period)
            {
                return Err("periodic placement provenance/lifetime".into());
            }
            for mask in &mut occupancy[p.low..p.low + p.width as usize] {
                if *mask & p.live_phases != 0 {
                    return Err("physical FF lifetime/write conflict".into());
                }
                *mask |= p.live_phases;
            }
        }
        if seen.len() != banks.len() * (self.period / ii) as usize {
            return Err("periodic placement incomplete".into());
        }
        let peak = (0..self.period)
            .map(|phase| {
                occupancy
                    .iter()
                    .filter(|mask| *mask & (1 << phase) != 0)
                    .count()
            })
            .max()
            .unwrap();
        let (write, read, control) = selectors(&self.placements, banks);
        if peak != self.peak_live_bits
            || write != self.write_selector_tree_bits
            || read != self.read_selector_tree_bits
            || self.valid_bits != banks.iter().map(|b| b.last_read + 1).max().unwrap()
            || self.control_ff_bits != self.period + self.valid_bits
            || self.control_boolean_gates != control
        {
            return Err("periodic storage bill mutation".into());
        }
        Ok(())
    }
    pub fn placement(
        &self,
        value: usize,
        source_low: u32,
        iteration: u64,
    ) -> Result<&Placement, String> {
        let key = (value, source_low, iteration % (self.period / self.ii));
        let i = self
            .placements
            .binary_search_by_key(&key, |p| (p.value, p.source_low, p.iteration))
            .map_err(|_| "unmapped physical value")?;
        Ok(&self.placements[i])
    }
}
fn selectors(ps: &[Placement], banks: &[FfBank]) -> (u64, u64, u64) {
    // One data selector per physical FF bit, including iteration choices.
    // Direct source references are kept distinct: no sampled-constant folding.
    let mut writes = BTreeMap::<usize, BTreeSet<(usize, u32)>>::new();
    let mut reads = BTreeMap::<(usize, u32), Vec<&Placement>>::new();
    let mut enables = BTreeMap::<usize, usize>::new();
    for p in ps {
        reads.entry((p.value, p.source_low)).or_default().push(p);
        for bit in 0..p.width {
            *enables.entry(p.low + bit as usize).or_default() += 1;
            writes
                .entry(p.low + bit as usize)
                .or_default()
                .insert((p.value, p.source_low + bit));
        }
    }
    let write = writes
        .values()
        .map(|s| s.len().saturating_sub(1) as u64)
        .sum();
    let read = reads
        .values()
        .map(|ps| {
            let field = banks
                .iter()
                .find(|b| b.value == ps[0].value && b.source_low == ps[0].source_low)
                .unwrap();
            field.read_times.len() as u64
                * u64::from(ps[0].width)
                * ps.iter()
                    .map(|p| p.low)
                    .collect::<BTreeSet<_>>()
                    .len()
                    .saturating_sub(1) as u64
        })
        .sum();
    let control = ps.len() as u64
        + enables
            .values()
            .map(|n| n.saturating_sub(1) as u64)
            .sum::<u64>();
    (write, read, control)
}

#[derive(Clone, Debug, Default)]
pub struct Traffic {
    pub bit_writes: u64,
    pub bit_reads: u64,
    pub peak_live_bits: usize,
}
struct Job<'a> {
    issue: u64,
    frame: &'a FrameReport,
}
type StoredBit = (u64, usize, u32, bool, u64);
/// Replay concrete bit addresses and owner identities. Numerical values come
/// from closed-frame goldens; this is a storage replay, not arithmetic emulation.
pub struct Replay<'a> {
    plan: &'a super::StagePlan,
    bits: Vec<Option<StoredBit>>,
    jobs: Vec<Job<'a>>,
    /// Host checker clock, not another retained datapath allocation. Every
    /// enabled edge is visited from zero, including idle bubbles; CE=0 never
    /// calls tick and therefore freezes this clock with the phase calendar.
    next_tick: u64,
    pub traffic: Traffic,
}
impl<'a> Replay<'a> {
    pub fn new(plan: &'a super::StagePlan) -> Self {
        Self {
            plan,
            bits: vec![None; plan.packed.ff_bits],
            jobs: vec![],
            next_tick: 0,
            traffic: Traffic::default(),
        }
    }
    pub fn issue(&mut self, issue: u64, frame: &'a FrameReport) -> Result<(), String> {
        if issue != self.next_tick
            || !issue.is_multiple_of(self.plan.ii())
            || self.jobs.iter().any(|j| j.issue == issue)
        {
            return Err("physical storage issue phase/collision".into());
        }
        self.jobs.push(Job { issue, frame });
        Ok(())
    }
    pub fn tick(&mut self, t: u64) -> Result<(), String> {
        if t != self.next_tick {
            return Err("physical storage enabled edge order".into());
        }
        for j in &self.jobs {
            let age = t.checked_sub(j.issue).ok_or("storage time reversal")?;
            for b in &self.plan.packed_fields {
                let p =
                    self.plan
                        .packed
                        .placement(b.value, b.source_low, j.issue / self.plan.ii())?;
                if age == b.birth {
                    for bit in 0..b.width {
                        let address = p.low + bit as usize;
                        if self.bits[address].is_some_and(|old| old.4 >= t) {
                            return Err("physical FF overwritten before last read".into());
                        }
                        let source = b.source_low + bit;
                        let value = (j.frame.values[b.value].raw as u128 >> source) & 1 != 0;
                        self.bits[address] =
                            Some((j.issue, b.value, source, value, j.issue + b.last_read));
                        self.traffic.bit_writes += 1;
                    }
                }
            }
        }
        for j in &self.jobs {
            let age = t - j.issue;
            for b in &self.plan.packed_fields {
                if !b.read_times.contains(&age) {
                    continue;
                }
                let p =
                    self.plan
                        .packed
                        .placement(b.value, b.source_low, j.issue / self.plan.ii())?;
                for bit in 0..b.width {
                    let source = b.source_low + bit;
                    let value = (j.frame.values[b.value].raw as u128 >> source) & 1 != 0;
                    if self.bits[p.low + bit as usize]
                        != Some((j.issue, b.value, source, value, j.issue + b.last_read))
                    {
                        return Err("physical FF read owner/value mismatch".into());
                    }
                    self.traffic.bit_reads += 1;
                }
            }
        }
        let live = self.bits.iter().flatten().filter(|b| b.4 >= t).count();
        self.traffic.peak_live_bits = self.traffic.peak_live_bits.max(live);
        if live > self.plan.packed.peak_live_bits {
            return Err("physical live peak exceeds certificate".into());
        }
        self.jobs
            .retain(|j| t < j.issue + self.plan.packed.valid_bits - 1);
        self.next_tick += 1;
        Ok(())
    }
    pub fn idle(&self) -> bool {
        self.jobs.is_empty()
    }
}
pub fn audit_trace(r: &super::system::Report) -> Result<Vec<Traffic>, String> {
    let b = &r.binding;
    let plans = [
        &b.derivative,
        &b.lod,
        &b.coordinate,
        &b.coefficient,
        &b.plane,
        &b.packet,
    ];
    let mut replays: Vec<_> = plans.into_iter().map(Replay::new).collect();
    let mut t = 0;
    for s in &r.preparation {
        if !s.ce {
            continue;
        }
        for e in &s.events {
            if let super::control::Event::Issue {
                stage,
                program,
                lane,
                plane,
                packet,
            } = e
            {
                let prep = &r.programs[*program].preparation;
                let (index, frame) = match *stage {
                    "derivative" => (0, &prep.derivative.frame),
                    "lod" => (1, &prep.lod.frame),
                    "coordinate" => (2, &prep.lanes[*lane].coordinate.frame),
                    "coefficient" => (3, &prep.lanes[*lane].coefficient.frame),
                    "membership" => (4, &prep.lanes[*lane].memberships[*plane].frame),
                    "packet" => (5, &prep.lanes[*lane].packets[*plane][*packet].frame),
                    _ => return Err("unknown physical stage".into()),
                };
                replays[index].issue(t, frame)?;
            }
        }
        for replay in &mut replays {
            replay.tick(t)?;
        }
        t += 1;
    }
    if replays.iter().any(|r| !r.idle()) {
        return Err("physical stage storage did not drain".into());
    }
    Ok(replays.into_iter().map(|r| r.traffic).collect())
}
