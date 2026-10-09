//! Actual Icarus per-edge co-simulation of the conserving UNORM9 coefficient RTL
//! against the independent register emulator
//! `texture::emu::coefficient::CoefficientEmu`.
//!
//! The Rust tests first establish the semantics with an independent integer
//! golden (`parent * fraction / 256`, nearest forcing zero fractions), then the
//! ignored Icarus tests drive the emitted module every edge and compare the
//! pre-edge handshake (`in_ready`/`in_accept`), the reservation pulse, the held
//! output (weights and packed metadata) and the latched fault with the emulator's
//! live result. No expected RTL answer is embedded in the module; the testbench
//! is generated from emulator steps at run time.
//!
//! Both tool invocations run under a wall-clock watchdog that kills and reaps the
//! child, and their stdout/stderr go to files rather than pipes so a large
//! diagnostic can never fill a pipe. The generated bench also keeps its own
//! finite HDL watchdog (`#2000000; $fatal`). The Icarus tests are `#[ignore]` so
//! `cargo test` stays tool-free.

use gpu_v2::texture::{
    emu::coefficient::{
        CoefficientEmu, Field, Input, Metadata, Output, Tick, ALLOCATION, COHORT_CAPACITY,
        NUMERIC_BITS, READY_CAPACITY,
    },
    rtl::coefficient,
};
use std::fs::File;
use std::process::Stdio;
use std::time::{Duration, Instant};

const OUT_DIR: &str = "../../target/opencode/coefficient-rtl-20261006";

/// One canonical legal input.
fn legal(index: usize, parent: u16, fractions: [[u8; 2]; 2], nearest: bool) -> Input {
    Input {
        parents: [parent, 511 - parent],
        fractions,
        nearest,
        metadata: Metadata {
            coordinates: std::array::from_fn(|p| {
                std::array::from_fn(|i| ((index * 37 + p * 173 + i * 59) & 1023) as u16)
            }),
            levels: [(index % 11) as u8, ((index + 7) % 11) as u8],
            slot: (index % 16) as u8,
            key: (index % 64) as u8,
            last_fine: parent == 511,
        },
    }
}

/// Independent integer golden: `bottom = parent*fv/256`, `top = parent-bottom`,
/// then `right = row*fu/256`. Nearest forces every fraction to zero.
fn rows(v: Input, plane: usize) -> [u16; 2] {
    let fraction = if v.nearest {
        0
    } else {
        u64::from(v.fractions[plane][1])
    };
    let bottom = (u64::from(v.parents[plane]) * fraction / 256) as u16;
    [v.parents[plane] - bottom, bottom]
}

fn golden(v: Input) -> Output {
    Output {
        metadata: v.metadata,
        weights: std::array::from_fn(|p| {
            let [top, bottom] = rows(v, p);
            let fraction = if v.nearest {
                0
            } else {
                u64::from(v.fractions[p][0])
            };
            let tr = (u64::from(top) * fraction / 256) as u16;
            let br = (u64::from(bottom) * fraction / 256) as u16;
            [top - tr, tr, bottom - br, br]
        }),
    }
}

/// Weight packing must match the RTL: index order is the corner order used by
/// the emulator's output register.
fn pack_weights(weights: &[[u16; 4]; 2]) -> u128 {
    let order = [
        weights[0][0],
        weights[0][1],
        weights[0][2],
        weights[0][3],
        weights[1][0],
        weights[1][1],
        weights[1][2],
        weights[1][3],
    ];
    let mut value = 0_u128;
    for (index, weight) in order.iter().enumerate() {
        value |= u128::from(*weight) << (9 * index);
    }
    value
}

/// Metadata packing must match the RTL and `CoefficientEmu::pack_metadata`.
fn pack_meta(m: &Metadata) -> u128 {
    let mut value = 0_u128;
    for (index, coordinate) in m.coordinates.iter().flatten().enumerate() {
        value |= u128::from(*coordinate) << (10 * index);
    }
    value
        | (u128::from(m.levels[0]) << 80)
        | (u128::from(m.levels[1]) << 84)
        | (u128::from(m.slot) << 88)
        | (u128::from(m.key) << 92)
        | (u128::from(m.last_fine) << 98)
}

/// Pack the 162-bit input bus: explicit scalar fields then packed metadata.
fn pack_input(v: &Input) -> (u64, u128) {
    let mut lo: u128 = 0;
    let mut hi: u64 = 0;
    let mut put = |base: usize, width: usize, val: u128| {
        for bit in 0..width {
            if (val >> bit) & 1 != 0 {
                let index = base + bit;
                if index < 128 {
                    lo |= 1_u128 << index;
                } else {
                    hi |= 1_u64 << (index - 128);
                }
            }
        }
    };
    put(0, 1, u128::from(v.nearest));
    put(1, 10, u128::from(v.parents[0]));
    put(11, 10, u128::from(v.parents[1]));
    put(21, 8, u128::from(v.fractions[0][0]));
    put(29, 8, u128::from(v.fractions[0][1]));
    put(37, 8, u128::from(v.fractions[1][0]));
    put(45, 8, u128::from(v.fractions[1][1]));
    for p in 0..2 {
        for i in 0..4 {
            put(
                53 + 11 * (p * 4 + i),
                11,
                u128::from(v.metadata.coordinates[p][i]),
            );
        }
    }
    put(141, 4, u128::from(v.metadata.levels[0]));
    put(145, 4, u128::from(v.metadata.levels[1]));
    put(149, 5, u128::from(v.metadata.slot));
    put(154, 7, u128::from(v.metadata.key));
    put(161, 1, u128::from(v.metadata.last_fine));
    (hi, lo)
}

fn input_literal(v: &Input) -> String {
    let (hi, lo) = pack_input(v);
    format!("162'h{hi:x}{lo:032x}")
}

fn weights_literal(w: u128) -> String {
    format!("72'h{w:018x}")
}

fn meta_literal(m: u128) -> String {
    format!("99'h{m:025x}")
}

/// Deterministic stream: legal split parents, joint fraction edges 0/255,
/// nearest on the single-parent plane, repeated keys for metadata-ring reuse.
fn stream() -> Vec<Input> {
    let parents = [0_u16, 1, 2, 127, 128, 255, 256, 383, 510, 511];
    let fractions = [0_u8, 1, 127, 128, 129, 254, 255];
    let mut out = Vec::new();
    for index in 0..96_usize {
        let parent = parents[index % parents.len()];
        let nearest = parent == 511 && index % 3 == 0;
        let pick = |k: usize| fractions[(index * 7 + k * 5) % fractions.len()];
        let mut v = legal(
            index,
            parent,
            [[pick(0), pick(1)], [pick(2), pick(3)]],
            nearest,
        );
        v.metadata.key = (index % 6) as u8;
        out.push(v);
    }
    out
}

#[derive(Clone, Copy)]
struct Stim {
    ce: bool,
    ready: bool,
    input: Option<Input>,
    work_available: u8,
    input_ready: bool,
    accepted: bool,
    output: Option<Output>,
    work_reserved: u8,
}

/// Drive the emulator with CE pauses, output backpressure and a work pattern
/// that includes starvation (0/1 free) and full (16) availability, and collect
/// the expected pre-edge handshake per edge. Returns `(edges, commits)`.
fn replay() -> (Vec<Stim>, Vec<Output>) {
    let input = stream();
    let mut emu = CoefficientEmu::new(2_000_000).unwrap();
    let mut cursor = 0;
    let mut edges = Vec::new();
    let mut commits = Vec::new();
    let mut peak_cohorts = 0_u8;
    let mut peak_ready = 0_u8;
    for wall in 0..20_000_u64 {
        let ce = !(wall % 17 == 3 || wall % 17 == 4);
        let ready = wall > 40 && (wall % 11 > 2);
        let work_available = match wall % 23 {
            0..=3 => 0_u8,
            4 => 1,
            5 => 2,
            _ => 16,
        };
        let before = emu.snapshot();
        let offer = input.get(cursor).copied();
        let step = emu
            .tick(Tick {
                ce,
                input: offer,
                output_ready: ready,
                work_available,
            })
            .unwrap();
        if step.accepted {
            cursor += 1;
        }
        if let Some(out) = step.output {
            if step.consumed {
                commits.push(out);
            }
        }
        if !ce {
            assert!(step.events.is_empty() && !step.accepted && !step.input_ready);
            assert_eq!(before.valid, step.snapshot.valid);
            assert_eq!(before.numeric_words, step.snapshot.numeric_words);
            assert_eq!(before.product_registers, step.snapshot.product_registers);
        }
        peak_cohorts = peak_cohorts.max(step.snapshot.cohorts);
        peak_ready = peak_ready.max(step.snapshot.queued);
        edges.push(Stim {
            ce,
            ready,
            input: offer,
            work_available,
            input_ready: step.input_ready,
            accepted: step.accepted,
            output: step.output,
            work_reserved: step.work_reserved,
        });
        if cursor == input.len() && emu.idle() {
            break;
        }
    }
    assert_eq!(cursor, input.len(), "replay did not offer every input");
    assert!(emu.idle(), "replay did not drain");
    assert_eq!(peak_cohorts, 6, "stimulus did not fill the cohort pipeline");
    assert_eq!(peak_ready, 2, "stimulus did not fill both ready rows");
    assert_eq!(
        commits,
        input.iter().map(|v| golden(*v)).collect::<Vec<_>>()
    );
    (edges, commits)
}

fn bit(value: bool) -> u8 {
    u8::from(value)
}

fn testbench(edges: &[Stim]) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0;\n");
    tb.push_str("reg [161:0] in_payload=0;\n");
    tb.push_str("reg [7:0] work_available=0;\n");
    tb.push_str("wire in_ready,in_accept,out_valid,fault;\n");
    tb.push_str("wire [71:0] out_weights; wire [98:0] out_meta; wire [1:0] work_reserved;\n");
    tb.push_str("gpu_v2_texture_coefficient dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_payload(in_payload),.work_available(work_available),.in_ready(in_ready),.in_accept(in_accept),.out_ready(out_ready),.out_valid(out_valid),.out_weights(out_weights),.out_meta(out_meta),.work_reserved(work_reserved),.fault(fault));\n");
    tb.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\n");
    tb.push_str("clk=0;reset=1;ce=0;in_valid=0;out_ready=0;work_available=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (edge, stim) in edges.iter().enumerate() {
        tb.push_str(&format!(
            "ce={};out_ready={};in_valid={};work_available=8'd{};\n",
            bit(stim.ce),
            bit(stim.ready),
            bit(stim.input.is_some()),
            stim.work_available
        ));
        tb.push_str(&format!(
            "in_payload={};\n",
            stim.input
                .as_ref()
                .map_or_else(|| "162'h0".to_string(), input_literal)
        ));
        tb.push_str("#1;\n");
        tb.push_str(&format!(
            "if (fault !== 1'b0) $fatal(1,\"unexpected fault edge {edge}\");\n"
        ));
        tb.push_str(&format!(
            "if (in_ready !== 1'b{}) $fatal(1,\"in_ready edge {edge}\");\n",
            bit(stim.input_ready)
        ));
        tb.push_str(&format!(
            "if (in_accept !== 1'b{}) $fatal(1,\"in_accept edge {edge}\");\n",
            bit(stim.accepted)
        ));
        tb.push_str(&format!(
            "if (work_reserved !== 2'd{}) $fatal(1,\"work_reserved edge {edge}\");\n",
            stim.work_reserved
        ));
        tb.push_str(&format!(
            "if (out_valid !== 1'b{}) $fatal(1,\"out_valid edge {edge}\");\n",
            bit(stim.output.is_some())
        ));
        if let Some(output) = stim.output {
            tb.push_str(&format!(
                "if (out_weights !== {}) $fatal(1,\"weights edge {edge}: got %h want %h\",out_weights,{});\n",
                weights_literal(pack_weights(&output.weights)),
                weights_literal(pack_weights(&output.weights))
            ));
            tb.push_str(&format!(
                "if (out_meta !== {}) $fatal(1,\"meta edge {edge}: got %h want %h\",out_meta,{});\n",
                meta_literal(pack_meta(&output.metadata)),
                meta_literal(pack_meta(&output.metadata))
            ));
        }
        tb.push_str("clk=1;#1;clk=0;#1;\n");
    }
    tb.push_str(&format!(
        "$display(\"PASS edges={}\");$finish;end endmodule\n",
        edges.len()
    ));
    tb
}

/// Run `command` with a wall-clock watchdog. Output goes to `{name}.out` and
/// `{name}.err` (never pipes), and a timeout kills and reaps the child.
fn bounded(mut command: std::process::Command, dir: &std::path::Path, name: &str, limit: Duration) {
    let stdout = File::create(dir.join(format!("{name}.out"))).unwrap();
    let stderr = File::create(dir.join(format!("{name}.err"))).unwrap();
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
                "{name} failed: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn out_dir() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(OUT_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile_and_run(dir: &std::path::Path, stem: &str, module: &str, bench: &str) {
    std::fs::write(dir.join(format!("{stem}.v")), module).unwrap();
    std::fs::write(dir.join(format!("{stem}_tb.v")), bench).unwrap();
    let compiler = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let runtime = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let mut compile = std::process::Command::new(&compiler);
    compile.current_dir(dir).args([
        "-g2012",
        "-s",
        "tb",
        "-o",
        &format!("{stem}.vvp"),
        &format!("{stem}.v"),
        &format!("{stem}_tb.v"),
    ]);
    bounded(
        compile,
        dir,
        &format!("{stem}_compile"),
        Duration::from_secs(120),
    );
    let mut run = std::process::Command::new(&runtime);
    run.current_dir(dir).arg(format!("{stem}.vvp"));
    bounded(run, dir, &format!("{stem}_run"), Duration::from_secs(120));
    let stdout = std::fs::read_to_string(dir.join(format!("{stem}_run.out"))).unwrap();
    assert!(
        stdout.contains("PASS edges="),
        "vvp did not pass: {stdout}\n{}",
        std::fs::read_to_string(dir.join(format!("{stem}_run.err"))).unwrap()
    );
    println!("{stdout}");
}

#[test]
fn rtl_declares_the_emulated_calendar_and_inventory() {
    let source = coefficient::verilog();
    assert!(source.contains("module gpu_v2_texture_coefficient("));
    assert_eq!(coefficient::latency(), 12);
    assert_eq!(coefficient::initiation_interval(), 2);
    for pattern in [
        "wire advance = ce && !fault && (queued < 2'd2);",
        "assign in_ready = advance && (cohorts < 3'd6) && phase_even",
        "wire [16:0] pm0 = {8'b0, rd_FP1} * {9'b0, rd_FV1};",
        "prod0[0] <= new0; prod0[1] <= prod0[0]; prod0[2] <= prod0[1];",
        "wire [2:0] idx10 = m6(crow, {3'b0, valid[10]});",
    ] {
        assert!(
            source.contains(pattern),
            "missing fixed-calendar RTL: {pattern}"
        );
    }
    // The leaf declares the real capacities and generic product registers.
    assert_eq!(ALLOCATION.multiply_sites, 3);
    assert_eq!(ALLOCATION.multiply_latency, 3);
    assert_eq!(ALLOCATION.initiation_interval, 2);
    assert_eq!(COHORT_CAPACITY, 6);
    assert_eq!(READY_CAPACITY, 2);
    assert_eq!(NUMERIC_BITS, 315);
    let allocation = coefficient::Allocation::default();
    assert_eq!(allocation.multiply_sites, 3);
    assert_eq!(allocation.products_per_site, 3);
    assert_eq!(allocation.multiply_latency, 3);
    assert_eq!(allocation.initiation_interval, 2);
    assert_eq!(allocation.numeric_ff_bits, 315);
    assert_eq!(allocation.metadata_ff_bits, 6 * 99);
    assert_eq!(allocation.ready_ff_bits, 2 * 171);
}

#[test]
fn independent_golden_matches_emulator_commits_in_stream_order() {
    let (edges, commits) = replay();
    assert!(edges.len() > 200, "stimulus too short: {}", edges.len());
    assert_eq!(commits.len(), stream().len());
    for (index, out) in commits.iter().enumerate() {
        assert_eq!(*out, golden(stream()[index]));
    }
    // The replay actually exercised CE pauses, work starvation and backpressure.
    assert!(edges.iter().any(|e| !e.ce));
    assert!(edges.iter().any(|e| e.work_available == 0));
    assert!(edges.iter().any(|e| e.work_available == 1));
    assert!(edges.iter().any(|e| !e.ready));
}

#[test]
fn full_ready_blocks_admission_until_a_later_edge() {
    let mut emu = CoefficientEmu::new(20_000).unwrap();
    let inputs = stream();
    let mut cursor = 0;
    // Fill both ready rows without consuming.
    for _ in 0..2_000 {
        let step = emu
            .tick(Tick {
                ce: true,
                input: inputs.get(cursor).copied(),
                output_ready: false,
                work_available: 16,
            })
            .unwrap();
        if step.accepted {
            cursor += 1;
        }
        if emu.snapshot().queued == 2 {
            break;
        }
    }
    assert_eq!(emu.snapshot().queued, 2);
    // A consumer return on the same edge must not admit the held offer.
    let held = inputs[cursor];
    let step = emu
        .tick(Tick {
            ce: true,
            input: Some(held),
            output_ready: true,
            work_available: 16,
        })
        .unwrap();
    assert!(!step.accepted && !step.input_ready && step.consumed);
    assert_eq!(emu.snapshot().queued, 1);
    // The freed row admits the held offer only on the next edge.
    let step = emu
        .tick(Tick {
            ce: true,
            input: Some(held),
            output_ready: true,
            work_available: 16,
        })
        .unwrap();
    assert!(step.accepted && step.input_ready);
}

/// Raw edge list for the fault and reset benches.
struct RawStim {
    ce: bool,
    ready: bool,
    input: Option<Input>,
    work_available: u8,
}

fn raw_testbench(edges: &[RawStim], fault_from: usize) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0;\n");
    tb.push_str("reg [161:0] in_payload=0;\n");
    tb.push_str("reg [7:0] work_available=0;\n");
    tb.push_str("wire in_ready,in_accept,out_valid,fault;\n");
    tb.push_str("wire [71:0] out_weights; wire [98:0] out_meta; wire [1:0] work_reserved;\n");
    tb.push_str("gpu_v2_texture_coefficient dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_payload(in_payload),.work_available(work_available),.in_ready(in_ready),.in_accept(in_accept),.out_ready(out_ready),.out_valid(out_valid),.out_weights(out_weights),.out_meta(out_meta),.work_reserved(work_reserved),.fault(fault));\n");
    tb.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\n");
    tb.push_str("clk=0;reset=1;ce=0;in_valid=0;out_ready=0;work_available=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (edge, stim) in edges.iter().enumerate() {
        tb.push_str(&format!(
            "ce={};out_ready={};in_valid={};work_available=8'd{};\n",
            bit(stim.ce),
            bit(stim.ready),
            bit(stim.input.is_some()),
            stim.work_available
        ));
        tb.push_str(&format!(
            "in_payload={};\n",
            stim.input
                .as_ref()
                .map_or_else(|| "162'h0".to_string(), input_literal)
        ));
        tb.push_str("#1;\n");
        if edge >= fault_from {
            tb.push_str(&format!(
                "if (fault !== 1'b1) $fatal(1,\"fault not latched by edge {edge}\");\n"
            ));
        } else {
            tb.push_str(&format!(
                "if (fault !== 1'b0) $fatal(1,\"unexpected fault before edge {edge}\");\n"
            ));
        }
        tb.push_str("clk=1;#1;clk=0;#1;\n");
    }
    assert!(edges.len() >= fault_from);
    tb.push_str("if (fault !== 1'b1) $fatal(1,\"final fault not latched\");\n");
    tb.push_str("ce=1;in_valid=1;#1;if(in_ready !== 1'b0 || in_accept !== 1'b0 || out_valid !== 1'b0) $fatal(1,\"terminal fault still handshakes\");\n");
    tb.push_str(&format!(
        "$display(\"PASS edges={}\");$finish;end endmodule\n",
        edges.len()
    ));
    tb
}

fn reset_testbench(input: &Input, expected: &Output) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0;\n");
    tb.push_str("reg [161:0] in_payload=0;\n");
    tb.push_str("reg [7:0] work_available=0;\n");
    tb.push_str("wire in_ready,in_accept,out_valid,fault;\n");
    tb.push_str("wire [71:0] out_weights; wire [98:0] out_meta; wire [1:0] work_reserved;\n");
    tb.push_str("gpu_v2_texture_coefficient dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_payload(in_payload),.work_available(work_available),.in_ready(in_ready),.in_accept(in_accept),.out_ready(out_ready),.out_valid(out_valid),.out_weights(out_weights),.out_meta(out_meta),.work_reserved(work_reserved),.fault(fault));\n");
    tb.push_str("integer i; reg seen;\n");
    tb.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\n");
    tb.push_str("clk=0;reset=1;ce=0;in_valid=0;out_ready=0;work_available=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    tb.push_str("seen=0;\n");
    tb.push_str("for (i=0;i<20;i=i+1) begin\n");
    tb.push_str(&format!(
        "  if (i==0) begin ce=1;in_valid=1;in_payload={};end else begin ce=1;in_valid=0;end\n",
        input_literal(input)
    ));
    tb.push_str("  out_ready=1;work_available=16;#1;\n");
    tb.push_str(&format!(
        "  if (out_valid) begin if (out_weights !== {} || out_meta !== {}) $fatal(1,\"reset-case payload\");seen=1;end\n",
        weights_literal(pack_weights(&expected.weights)),
        meta_literal(pack_meta(&expected.metadata))
    ));
    tb.push_str("  clk=1;#1;clk=0;#1;\n");
    tb.push_str("end\n");
    tb.push_str("if (!seen) $fatal(1,\"no output before reset\");\n");
    // Assert reset while a row is held, then confirm the leaf is cleared.
    tb.push_str("in_valid=0;out_ready=0;#1;clk=1;#1;clk=0;#1;\n");
    tb.push_str(&format!(
        "ce=1;in_valid=1;in_payload={};out_ready=0;work_available=16;#1;clk=1;#1;clk=0;#1;\n",
        input_literal(input)
    ));
    tb.push_str("reset=1;ce=1;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    tb.push_str("ce=1;in_valid=0;work_available=16;#1;\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"reset did not clear out_valid\");\n");
    tb.push_str("if (in_ready !== 1'b1) $fatal(1,\"not ready after reset\");\n");
    tb.push_str("if (fault !== 1'b0) $fatal(1,\"reset did not clear fault\");\n");
    tb.push_str("$display(\"PASS edges=reset\");$finish;end endmodule\n");
    tb
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_matches_emu_every_edge() {
    let (edges, commits) = replay();
    assert!(edges.len() > 200 && commits.len() == 96);
    let dir = out_dir();
    compile_and_run(
        &dir,
        "texture_coefficient_rtl",
        coefficient::verilog(),
        &testbench(&edges),
    );
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_latches_invalid_input_and_work_credit_faults() {
    let dir = out_dir();
    let good = legal(0, 255, [[128; 2]; 2], false);

    // Parents do not sum to 511.
    let mut bad_sum = good;
    bad_sum.parents = [0, 0];
    compile_and_run(
        &dir,
        "texture_coefficient_rtl_sum_fault",
        coefficient::verilog(),
        &raw_testbench(
            &[RawStim {
                ce: true,
                ready: true,
                input: Some(bad_sum),
                work_available: 16,
            }],
            1,
        ),
    );

    // An out-of-range coordinate.
    let mut bad_coord = good;
    bad_coord.metadata.coordinates[1][3] = 1024;
    compile_and_run(
        &dir,
        "texture_coefficient_rtl_coord_fault",
        coefficient::verilog(),
        &raw_testbench(
            &[RawStim {
                ce: true,
                ready: true,
                input: Some(bad_coord),
                work_available: 16,
            }],
            1,
        ),
    );

    // Out-of-range level, slot and key.
    for (stem, mutate) in [("level", 0_usize), ("slot", 1), ("key", 2)] {
        let mut bad = good;
        match mutate {
            0 => bad.metadata.levels[0] = 11,
            1 => bad.metadata.slot = 16,
            _ => bad.metadata.key = 64,
        }
        compile_and_run(
            &dir,
            &format!("texture_coefficient_rtl_{stem}_fault"),
            coefficient::verilog(),
            &raw_testbench(
                &[RawStim {
                    ce: true,
                    ready: true,
                    input: Some(bad),
                    work_available: 16,
                }],
                1,
            ),
        );
    }

    // nearest with a nonzero coarse parent.
    let mut bad_nearest = good;
    bad_nearest.nearest = true;
    compile_and_run(
        &dir,
        "texture_coefficient_rtl_nearest_fault",
        coefficient::verilog(),
        &raw_testbench(
            &[RawStim {
                ce: true,
                ready: true,
                input: Some(bad_nearest),
                work_available: 16,
            }],
            1,
        ),
    );

    // A work count above the leaf's capacity.
    compile_and_run(
        &dir,
        "texture_coefficient_rtl_work_fault",
        coefficient::verilog(),
        &raw_testbench(
            &[RawStim {
                ce: true,
                ready: true,
                input: Some(good),
                work_available: 17,
            }],
            1,
        ),
    );
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_reset_clears_state_and_recovers() {
    let input = legal(3, 511, [[255, 1], [128, 254]], true);
    let expected = golden(input);
    let dir = out_dir();
    compile_and_run(
        &dir,
        "texture_coefficient_rtl_reset",
        coefficient::verilog(),
        &reset_testbench(&input, &expected),
    );
}

#[test]
fn coefficient_rtl_is_independent_of_counted_helpers() {
    // The RTL source must not mention any counted/oracle entry point.
    let source = coefficient::verilog();
    for forbidden in ["oracle", "counted", "Program", "FrameReport", "Model::"] {
        assert!(!source.contains(forbidden), "RTL references {forbidden}");
    }
    // The emulator's public allocation is unchanged by this leaf.
    assert_eq!(ALLOCATION.numeric_ff_bits, 315);
    assert_eq!(ALLOCATION.metadata_bits, 99 * COHORT_CAPACITY);
    assert_eq!(ALLOCATION.ready_bits, 171 * READY_CAPACITY);
    let _ = Field::Nearest;
}
