//! Synthesizable static modulo pipeline. The generated source contains physical
//! lane input muxes, CE pipeline registers, ROM replicas and retained delays.
//! It is exported from the audited graph, not from numerical sample outputs.
use super::{
    datapath::{Instruction, Program},
    format::*,
    sim::timed::LaneKind,
    LightingProfile,
};
use audited::{Event, Format, MemoryKind, Observation, Operation};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};
mod cuts;
mod ranges;
use ranges::{infer_ranges, RawRange};

fn token_read_offset(depth: usize, read_edge_captures: bool) -> usize {
    debug_assert!(depth >= 4);
    depth - usize::from(read_edge_captures)
}

#[derive(Clone, Copy, Debug)]
pub struct LightingRtlOptions {
    pub hierarchy: bool,
    /// Experimental pure-logic boundary, zero retains the established depth.
    pub logic_depth: usize,
    /// Keep inexpensive logic at fixed pipeline sites; DSP/ROM/shift/LZD share.
    pub stationary_logic: bool,
    /// Bounded internal cut search; external issue/ready timing is unchanged.
    pub cost_cut: bool,
    /// Exact four-way q alignment; only its two-bit code crosses the cut.
    pub q_windows: bool,
    /// Fast experiment: native FFs for only the three diffuse raw-normal chains.
    pub shallow_normal_ff: bool,
    /// Free calendar slots may compute irrelevant values instead of forcing zero.
    pub free_slots: bool,
    pub split_cones: bool,
    pub dedicated_dsp: bool,
    pub share_scalars: bool,
    pub range_shifts: bool,
    pub scalar_norm: bool,
    pub scalar_normal: bool,
    pub exact_normal_gate: bool,
    pub shared_ids: bool,
    pub block_prescale: bool,
    pub role_schedule: bool,
    pub direct_square: bool,
    pub direct_all_squares: bool,
    pub square9: bool,
    pub dedicated_dots: bool,
    pub ram_retained: bool,
    pub id_ring: bool,
}
impl Default for LightingRtlOptions {
    fn default() -> Self {
        Self {
            hierarchy: false,
            logic_depth: 0,
            stationary_logic: false,
            cost_cut: false,
            q_windows: false,
            shallow_normal_ff: false,
            free_slots: true,
            split_cones: true,
            dedicated_dsp: false,
            share_scalars: true,
            range_shifts: true,
            scalar_norm: false,
            scalar_normal: false,
            exact_normal_gate: false,
            shared_ids: false,
            block_prescale: false,
            role_schedule: false,
            direct_square: false,
            direct_all_squares: false,
            square9: false,
            dedicated_dots: false,
            ram_retained: false,
            id_ring: false,
        }
    }
}
impl LightingRtlOptions {
    fn kernel(self) -> super::sim::counted::Config {
        super::sim::counted::Config {
            scalar_norm: self.scalar_norm,
            scalar_normal: self.scalar_normal,
            exact_normal_gate: self.exact_normal_gate,
            block_prescale: self.block_prescale,
            direct_square: self.direct_square,
            direct_all_squares: self.direct_all_squares,
            square9: self.square9,
            dedicated_dots: self.dedicated_dots,
            ..super::sim::counted::Config::architecture()
        }
    }
    /// Complete checked resource alternative with the original stream ports.
    pub fn resource_profile(profile: LightingProfile) -> Self {
        let k = super::sim::counted::Config::resource_profile(profile);
        Self {
            scalar_norm: k.scalar_norm,
            block_prescale: k.block_prescale,
            dedicated_dots: k.dedicated_dots,
            role_schedule: true,
            shared_ids: true,
            ram_retained: profile == LightingProfile::Compact,
            ..Self::default()
        }
    }
    /// Factor kernel with larger pure-logic boundaries and the chosen profile's IIs.
    /// Uses the previously reviewed scaled NL gate; half-vector limits remain.
    pub fn factor_profile() -> Self {
        Self {
            logic_depth: 8,
            exact_normal_gate: false,
            ..Self::system_profile()
        }
    }
    /// Integration candidate. Select explicitly pending numerical/visual review.
    pub fn system_profile() -> Self {
        Self {
            scalar_norm: true,
            scalar_normal: true,
            exact_normal_gate: true,
            direct_all_squares: true,
            square9: false,
            block_prescale: true,
            role_schedule: true,
            shared_ids: true,
            id_ring: true,
            ..Self::default()
        }
    }
}
pub struct LaneReport {
    pub lane: usize,
    pub kind: String,
    pub width: u32,
    pub latency: usize,
    pub full_operations: usize,
    pub diffuse_operations: usize,
}
pub struct LightingVerilog {
    pub lanes: Vec<LaneReport>,
    pub source: String,
    pub latency: usize,
    pub diffuse_latency: usize,
    pub specular_ii: usize,
    pub diffuse_ii: usize,
    /// Behavioral declaration bits, including retained delays, lanes and control.
    /// Not fitted FF: Gowin can absorb these into DSP/BSRAM/SSRAM.
    pub register_bits: usize,
    pub small_multipliers: usize,
    pub large_multipliers: usize,
    pub pair_macros: usize,
    pub normalization_roms: usize,
    /// Complete external-ID ring capacity, zero for historical delay chains.
    pub id_slots: usize,
    /// Retained/input lifetimes and complete control/ROM boundary inventory.
    pub storage_csv: String,
    pub cuts_csv: String,
    /// HDL signal/age for observing the same numerical stage boundaries.
    pub stages: Vec<StageProbe>,
}
pub struct StageProbe {
    pub full: bool,
    pub name: String,
    pub signal: String,
    pub age: usize,
    pub bits: u32,
}

// Emission view of two independently audited programs. This is deliberately
// not a forged FrameReport: relocation has no numerical sample/audit claim.
struct LoweredValue {
    format: Format,
    raw: i128,
    producer: usize,
}
struct LoweredMemory {
    name: String,
    kind: MemoryKind,
}
struct LoweredFrame {
    values: Vec<LoweredValue>,
    events: Vec<Event>,
    memories: Vec<LoweredMemory>,
    outputs: Vec<Observation>,
}
struct LoweredBinding {
    kinds: Vec<Option<LaneKind>>,
}
struct LoweredSchedule {
    nodes: Vec<resource_scheduler::ModuloNode>,
}
struct LoweredProgram {
    diffuse_calendar_offset: usize,
    frame: LoweredFrame,
    binding: LoweredBinding,
    schedule: LoweredSchedule,
    instructions: Vec<Instruction>,
    stable: Vec<bool>,
    ii: Vec<usize>,
    full: Vec<bool>,
    latencies: [usize; 2],
    output_values: [[usize; 2]; 2],
}
impl LoweredProgram {
    fn new(profile: LightingProfile, options: LightingRtlOptions) -> Result<Self, String> {
        let mut s = Self {
            diffuse_calendar_offset: if profile.system() { 4 } else { 3 },
            frame: LoweredFrame {
                values: Vec::new(),
                events: Vec::new(),
                memories: Vec::new(),
                outputs: Vec::new(),
            },
            binding: LoweredBinding { kinds: Vec::new() },
            schedule: LoweredSchedule { nodes: Vec::new() },
            instructions: Vec::new(),
            stable: Vec::new(),
            ii: Vec::new(),
            full: Vec::new(),
            latencies: [0; 2],
            output_values: [[0; 2]; 2],
        };
        for (mode, full) in [true, false].into_iter().enumerate() {
            let kernel = options.kernel();
            let p = Program::with_kernel_depth(
                profile,
                full,
                options.dedicated_dsp,
                kernel,
                options.role_schedule,
                options.logic_depth,
            )?;
            let event_base = s.frame.events.len();
            let value_base = s.frame.values.len();
            let memory_base = s.frame.memories.len();
            s.latencies[mode] = p.latency;
            s.output_values[mode] = p.output_values.map(|v| v + value_base);
            for v in p.frame.values {
                s.frame.values.push(LoweredValue {
                    format: v.format,
                    raw: v.raw,
                    producer: v.producer + event_base,
                });
            }
            for mut e in p.frame.events {
                e.id += event_base;
                for v in &mut e.inputs {
                    *v += value_base;
                }
                e.output = e.output.map(|v| v + value_base);
                e.control = e.control.map(|c| c + event_base);
                match &mut e.operation {
                    Operation::Read { memory, .. } | Operation::Write { memory, .. } => {
                        *memory += memory_base
                    }
                    _ => (),
                }
                s.frame.events.push(e);
            }
            for mut o in p.frame.outputs {
                o.value += value_base;
                s.frame.outputs.push(o);
            }
            s.frame
                .memories
                .extend(p.frame.memories.into_iter().map(|m| LoweredMemory {
                    name: m.name,
                    kind: m.kind,
                }));
            s.stable.extend(p.stable);
            s.ii.extend(std::iter::repeat_n(p.ii, p.binding.kinds.len()));
            s.full
                .extend(std::iter::repeat_n(full, p.binding.kinds.len()));
            s.binding.kinds.extend(p.binding.kinds);
            s.schedule.nodes.extend(
                p.schedule
                    .nodes
                    .into_iter()
                    .take(s.frame.events.len() - event_base),
            );
            for mut i in p.instructions {
                i.root += event_base;
                for m in &mut i.members {
                    *m += event_base;
                }
                for v in &mut i.inputs {
                    *v += value_base;
                }
                s.instructions.push(i);
            }
        }
        if options.stationary_logic {
            s.stationary_logic()?;
        }
        Ok(s)
    }
    fn stationary_logic(&mut self) -> Result<(), String> {
        let mut ordinals = BTreeMap::new();
        let mut occupied = BTreeSet::new();
        for i in &self.instructions {
            let Some(kind) = &self.binding.kinds[i.root] else {
                continue;
            };
            let expensive = matches!(
                kind,
                LaneKind::SmallMultiply
                    | LaneKind::LargeMultiply
                    | LaneKind::PairMultiplyAdd
                    | LaneKind::NormalizeRead
                    | LaneKind::PowerRead
                    | LaneKind::Shift(_)
                    | LaneKind::LeadingZeros(_)
            ) || i.members.iter().any(|&id| {
                matches!(
                    self.frame.events[id].operation,
                    Operation::Shift | Operation::LeadingZeros
                )
            });
            if !expensive {
                let ordinal = ordinals
                    .entry((self.full[i.root], kind.clone()))
                    .or_insert(0);
                self.schedule.nodes[i.root].lane = Some(*ordinal);
                *ordinal += 1;
            }
            // Independent physical issue audit for both drained, exclusive modes.
            let key = (
                self.full[i.root],
                kind.clone(),
                self.schedule.nodes[i.root].lane.unwrap(),
                i.issue % self.ii[i.root],
            );
            if !occupied.insert(key) {
                return Err("stationary lane phase collision".into());
            }
        }
        Ok(())
    }
    fn input_memory(&self, id: usize) -> bool {
        self.frame.memories[id].kind == MemoryKind::Input
    }
    fn phase(&self, root: usize, age: usize) -> String {
        let slot = (age + 1) % self.ii[root]
            + if self.full[root] {
                0
            } else {
                self.diffuse_calendar_offset
            };
        format!("(calendar[{slot}])")
    }
}

fn decl(f: Format) -> String {
    format!(
        "{}[{}:0]",
        if f.signed { "signed " } else { "" },
        f.bits - 1
    )
}
fn literal(raw: i128, bits: u32) -> String {
    format!("{bits}'h{:x}", raw & ((1_i128 << bits) - 1))
}
fn numeric(name: &str, f: Format) -> String {
    if f.signed {
        format!("$signed({name})")
    } else {
        format!("$signed({{1'b0, {name}}})")
    }
}

fn scalar_shareable(frame: &LoweredFrame, instructions: &[Instruction]) -> bool {
    let first = &instructions[0];
    if first.members.len() != 1 {
        return false;
    }
    let event = &frame.events[first.root];
    instructions.iter().all(|ins| {
        let other = &frame.events[ins.root];
        ins.members.len() == 1
            && other.operation == event.operation
            && frame.values[other.output.unwrap()].format
                == frame.values[event.output.unwrap()].format
            && ins.inputs.len() == first.inputs.len()
            && ins.inputs.iter().zip(&first.inputs).all(|(&a, &b)| {
                let av = &frame.values[a];
                let bv = &frame.values[b];
                let al = matches!(frame.events[av.producer].operation, Operation::Literal);
                let bl = matches!(frame.events[bv.producer].operation, Operation::Literal);
                av.format == bv.format && al == bl && (!al || av.raw == bv.raw)
            })
            && other
                .inputs
                .iter()
                .map(|v| ins.inputs.iter().position(|i| i == v))
                .eq(event
                    .inputs
                    .iter()
                    .map(|v| first.inputs.iter().position(|i| i == v)))
    })
}

struct Emitter {
    program: LoweredProgram,
    body: String,
    sequential: String,
    retained: BTreeMap<usize, usize>,
    retained_uses: BTreeMap<usize, BTreeSet<usize>>,
    input_delays: [[usize; 3]; 2],
    input_lsb_delays: [Option<usize>; 2],
    register_bits: usize,
    hierarchical: bool,
    free_slots: bool,
    split_cones: bool,
    share_scalars: bool,
    range_shifts: bool,
    cost_cut: bool,
    cuts_csv: String,
    q_amounts: BTreeSet<usize>,
    normal_ff: BTreeSet<usize>,
    ranges: Vec<RawRange>,
    active_ranges: BTreeMap<usize, RawRange>,
    modules: String,
    lanes: Vec<LaneReport>,
}
// A representation change is allowed only when all consumers are the proved
// unsigned q shift, and that result is used exclusively by the 14-bit slices.
fn q_window_amounts(program: &LoweredProgram, ranges: &[RawRange]) -> BTreeSet<usize> {
    let f = &program.frame;
    let mut amounts = BTreeSet::new();
    for e in &f.events {
        if e.operation != Operation::Shift {
            continue;
        }
        let q = f.values[e.inputs[0]].format;
        let amount = e.inputs[1];
        let r = ranges[amount];
        let v = e.output.unwrap();
        if q.bits != 30
            || q.signed
            || r.lo != -15
            || r.hi != -12
            || f.values[v].format != q
            || f.outputs.iter().any(|o| o.value == v || o.value == amount)
        {
            continue;
        }
        let consumers: Vec<_> = f.events.iter().filter(|c| c.inputs.contains(&v)).collect();
        if consumers.is_empty()
            || consumers.iter().any(|c| {
                !matches!(c.operation,Operation::Slice(n)
            if n + f.values[c.output.unwrap()].format.bits <= 14)
            })
        {
            continue;
        }
        if f.events
            .iter()
            .filter(|c| c.inputs.contains(&amount))
            .any(|c| c.id != e.id)
        {
            continue;
        }
        amounts.insert(amount);
    }
    // An operand position has one representation in a shared physical lane.
    // Decline the rewrite if any corresponding full/diffuse use is ineligible.
    let mut lanes = BTreeMap::<_, Vec<&Instruction>>::new();
    for ins in &program.instructions {
        let Some(kind) = program.binding.kinds[ins.root].as_ref() else {
            continue;
        };
        if !matches!(kind, LaneKind::LogicCone { .. }) {
            continue;
        }
        lanes
            .entry((kind, program.schedule.nodes[ins.root].lane.unwrap()))
            .or_default()
            .push(ins);
    }
    loop {
        let previous = amounts.len();
        for uses in lanes.values() {
            let values = |ins: &Instruction| {
                ins.inputs
                    .iter()
                    .copied()
                    .chain(ins.members.iter().map(|&id| f.events[id].output.unwrap()))
                    .collect::<Vec<_>>()
            };
            let positions: Vec<_> = uses.iter().map(|ins| values(ins)).collect();
            if positions.iter().any(|v| v.len() != positions[0].len()) {
                for v in positions.iter().flatten() {
                    amounts.remove(v);
                }
                continue;
            }
            for pos in 0..positions[0].len() {
                if positions.iter().any(|v| !amounts.contains(&v[pos])) {
                    for v in &positions {
                        amounts.remove(&v[pos]);
                    }
                }
            }
        }
        if previous == amounts.len() {
            break;
        }
    }
    amounts
}
const Q_WINDOWS: [(usize, usize); 4] = [(0, 12), (1, 15), (2, 14), (3, 13)];

// Select by the numerical input contract, never by relocated value numbers.
fn diffuse_raw_normals(program: &LoweredProgram) -> Result<BTreeSet<usize>, String> {
    let frame = &program.frame;
    let mut fields = BTreeMap::new();
    for e in &frame.events {
        let Operation::Slice(offset) = e.operation else {
            continue;
        };
        let v = e.output.unwrap();
        if program.full[e.id]
            || frame.values[v].format
                != (Format {
                    bits: 16,
                    fraction: 14,
                    signed: true,
                })
        {
            continue;
        }
        let source = &frame.events[frame.values[e.inputs[0]].producer];
        if let Operation::Read { memory, row } = source.operation {
            if frame.memories[memory].name == "pixel.rows"
                && [(0, 0), (0, 16), (1, 0)].contains(&(row, offset))
                && fields.insert((row, offset), v).is_some()
            {
                return Err("duplicate diffuse raw-normal field".into());
            }
        }
    }
    if fields.len() != 3 {
        return Err("diffuse raw-normal FF experiment requires exactly three input fields".into());
    }
    Ok(fields.into_values().collect())
}
impl Emitter {
    fn interface_format(&self, value: usize) -> Format {
        if self.q_amounts.contains(&value) {
            Format {
                bits: 2,
                fraction: 0,
                signed: false,
            }
        } else {
            self.program.frame.values[value].format
        }
    }
    fn retained_storage(&mut self, ram: bool) -> Result<(), String> {
        let info = |value: usize| {
            let root = self.program.frame.values[value].producer;
            let ready = self
                .program
                .instructions
                .iter()
                .find(|i| i.root == root)
                .unwrap()
                .ready;
            let depth = self.retained[&value].div_ceil(self.program.ii[root]);
            (root, ready, depth)
        };
        for &v in &self.normal_ff {
            if !self.retained.contains_key(&v) {
                return Err("missing diffuse normal retention".into());
            }
            let (root, ready, depth) = info(v);
            if self.program.full[root] || self.program.ii[root] != 1 || ready != 2 || depth != 5 {
                return Err(
                    "diffuse normal FF experiment requires the existing age2-to7 chain".into(),
                );
            }
        }
        let mut ram_candidates = Vec::new();
        if ram {
            for &value in self.retained.keys() {
                let f = self.program.frame.values[value].format;
                let (root, ready, depth) = info(value);
                let ii = self.program.ii[root];
                let long: BTreeSet<_> = self.retained_uses[&value]
                    .iter()
                    .map(|&age| (age - ready).div_ceil(ii))
                    .filter(|&d| d >= 4)
                    .collect();
                if f.bits <= 36 && f.bits >= 9 && long.len() == 1 {
                    ram_candidates.push((depth * f.bits as usize, value));
                }
            }
            ram_candidates.sort_by_key(|&(cost, value)| (std::cmp::Reverse(cost), value));
        }
        let ram_values: BTreeSet<_> = ram_candidates.iter().take(6).map(|&(_, v)| v).collect();
        let mut handled = BTreeSet::new();
        for &value in self.retained.keys() {
            if !handled.insert(value) {
                continue;
            }
            let f = self.program.frame.values[value].format;
            let (root, ready, depth) = info(value);
            if self.normal_ff.contains(&value) {
                writeln!(self.body,"// Explicit raw-normal FF chain v{value}: {depth}x{}; all original taps retained.", f.bits).unwrap();
                for d in 1..=depth {
                    // The declaration hoister is intentionally unchanged. Use
                    // distinct model state, so conditional wire/reg declarations
                    // cannot lose their preprocessor guards during hoisting.
                    writeln!(self.body,"wire {} v{value}_d{d};\nreg {} raw_normal_model_{value}_{d};\n`ifdef GPU_V2_GOWIN_DSP\nfor(genvar bit_{value}_{d}=0;bit_{value}_{d}<{};bit_{value}_{d}=bit_{value}_{d}+1) begin : raw_normal_{value}_{d}\n DFFE ff(.Q(v{value}_d{d}[bit_{value}_{d}]),.D(v{value}_d{}[bit_{value}_{d}]),.CLK(clk),.CE(diffuse_normal_ff_ce));\nend\n`else\nassign v{value}_d{d} = raw_normal_model_{value}_{d};\n`endif",decl(f),decl(f),f.bits,d-1).unwrap();
                    writeln!(
                        self.sequential,
                        "`ifndef GPU_V2_GOWIN_DSP\nif {} raw_normal_model_{value}_{d} <= v{value}_d{};\n`endif",
                        self.program.phase(root, ready),
                        d - 1
                    )
                    .unwrap();
                    self.register_bits += f.bits as usize;
                }
            } else if ram_values.contains(&value) {
                let ii = self.program.ii[root];
                let uses = &self.retained_uses[&value];
                let long = uses
                    .iter()
                    .map(|&age| (age - ready).div_ceil(ii))
                    .find(|&d| d >= 4)
                    .unwrap();
                let short = uses
                    .iter()
                    .map(|&age| (age - ready).div_ceil(ii))
                    .filter(|&d| d < 4)
                    .max()
                    .unwrap_or(0);
                for d in 1..=short {
                    writeln!(self.body, "reg {} v{value}_d{d};", decl(f)).unwrap();
                    writeln!(
                        self.sequential,
                        "if {} v{value}_d{d} <= v{value}_d{};",
                        self.program.phase(root, ready),
                        d - 1
                    )
                    .unwrap();
                    self.register_bits += f.bits as usize;
                }
                let words = (long + 1).next_power_of_two();
                let pointer_bits = words.trailing_zeros();
                writeln!(self.body,"// Token RAM v{value}: {words}x{}, remote tap {long}, one-edge pre-read.\n(* syn_ramstyle = \"block_ram\" *) reg {} token_ram{value} [0:{}];\nreg [{}:0] token_ptr{value};\nreg {} v{value}_d{long};",f.bits,decl(f),words-1,pointer_bits-1,decl(f)).unwrap();
                writeln!(self.body,"always @(posedge clk) begin\n if(reset || (ce && context_valid && context_ready)) token_ptr{value}<=0;\n else if(datapath_ce && {}) token_ptr{value}<=token_ptr{value}+1'b1;\nend\nalways @(posedge clk) if(datapath_ce) begin\n if {} token_ram{value}[token_ptr{value}]<=v{value}_d0;",self.program.phase(root,ready),self.program.phase(root,ready)).unwrap();
                let mut phases = BTreeSet::new();
                for &age in uses {
                    if (age - ready).div_ceil(ii) != long {
                        continue;
                    }
                    let phase = self.program.phase(root, age - 1);
                    if !phases.insert(phase.clone()) {
                        continue;
                    }
                    // If the pre-read edge also captures a new word, its
                    // pointer increment and FF tap shift happen on that edge.
                    // Read the previous word at D-1; otherwise read at D.
                    let offset = token_read_offset(long, phase == self.program.phase(root, ready));
                    writeln!(self.body," if {phase} v{value}_d{long}<=token_ram{value}[(token_ptr{value}-{pointer_bits}'d{offset}) & {pointer_bits}'d{}];",words-1).unwrap();
                }
                writeln!(self.body, "end").unwrap();
                self.register_bits +=
                    words * f.bits as usize + pointer_bits as usize + f.bits as usize;
            } else {
                for d in 1..=depth {
                    writeln!(self.body, "reg {} v{value}_d{d};", decl(f)).unwrap();
                    writeln!(
                        self.sequential,
                        "if {} v{value}_d{d} <= v{value}_d{};",
                        self.program.phase(root, ready),
                        d - 1
                    )
                    .unwrap();
                    self.register_bits += f.bits as usize;
                }
            }
        }
        Ok(())
    }
    fn union_lane_ranges(&mut self, instructions: &[Instruction]) {
        self.active_ranges.clear();
        let first = &instructions[0];
        for ins in instructions {
            for (&a, &b) in first.inputs.iter().zip(&ins.inputs).chain(
                first.members.iter().zip(&ins.members).map(|(&a, &b)| {
                    (
                        self.program.frame.events[a].output.as_ref().unwrap(),
                        self.program.frame.events[b].output.as_ref().unwrap(),
                    )
                }),
            ) {
                let r = self.ranges[b];
                self.active_ranges
                    .entry(a)
                    .and_modify(|x| *x = x.union(r))
                    .or_insert(r);
            }
        }
    }
    fn value(&mut self, value: usize, age: usize) -> Result<String, String> {
        let v = &self.program.frame.values[value];
        if matches!(
            self.program.frame.events[v.producer].operation,
            Operation::Literal
        ) {
            return Ok(literal(v.raw, v.format.bits));
        }
        if self.program.stable[value] {
            return Ok(format!("v{value}_d0"));
        }
        let ins = self
            .program
            .instructions
            .iter()
            .find(|ins| ins.root == v.producer)
            .ok_or("escaping contracted value")?;
        let delay = age
            .checked_sub(ins.ready)
            .ok_or("RTL operand before ready")?;
        self.retained_uses.entry(value).or_default().insert(age);
        self.retained
            .entry(value)
            .and_modify(|d| *d = (*d).max(delay))
            .or_insert(delay);
        Ok(format!(
            "v{value}_d{}",
            delay.div_ceil(self.program.ii[v.producer])
        ))
    }
    fn function(&mut self, name: &str, ins: &Instruction) -> Result<(), String> {
        self.function_outputs(
            name,
            ins,
            &[self.program.frame.events[ins.root].output.unwrap()],
        )
    }
    fn function_outputs(
        &mut self,
        name: &str,
        ins: &Instruction,
        outputs: &[usize],
    ) -> Result<(), String> {
        let frame = &self.program.frame;
        let output_format = if outputs.len() == 1 {
            self.interface_format(outputs[0])
        } else {
            Format {
                bits: outputs.iter().map(|&v| self.interface_format(v).bits).sum(),
                fraction: 0,
                signed: false,
            }
        };
        writeln!(self.body, "function {} {name};", decl(output_format)).unwrap();
        for (index, &v) in ins.inputs.iter().enumerate() {
            writeln!(
                self.body,
                "input {} a{index};",
                decl(self.interface_format(v))
            )
            .unwrap();
        }
        for &id in &ins.members {
            let v = frame.events[id].output.unwrap();
            writeln!(self.body, "reg {} t{v};", decl(frame.values[v].format)).unwrap();
            if self.range_shifts
                && matches!(frame.events[id].operation, Operation::Shift)
                && !self.q_amounts.contains(&frame.events[id].inputs[1])
            {
                let amount = frame.events[id].inputs[1];
                let range = self
                    .active_ranges
                    .get(&amount)
                    .copied()
                    .unwrap_or(self.ranges[amount]);
                let data = frame.values[frame.events[id].inputs[0]].format;
                let width = data.bits.max(frame.values[v].format.bits) + u32::from(!data.signed);
                for (direction, max) in [("l", range.hi.max(0)), ("r", (-range.lo).max(0))] {
                    if max != 0 {
                        let max = max.min(i128::from(width - 1)) as u32;
                        let bits = (32 - max.leading_zeros()).max(1);
                        writeln!(self.body, "reg [{}:0] shift{v}_{direction};", bits - 1).unwrap();
                    }
                }
            }
        }
        writeln!(self.body, "integer bit_index;\nbegin").unwrap();
        let members: BTreeSet<_> = ins.members.iter().copied().collect();
        for &id in &ins.members {
            let e = &frame.events[id];
            let v = e.output.unwrap();
            let out = frame.values[v].format;
            let get = |i: usize| -> String {
                let value = e.inputs[i];
                if matches!(
                    frame.events[frame.values[value].producer].operation,
                    Operation::Literal
                ) {
                    literal(frame.values[value].raw, frame.values[value].format.bits)
                } else if members.contains(&frame.values[value].producer) {
                    format!("t{value}")
                } else {
                    format!("a{}", ins.inputs.iter().position(|&v| v == value).unwrap())
                }
            };
            let num = |i: usize| numeric(&get(i), frame.values[e.inputs[i]].format);
            let expr = match e.operation {
                Operation::Literal => literal(frame.values[v].raw, out.bits),
                Operation::Add => format!("{} + {}", num(0), num(1)),
                Operation::Sub => format!("{} - {}", num(0), num(1)),
                Operation::Multiply => format!("{} * {}", num(0), num(1)),
                Operation::Resize | Operation::BinaryScale => num(0),
                Operation::ShiftLeft(n) => format!("{} <<< {n}", num(0)),
                Operation::Shift => {
                    if self.q_amounts.contains(&e.inputs[1]) {
                        // For [-15,-12], low two amount bits encode all four
                        // shifts bijectively. The full q result is exact; only
                        // its low14 bits have consumers. No RNE is moved.
                        writeln!(self.body,"// Exact q windows; code0/1/2/3 => right12/15/14/13.\ncase ({} & 2'b11)",get(1)).unwrap();
                        for (code, shift) in Q_WINDOWS {
                            writeln!(self.body, "2'd{code}: t{v} = {} >> {shift};", get(0))
                                .unwrap();
                        }
                        writeln!(self.body, "endcase").unwrap();
                        continue;
                    }
                    if self.range_shifts {
                        let amount = e.inputs[1];
                        let range = self
                            .active_ranges
                            .get(&amount)
                            .copied()
                            .unwrap_or(self.ranges[amount]);
                        let data = frame.values[e.inputs[0]].format;
                        let width = data.bits.max(out.bits) + u32::from(!data.signed);
                        writeln!(
                            self.body,
                            "// Proven raw shift range [{},{}], independent of sample values.",
                            range.lo, range.hi
                        )
                        .unwrap();
                        let emit = |body: &mut String, right: bool| {
                            let max = if right {
                                (-range.lo).max(0)
                            } else {
                                range.hi.max(0)
                            };
                            if max == 0 {
                                writeln!(body, "t{v} = {};", num(0)).unwrap();
                                return;
                            }
                            let suffix = if right { "r" } else { "l" };
                            let shift = format!("shift{v}_{suffix}");
                            let magnitude = if right {
                                // One extra signed bit also handles the most
                                // negative representable shift count correctly.
                                format!(
                                    "(-({} + {}'sd0))",
                                    num(1),
                                    frame.values[amount].format.bits + 1
                                )
                            } else {
                                num(1)
                            };
                            // Detect oversized counts before narrowing: do not let
                            // truncation turn a saturating shift into a small shift.
                            if max >= i128::from(width) {
                                let fill = if right {
                                    format!("{} >>> {width}", num(0))
                                } else {
                                    "0".into()
                                };
                                writeln!(
                                    body,
                                    "if ({magnitude} >= {width}) t{v} = {fill}; else begin"
                                )
                                .unwrap();
                            }
                            writeln!(
                                body,
                                "{shift} = {magnitude};\nt{v} = {} {} {shift};",
                                num(0),
                                if right { ">>>" } else { "<<<" }
                            )
                            .unwrap();
                            if max >= i128::from(width) {
                                writeln!(body, "end").unwrap();
                            }
                        };
                        if range.lo == range.hi {
                            if range.lo < 0 {
                                writeln!(self.body, "t{v} = {} >>> {};", num(0), -range.lo)
                                    .unwrap();
                            } else {
                                writeln!(self.body, "t{v} = {} <<< {};", num(0), range.lo).unwrap();
                            }
                        } else if range.lo >= 0 {
                            emit(&mut self.body, false);
                        } else if range.hi <= 0 {
                            emit(&mut self.body, true);
                        } else {
                            writeln!(self.body, "if ({} < 0) begin", num(1)).unwrap();
                            emit(&mut self.body, true);
                            writeln!(self.body, "end else begin").unwrap();
                            emit(&mut self.body, false);
                            writeln!(self.body, "end").unwrap();
                        }
                        continue;
                    }
                    // Never put signed arithmetic inside a ?: expression.
                    writeln!(
                        self.body,
                        "if ({} < 0) t{v} = {} >>> (-{}); else t{v} = {} <<< {};",
                        num(1),
                        num(0),
                        num(1),
                        num(0),
                        num(1)
                    )
                    .unwrap();
                    continue;
                }
                Operation::LeadingZeros => {
                    let bits = frame.values[e.inputs[0]].format.bits;
                    writeln!(self.body,"t{v} = {bits};\nfor (bit_index=0; bit_index<{bits}; bit_index=bit_index+1)\n if (({} >> bit_index) & 1'b1) t{v} = {bits}-1-bit_index;",get(0)).unwrap();
                    continue;
                }
                Operation::Slice(n) => format!("{} >> {n}", get(0)),
                Operation::RescaleFloor(n) => format!("{} >>> {n}", num(0)),
                Operation::Less => {
                    writeln!(
                        self.body,
                        "if ({} < {}) t{v}=1'b1; else t{v}=1'b0;",
                        num(0),
                        num(1)
                    )
                    .unwrap();
                    continue;
                }
                Operation::Select => {
                    writeln!(
                        self.body,
                        "if ({}) t{v}={}; else t{v}={};",
                        get(0),
                        get(1),
                        get(2)
                    )
                    .unwrap();
                    continue;
                }
                Operation::RoundIncrement(n) => {
                    let mask = literal((1_i128 << n) - 1, frame.values[e.inputs[0]].format.bits);
                    let half = literal(1_i128 << (n - 1), frame.values[e.inputs[0]].format.bits);
                    format!("(({} & {mask}) > {half}) || ((({} & {mask}) == {half}) && (({} >> {n}) & 1'b1))",get(0),get(0),get(0))
                }
                _ => return Err(format!("function operation {:?}", e.operation)),
            };
            writeln!(self.body, "t{v} = {expr};").unwrap();
        }
        let names = outputs
            .iter()
            .map(|v| {
                let encoded = self.q_amounts.contains(v);
                let part = if ins
                    .members
                    .iter()
                    .any(|&id| frame.events[id].output == Some(*v))
                {
                    format!("t{v}")
                } else {
                    format!("a{}", ins.inputs.iter().position(|i| i == v).unwrap())
                };
                if encoded {
                    format!("{part}[1:0]")
                } else {
                    part
                }
            })
            .collect::<Vec<_>>();
        let expression = if names.len() == 1 {
            names[0].clone()
        } else {
            format!("{{{}}}", names.join(","))
        };
        writeln!(self.body, "{name} = {expression};\nend\nendfunction").unwrap();
        Ok(())
    }
    fn input_read(&mut self, ins: &Instruction, memory: usize, row: usize) -> Result<(), String> {
        let frame = &self.program.frame;
        let value = frame.events[ins.root].output.unwrap();
        let f = frame.values[value].format;
        let source = match frame.memories[memory].name.as_str() {
            "pixel.normal-lsb" => {
                let mode = usize::from(!self.program.full[ins.root]);
                self.input_lsb_delays[mode] =
                    Some(self.input_lsb_delays[mode].unwrap_or(0).max(ins.issue));
                format!(
                    "normal_lsb{mode}_d{}",
                    ins.issue.div_ceil(self.program.ii[ins.root])
                )
            }
            "pixel.rows" => {
                let mode = usize::from(!self.program.full[ins.root]);
                self.input_delays[mode][row] = self.input_delays[mode][row].max(ins.issue);
                format!(
                    "pixel{mode}_{row}_d{}",
                    ins.issue.div_ceil(self.program.ii[ins.root])
                )
            }
            "context.light" => format!("ctx_l{row}"),
            "context.projection" => format!("ctx_p{row}"),
            "context.intensity" => format!("ctx_i{row}"),
            "context.mode" => {
                if self.program.full[ins.root] {
                    "2'd3".into()
                } else {
                    "2'd2".into()
                }
            }
            "context.shininess" => "ctx_code".into(),
            "context.power" => "ctx_power".into(),
            _ => return Err("unknown RTL input source".into()),
        };
        writeln!(self.body, "wire {} v{value}_d0 = {source};", decl(f)).unwrap();
        Ok(())
    }

    fn pipeline(&mut self, lane: usize, width: u32, latency: usize, calc: &str, dsp: bool) {
        if dsp {
            writeln!(self.sequential, "`ifndef GPU_V2_GOWIN_DSP").unwrap();
        }
        for stage in 0..latency {
            writeln!(self.body, "reg [{}:0] unit{lane}_p{stage};", width - 1).unwrap();
            let source = if stage == 0 {
                calc.to_owned()
            } else {
                format!("unit{lane}_p{}", stage - 1)
            };
            writeln!(self.sequential, "unit{lane}_p{stage} <= {source};").unwrap();
        }
        self.register_bits += width as usize * latency;
        if dsp {
            writeln!(self.sequential, "`endif").unwrap();
        }
    }

    fn dsp(&mut self, lane: usize, kind: &LaneKind) {
        writeln!(self.body, "`ifdef GPU_V2_GOWIN_DSP").unwrap();
        if matches!(kind, LaneKind::PairMultiplyAdd) {
            // Two product PIPE registers are parallel, not serial. C must be
            // delayed one extra edge before CREG to match the product stage.
            writeln!(
                self.body,
                "reg [53:0] unit{lane}_c_delay,unit{lane}_tail;\nwire [53:0] unit{lane}_dsp_out;"
            )
            .unwrap();
            writeln!(self.sequential,"`ifdef GPU_V2_GOWIN_DSP\nunit{lane}_c_delay<=unit{lane}_a4;\nunit{lane}_tail<=unit{lane}_dsp_out;\n`endif").unwrap();
            writeln!(
                self.body,
                r#"MULTADDALU18X18 #(
 .A0REG(1'b1),.B0REG(1'b1),.A1REG(1'b1),.B1REG(1'b1),.CREG(1'b1),
 .PIPE0_REG(1'b1),.PIPE1_REG(1'b1),.OUT_REG(1'b1),
 .ASIGN0_REG(1'b1),.ASIGN1_REG(1'b1),.BSIGN0_REG(1'b1),.BSIGN1_REG(1'b1),
 .ACCLOAD_REG0(1'b0),.ACCLOAD_REG1(1'b0),.SOA_REG(1'b0),
 .B_ADD_SUB(1'b0),.C_ADD_SUB(1'b0),.MULTADDALU18X18_MODE(0),.MULT_RESET_MODE("SYNC")
) dsp{lane}(
 .DOUT(unit{lane}_dsp_out),.CASO(),.SOA(),.SOB(),
 .A0(unit{lane}_a0[17:0]),.B0(unit{lane}_a1[17:0]),
 .A1(unit{lane}_a2[17:0]),.B1(unit{lane}_a3[17:0]),.C(unit{lane}_c_delay),
 .ASIGN({{unit{lane}_a2[18],unit{lane}_a0[18]}}),
 .BSIGN({{unit{lane}_a3[18],unit{lane}_a1[18]}}),
 .SIA(18'd0),.SIB(18'd0),.CASI(55'd0),.ACCLOAD(1'b0),.ASEL(2'b00),.BSEL(2'b00),
 .CLK(clk),.CE(datapath_ce),.RESET(1'b0)
);
assign unit{lane}_result=unit{lane}_tail;
"#
            )
            .unwrap();
        } else {
            let bits = if matches!(kind, LaneKind::SmallMultiply) {
                9
            } else {
                18
            };
            let top = bits - 1;
            writeln!(
                self.body,
                r#"MULT{bits}X{bits} #(
 .AREG(1'b1),.BREG(1'b1),.PIPE_REG(1'b1),.OUT_REG(1'b1),
 .ASIGN_REG(1'b1),.BSIGN_REG(1'b1),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")
) dsp{lane}(
 .DOUT(unit{lane}_result),.SOA(),.SOB(),
 .A(unit{lane}_a0[{top}:0]),.B(unit{lane}_a1[{top}:0]),
 .ASIGN(unit{lane}_a0[{bits}]),.BSIGN(unit{lane}_a1[{bits}]),
 .SIA({bits}'d0),.SIB({bits}'d0),.ASEL(1'b0),.BSEL(1'b0),
 .CLK(clk),.CE(datapath_ce),.RESET(1'b0)
);
"#
            )
            .unwrap();
        }
        writeln!(self.body, "`else").unwrap();
    }

    fn split_cone(&mut self, lane: usize, ins: &Instruction, width: u32) -> Result<bool, String> {
        let frame = &self.program.frame;
        let mut alternatives = cuts::candidates(frame, ins, &self.ranges, &self.active_ranges);
        if alternatives.is_empty() {
            return Ok(false);
        }
        let maximum = alternatives.last().unwrap().at + 1;
        let baseline = maximum.div_ceil(2);
        let selected = if self.cost_cut {
            cuts::choose(&alternatives, baseline)
        } else {
            baseline
        };
        let old = alternatives.iter().find(|c| c.at == baseline).unwrap();
        let chosen = alternatives.iter().find(|c| c.at == selected).unwrap();
        writeln!(
            self.cuts_csv,
            "{lane},{maximum},{baseline},{selected},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            old.bits,
            chosen.bits,
            old.effective_bits,
            chosen.effective_bits,
            old.align_bits,
            chosen.align_bits,
            old.delay[0],
            old.delay[1],
            chosen.delay[0],
            chosen.delay[1],
            chosen.levels[0],
            chosen.levels[1],
            chosen
                .crossing
                .iter()
                .map(|&v| self.interface_format(v).bits as usize)
                .sum::<usize>()
        )
        .unwrap();
        let chosen =
            alternatives.swap_remove(alternatives.iter().position(|c| c.at == selected).unwrap());
        let early = chosen.early;
        let late = chosen.late;
        let crossing = chosen.crossing;
        let formats: Vec<_> = crossing.iter().map(|&v| self.interface_format(v)).collect();
        let bits: u32 = formats.iter().map(|f| f.bits).sum();
        let first = Instruction {
            root: ins.root,
            members: early,
            inputs: ins.inputs.clone(),
            issue: ins.issue,
            ready: ins.ready,
        };
        let second = Instruction {
            root: ins.root,
            members: late,
            inputs: crossing.clone(),
            issue: ins.issue,
            ready: ins.ready,
        };
        self.function_outputs(&format!("cone{lane}_first"), &first, &crossing)?;
        self.function(&format!("cone{lane}_second"), &second)?;
        let args = (0..ins.inputs.len())
            .map(|a| format!("unit{lane}_a{a}"))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            self.body,
            "reg [{}:0] unit{lane}_mid;\nreg [{}:0] unit{lane}_p1;",
            bits - 1,
            width - 1
        )
        .unwrap();
        let mut offset = bits;
        let mut next = Vec::new();
        for f in formats {
            offset -= f.bits;
            next.push(format!("unit{lane}_mid[{offset}+:{}]", f.bits));
        }
        writeln!(
            self.sequential,
            "unit{lane}_mid <= cone{lane}_first({args});\nunit{lane}_p1 <= cone{lane}_second({});",
            next.join(",")
        )
        .unwrap();
        self.register_bits += (bits + width) as usize;
        Ok(true)
    }
    fn rom(&mut self, name: &str, width: usize, values: &[u64], depth: usize) {
        writeln!(
            self.body,
            "reg [{}:0] {name} [0:{}];\ninitial begin",
            width - 1,
            depth - 1
        )
        .unwrap();
        for i in 0..depth {
            writeln!(
                self.body,
                "{name}[{i}] = {width}'h{:x};",
                values.get(i).copied().unwrap_or(0)
            )
            .unwrap();
        }
        writeln!(self.body, "end").unwrap();
    }

    fn lane(&mut self, lane: usize, kind: &LaneKind, ids: &[usize]) -> Result<(), String> {
        let body_start = self.body.len();
        let seq_start = self.sequential.len();
        let instructions: Vec<_> = ids
            .iter()
            .map(|&id| {
                let i = &self.program.instructions[id];
                Instruction {
                    root: i.root,
                    members: i.members.clone(),
                    inputs: i.inputs.clone(),
                    issue: i.issue,
                    ready: i.ready,
                }
            })
            .collect();
        let width = match kind {
            LaneKind::SmallMultiply => 18,
            LaneKind::LargeMultiply => 36,
            LaneKind::PairMultiplyAdd => 54,
            LaneKind::NormalizeRead => 36,
            LaneKind::PowerRead => 28,
            _ => instructions
                .iter()
                .map(|i| {
                    self.program.frame.values[self.program.frame.events[i.root].output.unwrap()]
                        .format
                        .bits
                })
                .max()
                .unwrap(),
        };
        let latency = instructions[0].ready - instructions[0].issue;
        if instructions.iter().any(|i| i.ready - i.issue != latency) {
            return Err("lane latency mismatch".into());
        }
        let name = format!("calc{lane}");
        self.active_ranges.clear();
        if matches!(kind, LaneKind::LogicCone { .. }) {
            self.union_lane_ranges(&instructions);
        }
        if matches!(
            kind,
            LaneKind::SmallMultiply | LaneKind::LargeMultiply | LaneKind::PairMultiplyAdd
        ) {
            let operand_count = if matches!(kind, LaneKind::PairMultiplyAdd) {
                5
            } else {
                2
            };
            let operand_width = if matches!(kind, LaneKind::SmallMultiply) {
                10
            } else {
                19
            };
            // Extra sign bit permits mixed unsigned/signed operands without a
            // Verilog expression silently turning unsigned. The legal domain
            // remains <=9/18 payload bits from the independent DSP certificate.
            for a in 0..operand_count {
                let w = if a == 4 { 54 } else { operand_width };
                writeln!(self.body, "reg signed [{}:0] unit{lane}_a{a};", w - 1).unwrap();
            }
            writeln!(self.body, "always @* begin").unwrap();
            for a in 0..operand_count {
                writeln!(self.body, "unit{lane}_a{a}=0;").unwrap();
            }
            for ins in &instructions {
                writeln!(
                    self.body,
                    "if {} begin",
                    self.program.phase(ins.root, ins.issue)
                )
                .unwrap();
                for (a, &v) in ins.inputs.iter().enumerate() {
                    let n = self.value(v, ins.issue)?;
                    writeln!(
                        self.body,
                        "unit{lane}_a{a} = {};",
                        numeric(&n, self.program.frame.values[v].format)
                    )
                    .unwrap();
                }
                writeln!(self.body, "end").unwrap();
            }
            writeln!(self.body, "end").unwrap();
            if operand_count == 5 {
                writeln!(self.body,"wire signed [53:0] {name} = unit{lane}_a0 * unit{lane}_a1 + unit{lane}_a2 * unit{lane}_a3 + unit{lane}_a4;").unwrap();
            } else {
                writeln!(
                    self.body,
                    "wire signed [{}:0] {name} = unit{lane}_a0 * unit{lane}_a1;",
                    width - 1
                )
                .unwrap();
            }
        } else if matches!(kind, LaneKind::NormalizeRead | LaneKind::PowerRead) {
            let address_width = if matches!(kind, LaneKind::NormalizeRead) {
                9
            } else {
                10
            };
            writeln!(
                self.body,
                "reg [{}:0] unit{lane}_addr;\nalways @* begin\nunit{lane}_addr=0;",
                address_width - 1
            )
            .unwrap();
            for ins in &instructions {
                let e = &self.program.frame.events[ins.root];
                let Operation::Read { memory, row } = e.operation else {
                    return Err("ROM opcode".into());
                };
                let memory_name = self.program.frame.memories[memory].name.clone();
                let a = if ins.inputs.is_empty() {
                    row.to_string()
                } else {
                    self.value(ins.inputs[0], ins.issue)?
                };
                let address = if memory_name == "RSQRT" {
                    format!("9'd256 + {a}")
                } else {
                    a
                };
                writeln!(
                    self.body,
                    "if {} unit{lane}_addr={address};",
                    self.program.phase(ins.root, ins.issue)
                )
                .unwrap();
            }
            writeln!(self.body, "end").unwrap();
            if matches!(kind, LaneKind::NormalizeRead) {
                let mut entries = SQUARE_SIGNED_RAW.to_vec();
                entries.extend(RSQRT_RAW);
                self.rom(&format!("norm{lane}"), 36, &entries, 512);
                writeln!(
                    self.body,
                    "wire [35:0] {name} = norm{lane}[unit{lane}_addr];"
                )
                .unwrap();
            } else {
                self.rom("power_lo", 16, &POWER_RAW.map(|v| v & 65535), 1024);
                self.rom("power_hi", 12, &POWER_RAW.map(|v| v >> 16), 1024);
                writeln!(
                    self.body,
                    "wire [27:0] {name} = {{power_hi[unit{lane}_addr],power_lo[unit{lane}_addr]}};"
                )
                .unwrap();
            }
        } else if matches!(kind, LaneKind::LogicCone { .. }) {
            let function = format!("cone{lane}");
            self.function(&function, &instructions[0])?;
            for (index, &v) in instructions[0].inputs.iter().enumerate() {
                writeln!(
                    self.body,
                    "reg {} unit{lane}_a{index};",
                    decl(self.program.frame.values[v].format)
                )
                .unwrap();
            }
            writeln!(self.body, "always @* begin").unwrap();
            for a in 0..instructions[0].inputs.len() {
                writeln!(self.body, "unit{lane}_a{a}=0;").unwrap();
            }
            for ins in &instructions {
                if ins.inputs.len() != instructions[0].inputs.len() {
                    return Err("cone input shape".into());
                }
                writeln!(
                    self.body,
                    "if {} begin",
                    self.program.phase(ins.root, ins.issue)
                )
                .unwrap();
                for (a, &v) in ins.inputs.iter().enumerate() {
                    let n = self.value(v, ins.issue)?;
                    writeln!(self.body, "unit{lane}_a{a}={n};").unwrap();
                }
                writeln!(self.body, "end").unwrap();
            }
            writeln!(self.body, "end").unwrap();
            let args = (0..instructions[0].inputs.len())
                .map(|a| format!("unit{lane}_a{a}"))
                .collect::<Vec<_>>()
                .join(",");
            writeln!(
                self.body,
                "wire [{}:0] {name} = {function}({args});",
                width - 1
            )
            .unwrap();
        } else if self.share_scalars && scalar_shareable(&self.program.frame, &instructions) {
            let function = format!("scalar_shared{lane}");
            self.union_lane_ranges(&instructions);
            self.function(&function, &instructions[0])?;
            for (a, &v) in instructions[0].inputs.iter().enumerate() {
                writeln!(
                    self.body,
                    "reg {} unit{lane}_a{a};",
                    decl(self.program.frame.values[v].format)
                )
                .unwrap();
            }
            writeln!(self.body, "always @* begin").unwrap();
            for a in 0..instructions[0].inputs.len() {
                writeln!(self.body, "unit{lane}_a{a}=0;").unwrap();
            }
            for ins in &instructions {
                writeln!(
                    self.body,
                    "if {} begin",
                    self.program.phase(ins.root, ins.issue)
                )
                .unwrap();
                for (a, &v) in ins.inputs.iter().enumerate() {
                    let n = self.value(v, ins.issue)?;
                    writeln!(self.body, "unit{lane}_a{a}={n};").unwrap();
                }
                writeln!(self.body, "end").unwrap();
            }
            writeln!(self.body, "end").unwrap();
            let args = (0..instructions[0].inputs.len())
                .map(|a| format!("unit{lane}_a{a}"))
                .collect::<Vec<_>>()
                .join(",");
            writeln!(
                self.body,
                "wire [{}:0] {name} = {function}({args});",
                width - 1
            )
            .unwrap();
        } else {
            // Scalar lane class may contain different widths/opcodes. Functions
            // use exact typed operations; unlike types cannot share a function.
            writeln!(self.body, "reg [{}:0] {name};", width - 1).unwrap();
            let mut calls = Vec::new();
            for ins in &instructions {
                let function = format!("scalar{}", ins.root);
                self.function(&function, ins)?;
                let mut args = Vec::new();
                for &v in &ins.inputs {
                    args.push(self.value(v, ins.issue)?);
                }
                calls.push((
                    ins.root,
                    ins.issue,
                    format!("{function}({})", args.join(",")),
                ));
            }
            writeln!(self.body, "always @* begin\n{name}=0;").unwrap();
            for (root, issue, call) in calls {
                writeln!(
                    self.body,
                    "if {} {name}={call};",
                    self.program.phase(root, issue)
                )
                .unwrap();
            }
            writeln!(self.body, "end").unwrap();
        }
        let dsp = matches!(
            kind,
            LaneKind::SmallMultiply | LaneKind::LargeMultiply | LaneKind::PairMultiplyAdd
        );
        let split = self.split_cones
            && matches!(kind, LaneKind::LogicCone { .. })
            && latency == 2
            && self.split_cone(lane, &instructions[0], width)?;
        if !split {
            self.pipeline(lane, width, latency, &name, dsp);
        }
        writeln!(self.body, "wire [{}:0] unit{lane}_result;", width - 1).unwrap();
        if dsp {
            self.dsp(lane, kind);
        }
        writeln!(
            self.body,
            "assign unit{lane}_result = unit{lane}_p{};",
            latency - 1
        )
        .unwrap();
        if dsp {
            writeln!(self.body, "`endif").unwrap();
        }
        if self.free_slots {
            // Only occupied phases have consumers. The first mux choice is a
            // harmless default on otherwise unused slots; no zero mask needed.
            if let Some(offset) = self.body[body_start..].find("if (calendar[") {
                let start = body_start + offset;
                let end = start + self.body[start..].find(')').unwrap() + 1;
                self.body.replace_range(start..end, "if (1'b1)");
            }
        }
        self.lanes.push(LaneReport {
            lane,
            kind: format!("{kind:?}"),
            width,
            latency,
            full_operations: instructions
                .iter()
                .filter(|i| self.program.full[i.root])
                .count(),
            diffuse_operations: instructions
                .iter()
                .filter(|i| !self.program.full[i.root])
                .count(),
        });
        if self.hierarchical {
            let lane_body = self
                .body
                .split_off(body_start)
                .replace(&format!("wire [{}:0] unit{lane}_result;\n", width - 1), "");
            let lane_seq = self.sequential.split_off(seq_start);
            let mut ports = BTreeMap::new();
            for ins in &instructions {
                for &v in &ins.inputs {
                    let name = self.value(v, ins.issue)?;
                    if name.starts_with('v') {
                        ports.insert(name, self.program.frame.values[v].format);
                    }
                }
            }
            let (declarations, body) = hoist(&lane_body);
            writeln!(
                self.modules,
                "module lighting_lane_{lane}(input wire clk,datapath_ce,input wire [{}:0] calendar,",
                if self.program.diffuse_calendar_offset==4 {7}else{4}
            )
            .unwrap();
            for (name, f) in &ports {
                writeln!(self.modules, "input wire {} {name},", decl(*f)).unwrap();
            }
            writeln!(self.modules,"output wire [{}:0] unit{lane}_result);\n{declarations}\n{body}\nalways @(posedge clk) if(datapath_ce) begin\n{lane_seq}\nend\nendmodule",width-1).unwrap();
            writeln!(self.body,"wire [{}:0] unit{lane}_result;\nlighting_lane_{lane} lane{lane}(.clk(clk),.datapath_ce(datapath_ce),.calendar(calendar),.unit{lane}_result(unit{lane}_result)",width-1).unwrap();
            for name in ports.keys() {
                writeln!(self.body, ",.{name}({name})").unwrap();
            }
            writeln!(self.body, ");").unwrap();
        }
        for ins in &instructions {
            let v = self.program.frame.events[ins.root].output.unwrap();
            writeln!(
                self.body,
                "wire {} v{v}_d0 = unit{lane}_result;",
                decl(self.program.frame.values[v].format)
            )
            .unwrap();
        }
        Ok(())
    }
}

/// Export the default Fast architecture (full II2, diffuse II1).
/// Inputs must satisfy ports::validate; RTL accepts no malformed-data protocol.
pub fn generate() -> Result<LightingVerilog, String> {
    generate_with_profile(LightingProfile::Fast)
}
pub fn generate_with_profile(profile: LightingProfile) -> Result<LightingVerilog, String> {
    generate_with_options(profile, LightingRtlOptions::default())
}
/// Selected resource alternative; its specular coordinate rounding is distinct.
pub fn generate_resource_profile(profile: LightingProfile) -> Result<LightingVerilog, String> {
    generate_with_options(profile, LightingRtlOptions::resource_profile(profile))
}
/// Equivalent lane hierarchy for fitted resource attribution.
pub fn generate_hierarchical(profile: LightingProfile) -> Result<LightingVerilog, String> {
    generate_with_options(
        profile,
        LightingRtlOptions {
            hierarchy: true,
            ..Default::default()
        },
    )
}
pub fn generate_with_options(
    profile: LightingProfile,
    options: LightingRtlOptions,
) -> Result<LightingVerilog, String> {
    let program = LoweredProgram::new(profile, options)?;
    let normal_ff = if options.shallow_normal_ff {
        if profile != LightingProfile::Fast {
            return Err("shallow normal FF experiment is limited to Fast".into());
        }
        diffuse_raw_normals(&program)?
    } else {
        BTreeSet::new()
    };
    let latency = program.latencies[0];
    let diffuse_latency = program.latencies[1];
    let specular_ii = profile.ii(true);
    let diffuse_ii = profile.ii(false);
    let stages = program
        .frame
        .outputs
        .iter()
        .map(|o| {
            let v = &program.frame.values[o.value];
            let ins = program
                .instructions
                .iter()
                .find(|i| i.root == v.producer)
                .unwrap();
            StageProbe {
                full: program.full[ins.root],
                name: o.name.clone(),
                signal: if matches!(
                    program.frame.events[v.producer].operation,
                    Operation::Literal
                ) {
                    literal(v.raw, v.format.bits)
                } else {
                    format!("v{}_d0", o.value)
                },
                age: ins.ready,
                bits: v.format.bits,
            }
        })
        .collect();
    let ranges = infer_ranges(&program.frame);
    let q_amounts = if options.q_windows {
        q_window_amounts(&program, &ranges)
    } else {
        BTreeSet::new()
    };
    let mut e = Emitter {
        program,
        body: String::new(),
        sequential: String::new(),
        retained: BTreeMap::new(),
        retained_uses: BTreeMap::new(),
        input_delays: [[0; 3]; 2],
        input_lsb_delays: [None; 2],
        register_bits: 0,
        hierarchical: options.hierarchy,
        free_slots: options.free_slots,
        split_cones: options.split_cones,
        share_scalars: options.share_scalars,
        range_shifts: options.range_shifts,
        cost_cut: options.cost_cut,
        q_amounts,
        normal_ff,
        cuts_csv: String::from("lane,max_levels,baseline_cut,cut,old_bits,bits,old_effective_bits,effective_bits,old_align_bits,align_bits,old_delay0,old_delay1,delay0,delay1,levels0,levels1,packed_bits\n"),
        ranges,
        active_ranges: BTreeMap::new(),
        modules: String::new(),
        lanes: Vec::new(),
    };
    let mut lanes = BTreeMap::<(LaneKind, usize), Vec<usize>>::new();
    for (id, ins) in e.program.instructions.iter().enumerate() {
        if let Some(kind) = e.program.binding.kinds[ins.root].clone() {
            lanes
                .entry((kind, e.program.schedule.nodes[ins.root].lane.unwrap()))
                .or_default()
                .push(id);
        }
    }
    let mut small = 0;
    let mut large = 0;
    let mut pair = 0;
    let mut roms = 0;
    for (lane, ((kind, _), ids)) in lanes.iter().enumerate() {
        match kind {
            LaneKind::SmallMultiply => small += 1,
            LaneKind::LargeMultiply => large += 1,
            LaneKind::PairMultiplyAdd => pair += 1,
            LaneKind::NormalizeRead => roms += 1,
            _ => (),
        }
        e.lane(lane, kind, ids)?;
    }
    let unbound: Vec<_> = e
        .program
        .instructions
        .iter()
        .filter(|ins| e.program.binding.kinds[ins.root].is_none())
        .map(|i| Instruction {
            root: i.root,
            members: i.members.clone(),
            inputs: i.inputs.clone(),
            issue: i.issue,
            ready: i.ready,
        })
        .collect();
    for ins in unbound {
        let event = &e.program.frame.events[ins.root];
        let value = event.output.unwrap();
        if let Operation::Read { memory, row } = event.operation {
            if !e.program.input_memory(memory) {
                return Err("unbound ROM".into());
            }
            e.input_read(&ins, memory, row)?;
        } else if event.operation == Operation::Literal {
            // Literals have no registers, regardless of their template issue age.
        } else {
            let function = format!("wire{}", ins.root);
            e.function(&function, &ins)?;
            let mut args = Vec::new();
            for &v in &ins.inputs {
                args.push(e.value(v, ins.issue)?);
            }
            writeln!(
                e.body,
                "wire {} v{value}_d0 = {function}({});",
                decl(e.program.frame.values[value].format),
                args.join(",")
            )
            .unwrap();
        }
    }
    let g = e.value(e.program.output_values[0][0], latency)?;
    let h = e.value(e.program.output_values[0][1], latency)?;
    let dg = e.value(e.program.output_values[1][0], diffuse_latency)?;
    if !e.normal_ff.is_empty() {
        let v = *e.normal_ff.first().unwrap();
        let root = e.program.frame.values[v].producer;
        let ready = e
            .program
            .instructions
            .iter()
            .find(|i| i.root == root)
            .unwrap()
            .ready;
        writeln!(
            e.body,
            "wire diffuse_normal_ff_ce = datapath_ce && {};",
            e.program.phase(root, ready)
        )
        .unwrap();
    }
    e.retained_storage(options.ram_retained)?;
    for mode in 0..2 {
        let ii = profile.ii(mode == 0);
        for row in 0..3 {
            for d in 0..=e.input_delays[mode][row].div_ceil(ii) {
                writeln!(e.body, "reg [35:0] pixel{mode}_{row}_d{d};").unwrap();
                let source = if d == 0 {
                    format!("in_row{row}")
                } else {
                    format!("pixel{mode}_{row}_d{}", d - 1)
                };
                if d == 0 {
                    writeln!(e.sequential, "pixel{mode}_{row}_d{d} <= {source};").unwrap();
                } else {
                    writeln!(
                        e.sequential,
                        "if(full_mode == 1'b{} && phase == {}) pixel{mode}_{row}_d{d} <= {source};",
                        u8::from(mode == 0),
                        1 % ii
                    )
                    .unwrap();
                }
                e.register_bits += 36;
            }
        }
    }
    e.rom("context_rom", 43, &CONTEXT_RAW, 32);
    for mode in 0..2 {
        if let Some(last) = e.input_lsb_delays[mode] {
            let ii = profile.ii(mode == 0);
            for d in 0..=last.div_ceil(ii) {
                writeln!(e.body, "reg [8:0] normal_lsb{mode}_d{d};").unwrap();
                if d == 0 {
                    writeln!(
                        e.sequential,
                        "normal_lsb{mode}_d0<={{in_row1[2:0],in_row0[18:16],in_row0[2:0]}};"
                    )
                    .unwrap();
                } else {
                    writeln!(e.sequential,"if(full_mode==1'b{} && phase=={}) normal_lsb{mode}_d{d}<=normal_lsb{mode}_d{};",u8::from(mode==0),1%ii,d-1).unwrap();
                }
                e.register_bits += 9;
            }
        }
    }
    e.register_bits += if profile.system() { 9 } else { 5 };
    let full_id_depth = latency.div_ceil(specular_ii);
    let diff_id_depth = diffuse_latency.div_ceil(diffuse_ii);
    let shared_id_depth = full_id_depth.max(diff_id_depth);
    let id_slots = if options.id_ring {
        (shared_id_depth + 2).next_power_of_two()
    } else {
        0
    };
    let id_pointer_bits = id_slots.trailing_zeros();
    e.register_bits += if options.id_ring {
        id_slots * 32 + 2 * id_pointer_bits as usize + 32
    } else if options.shared_ids {
        (shared_id_depth + 1) * 32
    } else {
        (full_id_depth + diff_id_depth + 2) * 32
    };
    e.register_bits += latency + 1 + 16 + 3 * 16 + 3 * 16 + 2 * 9 + 2 + 5 + 43 + 2;
    // Declare all module nets before use. In particular, declaration-assignment
    // syntax otherwise creates implicit nets before later retained registers.
    let (declarations, body) = hoist(&e.body);
    // Modes are uniform until the valid pipeline drains. Therefore one token
    // delay store can use the active mode's capture phase and output tap.
    // Invalid entries need no reset; valid_pipe owns their lifetime.
    let id_declarations = if options.id_ring {
        format!("// FIFO boundary: {id_slots} complete external IDs; static ages are internal token identity.\n(* syn_ramstyle = \"block_ram\" *) reg [31:0] id_ring [0:{}];\nreg [{}:0] id_write,id_read;\nreg [31:0] id_output;",id_slots-1,id_pointer_bits-1)
    } else if options.shared_ids {
        format!("reg [31:0] id_storage [0:{shared_id_depth}];")
    } else {
        format!("reg [31:0] id_storage [0:{full_id_depth}];\nreg [31:0] id_diff_storage [0:{diff_id_depth}];")
    };
    let diffuse_id_store = if options.shared_ids {
        "id_storage"
    } else {
        "id_diff_storage"
    };
    let id_capture = if options.id_ring {
        String::new()
    } else if options.shared_ids {
        format!("id_storage[0]<=in_id;\n   if((full_mode && phase=={}) || (!full_mode && phase=={})) for(token_stage=1;token_stage<={shared_id_depth};token_stage=token_stage+1) id_storage[token_stage]<=id_storage[token_stage-1];",1 % specular_ii,1 % diffuse_ii)
    } else {
        "id_storage[0]<=in_id;id_diff_storage[0]<=in_id;".into()
    };
    let id_retime = if options.id_ring {
        String::new()
    } else {
        format!(
            r#"wire [31:0] id_pipe [0:{latency}];
genvar id_age;
generate for(id_age=0;id_age<={latency};id_age=id_age+1) begin: id_retime
 if(id_age<={diffuse_latency})
 assign id_pipe[id_age]=full_mode ? id_storage[(id_age+{specular_ii}-1)/{specular_ii}] : {diffuse_id_store}[(id_age+{diffuse_ii}-1)/{diffuse_ii}];
 else assign id_pipe[id_age]=id_storage[(id_age+{specular_ii}-1)/{specular_ii}];
end endgenerate"#
        )
    };
    let id_result = if options.id_ring {
        "id_output".to_string()
    } else {
        format!("full_mode ? id_pipe[{latency}] : id_pipe[{diffuse_latency}]")
    };
    let id_ring_body = if options.id_ring {
        format!(
            r#"// Independent one-write/one-read synchronous FIFO. Each accepted ID is
// read one advancing edge before its ordered result; no age-indexed multi-read.
always @(posedge clk) begin
 if(reset || (ce && context_valid && context_ready)) begin id_write<=0;id_read<=0;end
 else if(datapath_ce) begin
  if(in_valid && in_ready) id_write<=id_write+1'b1;
  if((full_mode && valid_pipe[{}]) || (!full_mode && valid_pipe[{}])) id_read<=id_read+1'b1;
 end
end
always @(posedge clk) if(datapath_ce) begin
 if(in_valid && in_ready) id_ring[id_write]<=in_id;
 if((full_mode && valid_pipe[{}]) || (!full_mode && valid_pipe[{}])) id_output<=id_ring[id_read];
end
"#,
            latency - 1,
            diffuse_latency - 1,
            latency - 1,
            diffuse_latency - 1
        )
    } else {
        String::new()
    };
    let system_calendar = profile.system();
    let calendar_top = if system_calendar { 7 } else { 4 };
    let full_initial = if system_calendar {
        "8'b00000001"
    } else {
        "5'b00001"
    };
    let diff_initial = if system_calendar {
        "8'b00010000"
    } else {
        "5'b01000"
    };
    let mut storage_csv = String::from("class,name,bits,ready,last_use,ii,words,payload_bits\n");
    for (&value, &distance) in &e.retained {
        let v = &e.program.frame.values[value];
        let ins = e
            .program
            .instructions
            .iter()
            .find(|i| i.root == v.producer)
            .unwrap();
        let ii = e.program.ii[ins.root];
        let words = distance.div_ceil(ii);
        writeln!(
            storage_csv,
            "retained,v{value},{},{},{},{ii},{words},{}",
            v.format.bits,
            ins.ready,
            ins.ready + distance,
            words * v.format.bits as usize
        )
        .unwrap();
    }
    for mode in 0..2 {
        for row in 0..3 {
            let ii = profile.ii(mode == 0);
            let last = e.input_delays[mode][row];
            let words = last.div_ceil(ii) + 1;
            writeln!(
                storage_csv,
                "input,pixel{mode}_{row},36,0,{last},{ii},{words},{}",
                words * 36
            )
            .unwrap();
        }
    }
    for mode in 0..2 {
        if let Some(last) = e.input_lsb_delays[mode] {
            let ii = profile.ii(mode == 0);
            let words = last.div_ceil(ii) + 1;
            writeln!(
                storage_csv,
                "input,normal_lsb{mode},9,0,{last},{ii},{words},{}",
                words * 9
            )
            .unwrap();
        }
    }
    if options.id_ring {
        writeln!(
            storage_csv,
            "boundary,external_ID_ring,32,0,{latency},{specular_ii},{id_slots},{}",
            id_slots * 32
        )
        .unwrap();
        writeln!(
            storage_csv,
            "boundary,ID_output,32,{},{latency},1,1,32",
            latency - 1
        )
        .unwrap();
        writeln!(
            storage_csv,
            "control,ID_pointers,{},0,{latency},1,1,{}",
            2 * id_pointer_bits,
            2 * id_pointer_bits
        )
        .unwrap();
    }
    writeln!(
        storage_csv,
        "control,valid_ages,1,0,{latency},1,{},{}",
        latency + 1,
        latency + 1
    )
    .unwrap();
    writeln!(
        storage_csv,
        "context,latched_uniform,180,0,{latency},1,1,180"
    )
    .unwrap();
    writeln!(
        storage_csv,
        "control,phase_calendar_configured,{},0,{latency},1,1,{}",
        calendar_top + 4,
        calendar_top + 4
    )
    .unwrap();
    writeln!(
        storage_csv,
        "ROM,normalization_declared,36,0,0,1,{},{}",
        roms * 512,
        roms * 512 * 36
    )
    .unwrap();
    writeln!(storage_csv, "ROM,power_declared,28,0,0,1,1024,28672").unwrap();
    writeln!(storage_csv, "ROM,context_declared,43,0,0,1,32,1376").unwrap();
    // Keep earlier fitted profiles byte-identical, including statement layout.
    let body = if options.id_ring {
        format!("{body}\n{id_ring_body}")
    } else {
        body
    };
    let sequential = if !options.id_ring && !options.shared_ids {
        format!("   if(full_mode && phase=={}) for(token_stage=1;token_stage<={full_id_depth};token_stage=token_stage+1)\n    id_storage[token_stage]<=id_storage[token_stage-1];\n   if(!full_mode && phase=={}) for(token_stage=1;token_stage<={diff_id_depth};token_stage=token_stage+1)\n    id_diff_storage[token_stage]<=id_diff_storage[token_stage-1];\n{}",1%specular_ii,1%diffuse_ii,e.sequential)
    } else {
        e.sequential
    };
    let source = format!(
        r#"// Generated by gpu_v2::lighting::rtl::generate. Do not hand edit.
// Full II={specular_ii}, latency={latency}; diffuse II={diffuse_ii}, latency={diffuse_latency}.
module gpu_v2_lighting(
 input wire clk, reset, ce,
 input wire context_valid, output wire context_ready,
 input wire [15:0] context_epoch,
 input wire [1:0] context_mode, input wire [4:0] context_code,
 input wire signed [15:0] light_x,light_y,light_z,
 input wire [8:0] ambient,directional,
 input wire signed [15:0] ray_x,ray_y,ray_k,
 input wire in_valid, output wire in_ready,
 input wire [31:0] in_id,
 input wire [35:0] in_row0,in_row1,in_row2,
 output wire out_valid, input wire out_ready,
 output wire [31:0] out_id, output wire [15:0] out_epoch,
 output reg [8:0] out_g,out_h
);
// Registered one-hot calendars remove mode/phase decoding from lane mux paths.
reg configured;reg [1:0] phase;reg [{calendar_top}:0] calendar;
reg [1:0] ctx_mode; reg [4:0] ctx_code; reg [15:0] ctx_epoch;
reg signed [15:0] ctx_l0,ctx_l1,ctx_l2,ctx_p0,ctx_p1,ctx_p2;
reg [8:0] ctx_i0,ctx_i1; reg [42:0] ctx_power;
reg [{latency}:0] valid_pipe;
{id_declarations}
wire full_mode;
{id_retime}
{declarations}
assign full_mode = ctx_mode==3;
wire [1:0] last_phase = full_mode ? 2'd{full_last_phase} : 2'd{diff_last_phase};
wire advance = ce && configured && !(out_valid && !out_ready);
wire datapath_ce = advance && !reset && !(context_valid && context_ready);
assign context_ready = ce && !reset && !(|valid_pipe);
assign in_ready = advance && !reset && phase==0 && !context_valid;
assign out_valid = full_mode ? valid_pipe[{latency}] : valid_pipe[{diffuse_latency}];
assign out_id = {id_result};
assign out_epoch = ctx_epoch;
always @* begin
 out_g={g}; out_h={h};
 case(ctx_mode)
  0: begin out_g=9'd256; out_h=0; end
  1: begin out_g=ctx_i0; out_h=0; end
  2: begin out_g={dg};out_h=0;end
 endcase
end
{body}
integer token_stage;
always @(posedge clk) begin
 if(reset) begin configured<=0; phase<=0; calendar<=0; valid_pipe<=0; end
 else if(ce) begin
  if(context_valid && context_ready) begin
   configured<=1; phase<=0;
   if(context_mode==3) calendar<={full_initial};else calendar<={diff_initial};
   ctx_mode<=context_mode; ctx_code<=context_code; ctx_epoch<=context_epoch;
   ctx_l0<=light_x; ctx_l1<=light_y; ctx_l2<=light_z;
   ctx_p0<=ray_x; ctx_p1<=ray_y; ctx_p2<=ray_k;
   ctx_i0<=ambient; ctx_i1<=directional; ctx_power<=context_rom[context_code];
  end else if(advance) begin
   if(phase==last_phase) phase<=0;else phase<=phase+1'b1;
   if(full_mode) calendar<={full_calendar_next};else calendar<={diff_calendar_next};
   valid_pipe[0]<=in_valid && in_ready;
   {id_capture}
   for(token_stage=1;token_stage<={latency};token_stage=token_stage+1) begin
    if(full_mode || token_stage<={diffuse_latency})
     valid_pipe[token_stage]<=valid_pipe[token_stage-1];
    else valid_pipe[token_stage]<=0;
   end
{sequential}
  end
 end
end
endmodule
"#,
        body = body,
        sequential = sequential,
        full_calendar_next = if system_calendar && specular_ii == 4 {
            "{4'b0000,calendar[2:0],calendar[3]}"
        } else if system_calendar {
            "{6'b000000,calendar[0],calendar[1]}"
        } else if specular_ii == 2 {
            "{3'b000,calendar[0],calendar[1]}"
        } else {
            "{2'b00,calendar[1:0],calendar[2]}"
        },
        diff_calendar_next = if system_calendar && diffuse_ii == 4 {
            "{calendar[6:4],calendar[7],4'b0000}"
        } else if system_calendar {
            "{2'b00,calendar[4],calendar[5],4'b0000}"
        } else if diffuse_ii == 1 {
            "5'b01000"
        } else {
            "{calendar[3],calendar[4],3'b000}"
        },
        full_last_phase = specular_ii - 1,
        diff_last_phase = diffuse_ii - 1
    );
    Ok(LightingVerilog {
        source: format!("{source}\n{}", e.modules),
        lanes: e.lanes,
        latency,
        diffuse_latency,
        specular_ii,
        diffuse_ii,
        register_bits: e.register_bits,
        small_multipliers: small,
        large_multipliers: large,
        pair_macros: pair,
        normalization_roms: roms,
        id_slots,
        storage_csv,
        cuts_csv: e.cuts_csv,
        stages,
    })
}

fn hoist(input: &str) -> (String, String) {
    let mut declarations = String::new();
    let mut body = String::new();
    let mut in_function = false;
    for line in input.lines() {
        if line.starts_with("function ") {
            in_function = true;
        }
        if !in_function && (line.starts_with("wire ") || line.starts_with("reg ")) {
            if let Some((declaration, expr)) = line.split_once(" = ") {
                writeln!(declarations, "{declaration};").unwrap();
                let name = declaration.split_whitespace().last().unwrap();
                writeln!(body, "assign {name} = {expr}").unwrap();
            } else {
                writeln!(declarations, "{line}").unwrap();
            }
        } else {
            writeln!(body, "{line}").unwrap();
        }
        if line == "endfunction" {
            in_function = false;
        }
    }
    (declarations, body)
}

/// Actual per-pixel issue/ready calendar, including physical lane relocation.
/// The block study below describes alternatives rather than this implemented body.
pub fn operation_calendar_with_options(
    profile: LightingProfile,
    options: LightingRtlOptions,
) -> Result<String, String> {
    let p = LoweredProgram::new(profile, options)?;
    let mut keys = BTreeSet::new();
    for i in &p.instructions {
        if let Some(k) = &p.binding.kinds[i.root] {
            keys.insert((k.clone(), p.schedule.nodes[i.root].lane.unwrap()));
        }
    }
    let lanes: BTreeMap<_, _> = keys
        .into_iter()
        .enumerate()
        .map(|(id, k)| (k, id))
        .collect();
    let mut csv = String::from("full,event,physical_lane,kind,issue,ready,phase,inputs,members\n");
    for i in &p.instructions {
        let (lane, kind) = if let Some(k) = &p.binding.kinds[i.root] {
            (
                lanes[&(k.clone(), p.schedule.nodes[i.root].lane.unwrap())].to_string(),
                format!("{k:?}").replace('"', "\"\""),
            )
        } else {
            (String::new(), "wiring/input".into())
        };
        writeln!(
            csv,
            "{},{},{},\"{}\",{},{},{},{},{}",
            p.full[i.root],
            i.root,
            lane,
            kind,
            i.issue,
            i.ready,
            i.issue % p.ii[i.root],
            i.inputs.len(),
            i.members.len()
        )
        .unwrap();
    }
    Ok(csv)
}

/// Bounded block-calendar experiment: B consecutive pixels followed by bubbles.
/// This is independently checked scheduling evidence, not an implemented burst interface.
pub fn calendar_study(profile: LightingProfile, full: bool) -> Result<String, String> {
    calendar_study_with_options(profile, full, LightingRtlOptions::default())
}
/// Block-calendar study of the selected physical resource pool.
/// Role restrictions belong to the implemented calendar, not these alternative bursts.
pub fn calendar_study_with_options(
    profile: LightingProfile,
    full: bool,
    options: LightingRtlOptions,
) -> Result<String, String> {
    if options.stationary_logic {
        return Err("block study does not relocate stationary logic sites".into());
    }
    use resource_scheduler::{
        Graph, Limits, ModuloGraph, ModuloNode, ModuloSchedule, SearchConfig,
    };
    let p = Program::with_kernel_depth(
        profile,
        full,
        options.dedicated_dsp,
        options.kernel(),
        options.role_schedule,
        options.logic_depth,
    )?;
    let mut csv =
        String::from("group,period,average_ii,latency,operand_choices,retained_value_peak_bits\n");
    for group in [1u64, 2, 3, 4] {
        let mut virtual_graph = p.graph.clone();
        for r in &mut virtual_graph.resources {
            r.latency = r.latency.div_ceil(group);
        }
        let vg =
            ModuloGraph::from_graph(&virtual_graph).map_err(|e| format!("virtual graph {e:?}"))?;
        let s = resource_scheduler::modulo_schedule_bounded(
            &vg,
            p.ii as u64,
            &Limits::new(4096, 20000, 32),
            &SearchConfig::default(),
        )
        .map_err(|e| format!("block {group}: {e:?}"))?;
        let mut real_graph = Graph {
            resources: p.graph.resources.clone(),
            nodes: Vec::new(),
        };
        let mut nodes = Vec::new();
        let mut span = 0;
        let period = p.ii as u64 * group;
        for pixel in 0..group {
            let base = real_graph.nodes.len();
            for (id, node) in p.graph.nodes.iter().enumerate() {
                let mut n = node.clone();
                for pred in &mut n.predecessors {
                    *pred += base;
                }
                n.earliest = pixel;
                let issue = s.nodes[id].issue * group + pixel;
                let latency = n.resource.map_or(0, |r| real_graph.resources[r].latency);
                span = span.max(issue + latency);
                real_graph.nodes.push(n);
                nodes.push(ModuloNode {
                    issue,
                    lane: s.nodes[id].lane,
                });
            }
        }
        let rg =
            ModuloGraph::from_graph(&real_graph).map_err(|e| format!("expanded graph {e:?}"))?;
        let expanded = ModuloSchedule {
            initiation_interval: period,
            nodes,
            span,
        };
        let check = resource_scheduler::check_modulo(&rg, &expanded);
        if !check.is_ok() {
            return Err(format!("expanded block {group}: {check:?}"));
        }
        let mut consumers = BTreeMap::<usize, u64>::new();
        for ins in &p.instructions {
            for &v in &ins.inputs {
                consumers
                    .entry(v)
                    .and_modify(|t| *t = (*t).max(s.nodes[ins.root].issue * group))
                    .or_insert(s.nodes[ins.root].issue * group);
            }
        }
        for v in p.output_values {
            consumers
                .entry(v)
                .and_modify(|t| *t = (*t).max(s.nodes.last().unwrap().issue * group))
                .or_insert(s.nodes.last().unwrap().issue * group);
        }
        let mut edges = BTreeMap::<u64, i64>::new();
        for (value, end) in consumers {
            if p.stable[value] {
                continue;
            }
            let ins = p
                .instructions
                .iter()
                .find(|i| i.root == p.frame.values[value].producer)
                .ok_or("escaping study value")?;
            let start = s.nodes[ins.root].issue * group + (ins.ready - ins.issue) as u64;
            if end < start {
                return Err("study liveness before ready".into());
            }
            for iteration in 0..64u64 {
                for pixel in 0..group {
                    let offset = iteration * period + pixel;
                    *edges.entry(start + offset).or_default() +=
                        i64::from(p.frame.values[value].format.bits);
                    *edges.entry(end + offset + 1).or_default() -=
                        i64::from(p.frame.values[value].format.bits);
                }
            }
        }
        let mut live = 0;
        let mut peak = 0;
        for delta in edges.values() {
            live += delta;
            peak = peak.max(live);
        }
        let choices = p
            .instructions
            .iter()
            .filter(|i| p.binding.kinds[i.root].is_some())
            .map(|i| i.inputs.len())
            .sum::<usize>();
        writeln!(
            csv,
            "{group},{period},{},{},{choices},{peak}",
            p.ii,
            s.nodes.last().unwrap().issue * group + 1
        )
        .unwrap();
    }
    Ok(csv)
}

#[cfg(test)]
mod token_ram_tests {
    use super::token_read_offset;
    use std::collections::VecDeque;

    #[test]
    fn synchronous_ring_matches_shift_fifo_for_every_phase_wrap_and_ce_pause() {
        for ii in 1..=3 {
            for depth in 4_usize..=64 {
                for capture in 0..ii {
                    for consume in 0..ii {
                        let words = (depth + 1).next_power_of_two();
                        let mut memory: Vec<_> =
                            (0..words).map(|i| 0x90000000_u64 + i as u64).collect();
                        let mut read = 0_u64;
                        for epoch in 0..2 {
                            // Context/reset invalidates ownership; RAM and read
                            // payload deliberately retain the previous epoch.
                            let mut pointer = 0_usize;
                            let mut reference = VecDeque::from(vec![u64::MAX; depth]);
                            let mut cycles = 0;
                            for wall in 0..4096 {
                                let phase = cycles % ii;
                                if phase == consume && cycles > depth * ii {
                                    assert_eq!(
                                        read,
                                        reference[depth - 1],
                                        "ii={ii},D={depth},W={capture},R={consume},cycle={cycles}"
                                    );
                                }
                                if wall % 7 == 0 || wall % 19 == 0 {
                                    continue;
                                }
                                let mut next_read = read;
                                if phase == (consume + ii - 1) % ii {
                                    let offset = token_read_offset(depth, phase == capture);
                                    let address = pointer.wrapping_sub(offset) & (words - 1);
                                    if phase == capture {
                                        assert_ne!(address, pointer, "no read/write collision");
                                    }
                                    next_read = memory[address];
                                }
                                if phase == capture {
                                    let value =
                                        (cycles as u64).wrapping_mul(0x9e3779b9) ^ (epoch << 48);
                                    memory[pointer] = value;
                                    pointer = (pointer + 1) & (words - 1);
                                    reference.push_front(value);
                                    reference.pop_back();
                                }
                                read = next_read;
                                cycles += 1;
                                if cycles > depth * ii + 80 {
                                    break;
                                }
                            }
                            assert!(cycles > depth * ii + 80, "cycle watchdog");
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod q_window_tests {
    use super::*;

    #[test]
    fn encoded_windows_preserve_all_unsigned_q_bits() {
        // Single-bit basis covers every input bit of these linear bit windows;
        // extremes and mixed patterns also exercise the signed amount encoding.
        let words = (0..30)
            .map(|n| 1_u32 << n)
            .chain([0, (1 << 30) - 1, 0x15555555, 0x2aaaaaaa]);
        for q in words {
            for amount in -15_i32..=-12 {
                let code = (amount & 3) as usize;
                let shift = Q_WINDOWS.iter().find(|x| x.0 == code).unwrap().1;
                assert_eq!(q >> shift, q >> amount.unsigned_abs());
            }
        }
    }

    #[test]
    fn shared_windows_reject_a_wider_range_or_observed_amount() {
        for profile in [LightingProfile::Fast, LightingProfile::Compact] {
            let mut p = LoweredProgram::new(profile, LightingRtlOptions::factor_profile()).unwrap();
            let mut ranges = infer_ranges(&p.frame);
            let eligible = q_window_amounts(&p, &ranges);
            assert!(!eligible.is_empty(), "test must reach the actual q rewrite");
            let amount = *eligible.first().unwrap();
            ranges[amount].lo = -16;
            let rejected = q_window_amounts(&p, &ranges);
            assert!(!rejected.contains(&amount));
            assert!(rejected.len() < eligible.len());
            ranges[amount].lo = -15;
            let mut observation = p.frame.outputs[0].clone();
            observation.value = amount;
            p.frame.outputs.push(observation);
            assert!(!q_window_amounts(&p, &ranges).contains(&amount));
        }
    }

    #[test]
    fn shared_windows_reject_extra_amount_consumers_and_wider_slices() {
        for profile in [LightingProfile::Fast, LightingProfile::Compact] {
            let mut p = LoweredProgram::new(profile, LightingRtlOptions::factor_profile()).unwrap();
            let mut ranges = infer_ranges(&p.frame);
            let eligible = q_window_amounts(&p, &ranges);
            assert!(!eligible.is_empty(), "test must reach the actual q rewrite");
            let amount = *eligible.first().unwrap();
            let shifted = p
                .frame
                .events
                .iter()
                .find(|e| e.operation == Operation::Shift && e.inputs[1] == amount)
                .unwrap()
                .output
                .unwrap();
            let consumer = p
                .frame
                .events
                .iter()
                .position(|e| {
                    e.inputs.contains(&shifted) && matches!(e.operation, Operation::Slice(_))
                })
                .unwrap();

            // A second use needs the signed amount, not its two-bit encoding.
            let mut extra = p.frame.events[consumer].clone();
            extra.id = p.frame.events.len();
            extra.inputs = vec![amount];
            extra.operation = Operation::Slice(0);
            extra.output = Some(p.frame.values.len());
            let format = p.frame.values[p.frame.events[consumer].output.unwrap()].format;
            p.frame.values.push(LoweredValue {
                format,
                raw: p.frame.values[amount].raw & ((1_i128 << format.bits) - 1),
                producer: extra.id,
            });
            ranges.push(RawRange::format(format));
            p.frame.events.push(extra);
            assert_eq!(
                p.frame
                    .events
                    .iter()
                    .filter(|e| e.inputs.contains(&amount))
                    .count(),
                2
            );
            assert!(!q_window_amounts(&p, &ranges).contains(&amount));
            p.frame.events.pop();
            p.frame.values.pop();
            ranges.pop();
            assert!(q_window_amounts(&p, &ranges).contains(&amount));

            // The rewrite's consumer contract permits only the low 14 bits.
            let original_slice = p.frame.events[consumer].operation.clone();
            let width = p.frame.values[p.frame.events[consumer].output.unwrap()]
                .format
                .bits;
            p.frame.events[consumer].operation = Operation::Slice(15 - width);
            assert!(!q_window_amounts(&p, &ranges).contains(&amount));
            p.frame.events[consumer].operation = original_slice;
            assert!(q_window_amounts(&p, &ranges).contains(&amount));

            let mut observation = p.frame.outputs[0].clone();
            observation.value = shifted;
            p.frame.outputs.push(observation);
            assert!(!q_window_amounts(&p, &ranges).contains(&amount));
        }
    }
}
