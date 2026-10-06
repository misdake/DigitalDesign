//! Actual D registers on the certified II8 calendar. The shared executor owns
//! only physical bits and phase/valid; structural descriptors contain no sampled
//! arithmetic answers. Input-only wires disappear after the acceptance edge.
use audited::{
    physical::{lowering::WiringAdd, LogicCone, Timing},
    Format, FrameReport, MemoryKind, Operation,
};

pub mod rtl;

pub const NUMERIC_BITS: usize = 1336;
pub const CONTROL_BITS: usize = 49;
pub const SPAN: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub quad: u8,
    pub mask: u8,
    pub slot: u8,
    pub max_n: u8,
    pub has_mip: bool,
    pub filter: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub uv: [i64; 8],
    pub bias: i16,
    pub header: Header,
}
impl Input {
    /// Quantization belongs to the actual admission, before any arithmetic.
    pub fn capture(
        q: &crate::texture::ports::QuadInput,
        slot: crate::texture::ports::Slot,
    ) -> Result<Self, String> {
        use crate::texture::ports::Filter;
        slot.validate()?;
        if q.quad_id >= 16
            || q.mask >= 16
            || q.slot >= 16
            || slot.max_size_log2 > 10
            || !slot.valid
            || !q.lod_bias.is_finite()
            || q.material_size_log2 != slot.max_size_log2
        {
            return Err("D input metadata range".into());
        }
        let mut uv = [0; 8];
        for (i, v) in q.uv.iter().flatten().enumerate() {
            if v.abs() > 1_048_576.0 {
                return Err("D external UV bound".into());
            }
            let raw = (v * 262144.0).round_ties_even();
            if !raw.is_finite() || !(-(1_i64 << 39) as f64..(1_i64 << 39) as f64).contains(&raw) {
                return Err("D signed UV40 range".into());
            }
            uv[i] = raw as i64;
        }
        // The real admission crosses the same generated protected memory type
        // as the legacy input. No frame/body is executed or retained here.
        let mut boundary = audited::Model::numerical();
        let codes = uv.map(i128::from);
        let _: crate::texture::format::UvStore = boundary
            .input("accepted_uv", &codes)
            .map_err(|e| format!("D UV boundary {e:?}"))?;
        let bias = (q.lod_bias.clamp(-32.0, 32.0) * 256.0).round_ties_even() as i16;
        let filter = match q.filter {
            Filter::Nearest => 0,
            Filter::Bilinear => 1,
            Filter::Trilinear => 2,
        };
        let _: crate::texture::format::BiasStore = boundary
            .input("accepted_bias", &[i128::from(bias)])
            .map_err(|e| format!("D bias boundary {e:?}"))?;
        let _: audited::Memory<4, 0, false> = boundary
            .input(
                "accepted_meta",
                &[
                    q.quad_id.into(),
                    q.mask.into(),
                    q.slot.into(),
                    slot.max_size_log2.into(),
                ],
            )
            .map_err(|e| format!("D metadata boundary {e:?}"))?;
        let _: audited::Memory<1, 0, false> = boundary
            .input("accepted_has_mip", &[i128::from(slot.has_full_mip)])
            .map_err(|e| format!("D mip boundary {e:?}"))?;
        let _: crate::texture::format::FilterCodeStore = boundary
            .input("accepted_filter", &[i128::from(filter)])
            .map_err(|e| format!("D filter boundary {e:?}"))?;
        Ok(Self {
            uv,
            bias,
            header: Header {
                quad: q.quad_id,
                mask: q.mask,
                slot: q.slot,
                max_n: slot.max_size_log2,
                has_mip: slot.has_full_mip,
                filter,
            },
        })
    }
    fn rows(self) -> Vec<(&'static str, usize, i128)> {
        let mut rows: Vec<_> = self
            .uv
            .into_iter()
            .enumerate()
            .map(|(i, v)| ("helper_uv", i, i128::from(v)))
            .collect();
        rows.push(("bias", 0, i128::from(self.bias)));
        for (i, v) in [
            self.header.quad,
            self.header.mask,
            self.header.slot,
            self.header.max_n,
        ]
        .into_iter()
        .enumerate()
        {
            rows.push(("meta", i, i128::from(v)));
        }
        rows.extend([
            ("has_mip", 0, i128::from(self.header.has_mip)),
            ("filter", 0, i128::from(self.header.filter)),
        ]);
        rows
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    pub uv: [u32; 8],
    pub slope: u64,
    pub bias: i16,
    pub header: Header,
}

/// A source slice and its periodic destinations, copied from the frozen
/// storage certificate. The last0 external input slices have no retained write.
/// `lows.len()` is `period / ii` rotating physical destinations, so the same
/// descriptor serves the II8 D/LOD banks and the II2 coordinate bank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub value: usize,
    pub source_low: u32,
    pub width: u32,
    pub birth: u8,
    pub last_read: u8,
    pub lows: Vec<usize>,
}
#[derive(Clone, Copy, Debug)]
enum Bit {
    Constant(bool),
    Root(usize, u32),
}
#[derive(Clone, Debug)]
struct Value {
    format: Format,
    view: Vec<Bit>,
}
#[derive(Clone, Debug)]
enum Op {
    Add,
    Sub,
    Less,
    Equal,
    Select,
    LeadingZeros,
    Shift,
    Round(u32),
    Rom(usize),
}
#[derive(Clone, Debug)]
struct Instruction {
    event: usize,
    age: u8,
    output: usize,
    operands: Vec<usize>,
    op: Op,
}
#[derive(Clone, Debug)]
struct RootInput {
    value: usize,
    name: String,
    row: usize,
}

/// Immutable wiring/calendar only: no FrameReport, nonliteral raw value, source
/// quad, per-job value array or finished stage output is retained here.
#[derive(Clone, Debug)]
pub struct Calendar {
    bits: usize,
    span: u8,
    period: u32,
    ii: u32,
    values: Vec<Value>,
    instructions: Vec<Instruction>,
    inputs: Vec<RootInput>,
    outputs: Vec<(String, usize)>,
    fields: Vec<Field>,
    roms: [Vec<i128>; 2],
}
impl Calendar {
    /// Extract immutable wiring/calendar from a (possibly poisoned) closed
    /// frame. `period`/`ii` come from the certified packed layout and fix the
    /// rotating bank count `period / ii`.
    #[allow(clippy::too_many_arguments)] // One explicit structural extraction.
    pub fn from_structure(
        frame: &FrameReport,
        times: &[Timing],
        cones: &[LogicCone],
        wiring_adds: &[WiringAdd],
        fields: Vec<Field>,
        bits: usize,
        span: u8,
        period: u32,
        ii: u32,
        outputs: &[&str],
    ) -> Result<Self, String> {
        if times.len() != frame.events.len()
            || span >= 32
            || period == 0
            || period > 32
            || ii == 0
            || !period.is_multiple_of(ii)
            || fields.iter().any(|f| {
                f.width == 0
                    || f.width > 64
                    || f.birth > f.last_read
                    || f.last_read > span
                    || f.lows.len() != (period / ii) as usize
                    || f.lows.iter().any(|l| l + f.width as usize > bits)
            })
        {
            return Err("D/LOD structural placement".into());
        }
        let mut values: Vec<Value> = frame
            .values
            .iter()
            .map(|v| Value {
                format: v.format,
                view: Vec::new(),
            })
            .collect();
        let mut inputs = vec![];
        let mut instructions = vec![];
        // `storage::fields` maps every read of one input `(memory,row)` to the
        // first value that touched it, so several numerical read events can
        // share a single physical bank entry. Canonicalize to the same origin
        // here; otherwise a later read would demand an unpublished field.
        let mut input_roots: std::collections::BTreeMap<(usize, usize), usize> =
            std::collections::BTreeMap::new();
        let wiring: std::collections::BTreeMap<usize, [u128; 2]> = wiring_adds
            .iter()
            .map(|w| (w.result_event, w.possible_ones))
            .collect();
        for e in &frame.events {
            let Some(v) = e.output else { continue };
            let format = values[v].format;
            // A disproven carry add is disjoint field wiring: every result bit
            // aliases one operand bit. `storage::fields` drops its own origin,
            // so the calendar must resolve the same source bits, not retain a
            // second register bank entry.
            if let Some(masks) = wiring.get(&e.id) {
                let left = &values[e.inputs[0]];
                let right = &values[e.inputs[1]];
                let (left_signed, right_signed) = (left.format.signed, right.format.signed);
                values[v].view = (0..format.bits)
                    .map(|b| {
                        let (source, signed) = if masks[0] & (1_u128 << b) != 0 {
                            (left, left_signed)
                        } else if masks[1] & (1_u128 << b) != 0 {
                            (right, right_signed)
                        } else {
                            return Bit::Constant(false);
                        };
                        source.view.get(b as usize).copied().unwrap_or_else(|| {
                            if signed {
                                *source.view.last().expect("signed wiring add")
                            } else {
                                Bit::Constant(false)
                            }
                        })
                    })
                    .collect();
                continue;
            }
            let alias = if e.inputs.len() == 1 {
                match e.operation {
                    Operation::Resize | Operation::BinaryScale => Some(0_i32),
                    Operation::Slice(n) | Operation::RescaleFloor(n) => Some(n as i32),
                    Operation::ShiftLeft(n) => Some(-(n as i32)),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(offset) = alias {
                let source = &values[e.inputs[0]];
                values[v].view = (0..format.bits)
                    .map(|b| {
                        let bit = b as i32 + offset;
                        if bit < 0 {
                            Bit::Constant(false)
                        } else if (bit as usize) < source.view.len() {
                            source.view[bit as usize]
                        } else if source.format.signed {
                            *source.view.last().expect("signed alias")
                        } else {
                            Bit::Constant(false)
                        }
                    })
                    .collect();
                continue;
            }
            if e.operation == Operation::Literal {
                // This is the only access to numerical raw data in the builder.
                values[v].view = (0..format.bits)
                    .map(|b| Bit::Constant((frame.values[v].raw as u128 >> b) & 1 != 0))
                    .collect();
                continue;
            }
            if cones.iter().any(|c| c.absorbed_events.contains(&e.id)) {
                continue;
            }
            if let Operation::Read { memory, row } = e.operation {
                if frame.memories[memory].kind == MemoryKind::Input {
                    let origin = *input_roots.entry((memory, row)).or_insert(v);
                    values[v].view = (0..format.bits).map(|b| Bit::Root(origin, b)).collect();
                    if origin == v {
                        inputs.push(RootInput {
                            value: v,
                            name: frame.memories[memory].name.clone(),
                            row,
                        });
                    }
                    continue;
                }
            }
            values[v].view = (0..format.bits).map(|b| Bit::Root(v, b)).collect();
            if e.control.is_some() || times[e.id].ready != times[e.id].issue + 1 {
                return Err("D/LOD requires unit registered operation".into());
            }
            // Charged singleton cones express dependency cuts, not equality
            // lowering. Their set-sorted operands must not reorder subtraction.
            let cone = cones
                .iter()
                .find(|c| c.result_event == e.id && !c.absorbed_events.is_empty());
            let op = if cone.is_some() {
                Op::Equal
            } else {
                match e.operation {
                    Operation::Add => Op::Add,
                    Operation::Sub => Op::Sub,
                    Operation::Less => Op::Less,
                    Operation::Select => Op::Select,
                    Operation::LeadingZeros => Op::LeadingZeros,
                    Operation::Shift => Op::Shift,
                    Operation::RoundIncrement(n) => Op::Round(n),
                    Operation::Read { memory, .. }
                        if frame.memories[memory].kind == MemoryKind::Rom =>
                    {
                        match frame.memories[memory].name.as_str() {
                            "log2_64" => Op::Rom(0),
                            "mip_prefix" => Op::Rom(1),
                            _ => return Err("D/LOD unknown ROM".into()),
                        }
                    }
                    _ => {
                        return Err(format!(
                            "D/LOD unsupported structural operation {:?}",
                            e.operation
                        ))
                    }
                }
            };
            instructions.push(Instruction {
                event: e.id,
                age: u8::try_from(times[e.id].issue).map_err(|_| "D/LOD age range")?,
                output: v,
                operands: cone.map_or_else(|| e.inputs.clone(), |c| c.operands.clone()),
                op,
            });
        }
        let outputs = outputs
            .iter()
            .map(|name| {
                frame
                    .outputs
                    .iter()
                    .find(|o| o.name == *name)
                    .map(|o| (o.name.clone(), o.value))
                    .ok_or_else(|| format!("D/LOD missing output {name}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let calendar = Self {
            bits,
            span,
            period,
            ii,
            values,
            instructions,
            inputs,
            outputs,
            fields,
            roms: if span == super::lod::SPAN {
                [
                    constant_codes(&crate::texture::format::LOG)?,
                    constant_codes(&crate::texture::format::PREFIX)?,
                ]
            } else {
                [vec![], vec![]]
            },
        };
        // All persistent roots must have a paid slice. Input-only birth0/last0
        // roots are read exclusively through acceptance wires on that edge.
        for i in &calendar.instructions {
            for &operand in &i.operands {
                calendar.audit_view(operand, i.age)?;
            }
        }
        for &(_, v) in &calendar.outputs {
            calendar.audit_view(v, span)?;
        }
        Ok(calendar)
    }
    fn audit_view(&self, v: usize, age: u8) -> Result<(), String> {
        for bit in &self.values[v].view {
            if let Bit::Root(root, bit) = *bit {
                if !self.fields.iter().any(|f| {
                    f.value == root
                        && bit >= f.source_low
                        && bit < f.source_low + f.width
                        && f.birth <= age
                        && age <= f.last_read
                }) {
                    return Err(format!(
                        "D/LOD unallocated read value{root} bit{bit} age{age}"
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
    pub fn numeric_bits(&self) -> usize {
        self.bits
    }
    pub fn span(&self) -> u8 {
        self.span
    }
    pub fn ii(&self) -> u32 {
        self.ii
    }
    pub fn period(&self) -> u32 {
        self.period
    }
}

/// Host instrumentation of this edge; never a persistent numerical register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calculation {
    pub event: usize,
    pub value: usize,
    pub age: u8,
    pub iteration: usize,
    pub operands: Vec<i128>,
    pub raw: i128,
    pub rom: Option<(usize, usize)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub low: usize,
    pub width: u32,
    pub value: usize,
    pub source_low: u32,
    pub raw: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub accepted: bool,
    pub calculations: Vec<Calculation>,
    pub writes: Vec<Write>,
}

/// A bit-addressed old-state bank. The phase ring and valid shift are the exact
/// paid period+(span+1) controls. Runtime owns errors; no additional fault FF.
pub(crate) struct Registers {
    calendar: Calendar,
    bank: Vec<u64>,
    phase: u32,
    valid: u32,
}
impl Registers {
    pub(crate) fn new(calendar: Calendar) -> Self {
        Self {
            bank: vec![0; calendar.bits.div_ceil(64)],
            calendar,
            phase: 1,
            valid: 0,
        }
    }
    pub(crate) fn idle(&self) -> bool {
        self.valid == 0
    }
    pub(crate) fn phase(&self) -> u8 {
        self.phase.trailing_zeros() as u8
    }
    pub(crate) fn output(&self) -> Result<Option<Vec<(String, i128)>>, String> {
        let age = self.calendar.span;
        if self.valid & (1 << age) == 0 {
            return Ok(None);
        }
        let iteration = self.iteration(age);
        self.calendar
            .outputs
            .iter()
            .map(|(n, v)| Ok((n.clone(), self.read(*v, age, iteration, &[])?)))
            .collect::<Result<Vec<_>, String>>()
            .map(Some)
    }
    fn iteration(&self, age: u8) -> usize {
        let period = self.calendar.period;
        let phase = u32::from(self.phase());
        usize::try_from((phase + period - u32::from(age) % period) % period).expect("period fits")
            / self.calendar.ii as usize
    }
    fn read(
        &self,
        v: usize,
        age: u8,
        iteration: usize,
        external: &[(usize, i128)],
    ) -> Result<i128, String> {
        let value = &self.calendar.values[v];
        let mut raw = 0_u128;
        for (b, bit) in value.view.iter().enumerate() {
            let set = match *bit {
                Bit::Constant(c) => c,
                Bit::Root(root, source_bit) => {
                    if let Some((_, raw)) = external.iter().find(|(r, _)| *r == root) {
                        (*raw as u128 >> source_bit) & 1 != 0
                    } else {
                        let f = self
                            .calendar
                            .fields
                            .iter()
                            .find(|f| {
                                f.value == root
                                    && source_bit >= f.source_low
                                    && source_bit < f.source_low + f.width
                                    && f.birth <= age
                                    && age <= f.last_read
                            })
                            .ok_or_else(|| {
                                format!("D/LOD invalid live read v{root} b{source_bit} age{age}")
                            })?;
                        let low = f.lows[iteration] + (source_bit - f.source_low) as usize;
                        self.bank[low / 64] >> (low % 64) & 1 != 0
                    }
                }
            };
            raw |= u128::from(set) << b;
        }
        if value.format.signed && raw >> (value.format.bits - 1) & 1 != 0 {
            Ok(raw as i128 - (1_i128 << value.format.bits))
        } else {
            Ok(raw as i128)
        }
    }
    pub(crate) fn tick(
        &mut self,
        ce: bool,
        rows: Option<&[(&str, usize, i128)]>,
    ) -> Result<Edge, String> {
        let mut edge = Edge {
            accepted: false,
            calculations: vec![],
            writes: vec![],
        };
        if !ce {
            return Ok(edge);
        }
        if rows.is_some() && !(u32::from(self.phase())).is_multiple_of(self.calendar.ii) {
            return Err("D/LOD initiation-interval admission".into());
        }
        let mut external = vec![];
        if let Some(rows) = rows {
            for input in &self.calendar.inputs {
                // base is neither a live operand nor retained output. It remains
                // in the separately paid slot table; do not duplicate it here.
                if input.name == "base" {
                    continue;
                }
                let raw = rows
                    .iter()
                    .find(|(n, r, _)| *n == input.name && *r == input.row)
                    .ok_or("D/LOD missing live input row")?
                    .2;
                range(self.calendar.values[input.value].format, raw)?;
                external.push((input.value, raw));
            }
            for &(v, raw) in &external {
                self.writes(v, raw, 0, self.iteration(0), true, &mut edge.writes);
            }
            edge.accepted = true;
        }
        let active = self.valid | u32::from(edge.accepted);
        let mut rom_reads = [0; 2];
        for i in &self.calendar.instructions {
            if active & (1 << i.age) == 0 {
                continue;
            }
            let iteration = self.iteration(i.age);
            let wires = if i.age == 0 { external.as_slice() } else { &[] };
            let operands = i
                .operands
                .iter()
                .map(|v| self.read(*v, i.age, iteration, wires))
                .collect::<Result<Vec<_>, _>>()?;
            let mut rom = None;
            let raw = match i.op {
                Op::Add => operands[0] + operands[1],
                Op::Sub => operands[0] - operands[1],
                Op::Less => i128::from(operands[0] < operands[1]),
                Op::Equal => i128::from(operands[0] == operands[1]),
                Op::Select => operands[if operands[0] != 0 { 1 } else { 2 }],
                Op::LeadingZeros => {
                    i128::from((operands[0] as u128).leading_zeros())
                        - (128 - i128::from(self.calendar.values[i.operands[0]].format.bits))
                }
                Op::Shift => {
                    let shift = operands[1];
                    if !(-126..=126).contains(&shift) {
                        return Err("D/LOD shift range".into());
                    }
                    if shift >= 0 {
                        operands[0] << shift
                    } else {
                        operands[0] >> -shift
                    }
                }
                Op::Round(n) => {
                    let rem = operands[0] & ((1_i128 << n) - 1);
                    let half = 1_i128 << (n - 1);
                    i128::from(rem > half || rem == half && (operands[0] >> n) & 1 != 0)
                }
                Op::Rom(which) => {
                    let index =
                        usize::try_from(operands[0]).map_err(|_| "LOD negative ROM index")?;
                    rom_reads[which] += 1;
                    if rom_reads[which] > 1 {
                        return Err("LOD ROM port collision".into());
                    }
                    rom = Some((which, index));
                    *self.calendar.roms[which]
                        .get(index)
                        .ok_or("LOD ROM index range")?
                }
            };
            range(self.calendar.values[i.output].format, raw)?;
            self.writes(i.output, raw, i.age + 1, iteration, false, &mut edge.writes);
            edge.calculations.push(Calculation {
                event: i.event,
                value: i.output,
                age: i.age,
                iteration,
                operands,
                raw,
                rom,
            });
        }
        // Read-before-write applies across all cohorts, input captures and ROM
        // returns. Scratch write intents are combinational, not another bank.
        let mut written = vec![false; self.calendar.bits];
        for w in &edge.writes {
            for bit in 0..w.width as usize {
                let low = w.low + bit;
                if std::mem::replace(&mut written[low], true) {
                    return Err("D/LOD physical FF write collision".into());
                }
            }
        }
        for w in &edge.writes {
            for bit in 0..w.width as usize {
                let low = w.low + bit;
                let mask = 1_u64 << (low % 64);
                let word = &mut self.bank[low / 64];
                *word = (*word & !mask) | (((w.raw >> bit) & 1) << (low % 64));
            }
        }
        self.valid = (active << 1) & ((1_u32 << (self.calendar.span + 1)) - 1);
        // Only the certified period's one-hot controls physically exist. The
        // host word must not introduce32 phase FFs into an eight-phase block.
        self.phase = if self.phase >> (self.calendar.period - 1) != 0 {
            1
        } else {
            self.phase << 1
        };
        Ok(edge)
    }
    fn writes(
        &self,
        v: usize,
        raw: i128,
        birth: u8,
        iteration: usize,
        input: bool,
        writes: &mut Vec<Write>,
    ) {
        for f in &self.calendar.fields {
            if f.value == v && f.birth == birth && !(input && f.last_read == 0) {
                writes.push(Write {
                    low: f.lows[iteration],
                    width: f.width,
                    value: v,
                    source_low: f.source_low,
                    raw: ((raw as u128 >> f.source_low) & ((1_u128 << f.width) - 1)) as u64,
                });
            }
        }
    }
    pub(crate) fn bank(&self) -> &[u64] {
        &self.bank
    }
}
fn range(f: Format, raw: i128) -> Result<(), String> {
    let valid = if f.signed {
        raw >= -(1_i128 << (f.bits - 1)) && raw < 1_i128 << (f.bits - 1)
    } else {
        raw >= 0 && raw < 1_i128 << f.bits
    };
    if valid {
        Ok(())
    } else {
        Err(format!("D/LOD scalar range {raw} for {f:?}"))
    }
}
fn constant_codes<const B: u32, const F: u32, const S: bool>(
    values: &[audited::Fixed<B, F, S>],
) -> Result<Vec<i128>, String> {
    // Audited Fixed intentionally offers no host raw escape. Materialize only
    // frozen literals through its table boundary, once at construction. These
    // immutable codes are the ROM contents, never sampled quad arithmetic.
    values
        .iter()
        .map(|value| {
            let mut m = audited::Model::numerical();
            let table = m
                .table("constant", &[*value])
                .map_err(|e| format!("ROM literal {e:?}"))?;
            let f = m
                .compute("constant", 4)
                .map_err(|e| format!("ROM literal {e:?}"))?;
            let raw = f
                .read(table.at::<0>())
                .map_err(|e| format!("ROM literal {e:?}"))?;
            f.publish("code", raw)
                .map_err(|e| format!("ROM literal {e:?}"))?;
            Ok(f.finish().outputs[0].raw)
        })
        .collect()
}
pub(crate) fn scalar(values: &[(String, i128)], name: &str) -> i128 {
    values
        .iter()
        .find(|(n, _)| n == name)
        .expect("fixed numerical port")
        .1
}

pub struct DerivativeEmu {
    registers: Registers,
}
impl DerivativeEmu {
    pub fn new(calendar: Calendar) -> Result<Self, String> {
        if calendar.bits != NUMERIC_BITS || calendar.span != SPAN {
            return Err("D allocation mismatch".into());
        }
        Ok(Self {
            registers: Registers::new(calendar),
        })
    }
    pub fn output(&self) -> Result<Option<Output>, String> {
        self.registers
            .output()?
            .map(|v| {
                Ok(Output {
                    uv: std::array::from_fn(|i| scalar(&v, &format!("uv{i}")) as u32),
                    slope: scalar(&v, "slope") as u64,
                    bias: scalar(&v, "bias") as i16,
                    header: Header {
                        quad: scalar(&v, "quad") as u8,
                        mask: scalar(&v, "mask") as u8,
                        slot: scalar(&v, "slot") as u8,
                        max_n: scalar(&v, "max_n") as u8,
                        has_mip: scalar(&v, "has_mip") != 0,
                        filter: scalar(&v, "filter") as u8,
                    },
                })
            })
            .transpose()
    }
    pub fn tick(&mut self, ce: bool, input: Option<Input>) -> Result<Edge, String> {
        if ce
            && input.is_some_and(|i| {
                i.header.max_n > 10 || i.header.filter > 2 || !(-8192..=8192).contains(&i.bias)
            })
        {
            return Err("D input range".into());
        }
        let rows = input.map(Input::rows);
        self.registers.tick(ce, rows.as_deref())
    }
    pub fn idle(&self) -> bool {
        self.registers.idle()
    }
    pub fn phase(&self) -> u8 {
        self.registers.phase()
    }
    pub fn bank(&self) -> &[u64] {
        self.registers.bank()
    }
}
