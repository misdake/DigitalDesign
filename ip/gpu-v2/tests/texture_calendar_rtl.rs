//! Icarus co-simulation of the generic numerical-kernel RTL emitter.
//!
//! Each calendar is built only through the current `Binding`/`prepare`
//! constructor. The emitted Verilog is driven with raw live input rows on real
//! clock edges and compared with the independent Rust register executor
//! (`DerivativeEmu`/`LodEmu`/`CoordinateEmu`) and the frozen semantic goldens.
//! The same constructor is poisoned to prove no sampled raw/output leaks into
//! the emitted source. Bounded Icarus compile/run are wall-watchdogged.
use audited::FrameReport;
use gpu_v2::texture::emu::{
    coordinate::{self, CoordinateEmu, Input as CoordInput},
    derivative::{self, rtl, Calendar, DerivativeEmu, Field},
    lod::{self, LodEmu},
};
use gpu_v2::texture::{ports::*, sim::staged::bound};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const D_OUTPUTS: &[&str] = &[
    "uv0", "uv1", "uv2", "uv3", "uv4", "uv5", "uv6", "uv7", "slope", "bias", "quad", "mask",
    "slot", "max_n", "has_mip", "filter",
];
const L_OUTPUTS: &[&str] = &[
    "shift0",
    "nearest",
    "halve",
    "side0",
    "side1",
    "parent0",
    "parent1",
    "n0",
    "n1",
    "last_fine",
    "quad",
    "mask",
    "slot",
];

fn calendar(
    plan: &bound::StagePlan,
    frame: &FrameReport,
    outputs: &[&str],
    poison: bool,
) -> Calendar {
    let evidence = gpu_v2::texture::sim::staged::binding::Evidence::build(frame).unwrap();
    let mut cones = evidence.lowering.logic_cones(frame, 1).unwrap();
    if frame.name == "texture_lod_context" {
        let shared = gpu_v2::texture::sim::staged::binding::lod_shared_h_cone(frame).unwrap();
        for id in [shared.absorbed_events[0], shared.result_event] {
            cones.push(audited::physical::LogicCone::singleton(frame, id, 1).unwrap());
        }
    }
    let fields = plan
        .packed_fields
        .iter()
        .map(|f| Field {
            value: f.value,
            source_low: f.source_low,
            width: f.width,
            birth: f.birth as u8,
            last_read: f.last_read as u8,
            lows: (0..plan.packed.period / plan.ii())
                .map(|i| {
                    plan.packed
                        .placements
                        .iter()
                        .find(|p| {
                            p.value == f.value && p.source_low == f.source_low && p.iteration == i
                        })
                        .unwrap()
                        .low
                })
                .collect(),
        })
        .collect();
    let mut frame = frame.clone();
    if poison {
        for value in &mut frame.values {
            if frame.events[value.producer].operation != audited::Operation::Literal {
                value.raw ^= 12345;
            }
        }
        for value in &mut frame.outputs {
            value.raw ^= 12345;
        }
    }
    Calendar::from_structure(
        &frame,
        &plan.times,
        &cones,
        &evidence.lowering.wiring_adds,
        fields,
        plan.packed.ff_bits,
        plan.span() as u8,
        plan.packed.period as u32,
        plan.ii() as u32,
        outputs,
    )
    .unwrap()
}

fn canonical() -> bound::Preparation {
    let mut q = QuadInput {
        quad_id: 0,
        mask: 1,
        uv: [[0.003, 0.003]; 4],
        slot: 0,
        material_size_log2: 9,
        filter: Filter::Trilinear,
        lod_bias: 0.5,
    };
    q.uv[1][0] += 1.0 / 512.0;
    bound::prepare(
        &q,
        &[Slot {
            base_address: 4096,
            max_size_log2: 9,
            has_full_mip: true,
            valid: true,
        }],
    )
    .unwrap()
}

fn coordinate_calendar(poison: bool) -> Calendar {
    let p = canonical();
    let b = bound::Binding::build().unwrap();
    let mut frame = p.lanes[0].coordinate.frame.clone();
    let plan = &b.coordinate;
    let evidence = gpu_v2::texture::sim::staged::binding::Evidence::build(&frame).unwrap();
    let cones = evidence.lowering.logic_cones(&frame, 1).unwrap();
    if poison {
        for value in &mut frame.values {
            if frame.events[value.producer].operation != audited::Operation::Literal {
                value.raw ^= 4095;
            }
        }
        for value in &mut frame.outputs {
            value.raw ^= 4095;
        }
    }
    let fields: Vec<Field> = plan
        .packed_fields
        .iter()
        .map(|f| Field {
            value: f.value,
            source_low: f.source_low,
            width: f.width,
            birth: f.birth as u8,
            last_read: f.last_read as u8,
            lows: (0..plan.packed.period / plan.ii())
                .map(|i| {
                    plan.packed
                        .placements
                        .iter()
                        .find(|pl| {
                            pl.value == f.value
                                && pl.source_low == f.source_low
                                && pl.iteration == i
                        })
                        .unwrap()
                        .low
                })
                .collect(),
        })
        .collect();
    Calendar::from_structure(
        &frame,
        &plan.times,
        &cones,
        &evidence.lowering.wiring_adds,
        fields,
        plan.packed.ff_bits,
        plan.span() as u8,
        plan.packed.period as u32,
        plan.ii() as u32,
        coordinate::OUTPUTS,
    )
    .unwrap()
}

// ---- Independent semantic goldens (frozen contracts, plain host integers) ----

fn d_golden(i: derivative::Input) -> derivative::Output {
    let slope = [(0, 1), (2, 3), (0, 2), (1, 3)]
        .into_iter()
        .flat_map(|(a, b)| {
            (0..2).map(move |axis| (i.uv[2 * b + axis] - i.uv[2 * a + axis]).unsigned_abs())
        })
        .max()
        .unwrap();
    derivative::Output {
        uv: i.uv.map(|v| v.rem_euclid(1 << 18) as u32),
        slope,
        bias: i.bias,
        header: i.header,
    }
}

fn l_golden(i: lod::Input) -> lod::Output {
    let maximum = if i.header.has_mip {
        i32::from(i.header.max_n) * 256
    } else {
        0
    };
    let l = if i.slope == 0 {
        0
    } else if i.slope > 524288 {
        maximum
    } else {
        let h = 63 - i.slope.leading_zeros();
        let norm = i.slope << (19 - h);
        let tail = norm & ((1 << 19) - 1);
        let quotient = tail / 8192;
        let remainder = tail % 8192;
        let k = quotient
            + u64::from(remainder > 4096 || remainder == 4096 && !quotient.is_multiple_of(2));
        let exponent = i32::from(i.header.max_n) + h as i32 - 18 + i32::from(k == 64);
        let log = ((1.0 + (k % 64) as f64 / 64.0).log2() * 256.0).round_ties_even() as i32;
        (256 * exponent + log + i32::from(i.bias)).clamp(0, maximum)
    };
    let fine = i.header.max_n - (l / 256) as u8;
    let levels = [fine, fine.saturating_sub(1)];
    let fraction = l % 256;
    let lambda = if i.header.filter == 2 {
        (2 * fraction - i32::from(fraction > 128)) as u16
    } else {
        0
    };
    lod::Output {
        context: lod::CoordinateContext {
            shift: i32::from(fine.max(1)) - 10,
            nearest: i.header.filter == 0,
            halve: fine > 1,
            side: levels.map(|n| 1_i16 << n.max(1)),
            parents: [511 - lambda, lambda],
            levels,
            last_fine: lambda == 0,
        },
        quad: i.header.quad,
        mask: i.header.mask,
        slot: i.header.slot,
    }
}

fn coord_golden(inp: CoordInput) -> coordinate::Output {
    let mut fractions = [[0u8; 2]; 2];
    let mut coordinates = [[0u16; 4]; 2];
    for axis in 0..2 {
        let uv = inp.uv[axis] as i64;
        let fine = if inp.shift >= 0 {
            uv << inp.shift
        } else {
            uv >> (-inp.shift)
        };
        let q0 = fine - if inp.nearest { 0 } else { 128 };
        let q1 = if inp.halve { (q0 - 128) >> 1 } else { q0 };
        for (which, q) in [q0, q1].into_iter().enumerate() {
            fractions[which][axis] = (q & 0xff) as u8;
            let integer = q >> 8;
            let side = i64::from(inp.side[which]);
            let a = if integer < 0 { side - 1 } else { integer };
            let next = integer + 1;
            let b = if next == side { 0 } else { next };
            coordinates[which][axis * 2] = a as u16;
            coordinates[which][axis * 2 + 1] = b as u16;
        }
    }
    coordinate::Output {
        fractions,
        coordinates,
    }
}

// ---- Stimulus (constructor-only) ----

fn d_stimulus(slope: u64, id: usize) -> (QuadInput, Slot) {
    let slot = Slot {
        base_address: 4096,
        max_size_log2: (id % 11) as u8,
        has_full_mip: id.is_multiple_of(2),
        valid: true,
    };
    let mut q = QuadInput {
        quad_id: (id % 16) as u8,
        mask: 1,
        uv: [[-0.75, 0.125]; 4],
        slot: 0,
        material_size_log2: slot.max_size_log2,
        filter: [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][id % 3],
        lod_bias: [-32.0, -0.5, 0.0, 0.5, 32.0][id % 5],
    };
    q.uv[1][0] += slope as f64 / 262144.0;
    if slope > (1 << 38) {
        q.uv[0][0] = -(1_i64 << 38) as f64 / 262144.0;
        q.uv[1][0] = (slope - (1 << 38)) as f64 / 262144.0;
    }
    (q, slot)
}

fn derivative_inputs() -> Vec<derivative::Input> {
    let mut slopes = vec![
        0,
        1,
        2,
        3,
        127,
        255,
        256,
        257,
        65536,
        262144,
        524288,
        524289,
        1 << 39,
    ];
    slopes.extend((0..64).map(|k| 262144 + k * 4096 + 2048));
    // Requested increments that land the actual LOD `RoundIncrement(13)` exactly
    // below, on and above the half. An actual slope of 2^18+t normalizes to a
    // 19-bit tail of 2t, so t selects the remainder/quotient pair; both retained
    // parity branches (even and odd quotient at the exact tie) are included.
    slopes.extend([32768, 33768, 34816, 35768, 36863, 36864, 38912, 43008]);
    let samples: Vec<_> = (0..6)
        .flat_map(|_| slopes.iter().copied())
        .enumerate()
        .map(|(id, s)| d_stimulus(s, id))
        .collect();
    samples
        .iter()
        .map(|(q, s)| derivative::Input::capture(q, *s).unwrap())
        .collect()
}

fn coordinate_inputs() -> Vec<CoordInput> {
    let mut v = vec![];
    for physical in 1..=10 {
        let shift = physical - 10;
        let side = 1i16 << physical;
        for nearest in [false, true] {
            let halve = physical > 1;
            v.push(CoordInput {
                uv: [0, 0],
                shift,
                nearest,
                halve,
                side: [side, (side / 2).max(2)],
            });
            v.push(CoordInput {
                uv: [(1u32 << 18) - 1, 256u32.wrapping_sub(1)],
                shift,
                nearest,
                halve,
                side: [side, (side / 2).max(2)],
            });
            v.push(CoordInput {
                uv: [128, (1u32 << 18) - 128],
                shift,
                nearest,
                halve,
                side: [side, (side / 2).max(2)],
            });
        }
    }
    v.push(CoordInput {
        uv: [1 << 17, (1 << 18) - 1],
        shift: -9,
        nearest: false,
        halve: false,
        side: [2, 2],
    });
    v
}

// ---- Raw row encoding (public Input fields; no live-executor broadening) ----

fn d_rows(i: &derivative::Input) -> Vec<(&'static str, usize, i128)> {
    let mut r: Vec<_> =
        i.uv.iter()
            .enumerate()
            .map(|(k, v)| ("helper_uv", k, i128::from(*v)))
            .collect();
    r.push(("bias", 0, i128::from(i.bias)));
    r.push(("meta", 0, i128::from(i.header.quad)));
    r.push(("meta", 1, i128::from(i.header.mask)));
    r.push(("meta", 2, i128::from(i.header.slot)));
    r.push(("meta", 3, i128::from(i.header.max_n)));
    r.push(("has_mip", 0, i128::from(i.header.has_mip)));
    r.push(("filter", 0, i128::from(i.header.filter)));
    r
}

fn l_rows(i: &lod::Input) -> Vec<(&'static str, usize, i128)> {
    vec![
        ("slope", 0, i128::from(i.slope)),
        ("bias", 0, i128::from(i.bias)),
        ("meta", 0, i128::from(i.header.max_n)),
        ("meta", 1, i128::from(i.header.quad)),
        ("meta", 2, i128::from(i.header.mask)),
        ("meta", 3, i128::from(i.header.slot)),
        ("has_mip", 0, i128::from(i.header.has_mip)),
        ("filter", 0, i128::from(i.header.filter)),
    ]
}

fn c_rows(i: &CoordInput) -> Vec<(&'static str, usize, i128)> {
    vec![
        ("wrapped_uv", 0, i128::from(i.uv[0])),
        ("wrapped_uv", 1, i128::from(i.uv[1])),
        ("coordinate_shift", 0, i128::from(i.shift)),
        ("flags", 0, i128::from(i.nearest)),
        ("flags", 1, i128::from(i.halve)),
        ("side", 0, i128::from(i.side[0])),
        ("side", 1, i128::from(i.side[1])),
    ]
}

// ---- Typed -> named raw outputs ----

fn d_named(o: &derivative::Output) -> Vec<(String, i128)> {
    let mut v: Vec<(String, i128)> = (0..8)
        .map(|i| (format!("uv{i}"), i128::from(o.uv[i])))
        .collect();
    v.push(("slope".into(), i128::from(o.slope)));
    v.push(("bias".into(), i128::from(o.bias)));
    v.push(("quad".into(), i128::from(o.header.quad)));
    v.push(("mask".into(), i128::from(o.header.mask)));
    v.push(("slot".into(), i128::from(o.header.slot)));
    v.push(("max_n".into(), i128::from(o.header.max_n)));
    v.push(("has_mip".into(), i128::from(o.header.has_mip)));
    v.push(("filter".into(), i128::from(o.header.filter)));
    v
}

fn l_named(o: &lod::Output) -> Vec<(String, i128)> {
    let c = o.context;
    vec![
        ("shift0".into(), i128::from(c.shift)),
        ("nearest".into(), i128::from(c.nearest)),
        ("halve".into(), i128::from(c.halve)),
        ("side0".into(), i128::from(c.side[0])),
        ("side1".into(), i128::from(c.side[1])),
        ("parent0".into(), i128::from(c.parents[0])),
        ("parent1".into(), i128::from(c.parents[1])),
        ("n0".into(), i128::from(c.levels[0])),
        ("n1".into(), i128::from(c.levels[1])),
        ("last_fine".into(), i128::from(c.last_fine)),
        ("quad".into(), i128::from(o.quad)),
        ("mask".into(), i128::from(o.mask)),
        ("slot".into(), i128::from(o.slot)),
    ]
}

fn c_named(o: &coordinate::Output) -> Vec<(String, i128)> {
    let mut v = vec![];
    for w in 0..2 {
        for a in 0..2 {
            v.push((format!("f{w}.{a}"), i128::from(o.fractions[w][a])));
        }
        for i in 0..4 {
            v.push((
                format!("t{w}.{}.{}", i / 2, i % 2),
                i128::from(o.coordinates[w][i]),
            ));
        }
    }
    v
}

// ---- Generic co-simulation harness ----

struct Row {
    ce: bool,
    in_valid: bool,
    inputs: Vec<(String, u128)>,
    out_valid: bool,
    outputs: Vec<(String, u128)>,
    /// Whole packed FF bank big-endian hex, when the module exposes it.
    bank: Option<String>,
    phase: u128,
    valid_state: u128,
}

fn phase_valid(enabled: u64, period: u32, span: u8, accepts: &[u64]) -> (u128, u128) {
    let phase = 1u128 << (enabled % u64::from(period));
    let mut valid = 0u128;
    for a in 1..=u64::from(span) {
        if enabled >= a && accepts.contains(&(enabled - a)) {
            valid |= 1u128 << a;
        }
    }
    (phase, valid)
}

fn bank_hex(words: &[u64], bits: usize) -> String {
    let digits = bits.div_ceil(4);
    let mut s = String::with_capacity(digits);
    for d in (0..digits).rev() {
        let mut nibble = 0u8;
        for k in (0..4).rev() {
            let bit = d * 4 + k;
            let value = if bit < bits {
                (words[bit / 64] >> (bit % 64)) & 1
            } else {
                0
            };
            nibble = (nibble << 1) | value as u8;
        }
        s.push(char::from_digit(u32::from(nibble), 16).unwrap());
    }
    s
}

fn mask(raw: i128, width: u32) -> u128 {
    let bits = raw as u128;
    if width >= 128 {
        bits
    } else {
        bits & ((1u128 << width) - 1)
    }
}

fn input_values(
    des: &rtl::Descriptor,
    rows: &[(&'static str, usize, i128)],
) -> Vec<(String, u128)> {
    des.inputs
        .iter()
        .map(|p| {
            let raw = rows
                .iter()
                .find(|(k, r, _)| *k == p.key && *r == p.row)
                .map_or(0, |(_, _, v)| *v);
            (p.name.clone(), mask(raw, p.width))
        })
        .collect()
}

fn output_values(des: &rtl::Descriptor, named: &[(String, i128)]) -> Vec<(String, u128)> {
    des.outputs
        .iter()
        .map(|p| {
            let raw = named
                .iter()
                .find(|(n, _)| *n == p.key)
                .map_or(0, |(_, v)| *v);
            (p.name.clone(), mask(raw, p.width))
        })
        .collect()
}

fn artifact_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/opencode/calendar-rtl-round2/rtl")
}

fn range(width: u32, signed: bool) -> String {
    let s = if signed { "signed " } else { "" };
    if width == 1 {
        s.to_string()
    } else {
        format!("{s}[{}:0] ", width - 1)
    }
}

fn build_testbench(des: &rtl::Descriptor, rows: &[Row]) -> String {
    let mut names: Vec<String> = vec!["ce".into(), "in_valid".into()];
    names.extend(des.inputs.iter().map(|p| p.name.clone()));
    names.push("exp_valid".into());
    names.extend(des.outputs.iter().map(|p| format!("exp_{}", p.name)));
    if des.debug_bank {
        names.push("exp_bank".into());
        names.push("exp_phase".into());
        names.push("exp_valid_state".into());
    }
    let fmts = vec!["%h"; names.len()].join(" ");
    let mut s = String::from("`timescale 1ns/1ps\nmodule tb;\nreg clk=0; always #5 clk=~clk;\nreg ce, reset, in_valid;\nwire in_ready, fault, out_valid;\n");
    for p in &des.inputs {
        s.push_str(&format!("reg {}{};\n", range(p.width, p.signed), p.name));
    }
    for p in &des.outputs {
        s.push_str(&format!(
            "reg {}exp_{};\nwire {}{};\n",
            range(p.width, p.signed),
            p.name,
            range(p.width, p.signed),
            p.name
        ));
    }
    s.push_str("reg exp_valid;\n");
    if des.debug_bank {
        s.push_str(&format!(
            "wire [{}:0] dbg_bank;\nreg [{}:0] exp_bank;\n",
            des.inventory.numeric_bits - 1,
            des.inventory.numeric_bits - 1
        ));
        s.push_str(&format!(
            "wire [{}:0] dbg_phase;\nreg [{}:0] exp_phase;\nwire [{}:0] dbg_valid;\nreg [{}:0] exp_valid_state;\n",
            des.inventory.period - 1,
            des.inventory.period - 1,
            des.inventory.span,
            des.inventory.span
        ));
    }
    s.push_str(&format!("{} dut(.clk(clk),.ce(ce),.reset(reset),.in_valid(in_valid),.in_ready(in_ready),.fault(fault),.out_valid(out_valid)",
        des.module));
    for p in &des.inputs {
        s.push_str(&format!(",.{}({})", p.name, p.name));
    }
    for p in &des.outputs {
        s.push_str(&format!(",.{}({})", p.name, p.name));
    }
    if des.debug_bank {
        s.push_str(",.dbg_bank(dbg_bank),.dbg_phase(dbg_phase),.dbg_valid(dbg_valid)");
    }
    s.push_str(");\ninteger n,r,f;\ninitial begin\n ce=0;reset=1;in_valid=0;");
    for p in &des.inputs {
        s.push_str(&format!("{}=0;", p.name));
    }
    for p in &des.outputs {
        s.push_str(&format!("exp_{}=0;", p.name));
    }
    if des.debug_bank {
        s.push_str("exp_bank=0;\nexp_phase=0;\nexp_valid_state=0;\n");
    }
    s.push_str(
        "exp_valid=0;\n repeat(3) @(posedge clk);\n reset=0;\n f=$fopen(\"vectors.txt\",\"r\");\n",
    );
    s.push_str(&format!(
        " for(n=0;n<{};n=n+1) begin\n  @(negedge clk);\n  r=$fscanf(f,\"{fmts}\",{});\n  if(r!={})$fatal(1,\"vector parse %0d\",n);\n  #1;\n  if(fault)$fatal(1,\"off-phase valid %0d\",n);\n  if(out_valid!==exp_valid)$fatal(1,\"out_valid %0d got %b exp %b\",n,out_valid,exp_valid);\n  if(exp_valid) begin\n",
        rows.len(),
        names.join(","),
        names.len()
    ));
    for p in &des.outputs {
        s.push_str(&format!(
            "   if({} !== exp_{})$fatal(1,\"{} %0d got %h exp %h\",n,{},exp_{});\n",
            p.name, p.name, p.name, p.name, p.name
        ));
    }
    s.push_str("  end\n");
    if des.debug_bank {
        s.push_str("  if(dbg_phase !== exp_phase) begin $display(\"PHASE n=%0d got %h exp %h\",n,dbg_phase,exp_phase); $fatal(1,\"phase\"); end\n");
        s.push_str("  if(dbg_valid !== exp_valid_state) begin $display(\"VALID n=%0d got %h exp %h\",n,dbg_valid,exp_valid_state); $fatal(1,\"valid\"); end\n");
        s.push_str("  if(dbg_bank !== exp_bank) begin $display(\"BANK n=%0d got %h exp %h\",n,dbg_bank,exp_bank); $fatal(1,\"bank\"); end\n");
    }
    s.push_str("  @(posedge clk);\n end\n");
    s.push_str(&format!(
        " $display(\"PASS {}\");\n $finish;\nend\ninitial begin #{}; $fatal(1,\"watchdog\"); end\nendmodule\n",
        des.module,
        rows.len() * 20 + 400
    ));
    s
}

fn bounded(mut command: Command, root: &Path, name: &str) {
    let stdout = fs::File::create(root.join(format!("{name}.out"))).unwrap();
    let stderr = fs::File::create(root.join(format!("{name}.err"))).unwrap();
    let mut child = command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                fs::read_to_string(root.join(format!("{name}.out"))).unwrap(),
                fs::read_to_string(root.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if start.elapsed() > Duration::from_secs(120) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{name} wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn simulate(des: &rtl::Descriptor, source: &str, rows: &[Row]) {
    let root = artifact_root().join(&des.module);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(format!("{}.v", des.module)), source).unwrap();
    let mut ports = String::new();
    for p in &des.inputs {
        ports.push_str(&format!("input {} {} {}\n", p.name, p.key, p.width));
    }
    for p in &des.outputs {
        ports.push_str(&format!("output {} {} {}\n", p.name, p.key, p.width));
    }
    ports.push_str(&format!("inventory {:?}\n", des.inventory));
    fs::write(root.join(format!("{}.ports.txt", des.module)), ports).unwrap();
    let mut vectors = String::new();
    for row in rows {
        let mut line = format!("{} {} ", u8::from(row.ce), u8::from(row.in_valid));
        for (_, v) in &row.inputs {
            line.push_str(&format!("{v:x} "));
        }
        line.push_str(&format!("{} ", u8::from(row.out_valid)));
        for (_, v) in &row.outputs {
            line.push_str(&format!("{v:x} "));
        }
        if des.debug_bank {
            line.push_str(row.bank.as_deref().unwrap_or("0"));
            line.push_str(&format!(" {:x} {:x} ", row.phase, row.valid_state));
        }
        line.push('\n');
        vectors.push_str(&line);
    }
    fs::write(root.join("vectors.txt"), vectors).unwrap();
    fs::write(root.join("tb.v"), build_testbench(des, rows)).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&root)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp"])
        .arg(format!("{}.v", des.module))
        .arg("tb.v");
    bounded(compile, &root, "compile");
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    run.current_dir(&root).arg("test.vvp");
    bounded(run, &root, "run");
    assert!(
        fs::read_to_string(root.join("run.out"))
            .unwrap()
            .contains("PASS"),
        "{} did not pass",
        des.module
    );
    println!("{}: {:?}", des.module, des.inventory);
}

fn run_derivative() -> (rtl::Descriptor, String, Vec<Row>) {
    let b = bound::Binding::build().unwrap();
    let c = canonical();
    let cal = calendar(&b.derivative, &c.derivative.frame, D_OUTPUTS, false);
    let des = rtl::describe_debug(&cal, "texture_derivative_calendar");
    let source = rtl::emit_debug(&cal, "texture_derivative_calendar");
    let mut emu = DerivativeEmu::new(cal.clone()).unwrap();
    let span = u64::from(cal.span());
    let ii = cal.ii();
    let mut pending: VecDeque<derivative::Input> = derivative_inputs().into();
    let mut expected: VecDeque<(u64, derivative::Output)> = VecDeque::new();
    let mut rows = vec![];
    let mut enabled = 0u64;
    let mut accepts: Vec<u64> = vec![];
    for wall in 0..200_000u64 {
        let ce = wall % 13 < 11;
        let out = emu.output().unwrap();
        if ce {
            if let Some(o) = &out {
                let (due, want) = expected.pop_front().unwrap();
                assert_eq!(due, enabled);
                assert_eq!(o, &want, "D emu/golden enabled {enabled}");
            }
        }
        let offer = if ce && enabled.is_multiple_of(u64::from(ii)) {
            pending.pop_front()
        } else {
            None
        };
        if let Some(input) = offer {
            expected.push_back((enabled + span, d_golden(input)));
        }
        if offer.is_some() {
            accepts.push(enabled);
        }
        let named = out.as_ref().map(d_named).unwrap_or_default();
        let bank = bank_hex(emu.bank(), cal.numeric_bits());
        let (phase, valid_state) = phase_valid(enabled, cal.period(), cal.span(), &accepts);
        rows.push(Row {
            ce,
            in_valid: offer.is_some(),
            inputs: input_values(&des, &offer.as_ref().map(d_rows).unwrap_or_default()),
            out_valid: out.is_some(),
            outputs: output_values(&des, &named),
            bank: Some(bank),
            phase,
            valid_state,
        });
        emu.tick(ce, offer).unwrap();
        if ce {
            enabled += 1;
        }
        if pending.is_empty() && emu.idle() {
            break;
        }
    }
    (des, source, rows)
}

fn run_lod_inputs(module: &str, inputs: Vec<lod::Input>) -> (rtl::Descriptor, String, Vec<Row>) {
    let b = bound::Binding::build().unwrap();
    let c = canonical();
    let cal = calendar(&b.lod, &c.lod.frame, L_OUTPUTS, false);
    let des = rtl::describe_debug(&cal, module);
    let source = rtl::emit_debug(&cal, module);
    let mut emu = LodEmu::new(cal.clone()).unwrap();
    let span = u64::from(cal.span());
    let ii = cal.ii();
    let mut pending: VecDeque<lod::Input> = inputs.into();
    let mut expected: VecDeque<(u64, lod::Output)> = VecDeque::new();
    let mut rows = vec![];
    let mut enabled = 0u64;
    let mut accepts: Vec<u64> = vec![];
    for wall in 0..200_000u64 {
        let ce = wall % 13 < 11;
        let out = emu.output().unwrap();
        if ce {
            if let Some(o) = &out {
                let (due, want) = expected.pop_front().unwrap();
                assert_eq!(due, enabled);
                assert_eq!(o, &want, "LOD emu/golden enabled {enabled}");
            }
        }
        let offer = if ce && enabled.is_multiple_of(u64::from(ii)) {
            pending.pop_front()
        } else {
            None
        };
        if let Some(input) = offer {
            expected.push_back((enabled + span, l_golden(input)));
        }
        if offer.is_some() {
            accepts.push(enabled);
        }
        let named = out.as_ref().map(l_named).unwrap_or_default();
        let bank = bank_hex(emu.bank(), cal.numeric_bits());
        let (phase, valid_state) = phase_valid(enabled, cal.period(), cal.span(), &accepts);
        rows.push(Row {
            ce,
            in_valid: offer.is_some(),
            inputs: input_values(&des, &offer.as_ref().map(l_rows).unwrap_or_default()),
            out_valid: out.is_some(),
            outputs: output_values(&des, &named),
            bank: Some(bank),
            phase,
            valid_state,
        });
        emu.tick(ce, offer).unwrap();
        if ce {
            enabled += 1;
        }
        if pending.is_empty() && emu.idle() {
            break;
        }
    }
    (des, source, rows)
}

fn run_lod() -> (rtl::Descriptor, String, Vec<Row>) {
    let inputs = derivative_inputs()
        .into_iter()
        .map(|d| lod::Input::from(d_golden(d)))
        .collect();
    run_lod_inputs("texture_lod_calendar", inputs)
}

fn run_coordinate() -> (rtl::Descriptor, String, Vec<Row>) {
    let cal = coordinate_calendar(false);
    let des = rtl::describe_debug(&cal, "texture_coordinate_calendar");
    let source = rtl::emit_debug(&cal, "texture_coordinate_calendar");
    let mut emu = CoordinateEmu::new(cal.clone()).unwrap();
    let span = u64::from(cal.span());
    let ii = cal.ii();
    let mut pending: VecDeque<CoordInput> = coordinate_inputs().into();
    let mut expected: VecDeque<(u64, coordinate::Output)> = VecDeque::new();
    let mut rows = vec![];
    let mut enabled = 0u64;
    let mut accepts: Vec<u64> = vec![];
    for wall in 0..200_000u64 {
        let ce = wall % 13 < 11;
        let out = emu.output().unwrap();
        if ce {
            if let Some(o) = &out {
                let (due, want) = expected.pop_front().unwrap();
                assert_eq!(due, enabled);
                assert_eq!(o, &want, "coordinate emu/golden enabled {enabled}");
            }
        }
        let offer = if ce && enabled.is_multiple_of(u64::from(ii)) {
            pending.pop_front()
        } else {
            None
        };
        if let Some(input) = offer {
            expected.push_back((enabled + span, coord_golden(input)));
        }
        if offer.is_some() {
            accepts.push(enabled);
        }
        let named = out.as_ref().map(c_named).unwrap_or_default();
        let bank = bank_hex(emu.bank(), cal.numeric_bits());
        let (phase, valid_state) = phase_valid(enabled, cal.period(), cal.span(), &accepts);
        rows.push(Row {
            ce,
            in_valid: offer.is_some(),
            inputs: input_values(&des, &offer.as_ref().map(c_rows).unwrap_or_default()),
            out_valid: out.is_some(),
            outputs: output_values(&des, &named),
            bank: Some(bank),
            phase,
            valid_state,
        });
        emu.tick(ce, offer).unwrap();
        if ce {
            enabled += 1;
        }
        if pending.is_empty() && emu.idle() {
            break;
        }
    }
    (des, source, rows)
}

#[test]
fn derivative_calendar_rtl_matches_independent_register_executor() {
    let (des, source, rows) = run_derivative();
    assert!(rtl::audit(&{
        let b = bound::Binding::build().unwrap();
        let c = canonical();
        calendar(&b.derivative, &c.derivative.frame, D_OUTPUTS, false)
    })
    .is_ok());
    simulate(&des, &source, &rows);
}

#[test]
fn lod_calendar_rtl_matches_independent_register_executor() {
    let (des, source, rows) = run_lod();
    simulate(&des, &source, &rows);
}

/// RNE (round-half-to-even) regression at the actual LOD `RoundIncrement(13)`.
///
/// An actual slope of `2^18 + t` normalizes to a 19-bit tail of `2t`, so `t`
/// selects the remainder/quotient pair: below half, the exact tie with an even
/// and an odd quotient (retained parity), above half, and exact multiples. Each
/// case is checked three ways: the independent `l_golden`, the emulator's own
/// `RoundIncrement` cut, and the same inputs driven through real Icarus RTL.
/// A signed-negative helper UV is retained while only its magnitude feeds the
/// unsigned rounding path.
#[test]
fn rne_below_tie_above_and_signed_negatives() {
    let b = bound::Binding::build().unwrap();
    let c = canonical();
    let cal = calendar(&b.lod, &c.lod.frame, L_OUTPUTS, false);
    let header = derivative::Header {
        quad: 0,
        mask: 1,
        slot: 0,
        max_n: 9,
        has_mip: true,
        filter: 2,
    };
    // (actual slope, expected RoundIncrement bit, label)
    let cases: [(u64, i128, &str); 8] = [
        (262144, 0, "tail zero"),
        (263144, 0, "below half"),
        (264192, 0, "tie, even quotient"),
        (268288, 1, "tie, odd quotient"),
        (265144, 1, "above half"),
        (266239, 1, "just below the next multiple"),
        (266240, 0, "exact multiple"),
        (272384, 0, "tie, even quotient q2"),
    ];
    let inputs: Vec<lod::Input> = cases
        .iter()
        .map(|(slope, _, _)| lod::Input {
            slope: *slope,
            bias: 0,
            header,
        })
        .collect();
    for ((slope, want_round, label), input) in cases.iter().zip(&inputs) {
        let mut emu = LodEmu::new(cal.clone()).unwrap();
        let mut round = None;
        for age in 0..=cal.span() {
            if let Some(o) = emu.output().unwrap() {
                assert_eq!(o, l_golden(*input), "LOD golden {label} slope{slope}");
            }
            let edge = emu.tick(true, (age == 0).then_some(*input)).unwrap();
            for calc in &edge.calculations {
                if calc.event == 31 {
                    round = Some(calc.raw);
                }
            }
        }
        assert_eq!(round, Some(*want_round), "RNE {label} slope{slope}");
    }
    // Signed-negative helper UV is retained; only its magnitude feeds slope.
    let mut q = QuadInput {
        quad_id: 0,
        mask: 1,
        uv: [[0.0, 0.0]; 4],
        slot: 0,
        material_size_log2: 9,
        filter: Filter::Trilinear,
        lod_bias: 0.0,
    };
    q.uv[1][0] = -(2048.0 / 262144.0);
    let d = derivative::Input::capture(
        &q,
        Slot {
            base_address: 4096,
            max_size_log2: 9,
            has_full_mip: true,
            valid: true,
        },
    )
    .unwrap();
    assert_eq!(d.uv[2], -2048);
    assert_eq!(d_golden(d).slope, 2048);
    // The same rounding inputs through real Icarus RTL.
    let (des, source, rows) = run_lod_inputs("texture_lod_rne", inputs);
    simulate(&des, &source, &rows);
}

#[test]
fn coordinate_calendar_rtl_matches_independent_register_executor() {
    let (des, source, rows) = run_coordinate();
    simulate(&des, &source, &rows);
}

#[test]
fn poisoned_constructor_emits_identical_source_and_hash() {
    let b = bound::Binding::build().unwrap();
    let c = canonical();
    assert_eq!(
        rtl::emit(
            &calendar(&b.derivative, &c.derivative.frame, D_OUTPUTS, false),
            "d"
        ),
        rtl::emit(
            &calendar(&b.derivative, &c.derivative.frame, D_OUTPUTS, true),
            "d"
        )
    );
    assert_eq!(
        rtl::emit(&calendar(&b.lod, &c.lod.frame, L_OUTPUTS, false), "l"),
        rtl::emit(&calendar(&b.lod, &c.lod.frame, L_OUTPUTS, true), "l")
    );
    assert_eq!(
        rtl::emit(&coordinate_calendar(false), "c"),
        rtl::emit(&coordinate_calendar(true), "c")
    );
}

/// A valid row presented off the certified phase must be refused (in_ready=0)
/// and raise `fault`, never silently dropped or admitted.
#[test]
fn off_phase_valid_is_refused_with_fault() {
    let b = bound::Binding::build().unwrap();
    let c = canonical();
    let des = rtl::describe(
        &calendar(&b.derivative, &c.derivative.frame, D_OUTPUTS, false),
        "texture_derivative_offphase",
    );
    let source = rtl::emit(
        &calendar(&b.derivative, &c.derivative.frame, D_OUTPUTS, false),
        "texture_derivative_offphase",
    );
    let root = artifact_root().join(&des.module);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(format!("{}.v", des.module)), source).unwrap();
    let mut tb = build_testbench(&des, &[]);
    // Replace the empty vector loop with an explicit off-phase probe: phase
    // index 0 is ready, index 1 is not. No vector file is read.
    tb = tb.replace(
        "f=$fopen(\"vectors.txt\",\"r\");",
        "f=0;\n ce=1;\n in_valid=1;\n if(!in_ready)$fatal(1,\"phase0 must be ready\");\n if(fault)$fatal(1,\"phase0 fault\");\n @(posedge clk);\n #1;\n if(in_ready)$fatal(1,\"phase1 must not be ready\");\n if(!fault)$fatal(1,\"off-phase valid not faulted\");\n in_valid=0;\n $display(\"PASS {}\");\n $finish;",
    );
    fs::write(root.join("tb.v"), tb).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&root)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp"])
        .arg(format!("{}.v", des.module))
        .arg("tb.v");
    bounded(compile, &root, "compile");
    let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    run.current_dir(&root).arg("test.vvp");
    bounded(run, &root, "run");
}
