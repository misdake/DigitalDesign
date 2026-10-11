//! Canonical24 storage against frozen owner38 storage and actual selected LOD.
use gpu_v2::texture::{
    emu::{
        coefficient, compact, coordinate,
        coordinate_store::{self, canonical, Owner},
        derivative::{self, DerivativeEmu},
        lod::LodEmu,
    },
    rtl::coordinate_store as rtl,
    sim::staged::bound::serial,
};
#[path = "fixtures/coordinate_store_wide_reference.rs"]
#[allow(dead_code)]
mod reference;
use std::{
    fmt::Write,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const BOUND: usize = 8000;
fn owner(i: usize) -> Owner {
    let p1 = (i % 512) as u16;
    let n = ((i / 47) % 11) as u8;
    Owner {
        parents: [511 - p1, p1],
        nearest: i & 1 != 0,
        levels: [n, n.saturating_sub(1)],
        slot: ((i / 7) % 16) as u8,
        key: ((i / 3) % 64) as u8,
        last_fine: p1 == 0,
    }
}
#[test]
fn codec_all_parents_levels_slots_keys_and_rejected_domains() {
    let mut cases = 0;
    for n in 0..=10 {
        for p1 in 0..=511 {
            for slot in 0..16 {
                for key in 0..64 {
                    for nearest in [false, true] {
                        let o = Owner {
                            parents: [511 - p1, p1],
                            nearest,
                            levels: [n, n.saturating_sub(1)],
                            slot,
                            key,
                            last_fine: p1 == 0,
                        };
                        let bits = canonical::pack(o).unwrap();
                        assert!(bits < 1 << 24);
                        assert_eq!(canonical::unpack(bits), o);
                        cases += 1;
                    }
                }
            }
        }
    }
    let good = owner(77);
    for bad in [
        Owner {
            parents: [0, 77],
            ..good
        },
        Owner {
            levels: [11, 10],
            ..good
        },
        Owner {
            levels: [good.levels[0], 1],
            ..good
        },
        Owner {
            last_fine: true,
            ..good
        },
        Owner { slot: 16, ..good },
        Owner { key: 64, ..good },
    ] {
        assert!(canonical::pack(bad).is_err());
    }
    println!("PASS canonical codec {cases} complete parent/level/slot/key/nearest cases");
}
#[test]
fn actual_d_lod_supplies_canonical_contract() {
    let (dc, lc, _) = serial::structural_calendars().unwrap();
    let lc = lc.with_issue_offset(0).unwrap();
    let mut levels = 0u16;
    let mut cases = 0;
    for level in 0..=10 {
        for fraction in 0..256 {
            for filter in 0..3 {
                for force in [false, true] {
                    for has_mip in [false, true] {
                        let slope = 1i64 << (16 - level);
                        let mut d = DerivativeEmu::new(dc.clone()).unwrap();
                        d.tick(
                            true,
                            Some(derivative::Input {
                                uv: [0, 0, slope, 0, 0, 0, slope, 0],
                                bias: fraction as i16,
                                force_coarsest: force,
                                header: derivative::Header {
                                    quad: (fraction % 16) as u8,
                                    mask: 15,
                                    slot: (fraction / 16) as u8,
                                    max_n: 10,
                                    has_mip,
                                    filter,
                                },
                            }),
                        )
                        .unwrap();
                        for _ in 0..2 {
                            d.tick(true, None).unwrap();
                        }
                        let derived = d.output().unwrap().unwrap();
                        let mut l = LodEmu::new(lc.clone()).unwrap();
                        l.tick(true, Some(derived.into())).unwrap();
                        for _ in 0..3 {
                            l.tick(true, None).unwrap();
                        }
                        let result = l.output().unwrap().unwrap();
                        levels |= 1 << result.context.levels[0];
                        for lane in 0..4 {
                            let o = Owner {
                                parents: result.context.parents,
                                nearest: result.context.nearest,
                                levels: result.context.levels,
                                slot: result.slot,
                                key: result.quad * 4 + lane,
                                last_fine: result.context.last_fine,
                            };
                            assert_eq!(canonical::unpack(canonical::pack(o).unwrap()), o);
                        }
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(levels, 2047);
    println!("PASS actual D/LOD {cases} filter/force/mip/bias fixtures; all 11 output levels");
}
struct Vector {
    t: coordinate_store::Tick,
    allow: bool,
    bad: bool,
    before: coordinate_store::State,
    after: coordinate_store::State,
    head: Option<coefficient::Input>,
}
fn vectors() -> Vec<Vector> {
    let mut narrow = coordinate_store::Store::new_canonical(6).unwrap();
    let mut wide = reference::Store::new_compact(6).unwrap();
    let mut rows = [owner(0); 6];
    let mut rng = 0x53175u32;
    let mut admitted = 0;
    let mut vectors = vec![];
    let mut full = 0;
    let mut denied = 0;
    let mut paused = 0;
    let mut faults = 0;
    let mut takes = 0;
    for wall in 0..BOUND {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let s = narrow.state();
        let mut t = coordinate_store::Tick {
            ce: wall % 9 != 0,
            reset: wall == 1700,
            ..Default::default()
        };
        let allow = wall % 11 != 0;
        if admitted < 1400 && (s.count < 6 && rng % 3 != 0 || s.count == 6 && wall % 13 == 0) {
            t.issue = Some(owner(admitted));
        }
        if s.count > s.ready && rng % 4 != 0 || s.count == s.ready && wall % 19 == 0 {
            let n = rows[s.returned].levels[0];
            let mask = compact::side_mask(n);
            let raw = u64::from((rng & 255) as u8)
                | (u64::from(((rng >> 8) & 255) as u8) << 8)
                | (u64::from((rng as u16) & mask) << 16)
                | (u64::from(((rng >> 13) as u16) & mask) << 26);
            t.returned = Some(compact::expand_result(raw, n));
        }
        if s.ready != 0 && (wall > 40 && rng % 5 != 0) || s.ready == 0 && wall % 17 == 0 {
            t.take = true;
        }
        let bad = !t.reset
            && t.ce
            && (t.issue.is_some() && s.count == 6
                || t.returned.is_some() && s.count == s.ready
                || t.take && s.ready == 0);
        let head = narrow.input();
        assert_eq!(format!("{head:?}"), format!("{:?}", wide.input()));
        let actual = coordinate_store::Tick {
            ce: t.ce && allow && !bad,
            ..t
        };
        let old = reference::Tick {
            reset: actual.reset,
            ce: actual.ce,
            issue: actual.issue.map(|o| reference::Owner {
                parents: o.parents,
                nearest: o.nearest,
                levels: o.levels,
                slot: o.slot,
                key: o.key,
                last_fine: o.last_fine,
            }),
            returned: actual.returned,
            take: actual.take,
        };
        if bad {
            assert!(narrow.tick(t).is_err());
            assert!(wide.tick(reference::Tick { ce: t.ce, ..old }).is_err());
            assert_eq!(narrow.state(), s);
        }
        let a = narrow.tick(actual).unwrap();
        let b = wide.tick(old).unwrap();
        assert_eq!(a.result_write, b.result_write);
        assert_eq!(
            a.owner_write
                .map(|(r, w)| (r, canonical::unpack(w as u32).pack().unwrap())),
            b.owner_write
        );
        assert_eq!(
            a.consume
                .map(|(r, w, d)| (r, canonical::unpack(w as u32).pack().unwrap(), d)),
            b.consume
        );
        let after = narrow.state();
        let z = wide.state();
        assert_eq!(
            (
                after.write,
                after.returned,
                after.read,
                after.count,
                after.ready
            ),
            (z.write, z.returned, z.read, z.count, z.ready)
        );
        if let Some((r, _)) = a.owner_write {
            rows[r] = t.issue.unwrap();
            admitted += 1;
        }
        takes += usize::from(a.consume.is_some());
        full += usize::from(s.count == 6);
        denied += usize::from(!allow);
        paused += usize::from(!t.ce);
        faults += usize::from(bad);
        vectors.push(Vector {
            t,
            allow,
            bad,
            before: s,
            after,
            head,
        });
        if admitted == 1400 && after.count == 0 {
            break;
        }
    }
    assert_eq!(admitted, 1400);
    assert_eq!(narrow.state().count, 0);
    assert!(full > 0 && denied > 0 && paused > 0 && faults > 0 && takes > 1000);
    println!("PASS canonical C6 {} edges,{admitted} issues,{takes} consumes,{full} full,{denied} shared-denied,{paused} CE0,{faults} raw rejections",vectors.len());
    vectors
}
#[test]
fn selected_model_matches_frozen_wide_old_state() {
    vectors();
    let mut s = coordinate_store::Store::new_canonical(6).unwrap();
    let before = s.state();
    let bad = Owner {
        parents: [0, 1],
        ..owner(1)
    };
    assert!(s
        .tick(coordinate_store::Tick {
            ce: true,
            issue: Some(bad),
            ..Default::default()
        })
        .is_err());
    assert_eq!(s.state(), before);
}
fn bounded(mut c: Command, dir: &Path, name: &str) {
    let mut child = c
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
        if let Some(s) = child.try_wait().unwrap() {
            assert!(
                s.success(),
                "{} {}",
                fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("canonical C6 wall bound");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "requires Icarus and Gowin native simulation library"]
fn native_and_portable_checked_and_trusted_shared_replay() {
    let trace = vectors();
    for native in [false, true] {
        for checked in [false, true] {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../target/c-owner-canonical-20261010/native-{native}-checked-{checked}"
            ));
            fs::create_dir_all(&dir).unwrap();
            let source = if checked {
                rtl::canonical_source(6, native).unwrap()
            } else {
                rtl::canonical_source_trusted(6, native).unwrap()
            };
            let source = source
                .replace(
                    "input wire clk,reset,ce,",
                    "input wire clk,reset,proposal_ce,commit_allow,",
                )
                .replace(
                    "localparam A=",
                    "wire ce=proposal_ce&&commit_allow;\n localparam A=",
                )
                .replace(
                    "assign bad=!reset && ce &&",
                    "assign bad=!reset && proposal_ce &&",
                );
            fs::write(dir.join("queue.v"), source).unwrap();
            let mut tb=String::from("module tb;reg clk=0,reset=1,proposal_ce=0,commit_allow=0,issue=0,returned=0,take=0;reg[37:0]owner_data=0;reg[35:0]result_data=0;wire in_ready,out_valid,bad,owner_write,result_write,consume;wire[37:0]out_owner;wire[35:0]out_result;wire[3:0]write_row,return_row,read_row;wire[4:0]occupied,completed;gpu_v2_coordinate_store dut(.*);reg gs=1;GSR global_reset(.GSRI(gs));initial begin #100000;$fatal(1,\"bound\");end initial begin #1;clk=1;#1;clk=0;reset=0;#1;\n");
            for (i, v) in trace.iter().enumerate() {
                let t = v.t;
                let fire = t.ce && v.allow && !v.bad && !t.reset;
                writeln!(tb,"reset={};proposal_ce={};commit_allow={};issue={};returned={};take={};owner_data=38'h{:x};result_data=36'h{:x};#1;",u8::from(t.reset),u8::from(t.ce),u8::from(v.allow),u8::from(t.issue.is_some()),u8::from(t.returned.is_some()),u8::from(t.take),t.issue.unwrap_or(owner(0)).pack().unwrap(),t.returned.map_or(0,compact::pack_result)).unwrap();
                writeln!(tb,"if(in_ready!=={} || out_valid!=={} || bad!=={} || owner_write!=={} || result_write!=={} || consume!=={})$fatal(1,\"pre {i}\");",u8::from(!t.reset&&v.before.count<6),u8::from(!t.reset&&v.before.ready!=0),u8::from(v.bad),u8::from(fire&&t.issue.is_some()),u8::from(fire&&t.returned.is_some()),u8::from(fire&&t.take)).unwrap();
                if !t.reset {
                    if let Some(h) = v.head {
                        let o = Owner {
                            parents: h.parents,
                            nearest: h.nearest,
                            levels: h.metadata.levels,
                            slot: h.metadata.slot,
                            key: h.metadata.key,
                            last_fine: h.metadata.last_fine,
                        };
                        let result = coordinate::Output {
                            fractions: h.fractions,
                            coordinates: h.metadata.coordinates,
                        };
                        writeln!(tb,"if(out_owner!==38'h{:x} || out_result!==36'h{:x})$fatal(1,\"head {i}\");",o.pack().unwrap(),compact::pack_result(result)).unwrap();
                    }
                }
                writeln!(tb,"clk=1;#1;clk=0;#1;if(occupied!=={} || completed!=={} || write_row!=={} || return_row!=={} || read_row!=={})$fatal(1,\"state {i}\");",v.after.count,v.after.ready,v.after.write,v.after.returned,v.after.read).unwrap();
            }
            if checked {
                tb.push_str("reset=1;clk=1;#1;clk=0;reset=0;proposal_ce=1;commit_allow=1;issue=1;returned=0;take=0;owner_data=38'h1;#1;if(bad!==1 || owner_write!==0)$fatal(1,\"domain\");clk=1;#1;clk=0;#1;if(occupied!==0)$fatal(1,\"domain atomicity\");\n");
            }
            writeln!(tb,"$display(\"PASS {} edges native={native} checked={checked}\");$finish;end endmodule",trace.len()).unwrap();
            fs::write(dir.join("tb.v"), tb).unwrap();
            let mut c =
                Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
            c.current_dir(&dir)
                .args(["-g2012", "-s", "tb", "-o", "test.vvp", "queue.v", "tb.v"]);
            let home = std::env::var_os("GOWIN_HOME").unwrap();
            c.arg(Path::new(&home).join("IDE/simlib/gw2a/prim_sim.v"));
            bounded(c, &dir, "compile");
            let mut c = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
            c.current_dir(&dir).arg("test.vvp");
            bounded(c, &dir, "run");
        }
    }
}
