//! Generic synthesizable RTL lowering for the immutable numerical calendars.
//!
//! This translates only the certified `Calendar` structure (packed `Field`
//! slices, `Instruction` ages/ops, literal ROM contents) into a one-hot phase
//! period, a `span+1` valid shift and an exact `numeric_bits` packed FF bank.
//! No sampled answer, per-input `prepare`, `FrameReport` raw value or embedded
//! replay is emitted: a poisoned constructor must produce byte-identical RTL.
//!
//! The physical scalar resource/mux topology belongs to the binding. This is a
//! numerical-kernel baseline, not a complete sampler/cache or a fitted result.
//! ROM tables lower to a literal `case`, a LUT case-ROM with one registered
//! destination edge, not a physical BSRAM claim.
use super::{Bit, Calendar, Op};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// One generated port. `name` is the Verilog signal; `key`/`row` recover the
/// calendar input it encodes (outputs carry the frozen output name).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Io {
    pub name: String,
    pub key: String,
    pub row: usize,
    pub width: u32,
    pub signed: bool,
}

/// Exact declared kernel inventory: numeric/control storage and ROM size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    pub numeric_bits: usize,
    pub span: u8,
    pub period: u32,
    pub ii: u32,
    pub phase_ff: u32,
    pub valid_ff: u32,
    pub control_ff: u32,
    /// Logical operand/output bit selects per enabled edge (binding owns muxes).
    pub read_selector_bits: u64,
    pub instructions: usize,
    pub fields: usize,
    pub rom_entries: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub module: String,
    pub inputs: Vec<Io>,
    pub outputs: Vec<Io>,
    pub inventory: Inventory,
    /// When set, the module exposes the whole packed `numeric_bits` FF bank.
    pub debug_bank: bool,
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn input_signal(key: &str, row: usize) -> String {
    format!("in_{}_{}", sanitize(key), row)
}

fn output_signal(name: &str) -> String {
    format!("out_{}", sanitize(name))
}

/// The generated `ito` cohort rotation for one age at one one-hot phase index.
fn iteration(period: u32, ii: u32, age: u8, phase: u32) -> usize {
    usize::try_from(((phase + period - (u32::from(age) % period)) % period) / ii).unwrap()
}

fn input_ports(cal: &Calendar) -> Vec<Io> {
    cal.inputs
        .iter()
        .filter(|i| i.name != "base")
        .map(|i| {
            let f = cal.values[i.value].format;
            Io {
                name: input_signal(&i.name, i.row),
                key: i.name.clone(),
                row: i.row,
                width: f.bits,
                signed: f.signed,
            }
        })
        .collect()
}

fn output_ports(cal: &Calendar) -> Vec<Io> {
    cal.outputs
        .iter()
        .map(|(name, v)| {
            let f = cal.values[*v].format;
            Io {
                name: output_signal(name),
                key: name.clone(),
                row: 0,
                width: f.bits,
                signed: f.signed,
            }
        })
        .collect()
}

fn inventory(cal: &Calendar) -> Inventory {
    let mut read_selector_bits = 0_u64;
    for i in &cal.instructions {
        for &operand in &i.operands {
            read_selector_bits += u64::from(cal.values[operand].format.bits);
        }
    }
    for (_, v) in &cal.outputs {
        read_selector_bits += u64::from(cal.values[*v].format.bits);
    }
    Inventory {
        numeric_bits: cal.bits,
        span: cal.span,
        period: cal.period,
        ii: cal.ii,
        phase_ff: cal.period,
        valid_ff: u32::from(cal.span) + 1,
        control_ff: cal.period + u32::from(cal.span) + 1,
        read_selector_bits,
        instructions: cal.instructions.len(),
        fields: cal.fields.len(),
        rom_entries: cal.roms[0].len() + cal.roms[1].len(),
    }
}

pub fn describe(cal: &Calendar, name: &str) -> Descriptor {
    describe_with(cal, name, false)
}

pub fn describe_debug(cal: &Calendar, name: &str) -> Descriptor {
    describe_with(cal, name, true)
}

pub fn describe_with(cal: &Calendar, name: &str, debug_bank: bool) -> Descriptor {
    Descriptor {
        module: sanitize(name),
        inputs: input_ports(cal),
        outputs: output_ports(cal),
        inventory: inventory(cal),
        debug_bank,
    }
}

struct Ctx<'a> {
    cal: &'a Calendar,
    root_ports: BTreeMap<usize, String>,
}

impl Ctx<'_> {
    fn bank_bit(&self, root: usize, sb: u32, age: u8, it: usize) -> Result<String, String> {
        for f in &self.cal.fields {
            if f.value == root
                && sb >= f.source_low
                && sb < f.source_low + f.width
                && f.birth <= age
                && age <= f.last_read
            {
                let low = f
                    .lows
                    .get(it)
                    .ok_or_else(|| format!("field v{root} missing iteration {it}"))?
                    + (sb - f.source_low) as usize;
                return Ok(format!("bank[{low}]"));
            }
        }
        Err(format!("unallocated read value{root} bit{sb} age{age}"))
    }

    fn value_bits(&self, v: usize, age: u8, it: usize) -> Result<Vec<String>, String> {
        let fmt = self.cal.values[v].format;
        let view = &self.cal.values[v].view;
        let mut bits = Vec::with_capacity(fmt.bits as usize);
        for b in 0..fmt.bits as usize {
            let bit = view
                .get(b)
                .ok_or_else(|| format!("value {v} view short at bit {b}"))?;
            let expr = match *bit {
                Bit::Constant(c) => {
                    if c {
                        "1'b1".to_string()
                    } else {
                        "1'b0".to_string()
                    }
                }
                Bit::Root(root, sb) => {
                    if age == 0 {
                        if let Some(sig) = self.root_ports.get(&root) {
                            let total = self.cal.values[root].format.bits;
                            part_select(sig, sb, 1, total)
                        } else {
                            self.bank_bit(root, sb, age, it)?
                        }
                    } else {
                        self.bank_bit(root, sb, age, it)?
                    }
                }
            };
            bits.push(expr);
        }
        Ok(bits)
    }
}

fn concat(bits: &[String]) -> String {
    if bits.len() == 1 {
        bits[0].clone()
    } else {
        let msb_first: Vec<&str> = bits.iter().rev().map(String::as_str).collect();
        format!("{{ {} }}", msb_first.join(", "))
    }
}

/// A width-exact slice; a scalar signal cannot be part-selected.
fn part_select(expr: &str, low: u32, width: u32, total: u32) -> String {
    if total == 1 {
        expr.to_string()
    } else {
        format!("{expr}[{low} +: {width}]")
    }
}

fn extend(expr: &str, from: u32, to: u32, signed: bool) -> String {
    if to <= from {
        return format!("{expr}[{}:0]", to - 1);
    }
    let pad = to - from;
    let fill = if signed {
        format!("{expr}[{}]", from - 1)
    } else {
        "1'b0".to_string()
    };
    format!("{{ {{{pad}{{{fill}}}}}, {expr} }}")
}

/// Half of a `n`-bit `RoundIncrement` remainder, as a width-exact literal.
fn half_literal(n: u32) -> String {
    if n <= 1 {
        "1'b1".to_string()
    } else {
        format!("{{ 1'b1, {}'d0 }}", n - 1)
    }
}

fn emit_arithmetic(ctx: &Ctx<'_>, decls: &mut String, body: &mut String) -> Result<(), String> {
    for i in &ctx.cal.instructions {
        let ev = i.event;
        let out_bits = ctx.cal.values[i.output].format.bits;
        match &i.op {
            Op::Add | Op::Sub => {
                let a = ctx.cal.values[i.operands[0]].format;
                let b = ctx.cal.values[i.operands[1]].format;
                let w = (a.bits.max(b.bits) + 1).max(out_bits);
                let e0 = extend(&format!("op0_e{ev}"), a.bits, w, a.signed);
                let e1 = extend(&format!("op1_e{ev}"), b.bits, w, b.signed);
                writeln!(decls, "  wire signed [{}:0] e0_e{ev} = {e0};", w - 1).unwrap();
                writeln!(decls, "  wire signed [{}:0] e1_e{ev} = {e1};", w - 1).unwrap();
                let sym = if matches!(&i.op, Op::Add) { "+" } else { "-" };
                writeln!(body, "  always @(*) r_e{ev} = e0_e{ev} {sym} e1_e{ev};").unwrap();
            }
            Op::Less | Op::Equal => {
                let a = ctx.cal.values[i.operands[0]].format;
                let b = ctx.cal.values[i.operands[1]].format;
                let w = a.bits.max(b.bits) + 1;
                let e0 = extend(&format!("op0_e{ev}"), a.bits, w, a.signed);
                let e1 = extend(&format!("op1_e{ev}"), b.bits, w, b.signed);
                writeln!(decls, "  wire signed [{}:0] e0_e{ev} = {e0};", w - 1).unwrap();
                writeln!(decls, "  wire signed [{}:0] e1_e{ev} = {e1};", w - 1).unwrap();
                let sym = if matches!(&i.op, Op::Less) { "<" } else { "==" };
                writeln!(body, "  always @(*) r_e{ev} = (e0_e{ev} {sym} e1_e{ev});").unwrap();
            }
            Op::Select => {
                writeln!(
                    body,
                    "  always @(*) r_e{ev} = op0_e{ev} ? op1_e{ev} : op2_e{ev};"
                )
                .unwrap();
            }
            Op::LeadingZeros => {
                let n = ctx.cal.values[i.operands[0]].format.bits;
                writeln!(
                    decls,
                    "  function [{}:0] lz_e{ev};\n    input [{}:0] a;\n    integer i;\n    begin\n      lz_e{ev} = {};\n      for (i=0;i<{};i=i+1) if (a[{} - i] && lz_e{ev} == {}) lz_e{ev} = i;\n    end\n  endfunction",
                    out_bits - 1,
                    n - 1,
                    n,
                    n,
                    n - 1,
                    n
                )
                .unwrap();
                writeln!(body, "  always @(*) r_e{ev} = lz_e{ev}(op0_e{ev});").unwrap();
            }
            Op::Shift => {
                let src = ctx.cal.values[i.operands[0]].format;
                let amt = ctx.cal.values[i.operands[1]].format.bits;
                writeln!(
                    decls,
                    "  wire [{}:0] mag_e{ev} = {{ {}'d0 }} - op1_e{ev};",
                    amt - 1,
                    amt
                )
                .unwrap();
                let right = if src.signed {
                    format!("$signed(op0_e{ev}) >>> mag_e{ev}")
                } else {
                    format!("op0_e{ev} >> mag_e{ev}")
                };
                writeln!(
                    body,
                    "  always @(*) begin\n    if (op1_e{ev}[{}]) r_e{ev} = {right};\n    else r_e{ev} = op0_e{ev} << op1_e{ev};\n  end",
                    amt - 1
                )
                .unwrap();
            }
            Op::Round(n) => {
                let n = *n;
                let bits = ctx.cal.values[i.operands[0]].format.bits;
                let w = bits.max(n + 1);
                let e = extend(
                    &format!("op0_e{ev}"),
                    bits,
                    w,
                    ctx.cal.values[i.operands[0]].format.signed,
                );
                let half = half_literal(n);
                writeln!(decls, "  wire [{}:0] rx_e{ev} = {e};", w - 1).unwrap();
                writeln!(
                    body,
                    "  always @(*) r_e{ev} = (rx_e{ev}[{}:0] > {half}) || (rx_e{ev}[{}:0] == {half} && rx_e{ev}[{n}]);",
                    n - 1,
                    n - 1
                )
                .unwrap();
            }
            Op::Rom(which) => {
                let table = ctx
                    .cal
                    .roms
                    .get(*which)
                    .ok_or_else(|| "ROM descriptor missing".to_string())?;
                let mut s = format!("  always @(*) begin\n    case (op0_e{ev})\n");
                for (idx, code) in table.iter().enumerate() {
                    writeln!(s, "      {idx}: r_e{ev} = {out_bits}'d{code};").unwrap();
                }
                writeln!(
                    s,
                    "      default: r_e{ev} = {out_bits}'d0;\n    endcase\n  end"
                )
                .unwrap();
                body.push_str(&s);
            }
        }
    }
    Ok(())
}

/// Collect the physical write ranges that can occur at one phase index.
fn phase_writes(ctx: &Ctx<'_>, phase: u32) -> Result<Vec<(usize, u32, String)>, String> {
    let cal = ctx.cal;
    let mut writes: Vec<(usize, u32, String)> = Vec::new();
    if phase.is_multiple_of(cal.ii) {
        let it0 = iteration(cal.period, cal.ii, 0, phase);
        for input in &cal.inputs {
            if input.name == "base" {
                continue;
            }
            for f in &cal.fields {
                if f.value == input.value && f.birth == 0 && f.last_read > 0 {
                    let low = f.lows[it0];
                    writes.push((low, f.width, format!("input {}", input.name)));
                }
            }
        }
    }
    for i in &cal.instructions {
        if u32::from(i.age) % cal.ii != phase % cal.ii {
            continue;
        }
        let it = iteration(cal.period, cal.ii, i.age, phase);
        for f in &cal.fields {
            if f.value == i.output && f.birth == i.age + 1 {
                writes.push((f.lows[it], f.width, format!("event {}", i.event)));
            }
        }
    }
    Ok(writes)
}

fn audit_writes(ctx: &Ctx<'_>) -> Result<(), String> {
    for phase in 0..ctx.cal.period {
        let mut used = vec![false; ctx.cal.bits];
        for (low, width, tag) in phase_writes(ctx, phase)? {
            for bit in 0..width as usize {
                let at = low + bit;
                if at >= ctx.cal.bits || std::mem::replace(&mut used[at], true) {
                    return Err(format!("phase {phase} write collision at bit {at} ({tag})"));
                }
            }
        }
    }
    Ok(())
}

/// Structural invariants the emitter relies on; independent of any input.
pub fn audit(cal: &Calendar) -> Result<(), String> {
    if cal.period == 0 || cal.ii == 0 || !cal.period.is_multiple_of(cal.ii) {
        return Err("calendar phase period".into());
    }
    if cal.span == 0 || cal.span >= 32 {
        return Err("calendar span".into());
    }
    let ctx = Ctx {
        cal,
        root_ports: cal
            .inputs
            .iter()
            .filter(|i| i.name != "base")
            .map(|i| (i.value, input_signal(&i.name, i.row)))
            .collect(),
    };
    for i in &cal.instructions {
        for &operand in &i.operands {
            ctx.value_bits(operand, i.age, 0)?;
        }
    }
    for (_, v) in &cal.outputs {
        ctx.value_bits(*v, cal.span, 0)?;
    }
    audit_writes(&ctx)
}

/// Emit one synthesizable numerical-kernel module. Panics only on a malformed
/// calendar, which [`audit`] reports cleanly.
pub fn emit(cal: &Calendar, name: &str) -> String {
    audit(cal).unwrap_or_else(|e| panic!("calendar RTL lowering: {e}"));
    emit_checked(cal, name, false).unwrap_or_else(|e| panic!("calendar RTL emission: {e}"))
}

/// Emit the same module plus a whole packed FF bank observation port.
pub fn emit_debug(cal: &Calendar, name: &str) -> String {
    audit(cal).unwrap_or_else(|e| panic!("calendar RTL lowering: {e}"));
    emit_checked(cal, name, true).unwrap_or_else(|e| panic!("calendar RTL emission: {e}"))
}

fn emit_checked(cal: &Calendar, name: &str, debug_bank: bool) -> Result<String, String> {
    let ctx = Ctx {
        cal,
        root_ports: cal
            .inputs
            .iter()
            .filter(|i| i.name != "base")
            .map(|i| (i.value, input_signal(&i.name, i.row)))
            .collect(),
    };
    let inputs = input_ports(cal);
    let outputs = output_ports(cal);
    let mut decls = String::new();
    let mut body = String::new();
    emit_arithmetic(&ctx, &mut decls, &mut body)?;

    let mut header = String::new();
    writeln!(header, "module {} (", sanitize(name)).unwrap();
    for line in [
        "  input clk,".to_string(),
        "  input ce,".to_string(),
        "  input reset,".to_string(),
        "  input in_valid,".to_string(),
        "  output in_ready,".to_string(),
        "  output fault,".to_string(),
        "  output out_valid,".to_string(),
    ] {
        writeln!(header, "{line}").unwrap();
    }
    let mut ports: Vec<String> = Vec::new();
    for p in &inputs {
        let sign = if p.signed { "signed " } else { "" };
        let range = if p.width == 1 {
            String::new()
        } else {
            format!("[{}:0] ", p.width - 1)
        };
        ports.push(format!("  input {sign}{range}{}", p.name));
    }
    for p in &outputs {
        let sign = if p.signed { "signed " } else { "" };
        let range = if p.width == 1 {
            String::new()
        } else {
            format!("[{}:0] ", p.width - 1)
        };
        ports.push(format!("  output reg {sign}{range}{}", p.name));
    }
    if debug_bank {
        ports.push(format!("  output [{}:0] dbg_bank", cal.bits - 1));
        ports.push(format!("  output [{}:0] dbg_phase", cal.period - 1));
        ports.push(format!("  output [{}:0] dbg_valid", cal.span));
    }
    header.push_str(&ports.join(",\n"));
    header.push_str("\n);\n");

    let mut s = header;
    writeln!(s, "  reg [{}:0] bank;", cal.bits - 1).unwrap();
    writeln!(s, "  reg [{}:0] phase;", cal.period - 1).unwrap();
    writeln!(s, "  reg [{}:0] valid;", cal.span).unwrap();
    if debug_bank {
        writeln!(s, "  assign dbg_bank = bank;").unwrap();
        writeln!(s, "  assign dbg_phase = phase;").unwrap();
        writeln!(s, "  assign dbg_valid = valid;").unwrap();
    }
    for i in &cal.instructions {
        let out_bits = cal.values[i.output].format.bits;
        let range = if out_bits == 1 {
            String::new()
        } else {
            format!("[{}:0] ", out_bits - 1)
        };
        writeln!(s, "  reg {range}r_e{};", i.event).unwrap();
        for (k, &operand) in i.operands.iter().enumerate() {
            let bits = cal.values[operand].format.bits;
            let range = if bits == 1 {
                String::new()
            } else {
                format!("[{}:0] ", bits - 1)
            };
            writeln!(s, "  reg {range}op{k}_e{};", i.event).unwrap();
        }
    }
    s.push_str(&decls);

    let zero_pad = format!("{{{} {{1'b0}}}}", cal.span);
    let mask = period_mask(cal.period, cal.ii);
    writeln!(
        s,
        "  assign in_ready = |(phase & {}'h{:x});",
        cal.period, mask
    )
    .unwrap();
    writeln!(s, "  wire accepted = ce & in_valid & in_ready;").unwrap();
    writeln!(s, "  assign fault = ce & in_valid & ~in_ready;").unwrap();
    writeln!(
        s,
        "  wire [{}:0] active = valid | {{ {}, accepted }};",
        cal.span, zero_pad
    )
    .unwrap();
    writeln!(s, "  assign out_valid = valid[{}];", cal.span).unwrap();
    writeln!(
        s,
        "  initial begin\n    bank = 0;\n    phase = 1;\n    valid = 0;\n  end"
    )
    .unwrap();

    // Combinational cohort reads. One branch per certified one-hot phase.
    let iterations = cal.period / cal.ii;
    if iterations == 1 {
        s.push_str("  always @(*) begin\n");
        emit_compute(&ctx, &mut s, None)?;
        s.push_str("  end\n");
    } else {
        s.push_str("  always @(*) begin\n    case (phase)\n");
        for p in 0..cal.period {
            writeln!(s, "      {}'h{:x}: begin", cal.period, 1_u64 << p).unwrap();
            emit_compute(&ctx, &mut s, Some(p))?;
            s.push_str("      end\n");
        }
        s.push_str("      default: begin\n");
        emit_compute_defaults(&ctx, &mut s);
        s.push_str("      end\n    endcase\n  end\n");
    }
    s.push_str(&body);

    // Clocked bank writes, destination cohorts selected by the one-hot phase.
    s.push_str("  always @(posedge clk) begin\n    if (reset) begin\n      bank <= 0;\n      phase <= 1;\n      valid <= 0;\n    end else if (ce) begin\n");
    if iterations == 1 {
        emit_writes(&ctx, &mut s, None)?;
    } else {
        s.push_str("      case (phase)\n");
        for p in 0..cal.period {
            writeln!(s, "        {}'h{:x}: begin", cal.period, 1_u64 << p).unwrap();
            emit_writes(&ctx, &mut s, Some(p))?;
            s.push_str("        end\n");
        }
        s.push_str("        default: ;\n      endcase\n");
    }
    writeln!(s, "      valid <= {{ active[{}:0], 1'b0 }};", cal.span - 1).unwrap();
    writeln!(
        s,
        "      phase <= phase[{}] ? 1 : (phase << 1);",
        cal.period - 1
    )
    .unwrap();
    s.push_str("    end\n  end\nendmodule\n");
    Ok(s)
}

fn period_mask(period: u32, ii: u32) -> u128 {
    let mut mask = 0_u128;
    for p in 0..period {
        if p.is_multiple_of(ii) {
            mask |= 1 << p;
        }
    }
    mask
}

fn emit_compute(ctx: &Ctx<'_>, s: &mut String, phase: Option<u32>) -> Result<(), String> {
    for i in &ctx.cal.instructions {
        for (k, &operand) in i.operands.iter().enumerate() {
            let it = match phase {
                Some(p) => iteration(ctx.cal.period, ctx.cal.ii, i.age, p),
                None => 0,
            };
            let bits = ctx.value_bits(operand, i.age, it)?;
            writeln!(s, "        op{k}_e{} = {};", i.event, concat(&bits)).unwrap();
        }
    }
    for (name, v) in &ctx.cal.outputs {
        let it = match phase {
            Some(p) => iteration(ctx.cal.period, ctx.cal.ii, ctx.cal.span, p),
            None => 0,
        };
        let bits = ctx.value_bits(*v, ctx.cal.span, it)?;
        writeln!(s, "        {} = {};", output_signal(name), concat(&bits)).unwrap();
    }
    Ok(())
}

fn emit_compute_defaults(ctx: &Ctx<'_>, s: &mut String) {
    for i in &ctx.cal.instructions {
        for (k, _) in i.operands.iter().enumerate() {
            writeln!(s, "        op{k}_e{} = 0;", i.event).unwrap();
        }
    }
    for (name, _) in &ctx.cal.outputs {
        writeln!(s, "        {} = 0;", output_signal(name)).unwrap();
    }
}

fn emit_writes(ctx: &Ctx<'_>, s: &mut String, phase: Option<u32>) -> Result<(), String> {
    let cal = ctx.cal;
    let it_at = |age: u8| match phase {
        Some(p) => iteration(cal.period, cal.ii, age, p),
        None => 0,
    };
    s.push_str("      if (accepted) begin\n");
    for input in &cal.inputs {
        if input.name == "base" {
            continue;
        }
        let sig = input_signal(&input.name, input.row);
        let total = cal.values[input.value].format.bits;
        for f in &cal.fields {
            if f.value == input.value && f.birth == 0 && f.last_read > 0 {
                writeln!(
                    s,
                    "        bank[{} +: {}] <= {};",
                    f.lows[it_at(0)],
                    f.width,
                    part_select(&sig, f.source_low, f.width, total)
                )
                .unwrap();
            }
        }
    }
    s.push_str("      end\n");
    for i in &cal.instructions {
        let total = cal.values[i.output].format.bits;
        writeln!(s, "      if (active[{}]) begin", i.age).unwrap();
        for f in &cal.fields {
            if f.value == i.output && f.birth == i.age + 1 {
                writeln!(
                    s,
                    "        bank[{} +: {}] <= {};",
                    f.lows[it_at(i.age)],
                    f.width,
                    part_select(&format!("r_e{}", i.event), f.source_low, f.width, total)
                )
                .unwrap();
            }
        }
        s.push_str("      end\n");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::half_literal;

    /// The RNE half for `RoundIncrement(n)` is `1 << (n - 1)`: a one followed by
    /// `n - 1` zero bits. The previous zero-padding-then-one encoding lowered the
    /// half to the value one; this locks the width-exact literal against it.
    #[test]
    fn half_literal_is_one_followed_by_zeroes() {
        assert_eq!(half_literal(1), "1'b1");
        assert_eq!(half_literal(2), "{ 1'b1, 1'd0 }");
        assert_eq!(half_literal(3), "{ 1'b1, 2'd0 }");
        assert_eq!(half_literal(13), "{ 1'b1, 12'd0 }");
        assert_eq!(half_literal(19), "{ 1'b1, 18'd0 }");
    }
}
