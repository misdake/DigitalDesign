//! Transaction golden and actual portable/native controller replay.
use gpu_v2::system::pixel::quad_ingress::{rtl, Beat, Begin, Descriptor, Ingress, Row, Step, Tick};
use std::{
    collections::VecDeque,
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Edge {
    tick: Tick,
    step: Step,
}

struct Published {
    slot: u8,
    light_done: bool,
    sample_done: bool,
}

fn beat(quad: usize, row: usize, profile: usize) -> Beat {
    let begin = Begin {
        mask: if quad % 11 == 3 {
            0
        } else {
            [1, 5, 15, 8][quad % 4]
        },
        draw: quad & 1 != 0,
        unlit: profile == 0 || profile == 2 || (profile == 3 && quad.is_multiple_of(3)),
        untextured: profile == 0 || profile == 1 || (profile == 3 && quad.is_multiple_of(5)),
        force_coarsest: quad.is_multiple_of(7),
        origin: ((quad * 613) & 0x7fff) as u16,
    };
    Beat {
        begin,
        basic: (quad * 817 + row * 101 + 0x315) as u32 & 0xffff,
        lighting: (quad * 0x11711 + row * 0x771 + 7) as u64,
        uv: (quad * 0x31a91 + row * 0x737 + 5) as u64,
    }
}

fn trace(profile: usize) -> Vec<Edge> {
    let mut dut = Ingress::new(18_000);
    let mut expected_light = VecDeque::new();
    let mut expected_sample = VecDeque::new();
    let mut global = VecDeque::<Published>::new();
    let mut allocated = 0usize;
    let mut quad = 0;
    let mut row = 0;
    let mut edges = Vec::new();
    let mut basic = [None; 256];
    let mut expected_return = None;
    let mut held_light = None;
    let mut held_sample = None;
    let mut rng = 0xe128_74ff_a9c1_9871u64;
    let mut full_global = false;
    let mut full_light = false;
    let mut full_sample = false;
    let mut paid_ce0 = false;
    let mut old_retire_credit = false;
    let mut old_branch_credit = false;
    for wall in 0..18_000 {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let ce = profile != 3 || rng & 7 != 0;
        let input =
            (quad < 140 && (profile != 3 || rng >> 12 & 3 != 0)).then(|| beat(quad, row, profile));
        let retirement = ce
            && wall > 350
            && (profile != 3 || rng >> 5 & 3 != 0)
            && global
                .front()
                .is_some_and(|g| g.light_done && g.sample_done);
        let mut tick = Tick {
            ce,
            input,
            light_ready: wall > 310 && (profile != 3 || rng >> 8 & 7 > 2),
            sample_ready: wall > 290 && (profile != 3 || rng >> 16 & 7 > 1),
            rop_row7: retirement.then(|| global.front().unwrap().slot),
            ..Tick::default()
        };
        // The test consumer reserves the return sink even at CE0. Read only
        // already written addresses and avoid the producer's current address.
        let signals = dut.signals(tick);
        let slot = if row == 0 {
            allocated & 31
        } else {
            allocated.wrapping_sub(1) & 31
        };
        let write_address = slot * 8 + row;
        if wall % 7 == 0 {
            let address = (wall * 17 + 131) & 255;
            if address != write_address && basic[address].is_some() {
                tick.basic_read = Some(address as u8);
            }
        }
        let step = dut.tick(tick).unwrap();
        assert_eq!(
            step.signals.basic_return, expected_return,
            "Basic wall return {wall}"
        );
        expected_return = tick
            .basic_read
            .filter(|_| step.signals.advance)
            .map(|a| basic[usize::from(a)].unwrap());
        assert!(!step.signals.fault);
        assert_eq!(step.signals, signals);
        if ce && row == 0 && tick.input.is_some_and(|b| b.begin.mask != 0) {
            let begin = tick.input.unwrap().begin;
            if step.signals.global_used == 32 && retirement {
                assert!(
                    !step.accepted,
                    "same-edge retirement funded row-zero allocation"
                );
                old_retire_credit = true;
            }
            if (!begin.unlit
                && step.signals.light_used == 2
                && step.light_accepted
                && step.signals.light.unwrap().last)
                || (!begin.untextured
                    && step.signals.sample_used == 8
                    && step.sample_accepted
                    && step.signals.sample.unwrap().last)
            {
                assert!(
                    !step.accepted,
                    "same-edge branch ACK funded row-zero reservation"
                );
                old_branch_credit = true;
            }
        }
        assert!(
            step.signals.global_used <= 32
                && step.signals.light_used <= 2
                && step.signals.sample_used <= 8
        );
        full_global |= step.signals.global_used == 32;
        full_light |= step.signals.light_used == 2;
        full_sample |= step.signals.sample_used == 8;
        paid_ce0 |= !ce
            && (step.light_returned || step.sample_returned || step.signals.basic_return.is_some());
        if let Some(previous) = held_light {
            assert_eq!(step.signals.light, Some(previous));
        }
        if let Some(previous) = held_sample {
            assert_eq!(step.signals.sample, Some(previous));
        }
        if step.light_accepted {
            let actual = step.signals.light.unwrap();
            assert_eq!(
                Some(actual),
                expected_light.pop_front(),
                "Light transaction golden {wall}"
            );
            if actual.last {
                global
                    .iter_mut()
                    .find(|g| g.slot == actual.descriptor.slot)
                    .unwrap()
                    .light_done = true;
            }
        }
        if step.sample_accepted {
            let actual = step.signals.sample.unwrap();
            assert_eq!(
                Some(actual),
                expected_sample.pop_front(),
                "Sample transaction golden {wall}"
            );
            if actual.last {
                global
                    .iter_mut()
                    .find(|g| g.slot == actual.descriptor.slot)
                    .unwrap()
                    .sample_done = true;
            }
        }
        held_light = step.signals.light.filter(|_| !step.light_accepted);
        held_sample = step.signals.sample.filter(|_| !step.sample_accepted);
        if let Some(retire) = tick.rop_row7 {
            assert_eq!(retire, global.pop_front().unwrap().slot);
        }
        if let Some((address, data)) = step.basic_write {
            basic[usize::from(address)] = Some(data);
        }
        if let Some((slot, begin)) = step.published {
            global.push_back(Published {
                slot,
                light_done: begin.unlit,
                sample_done: begin.untextured,
            });
            let descriptor = Descriptor {
                slot,
                mask: begin.mask,
                draw: begin.draw,
                force_coarsest: false,
            };
            if !begin.unlit {
                expected_light.extend((0..8).map(|r| Row {
                    descriptor,
                    row: r as u8,
                    last: r == 7,
                    data: beat(quad, r, profile).lighting,
                }));
            }
            if !begin.untextured {
                expected_sample.extend((0..4).map(|r| Row {
                    descriptor: Descriptor {
                        force_coarsest: begin.force_coarsest,
                        ..descriptor
                    },
                    row: r as u8,
                    last: r == 3,
                    data: beat(quad, r * 2 + 1, profile).uv,
                }));
            }
        }
        if step.accepted {
            if row == 0 && beat(quad, row, profile).begin.mask != 0 {
                allocated += 1;
            }
            row += 1;
            if row == 8 {
                row = 0;
                quad += 1;
            }
        }
        edges.push(Edge { tick, step });
        if quad == 140
            && global.is_empty()
            && step.signals.global_used == 0
            && expected_return.is_none()
        {
            assert!(expected_light.is_empty() && expected_sample.is_empty());
            if profile == 0 {
                assert!(full_global);
                assert!(old_retire_credit);
            }
            if profile == 1 {
                assert!(full_light);
                assert!(old_branch_credit);
            }
            if profile == 2 {
                assert!(full_sample);
                assert!(old_branch_credit);
            }
            if profile == 3 {
                assert!(paid_ce0);
            }
            println!(
                "profile {profile}: {} edges, {allocated} live quads",
                edges.len()
            );
            return edges;
        }
    }
    panic!("ingress transaction watchdog");
}

#[test]
fn branch_rows_publication_retirement_and_wall_returns_match_transactions() {
    for profile in 0..4 {
        trace(profile);
    }
}

#[test]
fn partial_and_zero_tails_and_branch_ack_do_not_retire_global() {
    let mut dut = Ingress::new(2_000);
    for row in 0..7 {
        let s = dut
            .tick(Tick {
                ce: true,
                input: Some(beat(1, row, 3)),
                light_ready: true,
                sample_ready: true,
                ..Tick::default()
            })
            .unwrap();
        assert!(
            s.accepted
                && s.published.is_none()
                && s.signals.light.is_none()
                && s.signals.sample.is_none()
        );
    }
    for _ in 0..31 {
        let s = dut
            .tick(Tick {
                ce: true,
                light_ready: true,
                sample_ready: true,
                ..Tick::default()
            })
            .unwrap();
        assert!(s.signals.light.is_none() && s.signals.sample.is_none());
        assert_eq!(s.signals.global_used, 1);
    }
    assert!(dut
        .tick(Tick {
            ce: true,
            input: Some(beat(1, 7, 3)),
            ..Tick::default()
        })
        .unwrap()
        .published
        .is_some());
    for _ in 0..100 {
        dut.tick(Tick {
            ce: true,
            light_ready: true,
            sample_ready: true,
            ..Tick::default()
        })
        .unwrap();
    }
    let s = dut.signals(Tick {
        ce: true,
        ..Tick::default()
    });
    assert_eq!((s.global_used, s.light_used, s.sample_used), (1, 0, 0));
    // An empty helper quad still consumes exactly eight input beats, but owns
    // no slot, queue descriptor or Basic payload, even with an older live quad.
    for row in 0..8 {
        let step = dut
            .tick(Tick {
                ce: true,
                input: Some(beat(3, row, 3)),
                ..Tick::default()
            })
            .unwrap();
        assert!(step.accepted && step.published.is_none() && step.basic_write.is_none());
        assert_eq!(
            (
                step.signals.global_used,
                step.signals.light_used,
                step.signals.sample_used
            ),
            (1, 0, 0)
        );
    }
    let mut bad = Ingress::new(8);
    bad.tick(Tick {
        ce: true,
        rop_row7: Some(0),
        ..Tick::default()
    })
    .unwrap();
    assert!(
        bad.signals(Tick {
            ce: true,
            ..Tick::default()
        })
        .fault
    );
}

fn fault_trace(kind: usize) -> Vec<Edge> {
    let mut dut = Ingress::new(256);
    let mut edges = Vec::new();
    for row in 0..8 {
        let tick = Tick {
            ce: true,
            input: Some(beat(1, row, 3)),
            ..Tick::default()
        };
        let step = dut.tick(tick).unwrap();
        assert!(step.accepted);
        edges.push(Edge { tick, step });
    }
    // Leave actual queued rows offered, pay a Basic read, then inject a parent
    // fault concurrently with input-valid and both branch consumer-ready bits.
    for tick in [
        Tick {
            ce: false,
            ..Tick::default()
        },
        Tick {
            ce: true,
            basic_read: Some(0),
            ..Tick::default()
        },
    ] {
        let step = dut.tick(tick).unwrap();
        edges.push(Edge { tick, step });
    }
    if kind == 2 {
        let tick = Tick {
            ce: true,
            input: Some(beat(2, 0, 3)),
            ..Tick::default()
        };
        let step = dut.tick(tick).unwrap();
        assert!(step.accepted);
        edges.push(Edge { tick, step });
    }
    let mut malformed = beat(2, usize::from(kind == 2), 3);
    if kind == 2 {
        malformed.basic = 0x10000;
    }
    let tick = Tick {
        ce: true,
        input: Some(malformed),
        light_ready: true,
        sample_ready: true,
        rop_row7: (kind == 0).then_some(19),
        basic_read: (kind == 1).then_some(8),
        ..Tick::default()
    };
    let step = dut.tick(tick).unwrap();
    assert!(step.signals.light.is_some() && step.signals.sample.is_some());
    assert!(
        !step.signals.advance
            && !step.accepted
            && step.published.is_none()
            && step.basic_write.is_none()
            && !step.light_accepted
            && !step.sample_accepted
    );
    let frozen = step.signals;
    edges.push(Edge { tick, step });
    for wall in 0..12 {
        let tick = Tick {
            ce: wall & 1 == 0,
            input: Some(beat(4, 0, 3)),
            light_ready: true,
            sample_ready: true,
            basic_read: Some(0),
            rop_row7: Some(0),
            ..Tick::default()
        };
        let step = dut.tick(tick).unwrap();
        assert!(
            step.signals.fault
                && !step.signals.advance
                && !step.accepted
                && !step.light_accepted
                && !step.sample_accepted
        );
        assert_eq!(
            (
                step.signals.input_row,
                step.signals.global_used,
                step.signals.light_used,
                step.signals.sample_used,
                step.signals.retire_slot
            ),
            (
                frozen.input_row,
                frozen.global_used,
                frozen.light_used,
                frozen.sample_used,
                frozen.retire_slot
            )
        );
        assert_eq!(
            (step.signals.light, step.signals.sample),
            (frozen.light, frozen.sample)
        );
        edges.push(Edge { tick, step });
    }
    let tick = Tick {
        reset: true,
        ce: false,
        ..Tick::default()
    };
    let step = dut.tick(tick).unwrap();
    edges.push(Edge { tick, step });
    for row in 0..8 {
        let tick = Tick {
            ce: true,
            input: Some(beat(4, row, 3)),
            ..Tick::default()
        };
        let step = dut.tick(tick).unwrap();
        assert!(!step.signals.fault && step.signals.advance && step.accepted);
        if row == 7 {
            assert_eq!(step.published.unwrap().0, 0);
        }
        edges.push(Edge { tick, step });
    }
    edges
}

#[test]
fn parent_fault_preflight_is_atomic_and_terminal() {
    for kind in 0..3 {
        fault_trace(kind);
    }
}

fn bounded(mut command: Command, dir: &std::path::Path, name: &str) {
    let mut child = command
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name}: {} {}",
                fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} watchdog");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn bench(edges: &[Edge], native: bool) -> String {
    let mut text = String::from("module tb; reg clk=0,reset=1,ce=0,in_valid=0,basic_read=0,light_ready=0,sample_ready=0,rop_row7_accepted=0; reg [22:0] in_begin=0; reg [31:0] in_basic=0; reg [35:0] in_light=0,in_uv=0;reg [7:0] basic_read_address=0;reg [4:0] rop_slot=0; wire advance,in_ready,published,basic_write,basic_return_valid,basic_collision,light_valid,sample_valid,light_last,sample_last,fault;wire [2:0] in_row,light_row;wire [1:0] sample_row,light_used;wire [3:0] sample_used;wire [4:0] published_slot,retire_slot;wire [22:0] published_begin;wire [7:0] basic_write_address;wire [23:0] basic_write_data,basic_return_data;wire [35:0] light_data,sample_data;wire [9:0] light_descriptor;wire [10:0] sample_descriptor;wire [5:0] global_used;gpu_v2_quad_ingress dut(.*); initial begin #200000; $fatal(1,\"HDL watchdog\"); end initial begin #1;clk=1;#1;clk=0;#1;reset=0;\n");
    if native {
        text = text.replace("module tb;", "module tb; GSR GSR(.GSRI(1'b1));");
    }
    for (i, edge) in edges.iter().enumerate() {
        let t = edge.tick;
        let s = edge.step;
        let b = t.input.unwrap_or_default();
        text.push_str(&format!("reset={};ce={};in_valid={};in_begin=23'h{:x};in_basic=32'h{:x};in_light=36'h{:x};in_uv=36'h{:x};light_ready={};sample_ready={};rop_row7_accepted={};rop_slot=5'd{};basic_read={};basic_read_address=8'd{};#1;\n",
            u8::from(t.reset),u8::from(t.ce), u8::from(t.input.is_some()),b.begin.packed(),b.basic,b.lighting,b.uv,u8::from(t.light_ready),u8::from(t.sample_ready),u8::from(t.rop_row7.is_some()),t.rop_row7.unwrap_or(0),u8::from(t.basic_read.is_some()),t.basic_read.unwrap_or(0)));
        text.push_str(&format!("if(advance!==1'b{} || in_ready!==1'b{} || in_row!==3'd{} || published!==1'b{} || basic_write!==1'b{} || light_valid!==1'b{} || sample_valid!==1'b{} || basic_return_valid!==1'b{} || global_used!==6'd{} || light_used!==2'd{} || sample_used!==4'd{} || retire_slot!==5'd{} || fault!==1'b{} || basic_collision!==1'b{}) $fatal(1,\"handshake edge {i} ready=%b row=%d pub=%b lv=%b sv=%b br=%b gu=%d lu=%d su=%d fault=%b\",in_ready,in_row,published,light_valid,sample_valid,basic_return_valid,global_used,light_used,sample_used,fault);\n",
            u8::from(s.signals.advance),u8::from(s.signals.input_ready),s.signals.input_row,u8::from(s.published.is_some()),u8::from(s.basic_write.is_some()),u8::from(s.signals.light.is_some()),u8::from(s.signals.sample.is_some()),u8::from(s.signals.basic_return.is_some()),s.signals.global_used,s.signals.light_used,s.signals.sample_used,s.signals.retire_slot,u8::from(s.signals.fault),u8::from(s.signals.basic_collision)));
        if let Some((slot, b)) = s.published {
            text.push_str(&format!("if(published_slot!==5'd{slot} || published_begin!==23'h{:x}) $fatal(1,\"descriptor edge {i}\");\n", b.packed()));
        }
        if let Some((a, d)) = s.basic_write {
            text.push_str(&format!("if(basic_write_address!==8'd{a} || basic_write_data!==24'h{d:x}) $fatal(1,\"Basic write {i}\");\n"));
        }
        if let Some(d) = s.signals.basic_return {
            text.push_str(&format!(
                "if(basic_return_data!==24'h{d:x}) $fatal(1,\"Basic return {i}\");\n"
            ));
        }
        for (prefix, word, width) in [
            ("light", s.signals.light, 10),
            ("sample", s.signals.sample, 11),
        ] {
            if let Some(w) = word {
                text.push_str(&format!("if({prefix}_data!==36'h{:x} || {prefix}_row!=={}'d{} || {prefix}_last!==1'b{} || {prefix}_descriptor!=={width}'h{:x}) $fatal(1,\"{prefix} payload {i} data=%h row=%d desc=%h\",{prefix}_data,{prefix}_row,{prefix}_descriptor);\n",w.data,if prefix=="light"{3}else{2},w.row,u8::from(w.last),w.descriptor.packed()));
            }
        }
        text.push_str("clk=1;#1;clk=0;#1;\n");
    }
    text.push_str(&format!(
        "$display(\"PASS edges={}\");$finish;end endmodule\n",
        edges.len()
    ));
    text
}

#[test]
#[ignore = "requires Icarus and GOWIN_HOME native primitive simulation"]
fn actual_ingress_portable_and_native_match_each_edge() {
    for profile in 0..7 {
        let edges = if profile < 4 {
            trace(profile)
        } else {
            fault_trace(profile - 4)
        };
        for native in [false, true] {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../target/pixel-quad-ingress-20261010/replay-{profile}-{native}"
            ));
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("dut.v"),
                rtl::verilog(if native {
                    rtl::Backend::Native
                } else {
                    rtl::Backend::Portable
                })
                .unwrap(),
            )
            .unwrap();
            fs::write(dir.join("tb.v"), bench(&edges, native)).unwrap();
            let mut compile =
                Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
            compile
                .current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"]);
            if native {
                compile.arg(
                    std::path::Path::new(
                        &std::env::var_os("GOWIN_HOME")
                            .expect("native simulation needs GOWIN_HOME"),
                    )
                    .join("IDE/simlib/gw2a/prim_sim.v"),
                );
            }
            bounded(compile, &dir, "compile");
            let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
            run.current_dir(&dir).arg("test.vvp");
            bounded(run, &dir, "run");
            println!(
                "profile {profile} native={native}: {}",
                fs::read_to_string(dir.join("run.out")).unwrap().trim()
            );
        }
    }
}

/// Reuse the already independent queue/Basic model and physical port fixture.
/// Raw attempted CE remains high while a peer denies actual compute commit.
#[test]
#[ignore = "requires Icarus and GOWIN_HOME native primitive simulation"]
fn shared_ingress_denial_and_bad_edge_are_atomic() {
    let mut dut = Ingress::new(1500);
    let mut edges = Vec::new();
    let mut allows = Vec::new();
    let mut bad = Vec::new();
    let mut quad = 0;
    let mut row = 0;
    let mut paid_denial = false;
    let mut held_denial = false;
    for wall in 0..1200 {
        let deny = wall % 4 == 1 || wall % 17 == 0;
        let mut b = beat(quad, row, 3);
        b.begin.mask = 15;
        b.begin.unlit = false;
        b.begin.untextured = false;
        let raw_tick = Tick {
            ce: true,
            input: (quad < 16).then_some(b),
            light_ready: true,
            sample_ready: true,
            basic_read: (quad > 0 && wall % 7 == 0).then_some(0),
            ..Default::default()
        };
        let proposal = dut.signals(raw_tick);
        assert!(proposal.advance);
        let step = dut
            .tick(Tick {
                ce: !deny,
                ..raw_tick
            })
            .unwrap();
        paid_denial |= deny
            && (step.light_returned || step.sample_returned || step.signals.basic_return.is_some());
        held_denial |= deny && (step.signals.light.is_some() || step.signals.sample.is_some());
        if step.accepted {
            row += 1;
            if row == 8 {
                quad += 1;
                row = 0;
            }
        }
        edges.push(Edge {
            tick: raw_tick,
            step,
        });
        allows.push(!deny);
        bad.push(false);
        if quad == 16 && proposal.light_used == 0 && proposal.sample_used == 0 {
            break;
        }
        assert!(wall < 1199, "bounded shared ingress drain");
    }
    assert!(paid_denial && held_denial);
    // Peer denial cannot hide local malformed retirement or partially push row0.
    let raw_tick = Tick {
        ce: true,
        input: Some(beat(17, 0, 3)),
        light_ready: true,
        sample_ready: true,
        rop_row7: Some(31),
        basic_read: Some(0),
        ..Default::default()
    };
    let proposal = dut.signals(raw_tick);
    assert!(!proposal.advance);
    let step = dut.tick(raw_tick).unwrap();
    edges.push(Edge {
        tick: raw_tick,
        step,
    });
    allows.push(false);
    bad.push(true);
    for _ in 0..4 {
        let t = Tick {
            ce: true,
            input: Some(beat(18, 0, 3)),
            light_ready: true,
            sample_ready: true,
            ..Default::default()
        };
        edges.push(Edge {
            tick: t,
            step: dut.tick(t).unwrap(),
        });
        allows.push(true);
        bad.push(false);
    }
    assert!(edges.last().unwrap().step.signals.fault);
    for native in [false, true] {
        let original = bench(&edges, native)
            .replace("reset=1,ce=0", "reset=1,proposal_ce=0,commit_allow=1")
            .replace(
                "wire advance,in_ready",
                "wire preflight_bad,advance,in_ready",
            )
            .replace("gpu_v2_quad_ingress dut", "gpu_v2_quad_ingress_shared dut");
        let mut tb = String::new();
        let mut index = 0;
        for line in original.lines() {
            if line.starts_with("reset=") {
                tb.push_str(&format!("commit_allow={};", u8::from(allows[index])));
            }
            if line.starts_with("clk=1;") {
                tb.push_str(&format!(
                    "if(preflight_bad!==1'b{})$fatal(1,\"quad raw preflight edge{index}\");\n",
                    u8::from(bad[index])
                ));
                index += 1;
            }
            tb.push_str(&line.replace(";ce=", ";proposal_ce="));
            tb.push('\n');
        }
        assert_eq!(index, edges.len());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/pixel-quad-shared-preflight-20261010/{native}"
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("dut.v"),
            rtl::verilog_shared(if native {
                rtl::Backend::Native
            } else {
                rtl::Backend::Portable
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(dir.join("tb.v"), tb).unwrap();
        let mut c = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        c.current_dir(&dir)
            .args(["-g2012", "-s", "tb", "-o", "test.vvp", "dut.v", "tb.v"]);
        if native {
            c.arg(
                std::path::Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                    .join("IDE/simlib/gw2a/prim_sim.v"),
            );
        }
        bounded(c, &dir, "compile");
        let mut c = Command::new(std::env::var_os("VVP_EXE").unwrap());
        c.current_dir(&dir).arg("test.vvp");
        bounded(c, &dir, "run");
        assert!(fs::read_to_string(dir.join("run.out"))
            .unwrap()
            .contains("PASS edges="));
        fs::write(dir.join("receipt.json"),format!("{{\"edges\":{},\"paid_denial\":true,\"held_denial\":true,\"bad_concurrent_push\":true,\"scope\":\"shared Quad ingress queues Basic only\"}}",edges.len())).unwrap();
    }
}
