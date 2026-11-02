mod native_support;
use native_support::run_tool;
// Bridge-only edge replay against the unchanged production RTL and a test peer.
use digital_design_hardware_gowin::sdram_memory_controller::{
    cycle::{gear::*, Edges},
    RtlSources,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy)]
struct Sample {
    edges: Edges,
    i: Input,
    n: Feedback,
}
fn b(v: bool) -> u64 {
    u64::from(v)
}
fn outputs(g: &Gear, i: &Input) -> Vec<u64> {
    let l = g.logic_output(i);
    let c = g.controller_input(i);
    vec![
        b(l.request_ready),
        b(l.stream_active),
        b(l.write_data_ready),
        l.read_data,
        b(l.read_valid),
        b(l.write_done),
        b(l.initialized),
        b(c.reset),
        b(c.request),
        b(c.writing),
        c.address.into(),
        c.words.into(),
        b(c.write_slot),
        b(c.next_valid),
        c.next_address.into(),
        c.next_words.into(),
        b(c.next_chainable),
        b(c.next_write_ready),
        b(c.next_generation),
        c.write_data.into(),
        c.write_mask.into(),
    ]
}
fn inputs(s: &Sample) -> Vec<u64> {
    let i = s.i;
    let n = s.n;
    vec![
        b(s.edges.logic_rise) | b(s.edges.native_rise) << 1 | b(s.edges.native_fall) << 2,
        b(i.reset),
        b(i.request_valid),
        b(i.continuation),
        b(i.writing),
        i.address.into(),
        i.words.into(),
        i.write_mask.into(),
        i.write_data,
        b(i.write_data_valid),
        b(i.next_valid),
        i.next_address.into(),
        b(n.ready),
        b(n.done),
        b(n.initialized),
        b(n.read_valid),
        n.read_data.into(),
        n.write_index.into(),
        n.phase.into(),
        b(n.chain_accept),
        b(n.read_chain_accept),
    ]
}
fn line(values: &[u64]) -> String {
    values
        .iter()
        .map(|x| format!("{x:x}"))
        .collect::<Vec<_>>()
        .join(" ")
        + "\n"
}

/// Deliberately independent controller feedback, not an MC or memory timing model.
/// Aligns write supply to the fitted logic/native edge relation.
#[derive(Default)]
struct Peer {
    active: Option<Input>,
    index: u8,
    writing_started: bool,
    done: bool,
}
impl Peer {
    fn feedback(&self, initialized: bool, c: ControllerInput) -> Feedback {
        let a = self.active;
        Feedback {
            initialized,
            ready: a.is_none(),
            done: self.done,
            read_valid: a.is_some_and(|a| !a.writing),
            read_data: 0xc000_0000 ^ a.map_or(0, |a| a.address << 8) ^ u32::from(self.index),
            write_index: self.index,
            phase: if a.is_some_and(|a| a.writing) && self.writing_started {
                15
            } else if a.is_some_and(|a| !a.writing) {
                18
            } else {
                0
            },
            chain_accept: a.is_some_and(|a| a.writing)
                && self.writing_started
                && a.is_some_and(|a| self.index + 1 == a.words)
                && c.next_chainable
                && c.writing
                && c.next_write_ready,
            read_chain_accept: a.is_some_and(|a| !a.writing)
                && a.is_some_and(|a| self.index + 1 == a.words)
                && c.next_chainable
                && !c.writing,
        }
    }
    fn clock(&mut self, s: &Sample, c: ControllerInput) {
        if !s.edges.native_rise {
            return;
        }
        self.done = false;
        if s.i.reset {
            self.active = None;
            self.index = 0;
            self.writing_started = false;
            return;
        }
        if s.n.chain_accept || s.n.read_chain_accept {
            self.active = Some(Input {
                writing: c.writing,
                address: c.address,
                words: c.words,
                ..Default::default()
            });
            self.index = 0;
            self.writing_started = c.writing;
            return;
        }
        if let Some(a) = self.active {
            if a.writing && !self.writing_started {
                if s.edges.logic_rise {
                    self.writing_started = true;
                }
            } else if self.index + 1 == a.words {
                self.active = None;
                self.done = true;
                self.index = 0;
                self.writing_started = false;
            } else {
                self.index += 1;
            }
        } else if c.request && s.n.ready {
            self.active = Some(Input {
                writing: c.writing,
                address: c.address,
                words: c.words,
                ..Default::default()
            });
            self.index = 0;
        }
    }
}
fn stimulus(cold: bool, late_logic: bool) -> Vec<Sample> {
    let mut jobs = vec![
        (4, false, 32, false),
        (5, false, 32, true),
        (6, false, 32, true),
        (70, true, 8, false),
        (71, true, 8, true),
        (90, false, 1, false),
        (105, true, 1, false),
        (120, true, 16, false),
        (140, false, 8, false),
        (165, false, 32, false),
        (185, true, 8, false),
    ];
    for mask in 0..16 {
        jobs.push((205 + mask * 10, true, 1, false));
    }
    let mut g = Gear::new(cold);
    let mut peer = Peer::default();
    let mut next = 0;
    let mut pending = None;
    let mut supply = 0u32;
    let mut supplied = 0u32;
    let mut data_word = 0u64;
    let mut trace = Vec::new();
    for step in 0..1600 {
        let phase = step % 4;
        let cycle = step / 4;
        let edges = Edges {
            logic_rise: phase == if late_logic { 2 } else { 0 },
            native_rise: phase == 0 || phase == 2,
            native_fall: phase == 1 || phase == 3,
        };
        if edges.logic_rise && pending.is_none() && next < jobs.len() && cycle >= jobs[next].0 {
            let (_, writing, words, continuation) = jobs[next];
            pending = Some(Input {
                request_valid: true,
                continuation,
                writing,
                words,
                address: 0x40 + next as u32 * 64,
                write_mask: if next >= 11 {
                    (next - 11) as u8
                } else if words == 1 {
                    12
                } else {
                    0
                },
                ..Default::default()
            });
            next += 1;
        }
        let mut i = pending.unwrap_or_default();
        i.reset = step < 8 || (671..677).contains(&step);
        i.write_data_valid = supplied < supply && !matches!(cycle, 71 | 72);
        i.next_valid = i.request_valid;
        i.next_address = i.address;
        i.write_data = ((0xb000_0000 ^ data_word) << 32) | (0x1000_0000 ^ data_word);
        let c = g.controller_input(&i);
        let n = peer.feedback(step >= 8 && !(671..681).contains(&step), c);
        let s = Sample { edges, i, n };
        let l = g.logic_output(&i);
        let c = g.controller_input(&i);
        if edges.logic_rise {
            if i.request_valid && l.request_ready {
                if i.writing {
                    supply += u32::from(i.words.div_ceil(2));
                }
                pending = None;
            }
            if i.write_data_valid && l.write_data_ready {
                supplied += 1;
                data_word += 1;
            }
        }
        peer.clock(&s, c);
        g.clock(edges, &i, &n);
        trace.push(s);
    }
    assert_eq!(next, jobs.len());
    assert!(pending.is_none());
    assert!(peer.active.is_none());
    trace
}
fn oracle(directory: &Path, cold: bool, trace: &[Sample], corrupt: bool) -> Vec<Vec<u64>> {
    fs::write(
        directory.join("input.txt"),
        trace.iter().map(|s| line(&inputs(s))).collect::<String>(),
    )
    .unwrap();
    // Keep the bridge body and assertions exact; only substitute its peer's type.
    fs::write(
        directory.join("production-bridge.v"),
        RtlSources::STREAM_GEARBOX,
    )
    .unwrap();
    fs::write(
        directory.join("production-controller.v"),
        RtlSources::CONTROLLER,
    )
    .unwrap();
    assert_eq!(
        RtlSources::STREAM_GEARBOX
            .matches(" SdramController #(")
            .count(),
        1
    );
    let bridge = RtlSources::STREAM_GEARBOX.replace(" SdramController #(", " GearOraclePeer #(");
    assert_ne!(bridge, RtlSources::STREAM_GEARBOX);
    let bridge = if corrupt {
        bridge.replace(
            "request_ready=initialized_q",
            "request_ready=!initialized_q",
        )
    } else {
        bridge
    };
    fs::write(directory.join("bridge.v"), bridge).unwrap();
    let source = RtlSources::CONTROLLER;
    let start = source.find("module SdramController #(").unwrap();
    let end = start + source[start..].find("\n);").unwrap() + 3;
    let header = source[start..end].replace("module SdramController", "module GearOraclePeer");
    let peer = header
        + r#"
reg[5:0]saved_last=0;
always @(posedge clk)if((request&&ready)||chain_accept||read_chain_accept)saved_last<=words-1;
assign ready=tb.p_ready;assign phase=tb.p_phase;
assign chain_accept=tb.p_chain;assign read_chain_accept=tb.p_read_chain;
always @* begin done=tb.p_done;initialized=tb.p_init;read_valid=tb.p_read_valid;
 read_data=tb.p_data;write_index=tb.p_index;end
endmodule
"#;
    fs::write(directory.join("peer.v"), peer).unwrap();
    let tb = include_str!("native_gear_tb.v").replace("__COLD__", if cold { "1" } else { "0" });
    fs::write(directory.join("tb.v"), tb).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile.args([
        "-g2012", "-s", "tb", "-o", "run.vvp", "bridge.v", "peer.v", "tb.v",
    ]);
    run_tool(compile, directory, "compile");
    let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    execute.arg("run.vvp");
    run_tool(execute, directory, "rtl");
    let text = fs::read_to_string(directory.join("rtl.stdout")).unwrap();
    assert!(
        text.contains(&format!("COMPLETE {}", trace.len())),
        "incomplete oracle in {}: {text}",
        directory.display()
    );
    text.lines()
        .filter_map(|l| l.strip_prefix("TRACE "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| u64::from_str_radix(v, 16).unwrap())
                .collect()
        })
        .collect()
}
const FIELDS: [&str; 21] = [
    "request_ready",
    "stream_active",
    "write_data_ready",
    "read_data",
    "read_valid",
    "write_done",
    "initialized",
    "controller.reset",
    "controller.request",
    "controller.writing",
    "controller.address",
    "controller.words",
    "controller.write_slot",
    "controller.next_valid",
    "controller.next_address",
    "controller.next_words",
    "controller.next_chainable",
    "controller.next_write_ready",
    "controller.next_generation",
    "controller.write_data",
    "controller.write_mask",
];
fn check_contract(cold: bool, trace: &[Sample], expected: &[Vec<u64>], directory: &Path) {
    let mut g = Gear::new(cold);
    let mut accepted = 0;
    let mut read_chains = 0;
    let mut write_chains = 0;
    let mut retirements = 0;
    let mut write_words = Vec::new();
    let mut masks = 0u16;
    let mut reads = Vec::new();
    for (step, (s, out)) in trace.iter().zip(expected).enumerate() {
        if s.edges.logic_rise {
            accepted += usize::from(s.i.request_valid && g.logic_output(&s.i).request_ready);
            retirements += out[5];
            if step < 660 && out[4] != 0 {
                reads.push(out[3]);
            }
        }
        if s.edges.native_rise {
            read_chains += usize::from(s.n.read_chain_accept);
            write_chains += usize::from(s.n.chain_accept);
            let native = g.controller_input(&s.i);
            if native.request && s.n.ready && native.writing && native.words == 1 {
                masks |= 1 << native.write_mask;
            }
            if s.n.phase == 15 {
                write_words.push(g.controller_input(&s.i).write_data);
            }
        }
        g.clock(s.edges, &s.i, &s.n);
    }
    let mut reference_reads = Vec::new();
    for (job, words) in [(0, 32), (1, 32), (2, 32), (5, 1), (8, 8)] {
        let base = 0xc000_0000u32 ^ ((0x40 + job * 64) << 8);
        for index in (0..words).step_by(2) {
            let low = u64::from(base ^ index);
            let high = if words == 1 {
                0
            } else {
                u64::from(base ^ (index + 1)) << 32
            };
            reference_reads.push(high | low);
        }
    }
    assert_eq!(reads, reference_reads, "independent returned pair sequence");
    let mut reference_writes = Vec::new();
    let mut pair = 0;
    for words in [8, 8, 1, 16, 8]
        .into_iter()
        .chain(std::iter::repeat_n(1, 16))
    {
        for index in 0..words {
            reference_writes.push(if index & 1 == 0 {
                0x1000_0000 ^ pair
            } else {
                0xb000_0000 ^ pair
            });
            if index & 1 != 0 || words == 1 {
                pair += 1;
            }
        }
    }
    assert_eq!(
        write_words, reference_writes,
        "independent consumed native word sequence"
    );
    assert_eq!(
        (accepted, read_chains, write_chains, retirements),
        (27, 2, 1, 21)
    );
    assert_eq!(
        masks,
        u16::MAX,
        "all byte masks crossed scalar native takes"
    );
    fs::write(directory.join("coverage.txt"), format!(
        "events={}\nlogic_rises=400\naccepted={accepted}\nread_chains={read_chains}\nwrite_chains={write_chains}\nwrite_retirements={retirements}\nread_pairs_before_reset={}\nnative_write_words={}\nmask_set={masks:04x}\nreset_events=0..8,671..677\n",
        trace.len(), reads.len(), write_words.len())).unwrap();
}
fn compare(expected: &[Vec<u64>], actual: &[Vec<u64>]) -> Result<(), String> {
    if expected.len() != actual.len() {
        return Err("edge count".into());
    }
    for (edge, (e, a)) in expected.iter().zip(actual).enumerate() {
        if e.len() != FIELDS.len() || a.len() != FIELDS.len() {
            return Err(format!("edge {edge}: field count"));
        }
        for (field, (e, a)) in FIELDS.iter().zip(e.iter().zip(a)) {
            if e != a {
                return Err(format!("edge {edge}, {field}: expected {e:x}, RTL {a:x}"));
            }
        }
    }
    Ok(())
}
#[test]
#[ignore = "bounded native gear edge comparison; requires Icarus"]
fn production_gear_matches_rust_at_every_edge() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("target/native-memory");
    for (cold, late_logic) in [(false, false), (false, true), (true, false), (true, true)] {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = root.join(format!("gear-{cold}-{late_logic}-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        let trace = stimulus(cold, late_logic);
        let mut g = Gear::new(cold);
        let expected: Vec<_> = trace
            .iter()
            .map(|s| {
                g.clock(s.edges, &s.i, &s.n);
                outputs(&g, &s.i)
            })
            .collect();
        fs::write(
            directory.join("rust.txt"),
            expected.iter().map(|v| line(v)).collect::<String>(),
        )
        .unwrap();
        check_contract(cold, &trace, &expected, &directory);
        let actual = oracle(&directory, cold, &trace, false);
        compare(&expected, &actual).unwrap_or_else(|e| panic!("{e}; {}", directory.display()));
        let negative = directory.join("mutated-ready");
        fs::create_dir_all(&negative).unwrap();
        let mutant = oracle(&negative, cold, &trace[..16], true);
        let mismatch =
            compare(&expected[..16], &mutant).expect_err("modified DUT ready must be detected");
        fs::write(negative.join("detected.txt"), mismatch).unwrap();
    }
}
