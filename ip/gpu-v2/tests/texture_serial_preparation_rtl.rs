//! Qualification of the generated finite one-quad serial preparation RTL.
//!
//! Two independent references are compared against the generated controller:
//! the closed `bound::prepare` numerical frame supplies the expected packet
//! sequence, and the independently tested [`serial::PreparationEmu`] supplies
//! the per-edge acceptance, output and phase trajectory. Expected values are
//! computed before any RTL run and never retained by the DUT.
//!
//! Every simulated kernel run is bounded: the Rust emulator has a wall watchdog,
//! the generated testbench carries an HDL `$fatal` watchdog, and the Icarus
//! compiler/`vvp` runs go through a kill-and-reap wall watchdog with file-backed
//! stdout/stderr (never a pipe).
use gpu_v2::texture::{
    emu::derivative,
    ports::*,
    sim::staged::bound::{
        self,
        serial::{Config, Phase, PreparationEmu, Tick},
        serial_rtl,
    },
};
use std::fs::File;
use std::process::Stdio;
use std::time::{Duration, Instant};

#[allow(dead_code)]
#[path = "support/texture.rs"]
mod support;

const LIMIT: u64 = 500_000;

/// The four elaboration-time configurations under test.
const BASELINE: Config = Config {
    nearest_bypass: false,
    short_alignment: false,
};
const NEAREST_BYPASS: Config = Config {
    nearest_bypass: true,
    short_alignment: false,
};
const SHORT_ALIGNMENT: Config = Config {
    nearest_bypass: false,
    short_alignment: true,
};
const SHORT_AND_BYPASS: Config = Config {
    nearest_bypass: true,
    short_alignment: true,
};
const CONFIGS: [(&str, Config); 4] = [
    ("baseline", BASELINE),
    ("nearest_bypass", NEAREST_BYPASS),
    ("short_alignment", SHORT_ALIGNMENT),
    ("short_and_bypass", SHORT_AND_BYPASS),
];

// ---------------------------------------------------------------------------
// Stimulus: all filters and size exponents, varied masks/lanes, negative,
// repeated and large UV, repeated quad identities, and a zero-mask quad.
// ---------------------------------------------------------------------------

fn cases() -> Vec<(derivative::Input, Vec<i128>)> {
    let mut cases = Vec::new();
    let mut push = |q: QuadInput, slot: Slot| {
        let expected = bound::prepare(&q, &[slot])
            .expect("closed preparation")
            .payloads;
        let input = derivative::Input::capture(&q, slot).expect("derivative capture");
        cases.push((input, expected));
    };

    let filters = [Filter::Nearest, Filter::Bilinear, Filter::Trilinear];
    for n in [0u8, 1, 3, 6, 10] {
        for (i, filter) in filters.into_iter().enumerate() {
            let mut q = support::input(n, filter, [-0.004, 0.999]);
            q.quad_id = (i % 4) as u8;
            q.mask = [1, 5, 10, 15][i % 4];
            q.lod_bias = [-2.0, 0.5, 3.25][i % 3];
            q.uv[1][0] += 1.0 / (1u32 << n.max(1)) as f64;
            q.uv[2][1] += 1.0 / (1u32 << n.max(1)) as f64;
            push(q, support::slot(n, true));
        }
    }
    // Sparse single-lane masks.
    for (mask, n, filter) in [
        (2u8, 6u8, Filter::Bilinear),
        (4, 3, Filter::Trilinear),
        (8, 10, Filter::Nearest),
    ] {
        let mut q = support::input(n, filter, [-1.5, -1.25]);
        q.mask = mask;
        q.quad_id = 1;
        q.lod_bias = 1.75;
        push(q, support::slot(n, true));
    }
    // Negative and repeated UV corners.
    {
        let mut q = support::input(6, Filter::Bilinear, [-1.75, -1.5]);
        q.uv = [[-1.75, -1.5]; 4];
        q.quad_id = 3;
        q.lod_bias = -3.5;
        push(q, support::slot(6, true));
    }
    // Large UV and a repeated quad identity across an independent sample.
    {
        let mut q = support::input(10, Filter::Trilinear, [0.0, 0.0]);
        q.uv = [
            [900_000.5, -500_000.25],
            [-123_456.75, 654_321.5],
            [1.0, 1.0],
            [0.5, 0.5],
        ];
        q.quad_id = 3;
        q.mask = 12;
        q.lod_bias = 0.25;
        push(q, support::slot(10, true));
    }
    // Missing mip chain with a sparse mask.
    {
        let mut q = support::input(6, Filter::Trilinear, [0.25, 0.75]);
        q.quad_id = 1;
        q.mask = 5;
        q.lod_bias = 2.0;
        push(q, support::slot(6, false));
    }
    // Zero mask: accepted but launches no kernel.
    {
        let mut q = support::input(6, Filter::Bilinear, [0.25, 0.75]);
        q.mask = 0;
        q.quad_id = 2;
        q.lod_bias = 0.0;
        push(q, support::slot(6, true));
    }
    // Minimum-size zero mask.
    {
        let mut q = support::input(0, Filter::Nearest, [-1.5, 2.5]);
        q.mask = 0;
        q.quad_id = 0;
        q.lod_bias = -1.0;
        push(q, support::slot(0, true));
    }
    cases
}

fn phase_code(phase: Phase) -> u8 {
    match phase {
        Phase::Idle => 0,
        Phase::Derivative => 1,
        Phase::LodIssue => 2,
        Phase::Lod => 3,
        Phase::CoordinateIssue => 4,
        Phase::Coordinate => 5,
        Phase::CoefficientIssue => 6,
        Phase::Coefficient => 7,
        Phase::MemberIssue => 8,
        Phase::Member => 9,
        Phase::PacketIssue => 10,
        Phase::Packet => 11,
        Phase::Head => 12,
    }
}

#[derive(Clone, Copy)]
struct Edge {
    ce: bool,
    offer: Option<derivative::Input>,
    out_ready: bool,
    in_ready: bool,
    accepted: bool,
    output: Option<i128>,
    phase: u8,
    fault: bool,
}

struct EmuTrace {
    edges: Vec<Edge>,
    received: Vec<i128>,
}

fn run_emu(
    cases: &[(derivative::Input, Vec<i128>)],
    ce: impl Fn(usize) -> bool,
    ready: impl Fn(usize) -> bool,
) -> EmuTrace {
    run_emu_config(cases, BASELINE, ce, ready)
}

fn run_emu_config(
    cases: &[(derivative::Input, Vec<i128>)],
    config: Config,
    ce: impl Fn(usize) -> bool,
    ready: impl Fn(usize) -> bool,
) -> EmuTrace {
    let expected: usize = cases.iter().map(|c| c.1.len()).sum();
    let mut emu = PreparationEmu::with_config(LIMIT, config).expect("serial emulator");
    let mut offered = 0usize;
    let mut received = Vec::new();
    let mut edges = Vec::new();
    for wall in 0..LIMIT as usize {
        let ce = ce(wall);
        let ready = ready(wall);
        let offer = cases.get(offered).map(|c| c.0);
        let step = emu
            .tick(Tick {
                ce,
                input: offer,
                output_ready: ready,
            })
            .expect("serial preparation emulator tick");
        if step.accepted {
            offered += 1;
        }
        if step.transferred {
            received.push(step.output.expect("transferred packet"));
        }
        edges.push(Edge {
            ce,
            offer,
            out_ready: ready,
            in_ready: step.input_ready,
            accepted: step.accepted,
            output: step.output,
            phase: phase_code(step.phase),
            fault: emu.faulted(),
        });
        if offered == cases.len() && emu.idle() && received.len() == expected {
            return EmuTrace { edges, received };
        }
    }
    panic!("serial preparation emulator watchdog");
}

// ---------------------------------------------------------------------------
// Independent reference agreement (always run).
// ---------------------------------------------------------------------------

#[test]
fn serial_rtl_inventory_and_widths_are_verified() {
    serial_rtl::audit().expect("serial preparation RTL audit");
    let plan = serial_rtl::build().expect("serial preparation RTL build");
    assert_eq!(
        plan.widths,
        serial_rtl::Widths {
            uv: 128,
            pending_lod: 54,
            lod_context: 83,
            coefficient: 171,
            member: 92,
            head: 72,
        }
    );
    assert_eq!(plan.widths.data(), 600);
    assert_eq!(serial_rtl::CONTROLLER_DATA_BITS, 600);
    assert_eq!(serial_rtl::CONTROLLER_CONTROL_BITS, 11);
    // The three kernel calendars are re-derived, not copied from the baseline.
    assert_eq!(plan.d.inventory.numeric_bits, 858);
    assert_eq!(plan.lod.inventory.numeric_bits, 264);
    assert_eq!(plan.coord.inventory.numeric_bits, 716);
    assert_eq!(plan.d.inventory.ii, 8);
    assert_eq!(plan.lod.inventory.ii, 8);
    assert_eq!(plan.coord.inventory.ii, 2);
    println!(
        "controller data={} control={} uv={} pending_lod={} lod_ctx={} coef={} member={} head={}",
        plan.widths.data(),
        serial_rtl::CONTROLLER_CONTROL_BITS,
        plan.widths.uv,
        plan.widths.pending_lod,
        plan.widths.lod_context,
        plan.widths.coefficient,
        plan.widths.member,
        plan.widths.head,
    );
    println!(
        "kernels D(numeric={},span={},period={},ii={}) LOD(numeric={},span={},period={},ii={}) COORD(numeric={},span={},period={},ii={})",
        plan.d.inventory.numeric_bits,
        plan.d.inventory.span,
        plan.d.inventory.period,
        plan.d.inventory.ii,
        plan.lod.inventory.numeric_bits,
        plan.lod.inventory.span,
        plan.lod.inventory.period,
        plan.lod.inventory.ii,
        plan.coord.inventory.numeric_bits,
        plan.coord.inventory.span,
        plan.coord.inventory.period,
        plan.coord.inventory.ii,
    );
}

#[test]
fn serial_reference_matches_closed_bound_packets() {
    let cases = cases();
    assert!(cases.iter().any(|c| !c.1.is_empty()), "no packets");
    let expected: Vec<i128> = cases.iter().flat_map(|c| c.1.iter().copied()).collect();
    let fast = run_emu(&cases, |_| true, |_| true);
    assert_eq!(fast.received, expected, "emulator fast-path packets");
    let throttled = run_emu(&cases, |w| w % 5 != 2, |w| w % 7 != 3);
    assert_eq!(throttled.received, expected, "emulator throttled packets");
    let stalled = run_emu(&cases, |w| w % 3 != 1, |w| w % 23 >= 6);
    assert_eq!(stalled.received, expected, "emulator stalled packets");
    for trace in [&throttled, &stalled] {
        assert!(trace.edges.iter().any(|e| !e.ce), "CE never low");
        assert!(
            trace.edges.iter().any(|e| !e.out_ready),
            "output never stalled"
        );
    }
    for trace in [&fast, &throttled, &stalled] {
        assert!(trace.edges.iter().all(|e| !e.fault), "unexpected fault");
    }
    println!(
        "serial emulator edge counts fast={} throttled={} stalled={}",
        fast.edges.len(),
        throttled.edges.len(),
        stalled.edges.len()
    );
}

#[test]
fn serial_rtl_config_inventories_and_anchors_are_verified() {
    for (label, config) in CONFIGS {
        serial_rtl::audit_with_config(config).unwrap_or_else(|e| panic!("{label} audit: {e}"));
        let plan = serial_rtl::build_with_config(config).expect("serial preparation RTL build");
        assert_eq!(
            (plan.config.nearest_bypass, plan.config.short_alignment),
            (config.nearest_bypass, config.short_alignment),
            "{label} config roundtrip"
        );
        // The additive controller bank is unchanged by either option.
        assert_eq!(plan.widths.data(), 600, "{label} controller data");
        assert_eq!(plan.widths.head, 72, "{label} controller head");
        if config.short_alignment {
            assert_eq!(plan.membership.data_bits, 464, "{label} membership data");
            assert_eq!(
                plan.membership.control_bits, 6,
                "{label} membership control"
            );
            assert_eq!(plan.membership.latency, 5, "{label} membership latency");
            assert_eq!(plan.packet.data_bits, 241, "{label} packet data");
            assert_eq!(plan.packet.control_bits, 4, "{label} packet control");
            assert_eq!(plan.packet.latency, 3, "{label} packet latency");
        } else {
            assert_eq!(plan.membership.data_bits, 648, "{label} membership data");
            assert_eq!(
                plan.membership.control_bits, 8,
                "{label} membership control"
            );
            assert_eq!(plan.membership.latency, 7, "{label} membership latency");
            assert_eq!(plan.packet.data_bits, 673, "{label} packet data");
            assert_eq!(plan.packet.control_bits, 10, "{label} packet control");
            assert_eq!(plan.packet.latency, 9, "{label} packet latency");
        }
        assert_eq!(
            u32::from(plan.membership.alignment_banks) + u32::from(plan.packet.alignment_banks),
            if config.short_alignment { 0 } else { 8 },
            "{label} retained alignment banks"
        );
        // Constructor-only source: structural RTL, never a sampled answer.
        assert!(plan
            .source
            .contains("module gpu_v2_texture_serial_preparation ("));
        assert!(plan.source.contains("localparam NEAREST_BYPASS"));
        println!(
            "{label} controller=600 membership={}/{} packet={}/{} banks={}",
            plan.membership.data_bits,
            plan.membership.control_bits,
            plan.packet.data_bits,
            plan.packet.control_bits,
            plan.membership.alignment_banks + plan.packet.alignment_banks,
        );
    }
}

#[test]
fn serial_reference_matches_closed_bound_packets_for_all_configs() {
    let cases = cases();
    let expected: Vec<i128> = cases.iter().flat_map(|c| c.1.iter().copied()).collect();
    for (label, config) in CONFIGS {
        let fast = run_emu_config(&cases, config, |_| true, |_| true);
        assert_eq!(fast.received, expected, "{label} fast packets");
        let throttled = run_emu_config(&cases, config, |w| w % 5 != 2, |w| w % 7 != 3);
        assert_eq!(throttled.received, expected, "{label} throttled packets");
        let stalled = run_emu_config(&cases, config, |w| w % 3 != 1, |w| w % 23 >= 6);
        assert_eq!(stalled.received, expected, "{label} stalled packets");
        for trace in [&fast, &throttled, &stalled] {
            assert!(
                trace.edges.iter().all(|e| !e.fault),
                "{label} unexpected fault"
            );
        }
        assert!(
            throttled.edges.iter().any(|e| !e.ce),
            "{label} CE never low"
        );
        assert!(
            stalled.edges.iter().any(|e| !e.out_ready),
            "{label} output never stalled"
        );
        println!(
            "{label} edges fast={} throttled={} stalled={}",
            fast.edges.len(),
            throttled.edges.len(),
            stalled.edges.len()
        );
    }
}

/// Four-lane-covered (mask 15) nearest fixture. Every lane selects nearest, so
/// the bypass is exercised for all four covered lanes.
fn nearest_cases() -> Vec<(derivative::Input, Vec<i128>)> {
    let mut q = support::input(6, Filter::Nearest, [-0.004, 0.999]);
    q.mask = 15;
    q.quad_id = 0;
    let slot = support::slot(6, true);
    let expected = bound::prepare(&q, &[slot])
        .expect("closed preparation")
        .payloads;
    assert_eq!(expected.len(), 4, "four covered lanes -> four packets");
    let input = derivative::Input::capture(&q, slot).expect("derivative capture");
    vec![(input, expected)]
}

#[test]
fn nearest_bypass_fixture_matches_edge_counts_and_packets() {
    let cases = nearest_cases();
    let expected: Vec<i128> = cases.iter().flat_map(|c| c.1.iter().copied()).collect();
    let baseline = run_emu_config(&cases, BASELINE, |_| true, |_| true);
    let bypass = run_emu_config(&cases, NEAREST_BYPASS, |_| true, |_| true);
    assert_eq!(baseline.received, expected, "baseline packets");
    assert_eq!(bypass.received, expected, "nearest bypass packets");
    let base_ce = baseline.edges.iter().filter(|e| e.ce).count();
    let bypass_ce = bypass.edges.iter().filter(|e| e.ce).count();
    assert_eq!(base_ce, 222, "baseline enabled edges");
    assert_eq!(bypass_ce, 173, "nearest bypass enabled edges");
    println!(
        "nearest fixture baseline_ce={base_ce} bypass_ce={bypass_ce} packets={}",
        expected.len()
    );
}

// ---------------------------------------------------------------------------
// Bounded Icarus co-simulation of the generated controller.
// ---------------------------------------------------------------------------

fn v18(value: i64) -> String {
    if value < 0 {
        format!("-18'sd{}", -(value as i128))
    } else {
        format!("18'sd{value}")
    }
}

fn v16(value: i64) -> String {
    if value < 0 {
        format!("-16'sd{}", -(value as i128))
    } else {
        format!("16'sd{value}")
    }
}

fn serial_tb(edges: &[Edge], name: &str) -> String {
    let mut s = String::new();
    s.push_str("module tb;\n");
    s.push_str("reg clk=0, reset=1, ce=0, in_valid=0, out_ready=0;\n");
    s.push_str("reg signed [17:0] in_uv_0=0,in_uv_1=0,in_uv_2=0,in_uv_3=0;\n");
    s.push_str("reg signed [17:0] in_uv_4=0,in_uv_5=0,in_uv_6=0,in_uv_7=0;\n");
    s.push_str("reg signed [15:0] in_bias=0;\n");
    s.push_str("reg [3:0] in_quad=0,in_mask=0,in_slot=0,in_max_n=0;\n");
    s.push_str("reg in_force_coarsest=0;reg in_has_mip=0;\nreg [1:0] in_filter=0;\n");
    s.push_str("wire in_ready,in_accept,out_valid,fault;\n");
    s.push_str("wire [71:0] out_packet;\nwire [3:0] out_state;\n");
    s.push_str(&format!(
        "{} dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),\
         .in_uv_0(in_uv_0),.in_uv_1(in_uv_1),.in_uv_2(in_uv_2),.in_uv_3(in_uv_3),\
         .in_uv_4(in_uv_4),.in_uv_5(in_uv_5),.in_uv_6(in_uv_6),.in_uv_7(in_uv_7),\
         .in_bias(in_bias),.in_quad(in_quad),.in_mask(in_mask),.in_slot(in_slot),\
         .in_max_n(in_max_n),.in_has_mip(in_has_mip),.in_filter(in_filter),.in_force_coarsest(in_force_coarsest),\
         .in_ready(in_ready),.in_accept(in_accept),.out_ready(out_ready),\
         .out_valid(out_valid),.out_packet(out_packet),.out_state(out_state),.fault(fault));\n",
        serial_rtl::TOP
    ));
    s.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
    s.push_str(
        "initial begin\nclk=0;reset=1;ce=0;in_valid=0;out_ready=0;#1;clk=1;#1;clk=0;#1;reset=0;\n",
    );
    for (index, edge) in edges.iter().enumerate() {
        let input = edge.offer;
        s.push_str(&format!(
            "in_force_coarsest={};\n",
            u8::from(input.is_some_and(|v| v.force_coarsest))
        ));
        let uv = |i: usize| input.map_or(0i64, |v| v.uv[i]);
        s.push_str(&format!(
            "ce={};in_valid={};out_ready={};in_quad=4'd{};in_mask=4'd{};in_slot=4'd{};\
             in_max_n=4'd{};in_has_mip={};in_filter=2'd{};\
             in_uv_0={};in_uv_1={};in_uv_2={};in_uv_3={};\
             in_uv_4={};in_uv_5={};in_uv_6={};in_uv_7={};in_bias={};\n",
            u8::from(edge.ce),
            u8::from(input.is_some()),
            u8::from(edge.out_ready),
            input.map_or(0, |v| v.header.quad),
            input.map_or(0, |v| v.header.mask),
            input.map_or(0, |v| v.header.slot),
            input.map_or(0, |v| v.header.max_n),
            u8::from(input.is_some_and(|v| v.header.has_mip)),
            input.map_or(0, |v| v.header.filter),
            v18(uv(0)),
            v18(uv(1)),
            v18(uv(2)),
            v18(uv(3)),
            v18(uv(4)),
            v18(uv(5)),
            v18(uv(6)),
            v18(uv(7)),
            v16(input.map_or(0, |v| i64::from(v.bias))),
        ));
        s.push_str("#1;\n");
        s.push_str(&format!(
            "if (in_ready !== 1'b{}) $fatal(1,\"in_ready edge {index}\");\n",
            u8::from(edge.in_ready)
        ));
        s.push_str(&format!(
            "if (in_accept !== 1'b{}) $fatal(1,\"in_accept edge {index}\");\n",
            u8::from(edge.accepted)
        ));
        s.push_str(&format!(
            "if (out_valid !== 1'b{}) $fatal(1,\"out_valid edge {index}\");\n",
            u8::from(edge.output.is_some())
        ));
        if let Some(packet) = edge.output {
            s.push_str(&format!(
                "if (out_valid === 1'b1 && out_packet !== 72'h{:018x}) $fatal(1,\"packet edge {index}\");\n",
                packet as u128
            ));
        }
        s.push_str(&format!(
            "if (fault !== 1'b{}) $fatal(1,\"fault edge {index}\");\n",
            u8::from(edge.fault)
        ));
        s.push_str("#1;clk=1;#1;\n");
        s.push_str(&format!(
            "if (out_state !== 4'd{}) $fatal(1,\"state edge {index}\");\n",
            edge.phase
        ));
        s.push_str("clk=0;#1;\n");
    }
    s.push_str(&format!(
        "$display(\"PASS {name} edges={}\");$finish;end endmodule\n",
        edges.len()
    ));
    s
}

/// Run `command` with a wall-clock watchdog. Output goes to `{name}.out` and
/// `{name}.err` (never pipes); a timeout kills and reaps the child.
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
                "{name} failed:\n{}\n{}",
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

fn run_iverilog(subdir: &str, tb: &str, top: &str) -> String {
    run_iverilog_config(subdir, tb, top, BASELINE)
}

fn run_iverilog_config(subdir: &str, tb: &str, top: &str, config: Config) -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/opencode/serial-opt-rtl-20261006")
        .join(subdir);
    std::fs::create_dir_all(&dir).unwrap();
    let rtl = serial_rtl::source_with_config(config).expect("serial preparation RTL");
    std::fs::write(dir.join("design.v"), &rtl).unwrap();
    std::fs::write(dir.join("tb.v"), tb).unwrap();
    let compiler = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let runtime = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let mut compile = std::process::Command::new(&compiler);
    compile
        .current_dir(&dir)
        .args(["-g2012".to_string(), "-s".to_string(), "tb".to_string()]);
    compile
        .arg("-o")
        .arg("test.vvp")
        .arg("design.v")
        .arg("tb.v");
    bounded(compile, &dir, "compile", Duration::from_secs(600));
    let mut run = std::process::Command::new(&runtime);
    run.current_dir(&dir).arg("test.vvp");
    bounded(run, &dir, "run", Duration::from_secs(600));
    let stdout = std::fs::read_to_string(dir.join("run.out")).unwrap();
    assert!(
        stdout.contains("PASS"),
        "vvp {top} did not pass: {stdout}\n{}",
        std::fs::read_to_string(dir.join("run.err")).unwrap()
    );
    stdout
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_serial_fast_matches_emulator_edges() {
    let cases = cases();
    let trace = run_emu(&cases, |_| true, |_| true);
    let tb = serial_tb(&trace.edges, "serial-fast");
    let stdout = run_iverilog("fast", &tb, serial_rtl::TOP);
    assert!(stdout.contains("PASS serial-fast edges="));
    println!("{stdout}");
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_serial_throttled_matches_emulator_edges() {
    let cases = cases();
    let trace = run_emu(&cases, |w| w % 5 != 2, |w| w % 7 != 3);
    let tb = serial_tb(&trace.edges, "serial-throttled");
    let stdout = run_iverilog("throttled", &tb, serial_rtl::TOP);
    assert!(stdout.contains("PASS serial-throttled edges="));
    println!("{stdout}");
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_serial_stalled_matches_emulator_edges() {
    let cases = cases();
    let trace = run_emu(&cases, |w| w % 3 != 1, |w| w % 23 >= 6);
    let tb = serial_tb(&trace.edges, "serial-stalled");
    let stdout = run_iverilog("stalled", &tb, serial_rtl::TOP);
    assert!(stdout.contains("PASS serial-stalled edges="));
    println!("{stdout}");
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_serial_all_configs_fast_matches_emulator_edges() {
    let cases = cases();
    for (label, config) in CONFIGS {
        let trace = run_emu_config(&cases, config, |_| true, |_| true);
        let tag = format!("serial-fast-{label}");
        let tb = serial_tb(&trace.edges, &tag);
        let stdout = run_iverilog_config(&format!("fast-{label}"), &tb, serial_rtl::TOP, config);
        assert!(
            stdout.contains(&format!("PASS {tag} edges=")),
            "{label}: {stdout}"
        );
        println!("{stdout}");
    }
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_serial_all_configs_stalled_matches_emulator_edges() {
    let cases = cases();
    for (label, config) in CONFIGS {
        let trace = run_emu_config(&cases, config, |w| w % 3 != 1, |w| w % 23 >= 6);
        let tag = format!("serial-stalled-{label}");
        let tb = serial_tb(&trace.edges, &tag);
        let stdout = run_iverilog_config(&format!("stalled-{label}"), &tb, serial_rtl::TOP, config);
        assert!(
            stdout.contains(&format!("PASS {tag} edges=")),
            "{label}: {stdout}"
        );
        println!("{stdout}");
    }
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_nearest_bypass_fixture_matches_emulator_edges() {
    let cases = nearest_cases();
    for (label, config) in CONFIGS {
        let trace = run_emu_config(&cases, config, |_| true, |_| true);
        let tag = format!("serial-nearest-{label}");
        let tb = serial_tb(&trace.edges, &tag);
        let stdout = run_iverilog_config(&format!("nearest-{label}"), &tb, serial_rtl::TOP, config);
        assert!(
            stdout.contains(&format!("PASS {tag} edges=")),
            "{label}: {stdout}"
        );
        println!("{stdout}");
    }
}

// ---------------------------------------------------------------------------
// Fault / reset boundary.
// ---------------------------------------------------------------------------

fn fault_tb() -> String {
    let mut s = String::new();
    s.push_str("module tb;\n");
    s.push_str("reg clk=0, reset=0, ce=0, in_valid=0, out_ready=0;\n");
    s.push_str("reg signed [17:0] in_uv_0=0,in_uv_1=0,in_uv_2=0,in_uv_3=0;\n");
    s.push_str("reg signed [17:0] in_uv_4=0,in_uv_5=0,in_uv_6=0,in_uv_7=0;\n");
    s.push_str("reg signed [15:0] in_bias=0;\n");
    s.push_str("reg [3:0] in_quad=0,in_mask=0,in_slot=0,in_max_n=0;\n");
    s.push_str("reg in_force_coarsest=0;reg in_has_mip=0;\nreg [1:0] in_filter=0;\n");
    s.push_str("wire in_ready,in_accept,out_valid,fault;\n");
    s.push_str("wire [71:0] out_packet;\nwire [3:0] out_state;\n");
    s.push_str(&format!(
        "{} dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),\
         .in_uv_0(in_uv_0),.in_uv_1(in_uv_1),.in_uv_2(in_uv_2),.in_uv_3(in_uv_3),\
         .in_uv_4(in_uv_4),.in_uv_5(in_uv_5),.in_uv_6(in_uv_6),.in_uv_7(in_uv_7),\
         .in_bias(in_bias),.in_quad(in_quad),.in_mask(in_mask),.in_slot(in_slot),\
         .in_max_n(in_max_n),.in_has_mip(in_has_mip),.in_filter(in_filter),.in_force_coarsest(in_force_coarsest),\
         .in_ready(in_ready),.in_accept(in_accept),.out_ready(out_ready),\
         .out_valid(out_valid),.out_packet(out_packet),.out_state(out_state),.fault(fault));\n",
        serial_rtl::TOP
    ));
    s.push_str("initial begin #200000; $fatal(1,\"simulation watchdog\"); end\n");
    // Reset boundary.
    s.push_str("initial begin\nclk=0;reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    // Filter=3 is outside the admitted 0..2 encoding and must latch a terminal fault.
    s.push_str("ce=1;in_valid=1;in_filter=2'd3;in_mask=4'd15;in_max_n=4'd6;#1;\n");
    s.push_str("if (fault !== 1'b0) $fatal(1,\"fault early\");\n");
    s.push_str("if (in_ready !== 1'b1) $fatal(1,\"ready before fault\");\n");
    s.push_str("clk=1;#1;clk=0;#1;\n");
    s.push_str("if (fault !== 1'b1) $fatal(1,\"fault not latched\");\n");
    s.push_str("if (in_ready !== 1'b0) $fatal(1,\"admission not blocked\");\n");
    s.push_str("if (out_valid !== 1'b0) $fatal(1,\"output not blocked\");\n");
    // A subsequent valid offer must stay blocked.
    s.push_str("ce=1;in_valid=1;in_filter=2'd1;#1;clk=1;#1;clk=0;#1;\n");
    s.push_str("if (in_ready !== 1'b0) $fatal(1,\"admission after fault\");\n");
    // Reset clears the terminal fault and the state.
    s.push_str("reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;#1;\n");
    s.push_str("if (fault !== 1'b0) $fatal(1,\"fault not cleared\");\n");
    s.push_str("if (out_valid !== 1'b0) $fatal(1,\"output after reset\");\n");
    s.push_str("if (out_state !== 4'd0) $fatal(1,\"state after reset\");\n");
    // A clean quad proceeds after reset.
    s.push_str(
        "ce=1;in_valid=1;in_filter=2'd0;in_mask=4'd1;in_max_n=4'd6;in_quad=4'd0;in_slot=4'd0;#1;\n",
    );
    s.push_str("if (in_ready !== 1'b1) $fatal(1,\"ready after reset\");\n");
    s.push_str("clk=1;#1;clk=0;#1;\n");
    s.push_str("if (out_state !== 4'd1) $fatal(1,\"derivative not entered\");\n");
    s.push_str("$display(\"PASS serial-fault\");$finish;end endmodule\n");
    s
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_serial_fault_blocks_and_reset_clears() {
    let stdout = run_iverilog("fault", &fault_tb(), serial_rtl::TOP);
    assert!(stdout.contains("PASS serial-fault"));
    println!("{stdout}");
}
