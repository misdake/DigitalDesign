//! Independent sequential helper fixtures, a register-level ingress model,
//! and actual behavioral/native RAM replay. All runs have fixed edge bounds.
use gpu_v2::texture::{
    emu::{context_store::Raw, derivative::Header, uv_rows, uv_store::Coordinate},
    rtl,
};
use std::{
    collections::VecDeque,
    fmt::Write,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn helpers(q: usize) -> [[i64; 2]; 4] {
    let corners = [-131072, -65537, -65536, -1, 0, 65535, 65536, 131071];
    std::array::from_fn(|lane| [corners[(q + lane) & 7], corners[(q * 3 + lane * 5 + 1) & 7]])
}
fn metadata(q: usize) -> u64 {
    Raw {
        bias: [-8192, -257, 0, 255, 8192][q % 5],
        force_coarsest: q.is_multiple_of(7),
        header: Header {
            quad: (q % 16) as u8,
            mask: [0, 1, 6, 15][q % 4],
            slot: (q % 16) as u8,
            max_n: (q % 11) as u8,
            has_mip: q.is_multiple_of(2),
            filter: (q % 3) as u8,
        },
    }
    .pack()
    .unwrap()
}
fn row(q: usize, lane: usize) -> uv_rows::Row {
    uv_rows::Row {
        slot: q % 8,
        metadata: metadata(q),
        uv: helpers(q)[lane],
    }
}

#[test]
fn publication_is_fourth_accept_and_releases_are_distinct() {
    let mut store = uv_rows::Store::new(8).unwrap();
    for bad in [
        uv_rows::Row {
            slot: 8,
            metadata: 0,
            uv: [0; 2],
        },
        uv_rows::Row {
            slot: 0,
            metadata: 1 << 36,
            uv: [0; 2],
        },
        uv_rows::Row {
            slot: 0,
            metadata: 0,
            uv: [131072, 0],
        },
        uv_rows::Row {
            slot: 0,
            metadata: 0,
            uv: [0, -131073],
        },
    ] {
        assert!(store
            .tick(uv_rows::Tick {
                ce: true,
                row: Some(bad),
                ..Default::default()
            })
            .is_err());
        assert!(store.idle());
    }
    for lane in 0..4 {
        assert_eq!(store.next_row(), lane);
        assert_eq!(store.derivative(), None);
        let e = store
            .tick(uv_rows::Tick {
                ce: true,
                row: Some(row(3, lane)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(e.raw_write, (lane == 0).then_some((3, metadata(3))));
        assert_eq!(e.write.unwrap().0, 12 + lane);
        assert_eq!(e.committed, (lane == 3).then_some(3));
        if lane < 3 {
            assert!(store
                .tick(uv_rows::Tick {
                    ce: true,
                    phase: 0,
                    take_derivative: true,
                    ..Default::default()
                })
                .is_err());
        }
    }
    assert_eq!(
        store.derivative().unwrap().1,
        std::array::from_fn(|i| helpers(3)[i / 2][i % 2])
    );
    // This edge's D release must not fund a new row0.
    assert!(store
        .tick(uv_rows::Tick {
            ce: true,
            row: Some(row(4, 0)),
            take_derivative: true,
            ..Default::default()
        })
        .is_err());
    store
        .tick(uv_rows::Tick {
            ce: true,
            take_derivative: true,
            ..Default::default()
        })
        .unwrap();
    assert!(store.input_ready());
    // D release preserves committed UV. A new partial slot can coexist with C.
    let e = store
        .tick(uv_rows::Tick {
            ce: true,
            phase: 1,
            row: Some(row(4, 0)),
            coordinate: Some(Coordinate { slot: 3, lane: 2 }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(e.read.unwrap().address, 14);
    let c = store.coordinate().unwrap();
    let e = store.tick(uv_rows::Tick::default()).unwrap();
    assert_eq!(e.returned.unwrap().address, 14);
    assert_eq!(store.coordinate(), Some(c));
    assert_eq!(c.1, helpers(3)[2].map(|v| v as u32 & 0xffff));
    assert!(store
        .tick(uv_rows::Tick {
            ce: true,
            phase: 0,
            coordinate: Some(Coordinate { slot: 4, lane: 0 }),
            take_coordinate: true,
            ..Default::default()
        })
        .is_err());
    store
        .tick(uv_rows::Tick {
            ce: true,
            take_coordinate: true,
            ..Default::default()
        })
        .unwrap();
    assert!(store
        .tick(uv_rows::Tick {
            ce: true,
            phase: 1,
            coordinate: Some(Coordinate { slot: 4, lane: 0 }),
            ..Default::default()
        })
        .is_err());
    assert!(store
        .tick(uv_rows::Tick {
            row: Some(row(4, 1)),
            ..Default::default()
        })
        .is_err());
}

#[derive(Clone)]
struct Vector {
    t: uv_rows::Tick,
    ready: bool,
    next: usize,
    d: Option<(usize, [i64; 8])>,
    c: Option<(Coordinate, [u32; 2])>,
    empty: bool,
    edge: uv_rows::Edge,
}
fn vectors() -> Vec<Vector> {
    let mut store = uv_rows::Store::new(8).unwrap();
    let mut result = vec![];
    let mut random = 0x8914_270bu32;
    let mut phase = 0;
    let mut quad = 0;
    let mut lane = 0;
    let mut last_read = false;
    let mut jobs: VecDeque<Coordinate> = VecDeque::new();
    let mut remaining = [0; 8];
    let mut payloads = [[[0i64; 2]; 4]; 8];
    let mut raw = [0; 8];
    let mut derivatives = 0;
    let mut coordinate_takes = 0;
    let mut paused_returns = 0;
    let mut simultaneous = 0;
    for wall in 0..20000 {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        // Every native read must cross at least one CE0 wall edge.
        let ce = !last_read && random & 7 > 0;
        let d = store.derivative();
        let c = store.coordinate();
        if let Some((owner, data)) = c {
            assert_eq!(
                data,
                payloads[owner.slot][owner.lane].map(|v| v as u32 & 0xffff),
                "C numerical wall {wall}"
            );
        }
        let mut t = uv_rows::Tick {
            ce,
            phase,
            ..Default::default()
        };
        if ce {
            if quad < 64
                && store.input_ready()
                && (lane != 0 || remaining[quad % 8] == 0)
                && !random.is_multiple_of(5)
            {
                let mut offer = row(quad, lane);
                if lane != 0 {
                    // Fields outside row0 are never resampled.
                    offer.slot = (quad + 4) % 8;
                    offer.metadata = (1 << 36) - 1;
                }
                t.row = Some(offer);
            }
            if let (0, Some((slot, uv)), true) = (phase, d, !random.is_multiple_of(3)) {
                t.take_derivative = true;
                assert_eq!(uv, std::array::from_fn(|i| payloads[slot][i / 2][i % 2]));
                let actual = Raw::unpack(raw[slot]);
                let expected = Raw::unpack(metadata(derivatives));
                assert_eq!(actual, expected);
                derivatives += 1;
            }
            if let (0, Some(output), true) = (phase & 1, c, !random.is_multiple_of(4)) {
                t.take_coordinate = true;
                remaining[output.0.slot] -= 1;
                coordinate_takes += 1;
            }
            if phase & 1 != 0 && store.coordinate_empty() {
                if let Some(owner) = jobs.front().copied() {
                    let writing_slot = t.row.map(|r| if lane == 0 { r.slot } else { quad % 8 });
                    if writing_slot != Some(owner.slot) {
                        t.coordinate = jobs.pop_front();
                    }
                }
            }
        }
        let ready = store.input_ready();
        let next = store.next_row();
        let empty = store.coordinate_empty();
        let edge = store.tick(t).unwrap();
        simultaneous += usize::from(edge.write.is_some() && edge.read.is_some());
        paused_returns += usize::from(!ce && edge.returned.is_some());
        if let Some((slot, word)) = edge.raw_write {
            raw[slot] = word;
        }
        if t.row.is_some() {
            lane += 1;
            if lane == 4 {
                payloads[quad % 8] = helpers(quad);
                remaining[quad % 8] = 4;
                jobs.extend((0..4).map(|lane| Coordinate {
                    slot: quad % 8,
                    lane,
                }));
                quad += 1;
                lane = 0;
            }
        }
        result.push(Vector {
            t,
            ready,
            next,
            d,
            c,
            empty,
            edge,
        });
        last_read = edge.read.is_some();
        if ce {
            phase = (phase + 1) & 7;
        }
        if quad == 64 && derivatives == 64 && coordinate_takes == 256 && store.idle() {
            break;
        }
    }
    assert_eq!((quad, derivatives, coordinate_takes), (64, 64, 256));
    assert_eq!(paused_returns, 256);
    assert!(
        simultaneous > 20,
        "exercise independent C reads and row writes"
    );
    result
}

#[test]
fn independent_unwrapped_helpers_wraps_aliases_and_ce0_returns() {
    vectors();
}
fn packed(uv: [i64; 8]) -> String {
    (0..4)
        .rev()
        .map(|i| {
            format!(
                "{:09x}",
                (uv[i * 2] as u64 & 0x3ffff) | ((uv[i * 2 + 1] as u64 & 0x3ffff) << 18)
            )
        })
        .collect()
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
            panic!("UV rows wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "requires Icarus and Gowin native simulation library"]
fn row_ingress_portable_and_native_rtl_match_actual_edges() {
    let trace = vectors();
    for native in [false, true] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/pixel-composition-20261010/uv-rows-{native}"
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("uv_rows.v"), rtl::uv_rows::source(native)).unwrap();
        let mut tb=String::from("module tb;reg clk=0,reset=1,ce=0;reg[2:0]phase=0,in_slot=0,c_slot=0;reg[1:0]c_lane=0;reg in_valid=0,d_take=0,c_request=0,c_take=0;reg[35:0]in_uv=0,in_metadata=0;wire in_ready,d_valid,c_empty,c_valid,write_enable,read_enable,raw_write,committed;wire[1:0]in_row;wire[2:0]d_slot,c_owner_slot,raw_slot,committed_slot;wire[1:0]c_owner_lane;wire[143:0]d_uv;wire[31:0]c_uv;wire[4:0]write_address,read_address;wire[35:0]write_data,raw_metadata;gpu_v2_uv_rows dut(.*);reg gs=1;GSR global_reset(.GSRI(gs));initial begin #100000;$fatal(1,\"watchdog\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n");
        for (i, v) in trace.iter().enumerate() {
            let r = v.t.row.unwrap_or(uv_rows::Row {
                slot: 0,
                metadata: 0,
                uv: [0; 2],
            });
            let owner = v.t.coordinate.unwrap_or(Coordinate { slot: 0, lane: 0 });
            let word = (r.uv[0] as u64 & 0x3ffff) | ((r.uv[1] as u64 & 0x3ffff) << 18);
            writeln!(tb,"ce={};phase={};in_valid={};in_slot={};in_metadata=36'h{:x};in_uv=36'h{word:x};d_take={};c_request={};c_slot={};c_lane={};c_take={};#1;", u8::from(v.t.ce),v.t.phase,u8::from(v.t.row.is_some()),r.slot,r.metadata,u8::from(v.t.take_derivative),u8::from(v.t.coordinate.is_some()),owner.slot,owner.lane,u8::from(v.t.take_coordinate)).unwrap();
            writeln!(tb,"if(in_ready!=={} || in_row!=={} || d_valid!=={} || c_valid!=={} || c_empty!=={} || write_enable!=={} || read_enable!=={} || raw_write!=={} || committed!=={}) $fatal(1,\"control edge {i}\");",u8::from(v.ready),v.next,u8::from(v.d.is_some()),u8::from(v.c.is_some()),u8::from(v.empty),u8::from(v.edge.write.is_some()),u8::from(v.edge.read.is_some()),u8::from(v.edge.raw_write.is_some()),u8::from(v.edge.committed.is_some())).unwrap();
            if let Some((slot, uv)) = v.d {
                writeln!(
                    tb,
                    "if(d_slot!=={slot} || d_uv!==144'h{}) $fatal(1,\"D edge {i}\");",
                    packed(uv)
                )
                .unwrap();
            }
            if let Some((owner, uv)) = v.c {
                let word = u64::from(uv[0]) | (u64::from(uv[1]) << 16);
                writeln!(tb,"if(c_owner_slot!=={} || c_owner_lane!=={} || c_uv!==32'h{word:x}) $fatal(1,\"C edge {i}\");",owner.slot,owner.lane).unwrap();
            }
            if let Some((address, data)) = v.edge.write {
                writeln!(tb,"if(write_address!=={address} || write_data!==36'h{data:x}) $fatal(1,\"write edge {i}\");").unwrap();
            }
            if let Some(read) = v.edge.read {
                writeln!(
                    tb,
                    "if(read_address!=={}) $fatal(1,\"read edge {i}\");",
                    read.address
                )
                .unwrap();
            }
            if let Some((slot, data)) = v.edge.raw_write {
                writeln!(tb,"if(raw_slot!=={slot} || raw_metadata!==36'h{data:x}) $fatal(1,\"Raw edge {i}\");").unwrap();
            }
            if let Some(slot) = v.edge.committed {
                writeln!(
                    tb,
                    "if(committed_slot!=={slot}) $fatal(1,\"commit edge {i}\");"
                )
                .unwrap();
            }
            tb.push_str("clk=1;#1;clk=0;#1;\n");
        }
        tb.push_str("ce=1;phase=0;in_valid=1;in_slot=7;in_metadata=36'h4567;in_uv=36'h321;d_take=0;c_request=0;c_take=0;#1;\n");
        for next in 0..3 {
            writeln!(tb,"if(in_row!=={next} || d_valid!==0 || committed!==0) $fatal(1,\"partial reset begin {next}\");clk=1;#1;clk=0;#1;").unwrap();
        }
        tb.push_str("reset=1;in_valid=0;clk=1;#1;clk=0;reset=0;#1;if(in_ready!==1 || in_row!==0 || d_valid!==0 || c_valid!==0) $fatal(1,\"partial reset\");\n");
        writeln!(
            tb,
            "$display(\"PASS {} edges; 64 quads; 256 C returns on CE0; partial reset\");$finish;end endmodule",
            trace.len()
        )
        .unwrap();
        fs::write(dir.join("tb.v"), tb).unwrap();
        let mut compile =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        compile.current_dir(&dir).args([
            "-g2012",
            "-s",
            "tb",
            "-o",
            "test.vvp",
            "uv_rows.v",
            "tb.v",
        ]);
        let home = std::env::var_os("GOWIN_HOME").expect("native simulation requires GOWIN_HOME");
        compile.arg(Path::new(&home).join("IDE/simlib/gw2a/prim_sim.v"));
        bounded(compile, &dir, "compile");
        let mut run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        run.current_dir(&dir).arg("test.vvp");
        bounded(run, &dir, "run");
    }
}
