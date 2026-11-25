//! Actual controller RTL replay, at the native edge and stable DQ input boundary.
mod native_support;
use digital_design_hardware_gowin::sdram_memory_controller::{
    cycle::{controller::Controller, gear::ControllerInput},
    RtlSources,
};
use native_support::run_tool;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Copy)]
struct Sample {
    i: ControllerInput,
    boundary: bool,
    external: u32,
}
#[derive(Clone, Copy)]
struct Job {
    address: u32,
    words: u8,
    writing: bool,
}
fn b(v: bool) -> u64 {
    u64::from(v)
}
fn inputs(s: &Sample) -> Vec<u64> {
    let i = s.i;
    vec![
        b(i.reset),
        b(i.request),
        b(i.writing),
        i.address.into(),
        i.words.into(),
        b(i.write_slot),
        b(i.next_valid),
        i.next_address.into(),
        i.next_words.into(),
        b(i.next_chainable),
        b(i.next_write_ready),
        b(i.next_generation),
        i.write_data.into(),
        i.write_mask.into(),
        b(s.boundary),
        s.external.into(),
    ]
}
fn outputs(c: &Controller, i: &ControllerInput) -> Vec<u64> {
    let o = c.output(i);
    let f = o.feedback;
    assert_eq!(o.read_known, u32::MAX, "injected stable DQ must stay known");
    vec![
        b(f.ready),
        b(f.done),
        b(f.initialized),
        b(f.read_valid),
        f.read_data.into(),
        f.write_index.into(),
        f.phase.into(),
        b(f.chain_accept),
        b(f.read_chain_accept),
        b(o.read_chain_pending),
        o.stream_stats.into(),
        o.command.into(),
        b(o.cke),
        o.address.into(),
        o.bank.into(),
        o.dqm.into(),
        o.dq_oe_n.into(),
        o.dq_out.into(),
    ]
}
fn line(v: &[u64]) -> String {
    v.iter()
        .map(|x| format!("{x:x}"))
        .collect::<Vec<_>>()
        .join(" ")
        + "\n"
}
const FIELDS: [&str; 18] = [
    "ready",
    "done",
    "initialized",
    "read_valid",
    "read_data",
    "write_index",
    "phase",
    "chain_accept",
    "read_chain_accept",
    "read_chain_pending",
    "stream_stats",
    "pad.command",
    "pad.cke",
    "pad.address",
    "pad.bank",
    "pad.dqm",
    "pad.dq_oe_n",
    "pad.dq_out",
];
fn jobs() -> Vec<Job> {
    let mut jobs = vec![
        Job {
            address: 0,
            words: 32,
            writing: false,
        },
        Job {
            address: 0x20,
            words: 8,
            writing: false,
        },
        Job {
            address: 0x40,
            words: 16,
            writing: false,
        },
        Job {
            address: 0x400,
            words: 32,
            writing: false,
        },
        Job {
            address: 0x404,
            words: 1,
            writing: false,
        },
        Job {
            address: 0x40,
            words: 32,
            writing: true,
        },
        Job {
            address: 0x60,
            words: 8,
            writing: true,
        },
        Job {
            address: 0x80,
            words: 16,
            writing: true,
        },
        Job {
            address: 0x80,
            words: 1,
            writing: true,
        },
        Job {
            address: 0x840,
            words: 8,
            writing: false,
        },
        Job {
            address: 0x848,
            words: 8,
            writing: false,
        },
        Job {
            address: 0xc48,
            words: 8,
            writing: false,
        },
        Job {
            address: 0x10000,
            words: 32,
            writing: true,
        },
    ];
    for mask in 0..16 {
        jobs.push(Job {
            address: 0x1000 + mask * 4,
            words: 1,
            writing: true,
        });
    }
    for n in 0..80 {
        jobs.push(Job {
            address: (n % 4) * 32 + (n / 4 % 2) * 128,
            words: 32,
            writing: false,
        });
    }
    jobs
}
fn stimulus(init: u16, total: usize) -> Vec<Sample> {
    let jobs = jobs();
    let mut next = 0;
    let mut active: Option<usize> = None;
    let mut generation = true;
    let mut c = Controller::new(init);
    let mut trace = Vec::new();
    let mut rng = 0x6145_3321u32;
    let mut read_chains = 0;
    let mut write_chains = 0;
    let mut refresh = 0;
    let mut credit_refusals = 0;
    let mut reset_probe = false;
    let mut masks = 0u16;
    for cycle in 0..total {
        let old = c.output(&ControllerInput::default());
        let reset =
            cycle < 2 || (init == 32 && !reset_probe && cycle > 1200 && old.feedback.read_valid);
        if reset && cycle >= 2 {
            reset_probe = true;
            active = None;
        }
        let j = jobs.get(next).copied();
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let i = ControllerInput {
            reset,
            request: j.is_some() && !reset,
            writing: j.is_some_and(|j| j.writing),
            address: j.map_or(0, |j| j.address),
            words: j.map_or(0, |j| j.words),
            write_slot: cycle % 2 == 0,
            next_valid: j.is_some() && old.feedback.initialized && !reset,
            next_address: j.map_or(0, |j| j.address),
            next_words: j.map_or(0, |j| j.words),
            next_chainable: active.zip(j).is_some_and(|(a, j)| {
                jobs[a].writing == j.writing && jobs[a].words >= 8 && j.words >= 8
            }),
            next_write_ready: active != Some(6),
            next_generation: generation,
            write_data: rng,
            write_mask: if (13..29).contains(&next) {
                (next - 13) as u8
            } else if j.is_some_and(|j| j.words == 1) {
                12
            } else {
                0
            },
        };
        let out = c.output(&i);
        let s = Sample {
            i,
            boundary: cycle % 5 == 0,
            external: rng.rotate_left(13) ^ 0x758a_9d24,
        };
        let f = out.feedback;
        if !reset && i.request && f.ready && i.writing && i.words == 1 {
            masks |= 1 << i.write_mask;
        }
        read_chains += usize::from(f.read_chain_accept);
        write_chains += usize::from(f.chain_accept);
        if f.phase & 31 == 15 && f.write_index == 7 && active == Some(6) && !f.chain_accept {
            credit_refusals += 1;
        }
        let sample = if out.dq_oe_n == u32::MAX {
            s.external
        } else {
            out.dq_out
        };
        c.clock(&i, s.boundary, sample);
        if !reset && ((i.request && f.ready) || f.chain_accept || f.read_chain_accept) {
            active = Some(next);
            next += 1;
            generation = !generation;
        }
        if c.output(&i).command == 1 && cycle > usize::from(init) + 40 {
            refresh += 1;
        }
        trace.push(s);
    }
    if init == 32 {
        assert_eq!(next, jobs.len(), "all descriptors accepted within bound");
        assert!(reset_probe);
        assert_eq!(masks, u16::MAX, "all masks accepted by native controller");
        assert!(
            read_chains > 10 && write_chains > 0 && refresh >= 2 && credit_refusals > 0,
            "coverage {read_chains} {write_chains} {refresh} {credit_refusals}"
        );
    }
    trace
}
fn oracle(
    dir: &Path,
    init: u16,
    trace: &[Sample],
    shared_bank_prepare: bool,
    corrupt: bool,
) -> Vec<Vec<u64>> {
    fs::write(
        dir.join("input.txt"),
        trace.iter().map(|s| line(&inputs(s))).collect::<String>(),
    )
    .unwrap();
    let source = RtlSources::controller(shared_bank_prepare);
    fs::write(dir.join("production-controller.v"), &source).unwrap();
    let compiled = if corrupt {
        source.replace(
            "state == ST_IDLE && initialized && !refresh_due_q",
            "state == ST_IDLE && !initialized && !refresh_due_q",
        )
    } else {
        source.clone()
    };
    if corrupt {
        assert_ne!(source, compiled);
    }
    fs::write(dir.join("controller.v"), compiled).unwrap();
    fs::write(dir.join("input-delay.v"), RtlSources::INPUT_DELAY_MODEL).unwrap();
    fs::write(
        dir.join("tb.v"),
        include_str!("native_controller_tb.v").replace("__INIT__", &init.to_string()),
    )
    .unwrap();
    let mut compiler =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compiler.args([
        "-g2012",
        "-DSTREAM_STATIC_INPUT_DELAY",
        "-s",
        "tb",
        "-o",
        "run.vvp",
        "controller.v",
        "input-delay.v",
        "tb.v",
    ]);
    run_tool(compiler, dir, "compile");
    let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    execute.arg("run.vvp");
    run_tool(execute, dir, "rtl");
    let text = fs::read_to_string(dir.join("rtl.stdout")).unwrap();
    assert!(
        text.contains(&format!("COMPLETE {}", trace.len())),
        "incomplete oracle: {}",
        dir.display()
    );
    text.lines()
        .filter_map(|l| l.strip_prefix("TRACE "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| {
                    u64::from_str_radix(v, 16)
                        .unwrap_or_else(|_| panic!("unknown RTL value {v}: {}", dir.display()))
                })
                .collect()
        })
        .collect()
}
fn compare(e: &[Vec<u64>], a: &[Vec<u64>]) -> Result<(), String> {
    if e.len() != a.len() {
        return Err("edge count".into());
    }
    for (edge, (e, a)) in e.iter().zip(a).enumerate() {
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
#[ignore = "bounded native MC edge replay; requires Icarus"]
fn production_controller_matches_rust_at_native_edges() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("target/native-memory");
    for (init, total) in [(32, 6000), (21600, 22050)] {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = root.join(format!("mc-{init}-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        let trace = stimulus(init, total);
        let mut c = Controller::new(init);
        let expected: Vec<_> = trace
            .iter()
            .map(|s| {
                let old = c.output(&s.i);
                let sample = if old.dq_oe_n == u32::MAX {
                    s.external
                } else {
                    old.dq_out
                };
                c.clock(&s.i, s.boundary, sample);
                outputs(&c, &s.i)
            })
            .collect();
        fs::write(
            dir.join("rust.txt"),
            expected.iter().map(|v| line(v)).collect::<String>(),
        )
        .unwrap();
        let actual = oracle(&dir, init, &trace, false, false);
        compare(&expected, &actual).unwrap_or_else(|e| panic!("{e}; {}", dir.display()));
        let shared = dir.join("shared-bank-prepare");
        fs::create_dir_all(&shared).unwrap();
        let actual = oracle(&shared, init, &trace, true, false);
        compare(&expected, &actual).unwrap_or_else(|e| panic!("{e}; {}", shared.display()));
        if init == 32 {
            let negative = dir.join("mutated-ready");
            fs::create_dir_all(&negative).unwrap();
            let mutated = oracle(&negative, init, &trace[..64], false, true);
            let mismatch =
                compare(&expected[..64], &mutated).expect_err("modified MC ready must fail");
            fs::write(negative.join("detected.txt"), mismatch).unwrap();
        }
    }
}
