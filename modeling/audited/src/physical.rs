//! Explicit placement and timing certificates, separate from numerical work.
//! The two-macro DSP policy describes a target topology, not fitted area/fmax.
use crate::{Fault, FrameReport, MemoryKind, Operation};
use std::collections::{BTreeMap, BTreeSet};

/// Structural logic bindings preserve the independently replayed numerical graph.
pub mod lowering;

#[cfg(test)]
mod logic_split_tests;

fn bad(message: &str) -> Fault {
    Fault::Audit(message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub issue: u64,
    pub ready: u64,
}

/// Closed certificate for A0*B0 + A1*B1 + C. Internal results cannot escape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FusedGroup {
    pub result_event: usize,
    pub absorbed_events: Vec<usize>,
    pub operands: Vec<usize>,
}
impl FusedGroup {
    pub fn audit(&self, frame: &FrameReport) -> Result<(), Fault> {
        frame.audit()?;
        let root = frame
            .events
            .get(self.result_event)
            .ok_or_else(|| bad("fusion event index"))?;
        if !matches!(root.operation, Operation::Add) || root.inputs.len() != 2 {
            return Err(bad("fusion root"));
        }
        let xy_id = frame.values[root.inputs[0]].producer;
        let xy = &frame.events[xy_id];
        if !matches!(xy.operation, Operation::Add) || xy.inputs.len() != 2 {
            return Err(bad("fusion pair sum"));
        }
        let product_ids = [
            frame.values[xy.inputs[0]].producer,
            frame.values[xy.inputs[1]].producer,
        ];
        if self.absorbed_events != [product_ids[0], product_ids[1], xy_id] {
            return Err(bad("fusion absorbed certificate"));
        }
        let mut operands = Vec::new();
        let fraction = frame.values[root.output.ok_or_else(|| bad("fusion output"))?]
            .format
            .fraction;
        for id in product_ids {
            let e = &frame.events[id];
            if !matches!(e.operation, Operation::Multiply) || e.inputs.len() != 2 {
                return Err(bad("fusion product"));
            }
            if e.inputs
                .iter()
                .any(|&v| frame.values[v].format.bits > 18 || !frame.values[v].format.signed)
                || frame.values[e.output.ok_or_else(|| bad("fusion product output"))?]
                    .format
                    .fraction
                    != fraction
            {
                return Err(bad("fusion product format"));
            }
            operands.extend(&e.inputs);
        }
        operands.push(root.inputs[1]);
        if self.operands != operands {
            return Err(bad("fusion operand certificate"));
        }
        let out = frame.values[root.output.unwrap()].format;
        let c = frame.values[operands[4]].format;
        if !out.signed || out.bits > 54 || !c.signed || c.bits > 54 || c.fraction != fraction {
            return Err(bad("fusion ALU format"));
        }
        for &id in &self.absorbed_events {
            let value = frame.events[id]
                .output
                .ok_or_else(|| bad("fusion internal output"))?;
            let internal = frame.values[value].format;
            if !internal.signed || internal.bits > 54 || internal.fraction != fraction {
                return Err(bad("fusion internal format"));
            }
            if frame.outputs.iter().any(|o| o.value == value)
                || frame.events.iter().any(|e| {
                    e.inputs.contains(&value)
                        && e.id != root.id
                        && !self.absorbed_events.contains(&e.id)
                        && !matches!(e.operation, Operation::ProductMapping { .. })
                })
            {
                return Err(bad("fusion internal value escapes"));
            }
        }
        Ok(())
    }
}

/// A bounded pure-logic subgraph implemented with one declared result latency.
/// An empty absorbed set declares a charged singleton, preserving shared outputs.
/// The certificate proves provenance and closure, not area or achievable fmax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicCone {
    pub result_event: usize,
    pub absorbed_events: Vec<usize>,
    /// Explicit additional result events produced at the same ready edge.
    /// Undeclared intermediates still cannot escape the physical boundary.
    pub exported_events: Vec<usize>,
    /// Sorted unique external value IDs, including constants.
    pub operands: Vec<usize>,
    pub max_width: u32,
    pub latency: u64,
}
impl LogicCone {
    /// Keep one charged logic operation as its own boundary. This does not fuse
    /// work, remove a resource, or shorten the path to any downstream consumer.
    pub fn singleton(
        frame: &FrameReport,
        result_event: usize,
        latency: u64,
    ) -> Result<Self, Fault> {
        frame.audit()?;
        let e = frame
            .events
            .get(result_event)
            .ok_or_else(|| bad("logic cone event index"))?;
        let output = e.output.ok_or_else(|| bad("logic cone output"))?;
        let operands: BTreeSet<_> = e.inputs.iter().copied().collect();
        let proof = Self {
            result_event,
            absorbed_events: Vec::new(),
            exported_events: Vec::new(),
            operands: operands.into_iter().collect(),
            max_width: e
                .inputs
                .iter()
                .copied()
                .chain([output])
                .map(|v| frame.values[v].format.bits)
                .max()
                .unwrap_or(1),
            latency,
        };
        proof.audit(frame)?;
        Ok(proof)
    }

    pub fn audit(&self, frame: &FrameReport) -> Result<(), Fault> {
        frame.audit()?;
        if !frame.valid {
            return Err(bad("logic cone requires a valid numerical frame"));
        }
        if self.absorbed_events.len() > 64
            || !(1..=126).contains(&self.max_width)
            || self.latency == 0
        {
            return Err(bad("logic cone bounds"));
        }
        let members: BTreeSet<_> = std::iter::once(self.result_event)
            .chain(self.absorbed_events.iter().copied())
            .collect();
        if members.len() != self.absorbed_events.len() + 1 {
            return Err(bad("logic cone duplicate member"));
        }
        let exports: BTreeSet<_> = self.exported_events.iter().copied().collect();
        if exports.len() != self.exported_events.len()
            || exports.iter().any(|id| !self.absorbed_events.contains(id))
        {
            return Err(bad("logic cone exported result certificate"));
        }
        let mut external = BTreeSet::new();
        for &id in &members {
            let e = frame
                .events
                .get(id)
                .ok_or_else(|| bad("logic cone event index"))?;
            if !matches!(
                e.operation,
                Operation::Add
                    | Operation::Sub
                    | Operation::Resize
                    | Operation::ShiftLeft(_)
                    | Operation::Shift
                    | Operation::LeadingZeros
                    | Operation::Slice(_)
                    | Operation::RescaleFloor(_)
                    | Operation::BinaryScale
                    | Operation::Less
                    | Operation::Select
                    | Operation::RoundIncrement(_)
            ) {
                return Err(bad("logic cone is not pure logic"));
            }
            let output = e.output.ok_or_else(|| bad("logic cone output"))?;
            if self.absorbed_events.is_empty() && e.resource.is_none() {
                return Err(bad("singleton cone requires charged logic"));
            }
            if std::iter::once(output)
                .chain(e.inputs.iter().copied())
                .any(|v| frame.values[v].format.bits > self.max_width)
            {
                return Err(bad("logic cone width"));
            }
            for &input in &e.inputs {
                if !members.contains(&frame.values[input].producer) {
                    external.insert(input);
                }
            }
            if e.control.is_some_and(|c| members.contains(&c)) {
                return Err(bad("logic cone internal control"));
            }
            if id != self.result_event
                && !exports.contains(&id)
                && (frame.outputs.iter().any(|o| o.value == output)
                    || frame.events.iter().any(|other| {
                        !members.contains(&other.id)
                            && (other.inputs.contains(&output) || other.control == Some(id))
                    }))
            {
                return Err(bad("logic cone internal value escapes"));
            }
        }
        if self.operands != external.into_iter().collect::<Vec<_>>() {
            return Err(bad("logic cone operand certificate"));
        }
        let mut reachable = BTreeSet::new();
        let mut pending: Vec<_> = std::iter::once(self.result_event)
            .chain(exports.iter().copied())
            .collect();
        while let Some(id) = pending.pop() {
            if reachable.insert(id) {
                pending.extend(
                    frame.events[id]
                        .inputs
                        .iter()
                        .map(|&v| frame.values[v].producer)
                        .filter(|p| members.contains(p)),
                );
            }
        }
        if reachable != members {
            return Err(bad("logic cone disconnected member"));
        }
        Ok(())
    }
}

pub fn bound_dependencies(
    frame: &FrameReport,
    groups: &[FusedGroup],
) -> Result<Vec<Vec<usize>>, Fault> {
    composed_dependencies(frame, groups, &[])
}

pub fn logic_dependencies(
    frame: &FrameReport,
    cones: &[LogicCone],
) -> Result<Vec<Vec<usize>>, Fault> {
    composed_dependencies(frame, &[], cones)
}

pub fn composed_dependencies(
    frame: &FrameReport,
    groups: &[FusedGroup],
    cones: &[LogicCone],
) -> Result<Vec<Vec<usize>>, Fault> {
    frame.audit()?;
    for g in groups {
        g.audit(frame)?;
    }
    for c in cones {
        c.audit(frame)?;
    }
    let mut deps: Vec<Vec<_>> = frame
        .events
        .iter()
        .map(|e| {
            let mut d: Vec<_> = e
                .inputs
                .iter()
                .map(|&v| frame.values[v].producer)
                .chain(e.control)
                .collect();
            d.sort_unstable();
            d.dedup();
            d
        })
        .collect();
    let mut used = BTreeSet::new();
    for (result, absorbed, operands) in groups
        .iter()
        .map(|g| (g.result_event, &g.absorbed_events, &g.operands))
        .chain(
            cones
                .iter()
                .map(|c| (c.result_event, &c.absorbed_events, &c.operands)),
        )
    {
        for &id in std::iter::once(&result).chain(absorbed) {
            if !used.insert(id) {
                return Err(bad("overlapping physical groups"));
            }
        }
        let mut d: Vec<_> = operands.iter().map(|&v| frame.values[v].producer).collect();
        for &id in std::iter::once(&result).chain(absorbed) {
            d.extend(frame.events[id].control);
        }
        d.sort_unstable();
        d.dedup();
        if d.iter().any(|id| *id == result || absorbed.contains(id)) {
            return Err(bad("fusion dependency cycle"));
        }
        deps[result] = d;
        for &id in absorbed {
            deps[id] = vec![result];
        }
    }
    Ok(deps)
}

/// Check operand/control gates without trusting an optimizer's internal state.
pub fn audit_dependencies(
    frame: &FrameReport,
    times: &[Timing],
    max_cycle: u64,
) -> Result<(), Fault> {
    audit_bound_dependencies(frame, times, &[], max_cycle)
}

pub fn audit_bound_dependencies(
    frame: &FrameReport,
    times: &[Timing],
    groups: &[FusedGroup],
    max_cycle: u64,
) -> Result<(), Fault> {
    audit_composed_dependencies(frame, times, groups, &[], max_cycle)
}

pub fn audit_logic_dependencies(
    frame: &FrameReport,
    times: &[Timing],
    cones: &[LogicCone],
    max_cycle: u64,
) -> Result<(), Fault> {
    audit_composed_dependencies(frame, times, &[], cones, max_cycle)
}

pub fn audit_composed_dependencies(
    frame: &FrameReport,
    times: &[Timing],
    groups: &[FusedGroup],
    cones: &[LogicCone],
    max_cycle: u64,
) -> Result<(), Fault> {
    let dependencies = composed_dependencies(frame, groups, cones)?;
    if times.len() != frame.events.len() || max_cycle == 0 {
        return Err(bad("physical timing shape"));
    }
    for cone in cones {
        let root = times[cone.result_event];
        if root.issue.checked_add(cone.latency) != Some(root.ready) {
            return Err(bad("logic cone latency"));
        }
        if cone.absorbed_events.iter().any(|&id| {
            times[id]
                != (Timing {
                    issue: root.ready,
                    ready: root.ready,
                })
        }) {
            return Err(bad("logic cone absorbed timing"));
        }
    }
    for (event, time) in frame.events.iter().zip(times) {
        if time.issue > time.ready || time.ready > max_cycle {
            return Err(bad("physical timing range"));
        }
        for &producer in &dependencies[event.id] {
            if times[producer].ready > time.issue {
                return Err(bad("physical operand before ready"));
            }
        }
        if event.control.is_some_and(|c| times[c].ready > time.issue) {
            return Err(bad("physical control before ready"));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspMode {
    Multiply9,
    Multiply18,
    PreAdd18,
    Alu54,
    PairMultiplyAdd,
    MultiplyAccumulate18,
    MultiplyAccumulate36x18,
    Multiply36,
}
#[derive(Clone, Debug)]
pub struct DspInstance {
    pub name: String,
    pub mode: DspMode,
    pub tile: usize,
    pub macro_index: usize,
    pub slot: usize,
    pub latency: u64,
    pub initiation_interval: u64,
}
#[derive(Clone, Debug)]
pub struct DspInventory {
    pub tiles: usize,
    pub instances: Vec<DspInstance>,
}
#[derive(Clone, Copy, Debug)]
pub struct DspIssue {
    pub instance: usize,
    pub issue: u64,
    pub ready: u64,
    pub work: DspWork,
}
#[derive(Clone, Copy, Debug)]
pub enum DspWork {
    Multiply {
        a_bits: u32,
        b_bits: u32,
    },
    PreAdd {
        bits: u32,
    },
    Alu {
        bits: u32,
    },
    PairMultiplyAdd {
        a_bits: [u32; 2],
        b_bits: [u32; 2],
        accumulator_bits: u32,
    },
    MultiplyAccumulate {
        a_bits: u32,
        b_bits: u32,
        accumulator_bits: u32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DspUsage {
    pub macros: usize,
    pub tiles: usize,
    pub multiplier_half_slots: usize,
}

impl DspInventory {
    /// Conservative kind-separated placement; wide instances are placed first.
    pub fn pack(tiles: usize, declarations: &[(DspMode, usize, u64, u64)]) -> Result<Self, Fault> {
        if tiles == 0 || tiles > 4096 || declarations.len() > 8 {
            return Err(bad("DSP packing bounds"));
        }
        let mut instances = Vec::new();
        let mut seen = BTreeSet::new();
        let mut macro_cursor = 0;
        let mut sorted = declarations.to_vec();
        sorted.sort_by_key(|d| d.0 != DspMode::Multiply36);
        for (mode, count, latency, ii) in sorted {
            if !seen.insert(mode as u8) || count > 16384 || instances.len() + count > 16384 {
                return Err(bad("DSP declaration bounds"));
            }
            let slots = match mode {
                DspMode::Multiply9 => 4,
                DspMode::Multiply18 | DspMode::PreAdd18 => 2,
                _ => 1,
            };
            for lane in 0..count {
                let macro_id = macro_cursor
                    + if mode == DspMode::Multiply36 {
                        lane * 2
                    } else {
                        lane / slots
                    };
                instances.push(DspInstance {
                    name: format!("{mode:?}.{lane}"),
                    mode,
                    tile: macro_id / 2,
                    macro_index: macro_id % 2,
                    slot: lane % slots,
                    latency,
                    initiation_interval: ii,
                });
            }
            macro_cursor += if mode == DspMode::Multiply36 {
                count * 2
            } else {
                count.div_ceil(slots)
            };
        }
        let inventory = Self { tiles, instances };
        inventory.audit()?;
        Ok(inventory)
    }
    /// Two macros per tile; kinds cannot mix in a macro. Wide36 owns a tile.
    pub fn audit(&self) -> Result<DspUsage, Fault> {
        if self.tiles == 0 || self.tiles > 4096 || self.instances.len() > 16384 {
            return Err(bad("DSP inventory bounds"));
        }
        let mut macros = BTreeMap::<(usize, usize), (DspMode, BTreeSet<usize>)>::new();
        let mut wide_tiles = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut half_slots = 0;
        for d in &self.instances {
            if d.tile >= self.tiles
                || d.macro_index > 1
                || d.latency == 0
                || d.initiation_interval == 0
                || !names.insert(&d.name)
            {
                return Err(bad("DSP instance declaration"));
            }
            let capacity = match d.mode {
                DspMode::Multiply9 => 4,
                DspMode::Multiply18 | DspMode::PreAdd18 => 2,
                _ => 1,
            };
            if d.slot >= capacity {
                return Err(bad("DSP slot range"));
            }
            if d.mode == DspMode::Multiply36 && (d.macro_index != 0 || !wide_tiles.insert(d.tile)) {
                return Err(bad("wide DSP tile conflict"));
            }
            let entry = macros
                .entry((d.tile, d.macro_index))
                .or_insert((d.mode, BTreeSet::new()));
            if entry.0 != d.mode || !entry.1.insert(d.slot) {
                return Err(bad("DSP packing/mode conflict"));
            }
            half_slots += match d.mode {
                DspMode::Multiply9 => 1,
                DspMode::Multiply18 | DspMode::MultiplyAccumulate18 => 2,
                DspMode::PairMultiplyAdd | DspMode::MultiplyAccumulate36x18 => 4,
                DspMode::Multiply36 => 8,
                _ => 0,
            };
        }
        for &tile in &wide_tiles {
            if macros.keys().filter(|(t, _)| *t == tile).count() != 1 {
                return Err(bad("wide DSP excludes other macro"));
            }
        }
        Ok(DspUsage {
            macros: macros.len() + wide_tiles.len(),
            tiles: macros.keys().map(|(t, _)| t).collect::<BTreeSet<_>>().len(),
            multiplier_half_slots: half_slots,
        })
    }
    pub fn audit_issues(
        &self,
        issues: &[DspIssue],
        period: Option<u64>,
        max_cycle: u64,
    ) -> Result<(), Fault> {
        self.audit()?;
        if issues.len() > 1_000_000 || max_cycle == 0 || period == Some(0) {
            return Err(bad("DSP issue bounds"));
        }
        let mut lanes = BTreeMap::<usize, Vec<u64>>::new();
        for i in issues {
            let d = self
                .instances
                .get(i.instance)
                .ok_or_else(|| bad("DSP instance index"))?;
            let within = |n: u32, limit: u32| n > 0 && n <= limit;
            let compatible = match (d.mode, i.work) {
                (DspMode::Multiply9, DspWork::Multiply { a_bits, b_bits }) => {
                    within(a_bits, 9) && within(b_bits, 9)
                }
                (DspMode::Multiply18, DspWork::Multiply { a_bits, b_bits }) => {
                    within(a_bits, 18) && within(b_bits, 18)
                }
                (DspMode::Multiply36, DspWork::Multiply { a_bits, b_bits }) => {
                    within(a_bits, 36) && within(b_bits, 36)
                }
                (DspMode::PreAdd18, DspWork::PreAdd { bits }) => within(bits, 18),
                (DspMode::Alu54, DspWork::Alu { bits }) => within(bits, 54),
                (
                    DspMode::PairMultiplyAdd,
                    DspWork::PairMultiplyAdd {
                        a_bits,
                        b_bits,
                        accumulator_bits,
                    },
                ) => {
                    a_bits.into_iter().chain(b_bits).all(|n| within(n, 18))
                        && within(accumulator_bits, 54)
                }
                (
                    DspMode::MultiplyAccumulate18,
                    DspWork::MultiplyAccumulate {
                        a_bits,
                        b_bits,
                        accumulator_bits,
                    },
                ) => within(a_bits, 18) && within(b_bits, 18) && within(accumulator_bits, 54),
                (
                    DspMode::MultiplyAccumulate36x18,
                    DspWork::MultiplyAccumulate {
                        a_bits,
                        b_bits,
                        accumulator_bits,
                    },
                ) => within(a_bits, 36) && within(b_bits, 18) && within(accumulator_bits, 54),
                _ => false,
            };
            if !compatible {
                return Err(bad("DSP mode/operand width mismatch"));
            }
            if i.issue.checked_add(d.latency) != Some(i.ready) || i.ready > max_cycle {
                return Err(bad("DSP result latency"));
            }
            lanes.entry(i.instance).or_default().push(i.issue);
        }
        for (instance, mut times) in lanes {
            let gap = self.instances[instance].initiation_interval;
            if let Some(ii) = period {
                if gap > ii {
                    return Err(bad("DSP self collision across periods"));
                }
                for t in &mut times {
                    *t %= ii;
                }
                times.sort_unstable();
                if times.windows(2).any(|w| w[1] - w[0] < gap)
                    || ii - (times[times.len() - 1] - times[0]) < gap
                {
                    return Err(bad("DSP periodic collision"));
                }
            } else {
                times.sort_unstable();
                if times.windows(2).any(|w| w[1] - w[0] < gap) {
                    return Err(bad("DSP initiation collision"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RamKind {
    Bsram,
    Ssram,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadDuringWrite {
    Forbidden,
    ReadFirst,
    WriteFirst,
}
#[derive(Clone, Copy, Debug)]
pub struct MemoryPort {
    pub read: bool,
    pub write: bool,
    pub read_latency: u64,
    pub write_latency: u64,
    pub initiation_interval: u64,
}
#[derive(Clone, Debug)]
pub struct MemoryBank {
    pub name: String,
    pub kind: RamKind,
    pub width: u32,
    pub depth: usize,
    pub ports: Vec<MemoryPort>,
    pub collision: ReadDuringWrite,
}
/// A slice of one logical word. Copies may span several physical banks.
#[derive(Clone, Debug)]
pub struct MemorySlice {
    pub bank: usize,
    pub base_row: usize,
    pub bit_offset: u32,
    pub source_low: u32,
    pub width: u32,
}
#[derive(Clone, Debug)]
pub struct MemoryCopy {
    pub slices: Vec<MemorySlice>,
}
#[derive(Clone, Debug)]
pub struct MemoryPlacement {
    pub memory: usize,
    pub copies: Vec<MemoryCopy>,
}
#[derive(Clone, Debug)]
pub struct MemoryAccess {
    pub event: usize,
    pub copy: usize,
    pub ports: Vec<usize>,
}
#[derive(Clone, Debug, Default)]
pub struct MemoryLayout {
    pub banks: Vec<MemoryBank>,
    pub placements: Vec<MemoryPlacement>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryUsage {
    pub logical_bits: u64,
    pub replica_payload_bits: u64,
    pub bank_capacity_bits: u64,
    pub bsram_banks: usize,
    pub ssram_banks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GowinMemoryUsage {
    pub bsram_blocks: usize,
    pub ssram_cells: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct GowinMemoryBudget {
    pub bsram_blocks: usize,
    pub ssram_cells: usize,
}

impl MemoryLayout {
    /// Target policy: one configured BSRAM per bank, RAM16 composition for SSRAM.
    /// Only declared primitive geometries/ports are counted; this is not PnR.
    pub fn audit_gowin_budget(
        &self,
        frame: &FrameReport,
        budget: GowinMemoryBudget,
    ) -> Result<GowinMemoryUsage, Fault> {
        self.audit(frame)?;
        let mut usage = GowinMemoryUsage {
            bsram_blocks: 0,
            ssram_cells: 0,
        };
        for b in &self.banks {
            match b.kind {
                RamKind::Bsram => {
                    if !matches!(
                        (b.width, b.depth),
                        (1, 16384) | (2, 8192) | (4, 4096) | (9, 2048) | (18, 1024) | (36, 512)
                    ) || b.ports.len() > 2
                    {
                        return Err(bad("unsupported BSRAM primitive geometry"));
                    }
                    usage.bsram_blocks += 1;
                }
                RamKind::Ssram => {
                    if !b.depth.is_power_of_two()
                        || b.depth < 16
                        || b.ports.iter().filter(|p| p.read).count() > 1
                        || b.ports.iter().filter(|p| p.write).count() > 1
                    {
                        return Err(bad("unsupported SSRAM composition"));
                    }
                    usage.ssram_cells += b.depth.div_ceil(16) * b.width as usize;
                }
            }
        }
        if usage.bsram_blocks > budget.bsram_blocks || usage.ssram_cells > budget.ssram_cells {
            return Err(bad("physical memory budget"));
        }
        Ok(usage)
    }
    pub fn audit(&self, frame: &FrameReport) -> Result<MemoryUsage, Fault> {
        frame.audit()?;
        if self.banks.len() > 4096 || self.placements.len() > frame.memories.len() {
            return Err(bad("memory layout bounds"));
        }
        let mut names = BTreeSet::new();
        let mut usage = MemoryUsage {
            logical_bits: 0,
            replica_payload_bits: 0,
            bank_capacity_bits: 0,
            bsram_banks: 0,
            ssram_banks: 0,
        };
        for b in &self.banks {
            if b.width == 0
                || b.width > 126
                || b.depth == 0
                || b.depth > 1_000_000
                || b.ports.is_empty()
                || b.ports.len() > 64
                || !names.insert(&b.name)
            {
                return Err(bad("bank geometry"));
            }
            for p in &b.ports {
                if (!p.read && !p.write)
                    || p.initiation_interval == 0
                    || (p.write && p.write_latency == 0)
                    || (p.read && b.kind == RamKind::Bsram && p.read_latency == 0)
                {
                    return Err(bad("bank port semantics"));
                }
            }
            usage.bank_capacity_bits += b.depth as u64 * u64::from(b.width);
            match b.kind {
                RamKind::Bsram => usage.bsram_banks += 1,
                RamKind::Ssram => usage.ssram_banks += 1,
            }
        }
        let mut seen = BTreeSet::new();
        let mut regions = Vec::new();
        for p in &self.placements {
            let m = frame
                .memories
                .get(p.memory)
                .ok_or_else(|| bad("logical memory index"))?;
            if m.kind == MemoryKind::Input
                || !seen.insert(p.memory)
                || p.copies.is_empty()
                || p.copies.len() > 64
            {
                return Err(bad("memory copy declaration"));
            }
            let bits = (m.rows as u64)
                .checked_mul(u64::from(m.format.bits))
                .ok_or_else(|| bad("memory size overflow"))?;
            usage.logical_bits = usage
                .logical_bits
                .checked_add(bits)
                .ok_or_else(|| bad("memory size overflow"))?;
            usage.replica_payload_bits = usage
                .replica_payload_bits
                .checked_add(
                    bits.checked_mul(p.copies.len() as u64)
                        .ok_or_else(|| bad("replica overflow"))?,
                )
                .ok_or_else(|| bad("replica overflow"))?;
            for copy in &p.copies {
                if copy.slices.is_empty() || copy.slices.len() > 126 {
                    return Err(bad("memory slice bounds"));
                }
                let mut covered = vec![false; m.format.bits as usize];
                for s in &copy.slices {
                    let bank = self
                        .banks
                        .get(s.bank)
                        .ok_or_else(|| bad("slice bank index"))?;
                    let row_end = s
                        .base_row
                        .checked_add(m.rows)
                        .ok_or_else(|| bad("slice depth overflow"))?;
                    let bit_end = s
                        .bit_offset
                        .checked_add(s.width)
                        .ok_or_else(|| bad("slice width overflow"))?;
                    let source_end = s
                        .source_low
                        .checked_add(s.width)
                        .ok_or_else(|| bad("slice source overflow"))?;
                    if s.width == 0
                        || row_end > bank.depth
                        || bit_end > bank.width
                        || source_end > m.format.bits
                    {
                        return Err(bad("slice capacity"));
                    }
                    for bit in s.source_low..source_end {
                        if std::mem::replace(&mut covered[bit as usize], true) {
                            return Err(bad("logical slice overlap"));
                        }
                    }
                    for &(other_bank, row_lo, row_hi, bit_lo, bit_hi, mutable) in &regions {
                        if s.bank == other_bank
                            && s.base_row < row_hi
                            && row_lo < row_end
                            && ((s.bit_offset < bit_hi && bit_lo < bit_end)
                                || mutable
                                || m.kind == MemoryKind::Ram)
                        {
                            return Err(bad("physical region overlap"));
                        }
                    }
                    regions.push((
                        s.bank,
                        s.base_row,
                        row_end,
                        s.bit_offset,
                        bit_end,
                        m.kind == MemoryKind::Ram,
                    ));
                }
                if covered.contains(&false) {
                    return Err(bad("logical slice coverage"));
                }
            }
        }
        if frame
            .memories
            .iter()
            .enumerate()
            .any(|(id, m)| m.kind != MemoryKind::Input && !seen.contains(&id))
        {
            return Err(bad("unplaced logical store"));
        }
        Ok(usage)
    }

    /// Check every non-input transaction, shared RW ports and mirrored writes.
    /// Finite schedules only; bank read/write effects occur at their issue edge.
    pub fn audit_accesses(
        &self,
        frame: &FrameReport,
        times: &[Timing],
        accesses: &[MemoryAccess],
        max_cycle: u64,
    ) -> Result<(), Fault> {
        self.audit_bound_accesses(frame, times, &[], accesses, max_cycle)
    }

    pub fn audit_bound_accesses(
        &self,
        frame: &FrameReport,
        times: &[Timing],
        groups: &[FusedGroup],
        accesses: &[MemoryAccess],
        max_cycle: u64,
    ) -> Result<(), Fault> {
        self.audit_composed_accesses(frame, times, groups, &[], accesses, max_cycle)
    }

    #[allow(clippy::too_many_arguments)] // Parallel DSP/logic certificates share the existing access API.
    pub fn audit_composed_accesses(
        &self,
        frame: &FrameReport,
        times: &[Timing],
        groups: &[FusedGroup],
        cones: &[LogicCone],
        accesses: &[MemoryAccess],
        max_cycle: u64,
    ) -> Result<(), Fault> {
        audit_composed_dependencies(frame, times, groups, cones, max_cycle)?;
        self.audit(frame)?;
        if accesses.len() > 1_000_000 {
            return Err(bad("access certificate bounds"));
        }
        let placements: BTreeMap<_, _> = self.placements.iter().map(|p| (p.memory, p)).collect();
        let mut seen = BTreeSet::new();
        let mut ports = BTreeMap::<(usize, usize), Vec<u64>>::new();
        let mut physical = BTreeMap::<(usize, usize, u64), Vec<(bool, usize)>>::new();
        let mut logical = BTreeMap::<(usize, usize), Vec<(usize, bool)>>::new();
        for a in accesses {
            let event = frame
                .events
                .get(a.event)
                .ok_or_else(|| bad("access event index"))?;
            let (memory, row, write) = match event.operation {
                Operation::Read { memory, row } => (memory, row, false),
                Operation::Write { memory, row } => (memory, row, true),
                _ => return Err(bad("access is not memory work")),
            };
            let placement = placements
                .get(&memory)
                .ok_or_else(|| bad("access unplaced store"))?;
            let copy = placement
                .copies
                .get(a.copy)
                .ok_or_else(|| bad("access replica index"))?;
            if !seen.insert((a.event, a.copy)) || a.ports.len() != copy.slices.len() {
                return Err(bad("access shape/duplicate"));
            }
            let time = times[a.event];
            let mut latency = 0;
            for (s, &port) in copy.slices.iter().zip(&a.ports) {
                let bank = &self.banks[s.bank];
                let p = bank
                    .ports
                    .get(port)
                    .ok_or_else(|| bad("access port index"))?;
                if (write && !p.write) || (!write && !p.read) {
                    return Err(bad("port direction"));
                }
                latency = latency.max(if write {
                    p.write_latency
                } else {
                    p.read_latency
                });
                ports.entry((s.bank, port)).or_default().push(time.issue);
                physical
                    .entry((s.bank, s.base_row + row, time.issue))
                    .or_default()
                    .push((write, a.event));
            }
            if time.issue.checked_add(latency) != Some(time.ready) {
                return Err(bad("memory access latency"));
            }
            if a.copy == 0 || !write {
                logical
                    .entry((memory, row))
                    .or_default()
                    .push((a.event, write));
            }
        }
        for event in &frame.events {
            let (memory, write) = match event.operation {
                Operation::Read { memory, .. } => (memory, false),
                Operation::Write { memory, .. } => (memory, true),
                _ => continue,
            };
            if frame.memories[memory].kind == MemoryKind::Input {
                continue;
            }
            let p = placements[&memory];
            let count = (0..p.copies.len())
                .filter(|copy| seen.contains(&(event.id, *copy)))
                .count();
            if (write && count != p.copies.len()) || (!write && count != 1) {
                return Err(bad("missing access or unbroadcast replicated write"));
            }
        }
        for ((bank, port), mut issues) in ports {
            issues.sort_unstable();
            let gap = self.banks[bank].ports[port].initiation_interval;
            if issues.windows(2).any(|w| w[1] - w[0] < gap) {
                return Err(bad("shared memory port collision"));
            }
        }
        for ((bank, _, _), events) in physical {
            let writes: Vec<_> = events.iter().filter(|e| e.0).collect();
            if writes.len() > 1 {
                return Err(bad("same-address simultaneous writes"));
            }
            if let Some(write) = writes.first() {
                for read in events.iter().filter(|e| !e.0) {
                    let required = if read.1 < write.1 {
                        ReadDuringWrite::ReadFirst
                    } else {
                        ReadDuringWrite::WriteFirst
                    };
                    if self.banks[bank].collision != required {
                        return Err(bad("read-during-write semantics"));
                    }
                }
            }
        }
        for events in logical.values_mut() {
            events.sort_unstable();
            let mut last_write = None;
            let mut last_reads = None;
            for &(event, write) in events.iter() {
                let issue = times[event].issue;
                if last_write.is_some_and(|previous| issue < previous)
                    || (write && last_reads.is_some_and(|previous| issue < previous))
                {
                    return Err(bad("logical memory hazard reordered"));
                }
                if write {
                    last_write = Some(issue);
                    last_reads = None;
                } else {
                    last_reads =
                        Some(last_reads.map_or(issue, |previous: u64| previous.max(issue)));
                }
            }
        }
        Ok(())
    }

    /// Repeating read-only body. Mutable iterations require stateful replay.
    pub fn audit_periodic_accesses(
        &self,
        frame: &FrameReport,
        times: &[Timing],
        groups: &[FusedGroup],
        accesses: &[MemoryAccess],
        ii: u64,
        max_cycle: u64,
    ) -> Result<(), Fault> {
        self.audit_periodic_composed_accesses(frame, times, groups, &[], accesses, ii, max_cycle)
    }

    #[allow(clippy::too_many_arguments)] // Explicit certificates, calendar and bound are independently audited.
    pub fn audit_periodic_composed_accesses(
        &self,
        frame: &FrameReport,
        times: &[Timing],
        groups: &[FusedGroup],
        cones: &[LogicCone],
        accesses: &[MemoryAccess],
        ii: u64,
        max_cycle: u64,
    ) -> Result<(), Fault> {
        self.audit_composed_accesses(frame, times, groups, cones, accesses, max_cycle)?;
        if ii == 0 {
            return Err(bad("memory periodic interval"));
        }
        let placements: BTreeMap<_, _> = self.placements.iter().map(|p| (p.memory, p)).collect();
        let mut calendars = BTreeMap::<(usize, usize), Vec<u64>>::new();
        for access in accesses {
            let memory = match frame.events[access.event].operation {
                Operation::Read { memory, .. } => memory,
                _ => return Err(bad("mutable periodic body needs state replay")),
            };
            for (s, &port) in placements[&memory].copies[access.copy]
                .slices
                .iter()
                .zip(&access.ports)
            {
                calendars
                    .entry((s.bank, port))
                    .or_default()
                    .push(times[access.event].issue % ii);
            }
        }
        for ((bank, port), mut phases) in calendars {
            let gap = self.banks[bank].ports[port].initiation_interval;
            phases.sort_unstable();
            if gap > ii
                || phases.windows(2).any(|w| w[1] - w[0] < gap)
                || ii - (phases[phases.len() - 1] - phases[0]) < gap
            {
                return Err(bad("periodic memory port collision"));
            }
        }
        Ok(())
    }
}
