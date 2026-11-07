//! Independent semantic goldens for the actual coordinate register calendar.
//!
//! The golden below is derived only from the frozen coordinate contract
//! (Q8 centered floor, signed repeat wrap, nearest/halve selection); it never
//! calls the counted `coordinate_values` body or reads a closed frame. The live
//! `CoordinateEmu` is driven with hand input bits, and a Runtime-level test
//! checks overlapping contexts, repeated quad IDs, CE and oracle equality.
#[allow(dead_code)]
use super::shared_support_sdram_physical_texture as physical;
#[allow(dead_code)]
use super::shared_support_texture as support;

use gpu_v2::texture::{
    emu::{
        color::Output,
        coordinate::{
            self, CoordinateEmu, Input as CoordInput, Output as CoordOutput, SharedCoordinateEmu,
            SPAN,
        },
        derivative::{Calendar, Field},
    },
    ports::*,
    sim::{
        oracle,
        staged::{binding, bound},
        timed,
    },
};
use std::collections::VecDeque;

const OUT: &[&str] = coordinate::OUTPUTS;

fn build_calendar(poison: bool) -> Calendar {
    let mut q = QuadInput {
        force_coarsest: false,
        quad_id: 0,
        mask: 1,
        uv: [[0.003, 0.003]; 4],
        slot: 0,
        material_size_log2: 9,
        filter: Filter::Trilinear,
        lod_bias: 0.5,
    };
    q.uv[1][0] += 1.0 / 512.0;
    let p = bound::prepare(
        &q,
        &[Slot {
            base_address: 4096,
            max_size_log2: 9,
            has_full_mip: true,
            valid: true,
        }],
    )
    .unwrap();
    let b = bound::Binding::build().unwrap();
    let mut frame = p.lanes[0].coordinate.frame.clone();
    let plan = &b.coordinate;
    let evidence = binding::Evidence::build(&frame).unwrap();
    let cones = plan.logic_cones();
    if poison {
        for value in &mut frame.values {
            if frame.events[value.producer].operation != audited::Operation::Literal {
                value.raw ^= 4095;
            }
        }
        for value in &mut frame.outputs {
            value.raw ^= 4095;
        }
    }
    let fields: Vec<Field> = plan
        .packed_fields
        .iter()
        .map(|f| Field {
            value: f.value,
            source_low: f.source_low,
            width: f.width,
            birth: f.birth as u8,
            last_read: f.last_read as u8,
            lows: (0..plan.packed.period / plan.ii())
                .map(|i| {
                    plan.packed
                        .placements
                        .iter()
                        .find(|pl| {
                            pl.value == f.value
                                && pl.source_low == f.source_low
                                && pl.iteration == i
                        })
                        .unwrap()
                        .low
                })
                .collect(),
        })
        .collect();
    Calendar::from_structure(
        &frame,
        &plan.times,
        cones,
        &evidence.lowering.wiring_adds,
        fields,
        plan.packed.ff_bits,
        plan.span() as u8,
        plan.packed.period as u32,
        plan.ii() as u32,
        OUT,
    )
    .unwrap()
}

/// The frozen coordinate contract, evaluated with plain host integers:
/// `fine[i] = (uv[i] << shift) - (nearest ? 0 : 128)`, coarse
/// `(fine-128)>>1` when halve; `integer=fine>>8`, `fraction=fine & 0xff`, with
/// the single-boundary repeat wrap `a = i<0 ? side-1 : i`, `b = i+1==side ? 0 : i+1`.
fn golden(inp: CoordInput) -> CoordOutput {
    let mut fractions = [[0u8; 2]; 2];
    let mut coordinates = [[0u16; 4]; 2];
    for axis in 0..2 {
        let uv = inp.uv[axis] as i64;
        let fine = if inp.shift >= 0 {
            uv << inp.shift
        } else {
            let _ = uv;
            uv >> (-inp.shift)
        };
        let q0 = fine - if inp.nearest { 0 } else { 128 };
        let q1 = if inp.halve { (q0 - 128) >> 1 } else { q0 };
        for (which, q) in [q0, q1].into_iter().enumerate() {
            fractions[which][axis] = (q & 0xff) as u8;
            let integer = q >> 8;
            let side = i64::from(inp.side[which]);
            let a = if integer < 0 { side - 1 } else { integer };
            let next = integer + 1;
            let b = if next == side { 0 } else { next };
            coordinates[which][axis * 2] = a as u16;
            coordinates[which][axis * 2 + 1] = b as u16;
        }
    }
    CoordOutput {
        fractions,
        coordinates,
    }
}

#[test]
fn shared_axis_all_codes_preserve_joint_publication_and_captured_v() {
    assert_eq!(
        coordinate::SHARED_NUMERIC_BITS,
        gpu_v2::texture::rtl::coordinate::NUMERIC_BITS
    );
    assert_eq!(
        coordinate::CONTROL_BITS,
        gpu_v2::texture::rtl::coordinate::CONTROL_BITS
    );
    let mut shared = SharedCoordinateEmu::new(build_calendar(true)).unwrap();
    let mut checks = 0;
    for n in 1..=10 {
        let side = 1_i16 << n;
        for nearest in [false, true] {
            for uv in 0..65536 {
                let input = CoordInput {
                    uv: [uv, 65535 - uv],
                    shift: n - 8,
                    nearest,
                    halve: n > 1,
                    side: [side, (side / 2).max(2)],
                };
                let u = shared.tick(true, Some(input)).unwrap();
                assert_eq!(u.axis.unwrap().0, 0);
                assert!(u.accepted && shared.output().unwrap().is_none());
                if uv % 8192 == 0 {
                    let before = (shared.bank(), shared.phase(), shared.output().unwrap());
                    // Poison all input fields while CE is low. The second axis
                    // must later use the captured V/configuration, not these.
                    let poison = CoordInput {
                        uv: [u32::MAX; 2],
                        shift: i32::MIN,
                        side: [-1; 2],
                        nearest: !nearest,
                        halve: n == 1,
                    };
                    assert!(!shared.tick(false, Some(poison)).unwrap().accepted);
                    assert_eq!(
                        (shared.bank(), shared.phase(), shared.output().unwrap()),
                        before
                    );
                }
                let v = shared.tick(true, None).unwrap();
                assert_eq!(v.axis.unwrap().0, 1);
                assert!(!v.accepted);
                assert_eq!(shared.output().unwrap(), Some(golden(input)));
                checks += 1;
            }
        }
    }
    assert_eq!(checks, 1_310_720);
}

#[test]
fn shared_axis_stream_matches_generic_calendar_and_rejects_atomically() {
    let calendar = build_calendar(false);
    let mut reference = CoordinateEmu::new(calendar.clone()).unwrap();
    let mut shared = SharedCoordinateEmu::new(build_calendar(true)).unwrap();
    let inputs = vectors();
    let mut accepted = 0;
    let mut retired = 0;
    let mut pauses = [0; 3];
    let mut pending_resets = 0;
    for wall in 0..2000 {
        assert_eq!(shared.phase(), reference.phase());
        assert_eq!(shared.idle(), reference.idle());
        assert_eq!(shared.output().unwrap(), reference.output().unwrap());
        // Five advancing edges per block alternate the paused phase.
        let ce = wall != 0 && wall % 6 != 2;
        if ce && shared.output().unwrap().is_some() {
            retired += 1;
        }
        if !ce {
            pauses[if shared.output().unwrap().is_some() {
                2
            } else {
                shared.phase() as usize
            }] += 1;
        }
        let input = (shared.phase() == 0)
            .then(|| inputs.get(accepted).copied())
            .flatten();
        let a = shared.tick(ce, input).unwrap();
        let b = reference.tick(ce, input).unwrap();
        assert_eq!(a.accepted, b.accepted);
        if a.accepted {
            accepted += 1;
            let snapshot = (shared.bank(), shared.phase(), shared.output().unwrap());
            assert!(shared.tick(true, input).is_err());
            assert_eq!(
                (shared.bank(), shared.phase(), shared.output().unwrap()),
                snapshot
            );
            if pending_resets == 0 {
                shared.reset();
                reference = CoordinateEmu::new(calendar.clone()).unwrap();
                pending_resets += 1;
            }
        }
        if accepted == inputs.len() && shared.idle() {
            break;
        }
    }
    assert!(shared.idle() && accepted == inputs.len());
    assert_eq!(retired, inputs.len() - 1);
    assert!(pauses.iter().all(|v| *v > 0), "pause coverage={pauses:?}");
    assert_eq!(pending_resets, 1);
    let snapshot = (shared.bank(), shared.phase(), shared.output().unwrap());
    let bad = CoordInput {
        uv: [65536, 0],
        ..inputs[0]
    };
    assert!(shared.tick(true, Some(bad)).is_err());
    assert_eq!(
        (shared.bank(), shared.phase(), shared.output().unwrap()),
        snapshot
    );
}

fn vectors() -> Vec<CoordInput> {
    let mut v = vec![];
    for physical in 1..=10 {
        let shift = physical - 8;
        let side = 1i16 << physical;
        for nearest in [false, true] {
            let halve = physical > 1;
            {
                let base = [
                    CoordInput {
                        uv: [0, 0],
                        shift,
                        nearest,
                        halve,
                        side: [side, (side / 2).max(2)],
                    },
                    CoordInput {
                        // last wrapped index and the centering boundary
                        uv: [(1u32 << 16) - 1, 256u32.wrapping_sub(1)],
                        shift,
                        nearest,
                        halve,
                        side: [side, (side / 2).max(2)],
                    },
                    CoordInput {
                        // negative-going boundary: fine-128 crosses zero
                        uv: [128, (1u32 << 16) - 128],
                        shift,
                        nearest,
                        halve,
                        side: [side, (side / 2).max(2)],
                    },
                ];
                v.extend(base);
            }
        }
    }
    // extreme unwrapped helper UV across the full Q16 range
    v.push(CoordInput {
        uv: [1 << 15, (1 << 16) - 1],
        shift: -7,
        nearest: false,
        halve: false,
        side: [2, 2],
    });
    v
}

#[test]
fn actual_coordinate_registers_match_independent_semantic_goldens() {
    let mut emu = CoordinateEmu::new(build_calendar(false)).unwrap();
    let vs = vectors();
    let total = vs.len();
    let mut pending = vs.into_iter();
    let mut expected: VecDeque<(u64, CoordOutput)> = VecDeque::new();
    let mut got = 0usize;
    for edge in 0..(total as u64 * 4 + 64) {
        if let Some(out) = emu.output().unwrap() {
            let (due, want) = expected.pop_front().expect("unexpected coordinate output");
            assert_eq!(due, edge, "coordinate publishes on its certified age");
            assert_eq!(out, want, "coordinate output at edge {edge}");
            got += 1;
        }
        let inp = if edge % 2 == 0 { pending.next() } else { None };
        if let Some(i) = inp {
            expected.push_back((edge + u64::from(SPAN), golden(i)));
        }
        emu.tick(true, inp).unwrap();
        if expected.is_empty() && emu.idle() {
            break;
        }
    }
    assert_eq!(got, total);
    assert!(emu.idle());
}

#[test]
fn coordinate_admission_requires_a_real_physical_mip_shape() {
    let calendar = build_calendar(false);
    let valid = CoordInput {
        uv: [0, (1 << 16) - 1],
        shift: -3,
        nearest: false,
        halve: true,
        side: [32, 16],
    };
    let invalid = [
        CoordInput {
            uv: [1 << 16, 0],
            ..valid
        },
        CoordInput {
            side: [0, 0],
            ..valid
        },
        CoordInput {
            side: [1, 1],
            ..valid
        },
        CoordInput {
            side: [30, 15],
            ..valid
        },
        CoordInput {
            side: [2048, 1024],
            ..valid
        },
        CoordInput { shift: -2, ..valid },
        CoordInput {
            shift: i32::MIN,
            ..valid
        },
        CoordInput {
            side: [32, 32],
            ..valid
        },
        CoordInput {
            halve: false,
            ..valid
        },
    ];
    for input in invalid {
        let mut emu = CoordinateEmu::new(calendar.clone()).unwrap();
        let before = emu.bank().to_vec();
        assert!(!emu.tick(false, Some(input)).unwrap().accepted);
        assert_eq!(emu.bank(), before);
        assert_eq!(emu.phase(), 0);
        assert_eq!(
            emu.tick(true, Some(input)).unwrap_err(),
            "coordinate input range"
        );
        assert!(emu.idle());
        assert!(emu.tick(true, Some(valid)).unwrap().accepted);
    }
    let period = calendar.period() as usize;
    let mut emu = CoordinateEmu::new(calendar).unwrap();
    for enabled in 0..64 {
        assert_eq!(usize::from(emu.phase()), enabled % period);
        emu.tick(true, None).unwrap();
    }
}

#[test]
fn ce_pause_freezes_coordinate_bank_phase_and_output() {
    let mut emu = CoordinateEmu::new(build_calendar(false)).unwrap();
    let inp = vectors()[0];
    let want = golden(inp);
    // Admit, then freeze for a long CE-low window.
    emu.tick(true, Some(inp)).unwrap();
    let bank = emu.bank().to_vec();
    let phase = emu.phase();
    for _ in 0..32 {
        emu.tick(false, Some(inp)).unwrap();
    }
    assert_eq!(emu.bank(), bank.as_slice());
    assert_eq!(emu.phase(), phase);
    assert_eq!(emu.output().unwrap(), None);
    // Resume and observe exactly the golden result.
    let mut seen = None;
    for edge in 0..64u64 {
        if let Some(out) = emu.output().unwrap() {
            seen = Some((edge, out));
            break;
        }
        emu.tick(true, None).unwrap();
    }
    let (_edge, out) = seen.expect("resumed coordinate output");
    assert_eq!(out, want);
}

#[test]
fn poisoned_structure_calendar_is_built_from_wiring_not_sampled_answers() {
    let mut clean = CoordinateEmu::new(build_calendar(false)).unwrap();
    let mut poisoned = CoordinateEmu::new(build_calendar(true)).unwrap();
    let vs = vectors();
    let total = vs.len();
    let mut pending = vs.into_iter();
    let mut matched = 0usize;
    for edge in 0..(total as u64 * 4 + 64) {
        assert_eq!(
            clean.output().unwrap(),
            poisoned.output().unwrap(),
            "poisoned structure must reproduce actual wiring at edge {edge}"
        );
        if let Some(out) = clean.output().unwrap() {
            assert_eq!(out, golden(vectors()[matched]), "poisoned golden {matched}");
            matched += 1;
        }
        let inp = if edge % 2 == 0 { pending.next() } else { None };
        clean.tick(true, inp).unwrap();
        poisoned.tick(true, inp).unwrap();
        if matched == total && clean.idle() {
            break;
        }
    }
    assert_eq!(matched, total);
}

#[test]
fn runtime_overlapping_contexts_and_repeated_quad_ids_match_oracle() {
    let a = support::slot(5, true);
    let mut bytes = support::asset(a, support::pattern);
    let b = Slot {
        base_address: support::BASE + bytes.len() as u32,
        max_size_log2: 4,
        has_full_mip: false,
        valid: true,
    };
    bytes.extend(support::asset(b, |n, x, y| {
        support::pattern(n, y, x) ^ 0x8410
    }));
    let slots = vec![a, b];
    let mut r = bound::runtime::Runtime::new(
        &slots,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        timed::Hardware {
            prefetch: false,
            max_quads: 1,
            ..Default::default()
        },
        40_000,
    )
    .unwrap();
    let mut memory = physical::Physical::new(u64::from(support::BASE), bytes.clone(), true, true);
    let mk = |id: u8, x: f64| {
        let mut q = support::input(5, Filter::Trilinear, [x, 0.07]);
        q.quad_id = id;
        q
    };
    let qs = [mk(0, 0.13), mk(1, 0.41), mk(2, 0.67), mk(3, 0.91)];
    // The single raw/D head serializes admission. Keep all four public owners
    // live before draining results; acceptance must obey its actual old ready.
    let mut admitted = 0;
    for _ in 0..1000 {
        let step = r
            .step(
                &mut memory,
                qs.get(admitted),
                timed::Control {
                    ce: true,
                    result_ready: false,
                },
            )
            .unwrap();
        assert!(step.results.is_empty());
        admitted += usize::from(step.accepted);
        if admitted == qs.len() {
            break;
        }
    }
    assert_eq!(admitted, qs.len());
    assert!(r.stats.peak_live_quads >= 4);
    let mut got = vec![];
    for _ in 0..40_000 {
        let st = r
            .step(
                &mut memory,
                None,
                timed::Control {
                    ce: true,
                    result_ready: true,
                },
            )
            .unwrap();
        got.extend(st.results);
        if r.idle() {
            break;
        }
    }
    assert!(r.idle());
    let mut want = expected(&qs, &slots, &bytes);
    // Reuse quad id 0 with different content after actual consumption.
    let reuse = mk(0, 0.29);
    assert!(
        r.step(&mut memory, Some(&reuse), timed::Control::default())
            .unwrap()
            .accepted
    );
    for _ in 0..40_000 {
        let st = r
            .step(
                &mut memory,
                None,
                timed::Control {
                    ce: true,
                    result_ready: true,
                },
            )
            .unwrap();
        got.extend(st.results);
        if r.idle() {
            break;
        }
    }
    want.extend(expected(&[reuse], &slots, &bytes));
    assert_eq!(got, want);
}

fn expected(inputs: &[QuadInput], slots: &[Slot], bytes: &[u8]) -> Vec<Output> {
    let mut cache = oracle::Cache::new(slots.to_vec()).unwrap();
    let mut image = support::Image {
        bytes: bytes.to_vec(),
        requests: vec![],
    };
    inputs
        .iter()
        .flat_map(|q| {
            oracle::sample(q, &mut cache, &mut image, Config::counted())
                .unwrap()
                .pixels
                .into_iter()
                .map(|p| Output {
                    key: q.quad_id * 4 + p.lane,
                    rgb: p.rgb,
                })
        })
        .collect()
}
