//! Independent transaction golden plus per-edge emulator/RTL agreement.
use gpu_v2::system::pixel::spsc::{emu::Queue, rtl, Config, ReadTiming, Step, Tick};
use std::{
    collections::VecDeque,
    fs::File,
    process::Stdio,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Edge {
    tick: Tick,
    step: Step,
}

fn trace(config: Config, random: bool) -> Vec<Edge> {
    let mut queue = Queue::new(config).unwrap();
    let mut expected = VecDeque::new();
    let mut unpublished = Vec::new();
    let mut written = 0usize;
    let target = 160 * config.rows;
    let mut random_state = 0x8f31_ae49_6417_056du64;
    let mut trace = Vec::new();
    let mut held = None;
    let mut consume_row = 0;
    for wall in 0..12_000 {
        random_state ^= random_state << 13;
        random_state ^= random_state >> 7;
        random_state ^= random_state << 17;
        let reset = random && wall == 137;
        let ce = !random || random_state & 7 != 0;
        let ready =
            wall >= config.entries * config.rows + 8 && (!random || (random_state >> 8) & 7 > 2);
        let offer = written < target && (!random || (random_state >> 16) & 3 != 0);
        let tick = Tick {
            reset,
            ce,
            output_ready: ready,
            input: offer.then_some(((written as u64 + 1) * 0x006d_5b13) & config.mask()),
        };
        let step = queue.tick(tick).unwrap();
        if reset {
            expected.clear();
            unpublished.clear();
            held = None;
            consume_row = 0;
            // Start a fresh complete entry, with distinct post-reset data.
            written = written.div_ceil(config.rows) * config.rows;
        } else {
            if let Some(previous) = held {
                assert_eq!(step.signals.output, Some(previous), "unstable held head");
            }
            if step.consumed {
                let word = step.signals.output.unwrap();
                assert_eq!(
                    Some(word.data),
                    expected.pop_front(),
                    "unpublished/stale/reordered row"
                );
                assert_eq!(word.row, consume_row);
                assert_eq!(word.last, consume_row + 1 == config.rows);
                consume_row = (consume_row + 1) % config.rows;
            }
            held = step.signals.output.filter(|_| !step.consumed);
            if step.accepted {
                unpublished.push(tick.input.unwrap());
                written += 1;
                assert_eq!(step.published, unpublished.len() == config.rows);
                if step.published {
                    expected.extend(unpublished.drain(..));
                }
            }
            if let Some(read) = step.read_address {
                assert_ne!(Some(read), step.write_address);
            }
            assert!(step.occupied_entries <= config.entries);
            if !ce {
                assert!(!step.accepted && !step.consumed && !step.returned);
                assert!(step.read_address.is_none());
            }
        }
        trace.push(Edge { tick, step });
        if written == target && queue.idle() && !reset {
            assert!(expected.is_empty() && unpublished.is_empty());
            return trace;
        }
    }
    panic!("published SPSC transaction watchdog");
}

#[test]
fn published_entries_survive_independent_stalls_wrap_and_reset() {
    for entries in [1, 2, 8, 16, 32] {
        for rows in [1, 3, 8, 16] {
            for read_timing in [ReadTiming::Capture, ReadTiming::Registered] {
                trace(
                    Config {
                        entries,
                        rows,
                        width: 36,
                        read_timing,
                        max_wall: 12_000,
                    },
                    true,
                );
            }
        }
    }
}

#[test]
fn continuous_rows_cross_entry_boundaries_without_bubbles() {
    let edges = trace(
        Config {
            entries: 16,
            rows: 8,
            width: 36,
            read_timing: ReadTiming::Capture,
            max_wall: 12_000,
        },
        false,
    );
    let consumed: Vec<_> = edges
        .iter()
        .enumerate()
        .filter(|(_, e)| e.step.consumed)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(consumed.len(), 1280);
    for pair in consumed.windows(2) {
        assert_eq!(pair[1] - pair[0], 1, "synchronous head/entry-switch bubble");
    }
    let published: Vec<_> = edges
        .iter()
        .enumerate()
        .filter(|(_, e)| e.step.published)
        .map(|(i, _)| i)
        .collect();
    // The initial producer fills the queue; the steady consumer permits every
    // following entry to publish on its last row, without a writer restart edge.
    for pair in published[..16].windows(2) {
        assert_eq!(pair[1] - pair[0], 8);
    }
}

#[test]
fn incomplete_tail_is_invisible_and_full_release_is_next_edge() {
    let config = Config {
        entries: 1,
        rows: 3,
        width: 18,
        read_timing: ReadTiming::Capture,
        max_wall: 128,
    };
    let mut q = Queue::new(config).unwrap();
    for data in [17, 23] {
        assert!(
            q.tick(Tick {
                ce: true,
                input: Some(data),
                ..Default::default()
            })
            .unwrap()
            .accepted
        );
    }
    for _ in 0..9 {
        let s = q
            .tick(Tick {
                ce: true,
                output_ready: true,
                ..Default::default()
            })
            .unwrap();
        assert!(s.signals.output.is_none() && s.read_address.is_none());
        assert_eq!(s.occupied_entries, 1);
    }
    assert!(
        q.tick(Tick {
            ce: true,
            input: Some(31),
            ..Default::default()
        })
        .unwrap()
        .published
    );
    let mut count = 0;
    for _ in 0..32 {
        let s = q
            .tick(Tick {
                ce: true,
                input: Some(99),
                output_ready: true,
                ..Default::default()
            })
            .unwrap();
        if s.consumed {
            count += 1;
            assert!(!s.accepted, "same-edge released entry credit was reused");
        }
        if count == 3 {
            assert!(q.signals(true).input_ready);
            return;
        }
    }
    panic!("full-entry release watchdog");
}

fn bounded(mut command: std::process::Command, dir: &std::path::Path, name: &str) {
    let mut child = command
        .stdout(Stdio::from(
            File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            File::create(dir.join(format!("{name}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn published_spsc_iverilog_matches_every_edge() {
    for (entries, rows, width, read_timing, primitive) in [
        (1, 3, 18, ReadTiming::Capture, false),
        (2, 1, 64, ReadTiming::Capture, false),
        (8, 8, 36, ReadTiming::Registered, false),
        (16, 16, 32, ReadTiming::Capture, false),
        (32, 8, 32, ReadTiming::Capture, false),
        (8, 4, 36, ReadTiming::Registered, true),
    ] {
        let config = Config {
            entries,
            rows,
            width,
            read_timing,
            max_wall: 12_000,
        };
        let edges = trace(config, true);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/pixel-foundation-20261007/spsc-{entries}-{rows}-{width}"
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source = if primitive {
            rtl::source_sdp36(config)
        } else {
            rtl::source(config)
        };
        std::fs::write(dir.join("queue.v"), source.unwrap()).unwrap();
        let mut bench = format!("module tb; reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0; reg [{}:0] in_data=0; wire in_ready,out_valid,out_last,published,read_request; wire [{}:0] out_data; wire [3:0] out_row; wire [9:0] read_address; gpu_v2_published_spsc dut(.*); initial begin #200000; $fatal(1,\"HDL watchdog\"); end initial begin reset=1; #1;clk=1;#1;clk=0;#1;reset=0;\n", width-1, width-1);
        for (i, edge) in edges.iter().enumerate() {
            let t = edge.tick;
            let s = &edge.step;
            bench.push_str(&format!(
                "reset={};ce={};in_valid={};out_ready={};in_data={width}'h{:x};#1;\n",
                u8::from(t.reset),
                u8::from(t.ce),
                u8::from(t.input.is_some()),
                u8::from(t.output_ready),
                t.input.unwrap_or(0)
            ));
            bench.push_str(&format!("if(in_ready!==1'b{} || out_valid!==1'b{} || published!==1'b{} || read_request!==1'b{}) $fatal(1,\"handshake edge {i}\");\n",u8::from(s.signals.input_ready),u8::from(s.signals.output.is_some()),u8::from(s.published),u8::from(s.read_address.is_some())));
            if let Some(word) = s.signals.output {
                bench.push_str(&format!("if(out_data!=={width}'h{:x} || out_row!==4'd{} || out_last!==1'b{}) $fatal(1,\"payload edge {i}\");\n", word.data,word.row,u8::from(word.last)));
            }
            if let Some(address) = s.read_address {
                bench.push_str(&format!(
                    "if(read_address!==10'd{address}) $fatal(1,\"read address edge {i}\");\n"
                ));
            }
            bench.push_str("clk=1;#1;clk=0;#1;\n");
        }
        bench.push_str(&format!(
            "$display(\"PASS edges={}\");$finish;end endmodule\n",
            edges.len()
        ));
        if primitive {
            bench = bench.replace("module tb;", "module tb; GSR GSR(.GSRI(1'b1));");
        }
        std::fs::write(dir.join("tb.v"), bench).unwrap();
        let mut compile = std::process::Command::new(
            std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
        );
        compile
            .current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "test.vvp", "queue.v", "tb.v"]);
        if primitive {
            let home =
                std::env::var_os("GOWIN_HOME").expect("SDP36 primitive test requires GOWIN_HOME");
            compile.arg(std::path::Path::new(&home).join("IDE/simlib/gw2a/prim_sim.v"));
        }
        bounded(compile, &dir, "compile");
        let mut run =
            std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        run.current_dir(&dir).arg("test.vvp");
        bounded(run, &dir, "run");
        let output = std::fs::read_to_string(dir.join("run.out")).unwrap();
        assert!(output.contains("PASS edges="));
        println!("{entries} entries/{rows} rows/{width} bits: {output}");
    }
}
