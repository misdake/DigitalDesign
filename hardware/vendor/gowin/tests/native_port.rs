mod native_support;
use digital_design_hardware_gowin::sdram_memory_controller::{
    cycle::port::{Input, Port},
    RtlSources,
};
use native_support::run_tool;
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
struct Run {
    trace: Vec<(Input, [u64; 7])>,
    writes: Vec<(u32, u8, u32)>,
    final_words: Vec<u32>,
}
fn input(i: Input) -> [u64; 9] {
    [
        i.reset.into(),
        i.request_valid.into(),
        i.writing.into(),
        i.line.into(),
        i.address.into(),
        i.line_count_minus_1.into(),
        i.chain_class.into(),
        i.write_data,
        i.response_ready.into(),
    ]
}
fn output(o: [u64; 7]) -> [u64; 7] {
    o
}
fn line(v: &[u64]) -> String {
    v.iter()
        .map(|x| format!("{x:x}"))
        .collect::<Vec<_>>()
        .join(" ")
        + "\n"
}
fn observe(p: &Port, i: &Input) -> [u64; 7] {
    let o = p.output(i);
    [
        o.cpu_request_ready.into(),
        o.cpu_lookahead_window.into(),
        o.cpu_write_data_ready.into(),
        o.cpu_response_valid.into(),
        o.cpu_read_data,
        o.cpu_response_last.into(),
        o.cpu_error.into(),
    ]
}
#[derive(Clone, Copy)]
struct Job {
    address: u32,
    write: bool,
    line: bool,
    count: u8,
    error: bool,
}
fn payload(id: usize, beat: usize) -> u64 {
    (0..4).fold(0, |v, k| {
        v | ((0x8000 + id as u64 * 128 + beat as u64 * 4 + k) << (k * 16))
    })
}
fn scalar(id: usize) -> u64 {
    0x5c73 ^ id as u64
}
fn seed() -> Vec<u16> {
    let mut b = vec![0xbad0; 1 << 22];
    for n in 0..4096 {
        let v = 0x10000000u32 | n;
        b[n as usize * 2] = v as u16;
        b[n as usize * 2 + 1] = (v >> 16) as u16;
    }
    b
}
fn apply(b: &mut [u16], j: Job, id: usize) {
    if j.line {
        for beat in 0..(usize::from(j.count) + 1) * 4 {
            let d = payload(id, beat);
            for k in 0..4 {
                b[j.address as usize + beat * 4 + k] = (d >> (k * 16)) as u16;
            }
        }
    } else {
        b[j.address as usize] = scalar(id) as u16;
    }
}
fn run() -> Run {
    let jobs = [
        Job {
            address: 0x80,
            write: true,
            line: true,
            count: 0,
            error: false,
        },
        Job {
            address: 0x100,
            write: true,
            line: true,
            count: 1,
            error: false,
        },
        Job {
            address: 0x80,
            write: false,
            line: true,
            count: 0,
            error: false,
        },
        Job {
            address: 0x100,
            write: false,
            line: true,
            count: 1,
            error: false,
        },
        Job {
            address: 0x83,
            write: true,
            line: false,
            count: 0,
            error: false,
        },
        Job {
            address: 0x83,
            write: false,
            line: false,
            count: 0,
            error: false,
        },
        Job {
            address: 0x82,
            write: false,
            line: false,
            count: 0,
            error: false,
        },
        Job {
            address: 0x200,
            write: true,
            line: true,
            count: 3,
            error: false,
        },
        Job {
            address: 0x240,
            write: true,
            line: true,
            count: 3,
            error: false,
        },
        Job {
            address: 0x200,
            write: false,
            line: true,
            count: 3,
            error: false,
        },
        Job {
            address: 0x240,
            write: false,
            line: true,
            count: 3,
            error: false,
        },
        Job {
            address: 0x85,
            write: false,
            line: true,
            count: 0,
            error: true,
        },
    ];
    let mut expected = seed();
    let mut writes = Vec::new();
    for (id, j) in jobs
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, j)| j.write && !j.error)
    {
        apply(&mut expected, j, id);
        let count = if j.line {
            (u32::from(j.count) + 1) * 8
        } else {
            1
        };
        for n in 0..count {
            let a = j.address / 2 + n;
            let mask = if j.line {
                0
            } else if j.address & 1 != 0 {
                3
            } else {
                12
            };
            let v =
                u32::from(expected[a as usize * 2]) | u32::from(expected[a as usize * 2 + 1]) << 16;
            writes.push((a, mask, v));
        }
    }
    let mut backing = seed();
    let mut reference = seed();
    let mut p = Port::new(32);
    let (mut issued, mut completed, mut received, mut held, mut idle, mut peak, mut payload_stalls) =
        (0, 0, 0, 0, 0, 0, 0);
    let mut supply = VecDeque::<(usize, usize)>::new();
    let mut trace = Vec::new();
    for cycle in 0..4000 {
        let mut i = Input {
            reset: cycle < 2,
            response_ready: true,
            ..Default::default()
        };
        if let Some(j) = jobs.get(issued) {
            i.request_valid = cycle >= 2;
            i.writing = j.write;
            i.line = j.line;
            i.address = j.address;
            i.line_count_minus_1 = j.count;
            i.chain_class = if j.line {
                if j.write {
                    3
                } else {
                    1
                }
            } else {
                0
            };
            i.write_data = scalar(issued);
        }
        if let Some(&(id, beat)) = supply.front() {
            i.write_data = payload(id, beat);
        }
        let first = observe(&p, &i);
        if first[3] != 0 && jobs.get(completed).is_some_and(|j| !j.line || j.error) && held < 2 {
            i.response_ready = false;
            held += 1;
        }
        let o = p.output(&i);
        if !supply.is_empty() && !o.cpu_write_data_ready {
            payload_stalls += 1;
        }
        if o.cpu_write_data_ready {
            let (id, beat) = supply
                .front_mut()
                .expect("line payload has an accepted owner");
            *beat += 1;
            if *beat == (usize::from(jobs[*id].count) + 1) * 4 {
                supply.pop_front();
            }
        }
        if i.request_valid && o.cpu_request_ready {
            if jobs[issued].write && jobs[issued].line {
                supply.push_back((issued, 0));
            }
            issued += 1;
        }
        peak = peak.max(issued - completed);
        if o.cpu_response_valid && i.response_ready {
            let j = jobs[completed];
            assert_eq!(o.cpu_error, j.error, "error response {completed}");
            let last =
                j.write || !j.line || j.error || received == (usize::from(j.count) + 1) * 4 - 1;
            assert_eq!(
                o.cpu_response_last, last,
                "last response {completed}/{received}"
            );
            if !j.error {
                if j.write {
                    assert_eq!(o.cpu_read_data, 0);
                    apply(&mut reference, j, completed);
                    let len = if j.line {
                        (usize::from(j.count) + 1) * 16
                    } else {
                        1
                    };
                    assert_eq!(
                        &backing[j.address as usize..j.address as usize + len],
                        &reference[j.address as usize..j.address as usize + len],
                        "write response before physical commit {completed}"
                    );
                } else {
                    let a = j.address as usize + received * 4;
                    let want = if j.line {
                        (0..4).fold(0, |v, k| v | u64::from(reference[a + k]) << (k * 16))
                    } else {
                        u64::from(reference[a])
                    };
                    assert_eq!(
                        o.cpu_read_data, want,
                        "read response {completed}/{received}, cycle {cycle}"
                    );
                }
            }
            if last {
                completed += 1;
                received = 0;
                held = 0;
            } else {
                received += 1;
            }
        }
        p.clock(&i, &mut backing)
            .unwrap_or_else(|e| panic!("cycle {cycle}: {e:?}"));
        trace.push((i, observe(&p, &i)));
        if completed == jobs.len() {
            idle += 1;
            if idle == 1200 {
                assert_eq!(issued, jobs.len());
                assert!(supply.is_empty());
                assert!(peak >= 2 && payload_stalls > 0);
                assert_eq!(backing, expected, "full physical memory guards");
                assert_eq!(p.native_cycle(), trace.len() as u64 * 2);
                return Run {
                    trace,
                    writes,
                    final_words: expected
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|v| u32::from(v[0]) | u32::from(v[1]) << 16)
                        .collect(),
                };
            }
        }
    }
    panic!("native port watchdog: issued {issued}, completed {completed}");
}
fn replay(dir: &Path, run: &Run, corrupt: bool) -> String {
    fs::write(
        dir.join("input.txt"),
        run.trace
            .iter()
            .map(|(i, _)| line(&input(*i)))
            .collect::<String>(),
    )
    .unwrap();
    fs::write(
        dir.join("rust.txt"),
        run.trace
            .iter()
            .map(|(_, o)| line(&output(*o)))
            .collect::<String>(),
    )
    .unwrap();
    fs::write(dir.join("production-controller.v"), RtlSources::CONTROLLER).unwrap();
    let controller = if corrupt {
        RtlSources::CONTROLLER.replace("dq_out <= write_data;", "dq_out <= write_data ^ 32'h1;")
    } else {
        RtlSources::CONTROLLER.to_owned()
    };
    if corrupt {
        assert_ne!(controller, RtlSources::CONTROLLER);
    }
    for (name, source) in [
        ("bridge.v", RtlSources::STREAM_GEARBOX),
        ("port.v", RtlSources::STREAM_SHARED_PORT),
        ("controller.v", controller.as_str()),
        ("pins.v", RtlSources::PIN_MODEL),
        ("input-delay.v", RtlSources::INPUT_DELAY_MODEL),
        ("cl3_pins.py", include_str!("cl3_pins.py")),
    ] {
        fs::write(dir.join(name), source).unwrap();
    }
    fs::write(
        dir.join("tb-base.v"),
        include_str!("native_port_tb.v").replace("__COUNT__", &run.trace.len().to_string()),
    )
    .unwrap();
    let mut prepare = Command::new("python");
    prepare.args(["-B","-c","from pathlib import Path;from cl3_pins import with_board_pins;Path('tb.v').write_text(with_board_pins(Path('tb-base.v').read_text(),'dut.u_controller'))"]);
    run_tool(prepare, dir, "prepare");
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile.args([
        "-g2012",
        "-DSTREAM_STATIC_INPUT_DELAY",
        "-s",
        "tb",
        "-o",
        "run.vvp",
        "bridge.v",
        "port.v",
        "controller.v",
        "pins.v",
        "input-delay.v",
        "tb.v",
    ]);
    run_tool(compile, dir, "compile");
    let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    execute.arg("run.vvp");
    run_tool(execute, dir, "rtl");
    fs::read_to_string(dir.join("rtl.stdout")).unwrap()
}
#[test]
#[ignore = "bounded shared port/native/pin closure; Icarus and Python required"]
fn production_stream_port_matches_native_pipeline_and_physical_commits() {
    let run = run();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(format!("target/native-memory/port-{nonce}"));
    fs::create_dir_all(&dir).unwrap();
    let raw = replay(&dir, &run, false);
    assert!(raw.contains("COMPLETE"), "{}", dir.display());
    let applied: Vec<Vec<u64>> = raw
        .lines()
        .filter_map(|l| l.strip_prefix("INPUT "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| u64::from_str_radix(v, 16).unwrap())
                .collect()
        })
        .collect();
    let expected_inputs: Vec<_> = run.trace.iter().map(|(i, _)| input(*i)).collect();
    assert_eq!(
        applied,
        expected_inputs,
        "actual RTL stimulus differs: {}",
        dir.display()
    );
    let actual: Vec<_> = raw
        .lines()
        .filter_map(|l| l.strip_prefix("TRACE "))
        .collect();
    assert_eq!(actual.len(), run.trace.len());
    let names = [
        "request_ready",
        "lookahead",
        "write_data_ready",
        "response_valid",
        "read_data",
        "response_last",
        "error",
    ];
    for (cycle, (line, (_, expected))) in actual.iter().zip(&run.trace).enumerate() {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 7);
        for (field, raw) in fields.iter().enumerate() {
            // Response data has no owner while invalid or on an error response.
            // Every control bit is always compared, and every owned data bit is strict.
            if field == 4 && (expected[3] == 0 || expected[6] != 0) {
                continue;
            }
            let value = u64::from_str_radix(raw, 16).unwrap_or_else(|_| {
                panic!(
                    "unknown owned value, cycle {cycle}, {}: {raw}; {}",
                    names[field],
                    dir.display()
                )
            });
            assert_eq!(
                value,
                expected[field],
                "cycle {cycle}, {}: {}",
                names[field],
                dir.display()
            );
        }
    }
    let writes: Vec<_> = fs::read_to_string(dir.join("writes.txt"))
        .unwrap()
        .lines()
        .map(|l| {
            let v: Vec<_> = l
                .split_whitespace()
                .map(|v| u32::from_str_radix(v, 16).unwrap())
                .collect();
            (v[0], v[1] as u8, v[2])
        })
        .collect();
    assert_eq!(
        writes,
        run.writes,
        "independent physical write address/mask/result sequence: {}",
        dir.display()
    );
    let final_words: Vec<_> = fs::read_to_string(dir.join("final.txt"))
        .unwrap()
        .lines()
        .map(|l| u32::from_str_radix(l, 16).unwrap())
        .collect();
    assert_eq!(final_words, &run.final_words[..4096]);
    let negative = dir.join("mutated-write");
    fs::create_dir_all(&negative).unwrap();
    replay(&negative, &run, true);
    let bad = fs::read_to_string(negative.join("writes.txt")).unwrap();
    let good = fs::read_to_string(dir.join("writes.txt")).unwrap();
    let mismatch = bad
        .lines()
        .zip(good.lines())
        .position(|(a, b)| a != b)
        .expect("DUT write-data mutation must change physical commits");
    fs::write(
        negative.join("detected.txt"),
        format!(
            "physical write {mismatch}: expected {}, mutated {}\n",
            good.lines().nth(mismatch).unwrap(),
            bad.lines().nth(mismatch).unwrap()
        ),
    )
    .unwrap();
}
