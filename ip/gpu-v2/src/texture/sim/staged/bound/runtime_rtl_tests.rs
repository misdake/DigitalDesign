use super::super::runtime_membership::{
    self, Input as MembershipInput, Pipeline as MembershipPipeline,
};
use super::super::runtime_packet::{self, Input as PacketInput, Pipeline as PacketPipeline};
use super::super::transport::Member;
use super::*;
use std::fs::File;
use std::process::Stdio;
use std::time::{Duration, Instant};

const MASK72: u128 = (1u128 << 72) - 1;

fn bit(value: bool) -> u128 {
    u128::from(value as u8)
}

// -----------------------------------------------------------------------
// Independent mathematical goldens. These use explicit bit layout and never
// the shared `Member`/transport emitter, so they cannot inherit its bugs.
// -----------------------------------------------------------------------

/// 92-bit Member word assembled from the membership lane operands.
fn golden_member(input: &MembershipInput) -> u128 {
    let coords = input.coordinates;
    let tiles = [
        u128::from(coords[0] >> 3),
        u128::from(coords[1] >> 3),
        u128::from(coords[2] >> 3),
        u128::from(coords[3] >> 3),
    ];
    let local = [u128::from(coords[0] & 7), u128::from(coords[2] & 7)];
    let same_x = tiles[0] == tiles[1];
    let same_y = tiles[2] == tiles[3];
    let nonzero: [bool; 4] = std::array::from_fn(|t| input.weights[t] != 0);
    let emit = [
        nonzero[0],
        nonzero[1] && !(nonzero[0] && same_x),
        nonzero[2] && !((nonzero[0] && same_y) || (nonzero[1] && (same_x && same_y))),
        nonzero[3]
            && !((nonzero[0] && (same_x && same_y))
                || (nonzero[1] && same_y)
                || (nonzero[2] && same_x)),
    ];
    let final_plane = !input.fine || input.last_fine;
    let mut word = 0u128;
    for (t, weight) in input.weights.iter().enumerate() {
        word |= u128::from(*weight) << (9 * t);
    }
    for (t, value) in emit.iter().enumerate() {
        word |= bit(*value) << (36 + t);
    }
    word |= tiles[0] << 40
        | tiles[1] << 47
        | tiles[2] << 54
        | tiles[3] << 61
        | local[0] << 68
        | local[1] << 71
        | bit(same_x) << 74
        | bit(same_y) << 75
        | u128::from(input.slot) << 76
        | u128::from(input.level) << 80
        | u128::from(input.key / 4) << 84
        | u128::from(input.key % 4) << 88
        | bit(input.fine) << 90
        | bit(final_plane) << 91;
    word
}

/// 72-bit packed packet word from the captured Member and selected tap.
fn golden_packet(member: u128, tap: u8) -> u128 {
    let cap = (member & ((1u128 << 36) - 1)) | ((member >> 37) << 36) | (u128::from(tap) << 91);
    let field = |bit: u32, width: u32| (cap >> bit) & ((1u128 << width) - 1);
    let x = field(91, 1) != 0;
    let y = field(92, 1) != 0;
    let same = [field(73, 1) != 0, field(74, 1) != 0];
    let tile_x = if x { field(46, 7) } else { field(39, 7) };
    let tile_y = if y { field(60, 7) } else { field(53, 7) };
    let header = field(75, 4)
        | (field(79, 4) << 4)
        | (tile_x << 8)
        | (tile_y << 15)
        | (field(67, 3) << 22)
        | (field(70, 3) << 25);
    let weights: [u128; 4] = std::array::from_fn(|i| field(9 * i as u32, 9));
    let key = (field(83, 4) * 4 + field(87, 2)) as u8;
    let higher = [field(36, 1) != 0, field(37, 1) != 0, field(38, 1) != 0];
    let higher = match tap {
        0 => higher[0] || higher[1] || higher[2],
        1 => higher[1] || higher[2],
        2 => higher[2],
        _ => false,
    };
    let masks: [bool; 4] = std::array::from_fn(|i| {
        ((i & 1) as u8 == u8::from(x) || same[0]) && ((i >> 1) as u8 == u8::from(y) || same[1])
    });
    let first = field(89, 1) != 0 && tap == 0;
    let last = field(90, 1) != 0 && !higher;
    let mut packed = header;
    for (i, (weight, mask)) in weights.iter().zip(masks).enumerate() {
        if mask {
            packed |= weight << (28 + 9 * i);
        }
    }
    packed
        | bit(first) << 64
        | bit(last) << 65
        | u128::from(key / 4) << 66
        | u128::from(key % 4) << 70
}

fn tap_emitted(member: u128, tap: u8) -> bool {
    (member >> (36 + u32::from(tap))) & 1 != 0
}

// -----------------------------------------------------------------------
// Deterministic stimuli.
// -----------------------------------------------------------------------

fn lcg(seed: &mut u64) -> u32 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 33) as u32
}

fn membership_cases() -> Vec<MembershipInput> {
    let mut seed = 0x243f_6a88_85a3_08d3u64;
    // Deliberate corners: max coordinate 1023, the 7/8 tile boundary, and a
    // single-nonzero plane that guarantees a non-emitted tap elsewhere.
    let corners = [
        [1023u16, 1023, 1023, 1023],
        [7, 8, 7, 8],
        [8, 7, 8, 7],
        [0, 0, 0, 0],
        [1023, 0, 1023, 0],
        [7, 7, 8, 8],
    ];
    let mut cases = Vec::new();
    for (index, coordinates) in corners.into_iter().enumerate() {
        cases.push(MembershipInput {
            weights: [7, 0, 0, 0],
            coordinates,
            slot: (index % 16) as u8,
            level: (index % 16) as u8,
            key: (index % 64) as u8,
            fine: index % 2 == 0,
            last_fine: index % 3 == 0,
        });
    }
    while cases.len() < 120 {
        let index = cases.len() as u32;
        let dup_x = index.is_multiple_of(3);
        let dup_y = index.is_multiple_of(4);
        let base_x = lcg(&mut seed) % 1024;
        let base_y = lcg(&mut seed) % 1024;
        let x1 = if dup_x {
            base_x
        } else {
            (base_x + 1 + lcg(&mut seed) % 8) % 1024
        };
        let y1 = if dup_y {
            base_y
        } else {
            (base_y + 1 + lcg(&mut seed) % 8) % 1024
        };
        let mut weights = [
            (lcg(&mut seed) % 512) as u16,
            (lcg(&mut seed) % 512) as u16,
            (lcg(&mut seed) % 512) as u16,
            (lcg(&mut seed) % 512) as u16,
        ];
        if index.is_multiple_of(5) {
            weights[1] = 0;
        }
        if index.is_multiple_of(7) {
            weights[2] = 0;
        }
        if index.is_multiple_of(11) {
            weights[3] = 0;
        }
        if weights == [0; 4] {
            weights[0] = 1 + (lcg(&mut seed) % 511) as u16;
        }
        cases.push(MembershipInput {
            weights,
            coordinates: [base_x as u16, x1 as u16, base_y as u16, y1 as u16],
            slot: (lcg(&mut seed) % 16) as u8,
            level: (lcg(&mut seed) % 16) as u8,
            key: (lcg(&mut seed) % 64) as u8,
            fine: lcg(&mut seed) & 1 == 0,
            last_fine: lcg(&mut seed) & 1 == 0,
        });
    }
    cases
}

#[derive(Clone, Copy, Debug)]
struct MembershipEdge {
    ce: bool,
    input: Option<MembershipInput>,
    expect: Option<u128>,
}

#[derive(Clone, Copy, Debug)]
struct PacketEdge {
    ce: bool,
    input: Option<(Member, u8)>,
    expect: Option<u128>,
}

/// Run the private membership reference on a CE-paused, bubble-filled
/// schedule and record the pre-edge published word for every edge. Also
/// returns the ordered published `Member`s for the packet stimulus.
fn membership_run(max_wall: usize) -> (Vec<MembershipEdge>, Vec<Member>) {
    let cases = membership_cases();
    let mut pipe = MembershipPipeline::default();
    let mut ptr = 0usize;
    let mut edges = Vec::new();
    let mut published = Vec::new();
    for wall in 0..max_wall {
        let ce = !(wall % 17 == 13 || wall % 17 == 14 || matches!(wall % 23, 5 | 6));
        let offer = ptr < cases.len() && ce && wall % 11 != 9;
        let input = offer.then(|| cases[ptr]);
        let published_member = pipe.output();
        if ce {
            if let Some(member) = published_member {
                published.push(member);
            }
        }
        let expect = published_member.map(Member::bits);
        let step = pipe.tick(ce, input).expect("membership private reference");
        if ce {
            assert_eq!(
                step.map(Member::bits),
                expect,
                "membership reference pre-edge output mismatch"
            );
        } else {
            assert!(
                step.is_none(),
                "membership reference returned output while ce=0"
            );
        }
        if offer {
            ptr += 1;
        }
        edges.push(MembershipEdge { ce, input, expect });
        if ptr == cases.len() && pipe.inflight() == 0 {
            break;
        }
    }
    assert_eq!(ptr, cases.len(), "membership stimulus omitted a lane");
    assert_eq!(pipe.inflight(), 0, "membership reference did not drain");
    (edges, published)
}

fn choose_tap(member: u128, index: usize) -> u8 {
    let taps: Vec<u8> = (0..4u8).filter(|&tap| tap_emitted(member, tap)).collect();
    assert!(!taps.is_empty(), "member has no emitted tap");
    taps[index % taps.len()]
}

fn packet_run(published: &[Member], max_wall: usize) -> Vec<PacketEdge> {
    let mut pipe = PacketPipeline::default();
    let mut ptr = 0usize;
    let mut edges = Vec::new();
    for wall in 0..max_wall {
        let ce = !(wall % 19 == 7 || wall % 19 == 8 || wall % 13 == 3);
        let offer = ptr < published.len() && ce && wall % 9 != 6;
        let input = offer.then(|| {
            let member = published[ptr];
            (member, choose_tap(member.bits(), ptr))
        });
        let expect = pipe.output().map(|value| (value as u128) & MASK72);
        let step = pipe
            .tick(ce, input.map(|(member, tap)| PacketInput { member, tap }))
            .expect("packet private reference");
        if ce {
            assert_eq!(
                step.map(|value| (value as u128) & MASK72),
                expect,
                "packet reference pre-edge output mismatch"
            );
        } else {
            assert!(
                step.is_none(),
                "packet reference returned output while ce=0"
            );
        }
        if offer {
            ptr += 1;
        }
        edges.push(PacketEdge { ce, input, expect });
        if ptr == published.len() && pipe.inflight() == 0 {
            break;
        }
    }
    assert_eq!(ptr, published.len(), "packet stimulus omitted a member");
    assert_eq!(pipe.inflight(), 0, "packet reference did not drain");
    edges
}

fn membership_golden_trajectory(edges: &[MembershipEdge]) -> Vec<Option<u128>> {
    let latency = usize::from(MEMBERSHIP_LATENCY);
    let mut accepted: Vec<Option<u128>> = Vec::new();
    let mut held = None;
    let mut result = Vec::with_capacity(edges.len());
    for edge in edges {
        result.push(held);
        if edge.ce {
            accepted.push(edge.input.map(|input| golden_member(&input)));
            held = if accepted.len() >= latency {
                accepted[accepted.len() - latency]
            } else {
                None
            };
        }
    }
    result
}

fn packet_golden_trajectory(edges: &[PacketEdge]) -> Vec<Option<u128>> {
    let latency = usize::from(PACKET_LATENCY);
    let mut accepted: Vec<Option<u128>> = Vec::new();
    let mut held = None;
    let mut result = Vec::with_capacity(edges.len());
    for edge in edges {
        result.push(held);
        if edge.ce {
            accepted.push(
                edge.input
                    .map(|(member, tap)| golden_packet(member.bits(), tap)),
            );
            held = if accepted.len() >= latency {
                accepted[accepted.len() - latency]
            } else {
                None
            };
        }
    }
    result
}

fn single_member(weights: [u16; 4], coordinates: [u16; 4]) -> Member {
    let input = MembershipInput {
        weights,
        coordinates,
        slot: 0,
        level: 0,
        key: 0,
        fine: false,
        last_fine: false,
    };
    let mut pipe = MembershipPipeline::default();
    let mut offered = Some(input);
    for _ in 0..=MEMBERSHIP_LATENCY {
        let step = pipe
            .tick(true, offered.take())
            .expect("single membership lane");
        if let Some(member) = step {
            return member;
        }
    }
    panic!("membership lane did not publish");
}

// -----------------------------------------------------------------------
// Rust-only tests.
// -----------------------------------------------------------------------

#[test]
fn declarations_bind_rtl_widths_to_the_private_pipelines() {
    timing::membership_audit(MEMBERSHIP_RTL).expect("membership rtl audit");
    timing::packet_audit(PACKET_RTL).expect("packet rtl audit");
    assert_eq!(MEMBERSHIP_DATA_BITS, runtime_membership::DATA_BITS as u32);
    assert_eq!(
        MEMBERSHIP_CONTROL_BITS,
        runtime_membership::CONTROL_BITS as u32
    );
    assert_eq!(PACKET_DATA_BITS, runtime_packet::DATA_BITS as u32);
    assert_eq!(PACKET_CONTROL_BITS, runtime_packet::CONTROL_BITS as u32);
    assert_eq!(MEMBERSHIP_LATENCY as usize, timing::MEMBERSHIP_STAGES.len());
    assert_eq!(PACKET_LATENCY as usize, timing::PACKET_STAGES.len());
    assert_eq!(MEMBERSHIP_RTL.matches("endmodule").count(), 1);
    assert_eq!(PACKET_RTL.matches("endmodule").count(), 1);
    // Inventory evidence; run with --nocapture to capture it.
    for declaration in [&timing::MEMBERSHIP, &timing::PACKET] {
        let stage_bits: u32 = declaration.stages.iter().map(|s| s.data_bits).sum();
        println!(
            "{} latency {} II {} data {} control {} stages {} stage_bits {}",
            declaration.name,
            declaration.latency,
            declaration.initiation_interval,
            declaration.data_bits,
            declaration.control_bits,
            declaration.stages.len(),
            stage_bits
        );
        for stage in declaration.stages {
            println!("  {}: {} bits", stage.name, stage.data_bits);
        }
    }
}

#[test]
fn membership_reference_matches_independent_golden() {
    let (edges, _published) = membership_run(4000);
    let golden = membership_golden_trajectory(&edges);
    assert!(edges.iter().any(|edge| edge.expect.is_some()));
    for (index, (edge, expected)) in edges.iter().zip(&golden).enumerate() {
        assert_eq!(edge.expect, *expected, "membership golden edge {index}");
    }
}

#[test]
fn packet_reference_matches_independent_golden() {
    let (_membership_edges, published) = membership_run(4000);
    assert!(!published.is_empty(), "no membership output to packetize");
    let edges = packet_run(&published, 8000);
    let golden = packet_golden_trajectory(&edges);
    assert!(edges.iter().any(|edge| edge.expect.is_some()));
    for (index, (edge, expected)) in edges.iter().zip(&golden).enumerate() {
        assert_eq!(edge.expect, *expected, "packet golden edge {index}");
    }
}

#[test]
fn membership_inactive_plane_is_terminal_until_recreated() {
    let mut pipe = MembershipPipeline::default();
    let inactive = MembershipInput {
        weights: [0; 4],
        coordinates: [1023, 0, 7, 8],
        slot: 0,
        level: 0,
        key: 63,
        fine: true,
        last_fine: false,
    };
    assert!(pipe.tick(true, Some(inactive)).is_err(), "inactive plane");
    let good = MembershipInput {
        weights: [1, 0, 0, 0],
        coordinates: [0, 0, 0, 0],
        slot: 0,
        level: 0,
        key: 0,
        fine: false,
        last_fine: false,
    };
    assert!(pipe.tick(true, Some(good)).is_err(), "fault must latch");
    // A fresh instance is the reset boundary: it accepts the good lane.
    let mut fresh = MembershipPipeline::default();
    let mut offered = Some(good);
    let mut published = false;
    for _ in 0..=MEMBERSHIP_LATENCY {
        let step = fresh.tick(true, offered.take()).expect("fresh lane");
        published |= step.is_some();
    }
    assert!(published, "fresh membership pipeline did not drain");
}

#[test]
fn packet_non_emitted_tap_is_terminal_until_recreated() {
    // Emit only bit0: coordinates all zero make every tile equal, so only
    // the first nonzero corner survives the representative selection.
    let member = single_member([9, 0, 0, 0], [0, 0, 0, 0]);
    assert_eq!(member.bits() >> 36 & 0xf, 1, "expected a single emit bit");
    let mut pipe = PacketPipeline::default();
    assert!(
        pipe.tick(true, Some(PacketInput { member, tap: 1 }))
            .is_err(),
        "non-emitted tap"
    );
    assert!(pipe.tick(true, None).is_err(), "fault must latch");
    let mut fresh = PacketPipeline::default();
    let step = fresh
        .tick(true, Some(PacketInput { member, tap: 0 }))
        .expect("fresh emitted tap");
    assert!(step.is_none());
}

// -----------------------------------------------------------------------
// Bounded Icarus co-simulation. Expected values come from the private live
// reference; the tb drives one edge at a time with CE pauses and bubbles.
// -----------------------------------------------------------------------

fn bool_bit(value: bool) -> u8 {
    u8::from(value)
}

fn membership_tb(edges: &[MembershipEdge]) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,in_fine=0,in_last_fine=0;\n");
    tb.push_str("reg [8:0] in_w0=0,in_w1=0,in_w2=0,in_w3=0;\n");
    tb.push_str("reg [9:0] in_c0=0,in_c1=0,in_c2=0,in_c3=0;\n");
    tb.push_str("reg [3:0] in_slot=0,in_level=0;\n");
    tb.push_str("reg [5:0] in_key=0;\n");
    tb.push_str("wire in_ready,out_valid,fault;\nwire [91:0] out_member;\n");
    tb.push_str(
        "membership_leaf dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),\
             .in_w0(in_w0),.in_w1(in_w1),.in_w2(in_w2),.in_w3(in_w3),\
             .in_c0(in_c0),.in_c1(in_c1),.in_c2(in_c2),.in_c3(in_c3),\
             .in_slot(in_slot),.in_level(in_level),.in_key(in_key),\
             .in_fine(in_fine),.in_last_fine(in_last_fine),\
             .in_ready(in_ready),.out_valid(out_valid),.out_member(out_member),.fault(fault));\n",
    );
    tb.push_str("initial begin #200000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\nclk=0;reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (edge, slot) in edges.iter().enumerate() {
        let input = slot.input;
        let w = |t: usize| input.map_or(0u16, |v| v.weights[t]);
        let c = |t: usize| input.map_or(0u16, |v| v.coordinates[t]);
        tb.push_str(&format!(
            "ce={};in_valid={};in_w0=9'd{};in_w1=9'd{};in_w2=9'd{};in_w3=9'd{};\
                 in_c0=10'd{};in_c1=10'd{};in_c2=10'd{};in_c3=10'd{};\
                 in_slot=4'd{};in_level=4'd{};in_key=6'd{};in_fine={};in_last_fine={};\n",
            bool_bit(slot.ce),
            bool_bit(input.is_some()),
            w(0),
            w(1),
            w(2),
            w(3),
            c(0),
            c(1),
            c(2),
            c(3),
            input.map_or(0, |v| v.slot),
            input.map_or(0, |v| v.level),
            input.map_or(0, |v| v.key),
            bool_bit(input.is_some_and(|v| v.fine)),
            bool_bit(input.is_some_and(|v| v.last_fine)),
        ));
        tb.push_str("#1;\n");
        tb.push_str(&format!(
            "if (in_ready !== 1'b{}) $fatal(1,\"in_ready edge {}\");\n",
            bool_bit(slot.ce),
            edge
        ));
        tb.push_str(&format!(
            "if (out_valid !== 1'b{}) $fatal(1,\"out_valid edge {}\");\n",
            bool_bit(slot.expect.is_some()),
            edge
        ));
        if let Some(member) = slot.expect {
            tb.push_str(&format!(
                "if (out_valid === 1'b1 && out_member !== 92'h{:023x}) \
                     $fatal(1,\"member edge {}\");\n",
                member, edge
            ));
        }
        tb.push_str("clk=1;#1;clk=0;#1;\n");
    }
    tb.push_str(&format!(
        "$display(\"PASS membership edges={}\");$finish;end endmodule\n",
        edges.len()
    ));
    tb
}

fn packet_tb(edges: &[PacketEdge]) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0;\n");
    tb.push_str("reg [91:0] in_member=0;\nreg [1:0] in_tap=0;\n");
    tb.push_str("wire in_ready,out_valid,fault;\nwire [71:0] out_packet;\n");
    tb.push_str(
        "packet_leaf dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),\
             .in_member(in_member),.in_tap(in_tap),.in_ready(in_ready),\
             .out_valid(out_valid),.out_packet(out_packet),.fault(fault));\n",
    );
    tb.push_str("initial begin #200000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\nclk=0;reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (edge, slot) in edges.iter().enumerate() {
        let member = slot.input.map_or(0u128, |(m, _)| m.bits());
        let tap = slot.input.map_or(0u8, |(_, t)| t);
        tb.push_str(&format!(
            "ce={};in_valid={};in_member=92'h{:023x};in_tap=2'd{};\n",
            bool_bit(slot.ce),
            bool_bit(slot.input.is_some()),
            member,
            tap
        ));
        tb.push_str("#1;\n");
        tb.push_str(&format!(
            "if (in_ready !== 1'b{}) $fatal(1,\"in_ready edge {}\");\n",
            bool_bit(slot.ce),
            edge
        ));
        tb.push_str(&format!(
            "if (out_valid !== 1'b{}) $fatal(1,\"out_valid edge {}\");\n",
            bool_bit(slot.expect.is_some()),
            edge
        ));
        if let Some(packet) = slot.expect {
            tb.push_str(&format!(
                "if (out_valid === 1'b1 && out_packet !== 72'h{:018x}) \
                     $fatal(1,\"packet edge {}\");\n",
                packet, edge
            ));
        }
        tb.push_str("clk=1;#1;clk=0;#1;\n");
    }
    tb.push_str(&format!(
        "$display(\"PASS packet edges={}\");$finish;end endmodule\n",
        edges.len()
    ));
    tb
}

fn membership_fault_tb() -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,in_fine=0,in_last_fine=0;\n");
    tb.push_str("reg [8:0] in_w0=0,in_w1=0,in_w2=0,in_w3=0;\n");
    tb.push_str("reg [9:0] in_c0=0,in_c1=0,in_c2=0,in_c3=0;\n");
    tb.push_str("reg [3:0] in_slot=0,in_level=0;\n");
    tb.push_str("reg [5:0] in_key=0;\n");
    tb.push_str("wire in_ready,out_valid,fault;\nwire [91:0] out_member;\n");
    tb.push_str(
        "membership_leaf dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),\
             .in_w0(in_w0),.in_w1(in_w1),.in_w2(in_w2),.in_w3(in_w3),\
             .in_c0(in_c0),.in_c1(in_c1),.in_c2(in_c2),.in_c3(in_c3),\
             .in_slot(in_slot),.in_level(in_level),.in_key(in_key),\
             .in_fine(in_fine),.in_last_fine(in_last_fine),\
             .in_ready(in_ready),.out_valid(out_valid),.out_member(out_member),.fault(fault));\n",
    );
    tb.push_str("initial begin #200000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\nclk=0;reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    tb.push_str(
        "ce=1;in_valid=1;in_w0=9'd100;in_c0=10'd0;in_c1=10'd8;in_c2=10'd0;in_c3=10'd8;\n\
             #1;clk=1;#1;clk=0;#1;\n",
    );
    tb.push_str("ce=1;in_valid=1;in_w0=9'd0;in_w1=9'd0;in_w2=9'd0;in_w3=9'd0;\n#1;\n");
    tb.push_str("if (fault !== 1'b0) $fatal(1,\"fault early\");\n");
    tb.push_str("if (in_ready !== 1'b1) $fatal(1,\"ready pre-fault\");\n");
    tb.push_str("clk=1;#1;clk=0;#1;\n");
    tb.push_str("if (fault !== 1'b1) $fatal(1,\"fault not latched\");\n");
    tb.push_str("if (in_ready !== 1'b0) $fatal(1,\"admission not blocked\");\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"output not blocked\");\n");
    tb.push_str("ce=1;in_valid=1;in_w0=9'd50;in_w1=9'd3;\n#1;\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"output after fault\");\n");
    tb.push_str("if (in_ready !== 1'b0) $fatal(1,\"admission after fault\");\n");
    tb.push_str("clk=1;#1;clk=0;#1;\n");
    tb.push_str("reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    tb.push_str("if (fault !== 1'b0) $fatal(1,\"fault not cleared\");\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"output after reset\");\n");
    tb.push_str("$display(\"PASS membership-fault\");$finish;end endmodule\n");
    tb
}

fn packet_fault_tb() -> String {
    let member = 5u128 | (1u128 << 36);
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0;\n");
    tb.push_str("reg [91:0] in_member=0;\nreg [1:0] in_tap=0;\n");
    tb.push_str("wire in_ready,out_valid,fault;\nwire [71:0] out_packet;\n");
    tb.push_str(
        "packet_leaf dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),\
             .in_member(in_member),.in_tap(in_tap),.in_ready(in_ready),\
             .out_valid(out_valid),.out_packet(out_packet),.fault(fault));\n",
    );
    tb.push_str("initial begin #200000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\nclk=0;reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    tb.push_str(&format!(
        "ce=1;in_valid=1;in_member=92'h{:023x};in_tap=2'd1;\n#1;\n",
        member
    ));
    tb.push_str("if (fault !== 1'b0) $fatal(1,\"fault early\");\n");
    tb.push_str("if (in_ready !== 1'b1) $fatal(1,\"ready pre-fault\");\n");
    tb.push_str("clk=1;#1;clk=0;#1;\n");
    tb.push_str("if (fault !== 1'b1) $fatal(1,\"fault not latched\");\n");
    tb.push_str("if (in_ready !== 1'b0) $fatal(1,\"admission not blocked\");\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"output not blocked\");\n");
    tb.push_str("ce=1;in_valid=1;in_tap=2'd0;\n#1;\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"output after fault\");\n");
    tb.push_str("if (in_ready !== 1'b0) $fatal(1,\"admission after fault\");\n");
    tb.push_str("clk=1;#1;clk=0;#1;\n");
    tb.push_str("reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    tb.push_str("if (fault !== 1'b0) $fatal(1,\"fault not cleared\");\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"output after reset\");\n");
    tb.push_str("$display(\"PASS packet-fault\");$finish;end endmodule\n");
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
                "{name} failed: {}\n{}",
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

fn run_iverilog(subdir: &str, rtl: &str, tb: &str) -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/opencode/member-packet-rtl-20261006")
        .join(subdir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("leaf.v"), rtl).unwrap();
    std::fs::write(dir.join("tb.v"), tb).unwrap();
    let compiler = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let runtime = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let mut compile = std::process::Command::new(&compiler);
    compile
        .current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp", "leaf.v", "tb.v"]);
    bounded(compile, &dir, "compile", Duration::from_secs(120));
    let mut run = std::process::Command::new(&runtime);
    run.current_dir(&dir).arg("test.vvp");
    bounded(run, &dir, "run", Duration::from_secs(120));
    let stdout = std::fs::read_to_string(dir.join("run.out")).unwrap();
    assert!(
        stdout.contains("PASS"),
        "vvp did not pass: {stdout}\n{}",
        std::fs::read_to_string(dir.join("run.err")).unwrap()
    );
    stdout
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_membership_matches_private_pipeline_every_edge() {
    let (edges, _published) = membership_run(4000);
    let tb = membership_tb(&edges);
    let stdout = run_iverilog("membership", MEMBERSHIP_RTL, &tb);
    assert!(stdout.contains("PASS membership edges="));
    println!("{stdout}");
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_packet_matches_private_pipeline_every_edge() {
    let (_membership_edges, published) = membership_run(4000);
    let edges = packet_run(&published, 8000);
    let tb = packet_tb(&edges);
    let stdout = run_iverilog("packet", PACKET_RTL, &tb);
    assert!(stdout.contains("PASS packet edges="));
    println!("{stdout}");
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_membership_fault_blocks_and_reset_clears() {
    let stdout = run_iverilog("membership_fault", MEMBERSHIP_RTL, &membership_fault_tb());
    assert!(stdout.contains("PASS membership-fault"));
    println!("{stdout}");
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_packet_fault_blocks_and_reset_clears() {
    let stdout = run_iverilog("packet_fault", PACKET_RTL, &packet_fault_tb());
    assert!(stdout.contains("PASS packet-fault"));
    println!("{stdout}");
}
