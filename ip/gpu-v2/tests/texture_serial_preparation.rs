//! Independent registered preparation compared with closed packet goldens.
//! Expected frames are generated before ticking, never retained by the DUT.
use gpu_v2::texture::{
    emu::derivative,
    ports::*,
    sim::staged::bound::{self, serial::*},
};
#[allow(dead_code)]
#[path = "support/texture.rs"]
mod support;

#[test]
fn complete_serial_preparation_packets_ce_backpressure_and_reuse() {
    const LIMIT: u64 = 100_000;
    let mut emu = PreparationEmu::new(LIMIT).unwrap();
    let mut cases = Vec::new();
    for n in [0, 1, 3, 6, 10] {
        for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
            let mut q = support::input(n, filter, [-0.004, 0.999]);
            q.quad_id = cases.len() as u8;
            q.mask = [1, 5, 10, 15][cases.len() % 4];
            q.lod_bias = [-2.0, 0.5, 3.25][cases.len() % 3];
            q.uv[1][0] += 1.0 / (1u32 << n.max(1)) as f64;
            q.uv[2][1] += 1.0 / (1u32 << n.max(1)) as f64;
            let slot = support::slot(n, true);
            let expected = bound::prepare(&q, &[slot]).unwrap().payloads;
            cases.push((derivative::Input::capture(&q, slot).unwrap(), expected));
        }
    }
    let mut offered = 0;
    let mut received = Vec::new();
    let expected: Vec<_> = cases.iter().flat_map(|c| c.1.iter().copied()).collect();
    let mut held = None;
    let mut accepted = 0;
    for wall in 0..LIMIT {
        let ce = wall % 11 != 3 && wall % 17 != 4;
        let ready = wall % 13 != 7 && wall % 13 != 8;
        let step = emu
            .tick(Tick {
                ce,
                input: cases.get(offered).map(|c| c.0),
                output_ready: ready,
            })
            .unwrap();
        if let Some(word) = held {
            assert_eq!(step.output, Some(word), "held packet changed");
        }
        if step.accepted {
            assert!(ce);
            offered += 1;
            accepted += 1;
        }
        if step.transferred {
            received.push(step.output.unwrap());
            held = None;
        } else {
            held = step.output;
        }
        if offered == cases.len() && emu.idle() {
            break;
        }
        assert!(wall + 1 < LIMIT, "serial preparation watchdog");
    }
    assert_eq!(accepted, cases.len());
    assert_eq!(received, expected);
    assert!(!emu.faulted());
    let mut zero = cases[0].0;
    zero.header.mask = 0;
    for _ in 0..16 {
        let s = emu
            .tick(Tick {
                ce: true,
                input: Some(zero),
                output_ready: true,
            })
            .unwrap();
        if s.accepted {
            assert!(emu.idle());
            return;
        }
    }
    panic!("zero-mask initiation admission");
}

#[test]
fn serial_short_alignment_removes_no_arithmetic_and_preserves_all_packets() {
    const LIMIT: u64 = 30_000;
    fn run(i: derivative::Input, cfg: bound::serial::Config) -> (Vec<i128>, u64) {
        let mut e = PreparationEmu::with_config(LIMIT, cfg).unwrap();
        let mut sent = false;
        let mut out = Vec::new();
        for edge in 0..LIMIT {
            let s = e
                .tick(Tick {
                    ce: true,
                    input: (!sent).then_some(i),
                    output_ready: true,
                })
                .unwrap();
            sent |= s.accepted;
            if s.transferred {
                out.push(s.output.unwrap());
            }
            if sent && e.idle() {
                return (out, edge + 1);
            }
        }
        panic!("short alignment wall watchdog");
    }
    for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
        let mut q = support::input(6, filter, [0.124, 0.124]);
        q.uv[1][0] += 1.0 / 64.0;
        q.uv[2][1] += 1.0 / 32.0;
        q.lod_bias = 0.5;
        let slot = support::slot(6, true);
        let i = derivative::Input::capture(&q, slot).unwrap();
        let expected = bound::prepare(&q, &[slot]).unwrap().payloads;
        for bypass in [false, true] {
            let baseline = run(
                i,
                bound::serial::Config {
                    nearest_bypass: bypass,
                    short_alignment: false,
                },
            );
            let shorter = run(
                i,
                bound::serial::Config {
                    nearest_bypass: bypass,
                    short_alignment: true,
                },
            );
            assert_eq!(baseline.0, expected);
            assert_eq!(shorter.0, expected);
            assert!(shorter.1 < baseline.1);
            println!(
                "{filter:?} nearest_bypass={bypass} aligned={} short={} saved={}",
                baseline.1,
                shorter.1,
                baseline.1 - shorter.1
            );
        }
    }
}

#[test]
fn serial_preparation_input_fault_and_bound_are_terminal() {
    let mut emu = PreparationEmu::new(32).unwrap();
    let q = support::input(4, Filter::Bilinear, [0.2, 0.3]);
    let mut bad = derivative::Input::capture(&q, support::slot(4, true)).unwrap();
    bad.uv[0] = 1_i64 << 39;
    assert!(emu
        .tick(Tick {
            ce: true,
            input: Some(bad),
            output_ready: true
        })
        .is_err());
    assert!(emu.faulted());
    assert!(emu
        .tick(Tick {
            ce: true,
            input: None,
            output_ready: true
        })
        .is_err());
    let mut emu = PreparationEmu::new(1).unwrap();
    emu.tick(Tick {
        ce: false,
        input: None,
        output_ready: false,
    })
    .unwrap();
    assert!(emu
        .tick(Tick {
            ce: false,
            input: None,
            output_ready: false
        })
        .is_err());
}

#[test]
fn nearest_literal_bypass_preserves_packets_and_removes_real_coefficient_edges() {
    const LIMIT: u64 = 20_000;
    fn run(input: derivative::Input, fast: bool) -> (Vec<i128>, u64) {
        let mut emu = PreparationEmu::with_config(
            LIMIT,
            bound::serial::Config {
                nearest_bypass: fast,
                ..Default::default()
            },
        )
        .unwrap();
        let mut admitted = false;
        let mut out = Vec::new();
        for edge in 0..LIMIT {
            let step = emu
                .tick(Tick {
                    ce: true,
                    input: (!admitted).then_some(input),
                    output_ready: true,
                })
                .unwrap();
            admitted |= step.accepted;
            if step.transferred {
                out.push(step.output.unwrap());
            }
            if admitted && emu.idle() {
                return (out, edge + 1);
            }
        }
        panic!("bounded preparation optimization");
    }
    for filter in [Filter::Nearest, Filter::Bilinear, Filter::Trilinear] {
        let mut q = support::input(6, filter, [-0.12, 0.91]);
        q.mask = 15;
        q.uv[1][0] += 0.03125;
        q.uv[2][1] += 0.0625;
        let slot = support::slot(6, true);
        let i = derivative::Input::capture(&q, slot).unwrap();
        let a = run(i, false);
        let b = run(i, true);
        assert_eq!(a.0, b.0);
        assert_eq!(a.0, bound::prepare(&q, &[slot]).unwrap().payloads);
        if filter == Filter::Nearest {
            assert!(a.1 >= b.1 + 48);
            println!(
                "nearest4 baseline={} bypass={} saved={}",
                a.1,
                b.1,
                a.1 - b.1
            );
        } else {
            assert_eq!(a.1, b.1);
        }
    }
}
