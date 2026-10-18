//! Actual Sample8 queue → four-row UV RAM/D ingress. Independent attributes and
//! parent context leases drive both portable/native tops; this is not Runtime.
use gpu_v2::{
    system::pixel::{
        quad_ingress::{self as quad, Beat, Begin, Descriptor, Row},
        quad_sampling_ingress as composition,
    },
    texture::emu::{context_store::Raw, derivative::Header, uv_store::Coordinate},
};
use std::{
    collections::VecDeque,
    fmt::Write,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const CONFIGS: [u32; 2] = [
    0x0100 | (3 << 16) | (9 << 20) | (1 << 24) | (2 << 25),
    0xff00 | (11 << 16) | (5 << 20) | (1 << 25),
];
fn uv(q: usize) -> [[i64; 2]; 4] {
    let values = [-131072, -65537, -1, 0, 65535, 65536, 131071];
    std::array::from_fn(|lane| [values[(q + lane * 3) % 7], values[(q * 3 + lane + 1) % 7]])
}
fn word(pair: [i64; 2]) -> u64 {
    (pair[0] as u64 & 0x3ffff) | ((pair[1] as u64 & 0x3ffff) << 18)
}
fn beat(q: usize, row: usize) -> Beat {
    Beat {
        begin: Begin {
            mask: if q % 13 == 4 { 0 } else { [1, 6, 15, 8][q % 4] },
            draw: !q.is_multiple_of(2),
            unlit: q.is_multiple_of(3),
            untextured: q % 7 == 2,
            force_coarsest: q.is_multiple_of(5),
            origin: ((q * 127) & 0x7fff) as u16,
        },
        basic: if row.is_multiple_of(2) {
            ((q * 1957 + row * 149) & 0xffffff) as u32
        } else {
            ((q * 937 + row * 211) & 0xffff) as u32
        },
        lighting: ((q * 0x71237 + row * 0x1139) & 0xfffffffff) as u64,
        uv: word(uv(q)[row / 2]),
    }
}
#[derive(Clone, Copy)]
struct Published {
    slot: u8,
    light_done: bool,
    sample_done: bool,
}
#[derive(Clone, Copy)]
struct Lease {
    global: u8,
    q: usize,
    metadata: u64,
    committed: usize,
}
#[derive(Clone, Copy)]
struct Vector {
    t: composition::Tick,
    step: composition::Step,
    atomic_write: Option<(usize, u64)>,
}
fn trace(paused: bool) -> Vec<Vector> {
    let mut dut = composition::Ingress::new(20000);
    let mut light = VecDeque::<Row>::new();
    let mut sample = VecDeque::<(Row, usize)>::new();
    let mut globals = VecDeque::<Published>::new();
    let mut jobs = VecDeque::<Coordinate>::new();
    let mut leases = [None::<Lease>; 8];
    let mut c_remaining = [0usize; 8];
    let mut committed_uv = [[[0i64; 2]; 4]; 8];
    let mut basic = [None; 256];
    let mut basic_return = None;
    let mut next_context = 0usize;
    let mut allocated = 0usize;
    let mut q = 0;
    let mut source_row = 0;
    let mut enabled = 0usize;
    let mut phase = 0u8;
    let mut rng = 0x7123_4568u32;
    let mut held_sample = None;
    let mut atomic = None::<(usize, [u64; 4], usize)>;
    let mut vectors = vec![];
    let mut raw_count = 0;
    let mut d_count = 0;
    let mut c_count = 0;
    let mut full_context = false;
    let mut full_sample = false;
    let mut full_global = false;
    let mut ce0_returns = 0;
    let mut simultaneous = 0;
    let mut previous_c_read = false;
    for wall in 0..20000 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let ce = (!paused || rng & 7 != 0) && !previous_c_read;
        let mut t = composition::Tick {
            quad: quad::Tick {
                ce,
                input: (q < 80).then(|| beat(q, source_row)),
                light_ready: wall > 300 && (!paused || rng & 31 > 5),
                rop_row7: (ce
                    && wall > 1200
                    && (!paused || rng & 63 > 9)
                    && globals
                        .front()
                        .is_some_and(|g| g.light_done && g.sample_done))
                .then(|| globals.front().unwrap().slot),
                ..Default::default()
            },
            phase,
            context_credit: wall > 200 && leases[next_context % 8].is_none(),
            context_slot: next_context % 8,
            config: CONFIGS[0],
            ..Default::default()
        };
        let preview = dut.signals(t);
        if let Some(s) = preview.quad.sample {
            t.config = CONFIGS[usize::from(s.descriptor.draw)];
        }
        if ce {
            if let (0, Some((slot, _))) = (phase, preview.derivative) {
                if enabled >= leases[slot].unwrap().committed + 4
                    && (!paused || !rng.is_multiple_of(3))
                {
                    t.take_derivative = true;
                }
            }
            if phase.is_multiple_of(2)
                && preview.coordinate.is_some()
                && (!paused || !rng.is_multiple_of(5))
            {
                t.take_coordinate = true;
            }
            if !phase.is_multiple_of(2) && preview.coordinate_empty && wall > 700 {
                t.coordinate = jobs.front().copied();
            }
        }
        // Independent Basic contents supply read-return expected values. No writer collision.
        let writer = (if source_row == 0 {
            allocated
        } else {
            allocated.wrapping_sub(1)
        }) % 32
            * 8
            + source_row;
        let address = (wall * 19 + 91) % 256;
        if wall % 7 == 0 && address != writer && basic[address].is_some() {
            t.quad.basic_read = Some(address as u8);
        }
        let s = dut.signals(t);
        assert_eq!(s.quad.basic_return, basic_return, "Basic old return {wall}");
        basic_return = if s.quad.advance {
            t.quad.basic_read.and_then(|a| basic[usize::from(a)])
        } else {
            None
        };
        if let Some(actual) = s.quad.light {
            assert_eq!(Some(&actual), light.front(), "Light source {wall}");
        }
        if let Some(actual) = s.quad.sample {
            assert_eq!(
                Some(&actual),
                sample.front().map(|p| &p.0),
                "Sample source {wall}"
            );
        }
        if let Some(old) = held_sample {
            assert_eq!(s.quad.sample, Some(old), "blocked source stable {wall}");
        }
        held_sample = s.quad.sample.filter(|_| !s.quad.advance || !s.sample_ready);
        if let Some((owner, data)) = s.coordinate {
            assert_eq!(
                data,
                committed_uv[owner.slot][owner.lane].map(|v| v as u32 & 0xffff),
                "C data {wall}"
            );
        }
        full_context |= leases.iter().all(Option::is_some);
        full_sample |= s.quad.sample_used == 8;
        full_global |= s.quad.global_used == 32;
        let step = dut.tick(t).unwrap();
        let mut atomic_write = None;
        if s.quad.advance {
            if let Some((slot, words, next)) = atomic.as_mut() {
                if *next < 4 {
                    atomic_write = Some((*slot * 4 + *next, words[*next]));
                    *next += 1;
                }
            }
        }
        if step.quad.accepted {
            if source_row == 0 && beat(q, 0).begin.mask != 0 {
                allocated += 1;
            }
            if source_row == 7 {
                let b = beat(q, 0).begin;
                if b.mask != 0 {
                    let slot = ((allocated - 1) % 32) as u8;
                    assert_eq!(step.quad.published, Some((slot, b)));
                    globals.push_back(Published {
                        slot,
                        light_done: b.unlit,
                        sample_done: b.untextured,
                    });
                    let descriptor = Descriptor {
                        slot,
                        mask: b.mask,
                        draw: b.draw,
                        force_coarsest: b.force_coarsest,
                    };
                    if !b.unlit {
                        light.extend((0..8).map(|row| Row {
                            descriptor: Descriptor {
                                force_coarsest: false,
                                ..descriptor
                            },
                            row,
                            last: row == 7,
                            data: beat(q, usize::from(row)).lighting,
                        }));
                    }
                    if !b.untextured {
                        sample.extend((0..4).map(|row| {
                            (
                                Row {
                                    descriptor,
                                    row,
                                    last: row == 3,
                                    data: word(uv(q)[usize::from(row)]),
                                },
                                q,
                            )
                        }));
                    }
                }
                source_row = 0;
                q += 1;
            } else {
                source_row += 1;
            }
        }
        if let Some((address, data)) = step.quad.basic_write {
            basic[usize::from(address)] = Some(data);
        }
        if step.quad.light_accepted {
            let row = light.pop_front().unwrap();
            if row.last {
                globals
                    .iter_mut()
                    .find(|g| g.slot == row.descriptor.slot)
                    .unwrap()
                    .light_done = true;
            }
        }
        if step.quad.sample_accepted {
            let (row, idx) = sample.pop_front().unwrap();
            if row.row == 0 {
                let cfg = CONFIGS[usize::from(row.descriptor.draw)];
                let expected = Raw {
                    bias: cfg as u16 as i16,
                    force_coarsest: row.descriptor.force_coarsest,
                    header: Header {
                        quad: row.descriptor.slot & 15,
                        mask: row.descriptor.mask,
                        slot: ((cfg >> 16) & 15) as u8,
                        max_n: ((cfg >> 20) & 15) as u8,
                        has_mip: cfg >> 24 & 1 != 0,
                        filter: ((cfg >> 25) & 3) as u8,
                    },
                }
                .pack()
                .unwrap();
                assert_eq!(step.uv.raw_write, Some((next_context % 8, expected)));
                assert!(leases[next_context % 8].is_none());
                leases[next_context % 8] = Some(Lease {
                    global: row.descriptor.slot,
                    q: idx,
                    metadata: expected,
                    committed: usize::MAX,
                });
                raw_count += 1;
            } else {
                assert_eq!(step.uv.raw_write, None);
            }
            assert_eq!(
                step.uv.write,
                Some((next_context % 8 * 4 + usize::from(row.row), row.data))
            );
            if row.last {
                assert_eq!(step.uv.committed, Some(next_context % 8));
                leases[next_context % 8].as_mut().unwrap().committed = enabled;
                committed_uv[next_context % 8] = uv(idx);
                atomic = Some((next_context % 8, uv(idx).map(word), 1));
                atomic_write = Some((next_context % 8 * 4, word(uv(idx)[0])));
                next_context += 1;
            }
        } else {
            assert_eq!(step.uv.raw_write, None);
            assert_eq!(step.uv.committed, None);
        }
        if t.take_derivative && s.quad.advance {
            let (slot, data) = s.derivative.unwrap();
            let lease = leases[slot].unwrap();
            assert_eq!(data, std::array::from_fn(|i| uv(lease.q)[i / 2][i % 2]));
            assert_eq!(Raw::unpack(lease.metadata).header.quad, lease.global & 15);
            jobs.extend((0..4).map(|lane| Coordinate { slot, lane }));
            c_remaining[slot] = 4;
            d_count += 1;
        }
        if let (Some(coordinate), true) = (t.coordinate, s.quad.advance) {
            assert_eq!(
                step.uv.read.unwrap().address,
                jobs.pop_front().unwrap().slot * 4 + coordinate.lane
            );
        }
        if t.take_coordinate && s.quad.advance {
            let slot = s.coordinate.unwrap().0.slot;
            c_remaining[slot] -= 1;
            c_count += 1;
            if c_remaining[slot] == 0 {
                let lease = leases[slot].take().unwrap();
                globals
                    .iter_mut()
                    .find(|g| g.slot == lease.global)
                    .unwrap()
                    .sample_done = true;
            }
        }
        if let (Some(row7), true) = (t.quad.rop_row7, s.quad.advance) {
            assert_eq!(globals.pop_front().unwrap().slot, row7);
        }
        ce0_returns += usize::from(!s.quad.advance && step.uv.returned.is_some());
        simultaneous += usize::from(step.uv.read.is_some() && step.uv.write.is_some());
        previous_c_read = step.uv.read.is_some();
        vectors.push(Vector {
            t,
            step,
            atomic_write,
        });
        if s.quad.advance {
            enabled += 1;
            phase = (phase + 1) & 7;
        }
        if q == 80
            && globals.is_empty()
            && light.is_empty()
            && sample.is_empty()
            && jobs.is_empty()
            && leases.iter().all(Option::is_none)
        {
            break;
        }
    }
    assert_eq!(q, 80);
    assert!(globals.is_empty());
    assert_eq!(raw_count, d_count);
    assert_eq!(c_count, raw_count * 4);
    assert!(
        full_context && full_sample && full_global,
        "real old-capacity backpressure"
    );
    assert_eq!(ce0_returns, c_count);
    assert!(simultaneous > 0);
    vectors
}

#[test]
fn real_queue_rows_raw_metadata_leases_and_backpressure() {
    trace(false);
    trace(true);
}

#[test]
fn fault_preflight_freezes_row_consumption() {
    let mut dut = composition::Ingress::new(100);
    for row in 0..8 {
        dut.tick(composition::Tick {
            quad: quad::Tick {
                ce: true,
                input: Some(beat(0, row)),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    }
    for _ in 0..8 {
        dut.tick(composition::Tick {
            quad: quad::Tick {
                ce: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    }
    let t = composition::Tick {
        quad: quad::Tick {
            ce: true,
            rop_row7: Some(7),
            ..Default::default()
        },
        context_credit: true,
        ..Default::default()
    };
    let offered = dut.signals(t).quad.sample;
    assert!(offered.is_some());
    let step = dut.tick(t).unwrap();
    assert!(!step.signals.quad.advance);
    assert!(!step.quad.sample_accepted);
    assert_eq!(step.uv.write, None);
    let t = composition::Tick {
        quad: quad::Tick {
            ce: true,
            ..Default::default()
        },
        context_credit: true,
        ..Default::default()
    };
    assert_eq!(dut.signals(t).quad.sample, offered);
    assert!(dut.signals(t).quad.fault);
}

fn bounded(mut command: Command, dir: &Path, name: &str) {
    let mut child = command
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{name}.out"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{name}.err"))).unwrap(),
        ))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{} {}",
                fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("composition wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn packed(uv: [i64; 8]) -> String {
    (0..4)
        .rev()
        .map(|i| format!("{:09x}", word([uv[i * 2], uv[i * 2 + 1]])))
        .collect()
}
fn check(tb: &mut String, signal: &str, width: usize, value: u64, edge: usize) {
    writeln!(
        tb,
        "if({signal}!=={width}'h{value:x}) $fatal(1,\"{signal} edge {edge}\");"
    )
    .unwrap();
}
#[test]
#[ignore = "requires Icarus and Gowin native RAM models"]
fn real_quad_uv_portable_native_and_atomic_reference_rtl() {
    for paused in [false, true] {
        let vectors = trace(paused);
        for native in [false, true] {
            for atomic in [false, true] {
                let backend = if native {
                    quad::rtl::Backend::Native
                } else {
                    quad::rtl::Backend::Portable
                };
                let source = composition::verilog(backend, atomic).unwrap();
                let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                    "../../target/pixel-composition-20261010/quad-uv-{paused}-{native}-{atomic}"
                ));
                fs::create_dir_all(&dir).unwrap();
                fs::write(dir.join("top.v"), &source).unwrap();
                let header = source.split(");").next().unwrap();
                let mut tb = String::from("module tb;\n");
                for line in header.lines().skip(1) {
                    let line = line.trim().trim_end_matches(',');
                    if let Some(rest) = line.strip_prefix("input wire ") {
                        writeln!(tb, "reg {rest}=0;").unwrap();
                    }
                    if let Some(rest) = line.strip_prefix("output wire ") {
                        writeln!(tb, "wire {rest};").unwrap();
                    }
                }
                tb.push_str("gpu_v2_quad_sampling_ingress dut(.*);reg gs=1;GSR global_reset(.GSRI(gs));initial begin #200000;$fatal(1,\"watchdog\");end initial begin reset=1;#1;clk=1;#1;clk=0;reset=0;#1;\n");
                for (i, v) in vectors.iter().enumerate() {
                    let t = v.t;
                    let s = v.step.signals;
                    let e = v.step;
                    let b = t.quad.input.unwrap_or_default();
                    let owner = t.coordinate.unwrap_or(Coordinate { slot: 0, lane: 0 });
                    writeln!(tb,"ce={};in_valid={};in_begin=23'h{:x};in_basic=32'h{:x};in_light=36'h{:x};in_uv=36'h{:x};light_ready={};rop_row7_accepted={};rop_slot={};basic_read={};basic_read_address={};phase={};context_credit={};context_slot={};draw_config=27'h{:x};d_take={};c_request={};c_slot={};c_lane={};c_take={};#1;",u8::from(t.quad.ce),u8::from(t.quad.input.is_some()),b.begin.packed(),b.basic,b.lighting,b.uv,u8::from(t.quad.light_ready),u8::from(t.quad.rop_row7.is_some()),t.quad.rop_row7.unwrap_or(0),u8::from(t.quad.basic_read.is_some()),t.quad.basic_read.unwrap_or(0),t.phase,u8::from(t.context_credit),t.context_slot,t.config,u8::from(t.take_derivative),u8::from(t.coordinate.is_some()),owner.slot,owner.lane,u8::from(t.take_coordinate)).unwrap();
                    for (name, width, value) in [
                        ("advance", 1, u64::from(s.quad.advance)),
                        ("in_ready", 1, u64::from(s.quad.input_ready)),
                        ("in_row", 3, u64::from(s.quad.input_row)),
                        ("published", 1, u64::from(e.quad.published.is_some())),
                        ("basic_write", 1, u64::from(e.quad.basic_write.is_some())),
                        (
                            "basic_return_valid",
                            1,
                            u64::from(s.quad.basic_return.is_some()),
                        ),
                        ("basic_collision", 1, u64::from(s.quad.basic_collision)),
                        ("light_valid", 1, u64::from(s.quad.light.is_some())),
                        ("sample_valid", 1, u64::from(s.quad.sample.is_some())),
                        ("sample_ready", 1, u64::from(s.sample_ready)),
                        ("sample_fire", 1, u64::from(e.quad.sample_accepted)),
                        ("retire_slot", 5, u64::from(s.quad.retire_slot)),
                        ("global_used", 6, u64::from(s.quad.global_used)),
                        ("light_used", 2, u64::from(s.quad.light_used)),
                        ("sample_used", 4, u64::from(s.quad.sample_used)),
                        ("fault", 1, u64::from(s.quad.fault)),
                        ("raw_write", 1, u64::from(e.uv.raw_write.is_some())),
                        ("committed", 1, u64::from(e.uv.committed.is_some())),
                        ("d_valid", 1, u64::from(s.derivative.is_some())),
                        ("c_valid", 1, u64::from(s.coordinate.is_some())),
                        ("c_empty", 1, u64::from(s.coordinate_empty)),
                        ("uv_read", 1, u64::from(e.uv.read.is_some())),
                    ] {
                        check(&mut tb, name, width, value, i);
                    }
                    if let Some((slot, b)) = e.quad.published {
                        check(&mut tb, "published_slot", 5, u64::from(slot), i);
                        check(&mut tb, "published_begin", 23, u64::from(b.packed()), i);
                    }
                    if let Some((address, data)) = e.quad.basic_write {
                        check(&mut tb, "basic_write_address", 8, u64::from(address), i);
                        check(&mut tb, "basic_write_data", 24, u64::from(data), i);
                    }
                    if let Some(data) = s.quad.basic_return {
                        check(&mut tb, "basic_return_data", 24, u64::from(data), i);
                    }
                    for (prefix, row) in [("light", s.quad.light), ("sample", s.quad.sample)] {
                        if let Some(row) = row {
                            check(&mut tb, &format!("{prefix}_data"), 36, row.data, i);
                            check(
                                &mut tb,
                                &format!("{prefix}_row"),
                                if prefix == "light" { 3 } else { 2 },
                                u64::from(row.row),
                                i,
                            );
                            check(
                                &mut tb,
                                &format!("{prefix}_last"),
                                1,
                                u64::from(row.last),
                                i,
                            );
                            check(
                                &mut tb,
                                &format!("{prefix}_descriptor"),
                                if prefix == "light" { 10 } else { 11 },
                                u64::from(row.descriptor.packed()),
                                i,
                            );
                        }
                    }
                    if let Some((slot, word)) = e.uv.raw_write {
                        check(&mut tb, "raw_slot", 3, slot as u64, i);
                        check(&mut tb, "raw_metadata", 36, word, i);
                    }
                    if let Some(slot) = e.uv.committed {
                        check(&mut tb, "committed_slot", 3, slot as u64, i);
                    }
                    if let Some((slot, uv)) = s.derivative {
                        check(&mut tb, "d_slot", 3, slot as u64, i);
                        writeln!(
                            tb,
                            "if(d_uv!==144'h{}) $fatal(1,\"D data edge {i}\");",
                            packed(uv)
                        )
                        .unwrap();
                    }
                    if let Some((owner, uv)) = s.coordinate {
                        check(&mut tb, "c_owner_slot", 3, owner.slot as u64, i);
                        check(&mut tb, "c_owner_lane", 2, owner.lane as u64, i);
                        check(
                            &mut tb,
                            "c_uv",
                            32,
                            u64::from(uv[0]) | (u64::from(uv[1]) << 16),
                            i,
                        );
                    }
                    if let Some(r) = e.uv.read {
                        check(&mut tb, "uv_read_address", 5, r.address as u64, i);
                    }
                    let write = if atomic { v.atomic_write } else { e.uv.write };
                    check(&mut tb, "uv_write", 1, u64::from(write.is_some()), i);
                    if let Some((address, data)) = write {
                        check(&mut tb, "uv_write_address", 5, address as u64, i);
                        check(&mut tb, "uv_write_data", 36, data, i);
                    }
                    tb.push_str("clk=1;#1;clk=0;#1;\n");
                }
                writeln!(tb,"$display(\"PASS {} Quad UV edges;80 source quads;atomic={atomic};native={native};paused={paused}\");$finish;end endmodule",vectors.len()).unwrap();
                fs::write(dir.join("tb.v"), tb).unwrap();
                let mut compile = Command::new(
                    std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
                );
                compile
                    .current_dir(&dir)
                    .args(["-g2012", "-s", "tb", "-o", "test.vvp", "top.v", "tb.v"]);
                compile.arg(
                    Path::new(&std::env::var_os("GOWIN_HOME").unwrap())
                        .join("IDE/simlib/gw2a/prim_sim.v"),
                );
                bounded(compile, &dir, "compile");
                let mut run =
                    Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
                run.current_dir(&dir).arg("test.vvp");
                bounded(run, &dir, "run");
            }
        }
    }
}
