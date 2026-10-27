//! Actual D registers on the certified II8 calendar. The shared executor owns
//! only physical bits and phase/valid; structural descriptors contain no sampled
//! arithmetic answers. Input-only wires disappear after the acceptance edge.
use audited::{
    physical::{lowering::WiringAdd, LogicCone, Timing},
    Format, FrameReport, MemoryKind, Operation,
};

#[cfg(test)]
mod fusion_tests;
pub mod rtl;

pub const NUMERIC_BITS: usize = 219;
pub const CONTROL_BITS: usize = 12;
pub const SPAN: u8 = 3;

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
    pub force_coarsest: bool,
    pub header: Header,
}
impl Input {
    /// Quantization belongs to the actual admission, before any arithmetic.
    pub fn capture(
        q: &crate::texture::ports::QuadInput,
        slot: crate::texture::ports::Slot,
    ) -> Result<Self, String> {
        Self::from_raw(&crate::texture::ports::RawQuadInput::capture(q)?, slot)
    }
    /// Integer-only input from the published Sampling queue and draw context.
    pub fn from_raw(
        q: &crate::texture::ports::RawQuadInput,
        slot: crate::texture::ports::Slot,
    ) -> Result<Self, String> {
        use crate::texture::ports::Filter;
        slot.validate()?;
        if q.quad_id >= 16
            || q.mask >= 16
            || q.slot >= 16
            || slot.max_size_log2 > 10
            || !slot.valid
            || q.material_size_log2 != slot.max_size_log2
            || q.uv_q16.iter().flatten().any(|v| {
                !(crate::texture::ports::UV_MIN..=crate::texture::ports::UV_MAX).contains(v)
            })
        {
            return Err("D input metadata range".into());
        }
        let force_coarsest = q.force_coarsest;
        let uv = std::array::from_fn(|i| q.uv_q16[i / 2][i % 2]);
        // The real admission crosses the same generated protected memory type
        // as the legacy input. No frame/body is executed or retained here.
        let mut boundary = audited::Model::numerical();
        let codes = uv.map(i128::from);
        let _: crate::texture::format::UvStore = boundary
            .input("accepted_uv", &codes)
            .map_err(|e| format!("D UV boundary {e:?}"))?;
        // The stable draw field is signed16 Q8. Beyond +/-32 levels the
        // clamped LOD is unchanged; retain the same protected boundary as the
        // legacy quantizer without converting through floating point.
        let bias = q.bias_q8.clamp(-8192, 8192);
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
            force_coarsest,
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
        rows.push(("force_coarsest", 0, i128::from(self.force_coarsest)));
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bit {
    Constant(bool),
    Root(usize, u32),
}
#[derive(Clone, Debug)]
struct Value {
    format: Format,
    view: Vec<Bit>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Same-edge scalar wires inside an audited fabric cut: value -> event/age.
    /// They never allocate a hidden intermediate register or numerical oracle.
    combinational: std::collections::BTreeMap<usize, (usize, u8)>,
    bits: usize,
    span: u8,
    period: u32,
    ii: u32,
    initial_phase: u32,
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
        let mut members = std::collections::BTreeSet::new();
        for c in cones {
            c.audit_structure(frame)
                .map_err(|e| format!("calendar cone: {e:?}"))?;
            if c.latency != 1
                || times[c.result_event].ready != times[c.result_event].issue + 1
                || c.absorbed_events.iter().any(|&id| {
                    times[id].issue != times[c.result_event].ready
                        || times[id].ready != times[c.result_event].ready
                })
                || c.absorbed_events
                    .iter()
                    .copied()
                    .chain([c.result_event])
                    .any(|id| !members.insert(id))
            {
                return Err("calendar cone timing or overlap".into());
            }
        }
        // Same-edge wires are local to one cone. Validate every external
        // dependency before allowing scratch reads: field coverage alone
        // cannot prove that another boundary's value is already registered.
        let absorbed: std::collections::BTreeSet<_> = cones
            .iter()
            .flat_map(|c| c.absorbed_events.iter().copied())
            .collect();
        for e in &frame.events {
            let t = times[e.id];
            if t.issue > t.ready || t.ready > u64::from(span) {
                return Err("calendar timing range".into());
            }
            if absorbed.contains(&e.id) {
                continue;
            }
            let cone = cones.iter().find(|c| c.result_event == e.id);
            let operands = cone.map_or(e.inputs.as_slice(), |c| c.operands.as_slice());
            let mut predecessors: Vec<_> =
                operands.iter().map(|&v| frame.values[v].producer).collect();
            predecessors.extend(e.control);
            if let Some(c) = cone {
                predecessors.extend(
                    c.absorbed_events
                        .iter()
                        .filter_map(|&id| frame.events[id].control),
                );
            }
            if predecessors.iter().any(|&id| times[id].ready > t.issue) {
                return Err("calendar external operand before registered return".into());
            }
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
        let equality: std::collections::BTreeSet<_> = cones
            .iter()
            .filter(|c| equality_recipe(frame, c))
            .map(|c| c.result_event)
            .collect();
        let fused: std::collections::BTreeMap<_, _> = cones
            .iter()
            .filter(|c| !equality.contains(&c.result_event) && !c.absorbed_events.is_empty())
            .flat_map(|c| {
                std::iter::once(c.result_event)
                    .chain(c.absorbed_events.iter().copied())
                    .map(move |id| (id, c.result_event))
            })
            .collect();
        let mut combinational = std::collections::BTreeMap::new();
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
            if cones
                .iter()
                .any(|c| equality.contains(&c.result_event) && c.absorbed_events.contains(&e.id))
            {
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
            let boundary = fused.get(&e.id).copied().unwrap_or(e.id);
            let age = u8::try_from(times[boundary].issue).map_err(|_| "D/LOD age range")?;
            if e.control.is_some() || times[boundary].ready != times[boundary].issue + 1 {
                return Err("D/LOD requires unit registered operation".into());
            }
            // Charged singleton cones express dependency cuts, not equality
            // lowering. Their set-sorted operands must not reorder subtraction.
            let cone = cones
                .iter()
                .find(|c| c.result_event == e.id && equality.contains(&e.id));
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
                age,
                output: v,
                operands: cone.map_or_else(|| e.inputs.clone(), |c| c.operands.clone()),
                op,
            });
            if fused.contains_key(&e.id) {
                combinational.insert(v, (e.id, age));
            }
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
            combinational,
            bits,
            span,
            period,
            ii,
            initial_phase: 0,
            values,
            instructions,
            inputs,
            outputs,
            fields,
            roms: if frame.name == "texture_lod_context" {
                [
                    constant_codes(&crate::texture::format::LOG)?,
                    if frame.memories.iter().any(|m| m.name == "mip_prefix") {
                        constant_codes(&crate::texture::format::PREFIX)?
                    } else {
                        vec![]
                    },
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
                if self
                    .combinational
                    .get(&root)
                    .is_some_and(|(_, a)| *a == age)
                {
                    continue;
                }
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
    /// Choose a fixed reset phase for composition. Every live iteration still
    /// uses the same calendar and advances only on CE; no in-flight rephasing.
    pub fn with_issue_offset(mut self, offset: u32) -> Result<Self, String> {
        if offset >= self.ii {
            return Err("calendar issue offset".into());
        }
        self.initial_phase = (self.period - offset) % self.period;
        Ok(self)
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
            phase: 1 << calendar.initial_phase,
            calendar,
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
    /// Read a held input-derived output from its actual physical slices. A
    /// closed caller owns the lease until its last use and must prevent a new
    /// acceptance. No valid-token extension, phase change or shadow is added.
    fn retained_input_output(&self, name: &str, width: usize) -> Result<i128, String> {
        let c = &self.calendar;
        if c.period != c.ii {
            return Err("retained output requires one physical input bank".into());
        }
        let value = c
            .outputs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .ok_or("retained output missing Calendar port")?;
        if c.values[value].view.len() != width {
            return Err("retained output width".into());
        }
        for bit in &c.values[value].view {
            let Bit::Root(root, source_bit) = *bit else {
                return Err("retained output must directly borrow an input slice".into());
            };
            let field = c
                .fields
                .iter()
                .find(|f| {
                    f.value == root
                        && source_bit >= f.source_low
                        && source_bit < f.source_low + f.width
                        && f.birth == 0
                        && f.last_read >= c.span
                        && f.lows.len() == 1
                })
                .ok_or("retained output has no stable input field")?;
            if !c.inputs.iter().any(|i| i.value == root) {
                return Err("retained output is not an acceptance-only input".into());
            }
            if c.instructions.iter().any(|i| i.output == root) {
                return Err("retained input root can be overwritten by an instruction".into());
            }
            let physical = field.lows[0] + (source_bit - field.source_low) as usize;
            if c.fields.iter().any(|f| {
                f.lows.iter().any(|&low| {
                    physical >= low
                        && physical < low + f.width as usize
                        && (f.birth != 0
                            || f.value != root
                            || f.source_low + (physical - low) as u32 != source_bit)
                })
            }) {
                return Err("retained output slice can be overwritten by another field".into());
            }
        }
        // The audit above proves phase rotation cannot select a different row
        // and all writes to these slices are acceptance-only. Read actual bits
        // using the certified output view even after its valid token expires.
        self.read(value, c.span, 0, &[])
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
        let mut intermediate = Vec::<(usize, u8, i128)>::new();
        for i in &self.calendar.instructions {
            if active & (1 << i.age) == 0 {
                continue;
            }
            let iteration = self.iteration(i.age);
            let mut wires = if i.age == 0 { external.clone() } else { vec![] };
            wires.extend(
                intermediate
                    .iter()
                    .filter(|(_, age, _)| *age == i.age)
                    .map(|(v, _, raw)| (*v, *raw)),
            );
            let operands = i
                .operands
                .iter()
                .map(|v| self.read(*v, i.age, iteration, &wires))
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
            if self.calendar.combinational.contains_key(&i.output) {
                intermediate.push((i.output, i.age, raw));
            }
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
// Constructor classification is structural: sampled nonliteral values may be
// deliberately poisoned. The numerical frame auditor is not an executor input.
fn equality_recipe(f: &FrameReport, c: &LogicCone) -> bool {
    if c.absorbed_events.len() != 3 || !c.exported_events.is_empty() {
        return false;
    }
    let Some(root) = f.events.get(c.result_event) else {
        return false;
    };
    if root.operation != Operation::Less || root.inputs.len() != 2 {
        return false;
    }
    let threshold = &f.values[root.inputs[1]];
    if f.events[threshold.producer].operation != Operation::Literal || threshold.raw != 1 {
        return false;
    }
    let sum = &f.events[f.values[root.inputs[0]].producer];
    if sum.operation != Operation::Add || sum.inputs.len() != 2 {
        return false;
    }
    let a = &f.events[f.values[sum.inputs[0]].producer];
    let b = &f.events[f.values[sum.inputs[1]].producer];
    let same = |x: usize, y: usize| {
        x == y
            || (f.events[f.values[x].producer].operation == Operation::Literal
                && f.events[f.values[y].producer].operation == Operation::Literal
                && f.values[x].raw == f.values[y].raw
                && f.values[x].format == f.values[y].format)
    };
    a.operation == Operation::Less
        && b.operation == Operation::Less
        && a.inputs.len() == 2
        && b.inputs.len() == 2
        && same(a.inputs[0], b.inputs[1])
        && same(a.inputs[1], b.inputs[0])
        && [a.id, b.id, sum.id]
            .iter()
            .all(|id| c.absorbed_events.contains(id))
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
            return Err(format!(
                "D allocation mismatch: bits={} span={}",
                calendar.bits, calendar.span
            ));
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
    /// Runtime's D-to-LOD lease reads actual terminal bank slices even after
    /// the output token expires. The caller must finish reads before the next
    /// enabled D acceptance; no sampled payload or extended valid is retained.
    pub(crate) fn runtime_lod(&self) -> Result<super::lod::Input, String> {
        let c = &self.registers.calendar;
        if c.period != 8
            || c.ii != 8
            || c.span != 3
            || c.fields.iter().any(|f| f.lows.len() != 1)
            || c.instructions.iter().any(|i| i.age >= c.span)
        {
            return Err("Runtime D loan requires one II8 terminal bank".into());
        }
        let read = |name: &str| {
            let v = c
                .outputs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| *v)
                .ok_or("Runtime D loan output")?;
            self.registers.read(v, c.span, 0, &[])
        };
        Ok(super::lod::Input {
            slope: read("slope")? as u64,
            bias: read("bias")? as i16,
            header: Header {
                quad: read("quad")? as u8,
                mask: read("mask")? as u8,
                slot: read("slot")? as u8,
                max_n: read("max_n")? as u8,
                has_mip: read("has_mip")? != 0,
                filter: read("filter")? as u8,
            },
        })
    }
    #[cfg(test)]
    pub(crate) fn corrupt_runtime_lod_quad(&mut self) {
        let c = &self.registers.calendar;
        let value = c.outputs.iter().find(|(n, _)| n == "quad").unwrap().1;
        let Bit::Root(root, bit) = c.values[value].view[0] else {
            panic!("quad root")
        };
        let f = c
            .fields
            .iter()
            .find(|f| {
                f.value == root
                    && f.source_low <= bit
                    && bit < f.source_low + f.width
                    && f.birth <= c.span
                    && c.span <= f.last_read
            })
            .unwrap();
        let at = f.lows[0] + (bit - f.source_low) as usize;
        self.registers.bank[at / 64] ^= 1u64 << (at % 64);
    }
    /// Closed Serial's Coordinate lease only. UV remains in D's original bank
    /// until a later D admission; the caller retires all Coordinate reads first.
    /// Returns combinational operands, not another retained eight-value row.
    pub(crate) fn serial_uv(&self, lane: u8) -> Result<[u32; 2], String> {
        if lane >= 4 {
            return Err("Serial borrowed UV lane".into());
        }
        let read = |axis| {
            self.registers
                .retained_input_output(&format!("uv{}", usize::from(lane) * 2 + axis), 16)
                .map(|v| v as u32)
        };
        Ok([read(0)?, read(1)?])
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

#[cfg(test)]
mod retained_uv_tests {
    use super::*;
    #[test]
    fn serial_borrow_reads_actual_bank_after_valid_expires_and_before_next_admission() {
        let (calendar, _, _) =
            crate::texture::sim::staged::bound::serial::structural_calendars().unwrap();
        let mut d = DerivativeEmu::new(calendar.clone()).unwrap();
        let mut input = Input {
            uv: [-131072, -1, 0, 1, 65535, 65536, 131070, 131071],
            bias: 0,
            force_coarsest: false,
            header: Header {
                quad: 15,
                mask: 15,
                slot: 0,
                max_n: 10,
                has_mip: true,
                filter: 2,
            },
        };
        for reuse in 0..3 {
            for _ in 0..8 {
                if d.phase() == 0 {
                    break;
                }
                d.tick(true, None).unwrap();
            }
            assert!(d.tick(true, Some(input)).unwrap().accepted);
            let expected = input.uv.map(|v| v as u32 & 65535);
            let mut saw_output = false;
            let mut saw_expired = false;
            for wall in 0..80 {
                for lane in 0..4 {
                    assert_eq!(
                        d.serial_uv(lane).unwrap(),
                        [
                            expected[usize::from(lane) * 2],
                            expected[usize::from(lane) * 2 + 1]
                        ]
                    );
                }
                saw_output |= d.output().unwrap().is_some();
                saw_expired |= saw_output && d.output().unwrap().is_none();
                let before = d.bank().to_vec();
                let ce = wall % 5 != 1;
                d.tick(ce, None).unwrap();
                if !ce {
                    assert_eq!(d.bank(), before);
                }
            }
            assert!(
                saw_output && saw_expired,
                "lease spans D validity and phase rotation"
            );
            input.uv = input.uv.map(|v| (v + 101 + reuse).clamp(-131072, 131071));
        }
        // Perturb the certified UV root itself. A shadow or answer replay
        // would miss this change; the borrowed physical port must observe it.
        let old = d.serial_uv(0).unwrap();
        let c = &d.registers.calendar;
        let value = c.outputs.iter().find(|(n, _)| n == "uv0").unwrap().1;
        let Bit::Root(root, source_bit) = c.values[value].view[0] else {
            panic!("UV must be a root input");
        };
        let f = c
            .fields
            .iter()
            .find(|f| {
                f.value == root && source_bit >= f.source_low && source_bit < f.source_low + f.width
            })
            .unwrap();
        let bit = f.lows[0] + (source_bit - f.source_low) as usize;
        d.registers.bank[bit / 64] ^= 1 << (bit % 64);
        assert_eq!(d.serial_uv(0).unwrap(), [old[0] ^ 1, old[1]]);
        assert!(d.serial_uv(4).is_err());
        // Reject rotating banks and a future overlapping arithmetic allocation.
        let mut rotating = calendar.clone();
        rotating.ii = 4;
        assert!(DerivativeEmu::new(rotating).unwrap().serial_uv(0).is_err());
        let mut overwritten = calendar.clone();
        overwritten.instructions[0].output = root;
        assert!(DerivativeEmu::new(overwritten)
            .unwrap()
            .serial_uv(0)
            .is_err());
        let mut overlapping = calendar;
        let other = overlapping.fields.iter_mut().find(|f| f.birth > 0).unwrap();
        other.lows[0] = bit;
        assert!(DerivativeEmu::new(overlapping)
            .unwrap()
            .serial_uv(0)
            .is_err());
    }
}
