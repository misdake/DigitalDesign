//! Fixed-stage execution versus independent calendar registers and integer goldens.
use gpu_v2::texture::emu::coefficient::{
    fixed_stage::{FixedCoefficientEmu, ALLOCATION, NUMERIC_BITS},
    CoefficientEmu, Fault, Input, Metadata, Output, Tick,
};

fn input(index: usize, parent: u16, fractions: [[u8; 2]; 2], nearest: bool) -> Input {
    Input {
        parents: [parent, 511 - parent],
        fractions,
        nearest,
        metadata: Metadata {
            coordinates: std::array::from_fn(|p| {
                std::array::from_fn(|t| ((index * 37 + p * 171 + t * 257) & 1023) as u16)
            }),
            levels: [(index % 11) as u8, ((index + 3) % 11) as u8],
            slot: (index % 16) as u8,
            key: (index % 64) as u8,
            last_fine: parent == 511,
        },
    }
}
fn golden(v: Input) -> Output {
    Output {
        metadata: v.metadata,
        weights: std::array::from_fn(|p| {
            let [fu, fv] = if v.nearest { [0; 2] } else { v.fractions[p] }.map(u32::from);
            let b = u32::from(v.parents[p]) * fv / 256;
            let t = u32::from(v.parents[p]) - b;
            let br = b * fu / 256;
            let tr = t * fu / 256;
            [(t - tr) as u16, tr as u16, (b - br) as u16, br as u16]
        }),
    }
}
fn packed_weights(v: Output) -> [u64; 3] {
    let w = v
        .weights
        .into_iter()
        .flatten()
        .enumerate()
        .fold(0u128, |w, (i, v)| w | (u128::from(v) << (9 * i)));
    [w as u64, (w >> 64) as u64, 0]
}

#[test]
fn independent_grid_matches_calendar_and_integer_results_on_every_edge() {
    let mut inputs = Vec::new();
    for parent in 0..=511 {
        inputs.push(input(
            inputs.len(),
            parent,
            [[parent as u8, 255], [128, (parent >> 1) as u8]],
            false,
        ));
    }
    let edges = [0, 1, 127, 128, 129, 254, 255];
    for parent in [0, 1, 255, 256, 510, 511] {
        for fu in edges {
            for fv in edges {
                for cu in edges {
                    for cv in edges {
                        inputs.push(input(inputs.len(), parent, [[fu, fv], [cu, cv]], false));
                    }
                }
            }
        }
    }
    for poison in 0..=255 {
        inputs.push(input(
            inputs.len(),
            511,
            [[poison, 255 - poison], [255 - poison, poison]],
            true,
        ));
    }
    let mut selected = FixedCoefficientEmu::new(200_000).unwrap();
    let mut reference = CoefficientEmu::new(200_000).unwrap();
    let (mut sent, mut taken, mut written, mut frozen, mut same_pop) = (0, 0, 0, 0, 0);
    for wall in 0..200_000 {
        let tick = Tick {
            ce: wall % 17 != 3 && wall % 17 != 4,
            input: inputs.get(sent).copied(),
            output_ready: wall > 80 && wall % 11 > 2,
            work_available: match wall % 23 {
                0..=3 => 0,
                4 => 1,
                5 => 2,
                _ => 16,
            },
        };
        let before = selected.snapshot();
        let registers = selected.registers();
        let a = selected.tick(tick).unwrap();
        let b = reference.tick(tick).unwrap();
        assert_eq!(
            (
                a.input_ready,
                a.accepted,
                a.output,
                a.consumed,
                a.work_reserved
            ),
            (
                b.input_ready,
                b.accepted,
                b.output,
                b.consumed,
                b.work_reserved
            ),
            "wall {wall}"
        );
        assert_eq!(a.ready_access.write, b.ready_access.write);
        assert_eq!(a.ready_access.publish, b.ready_access.publish);
        assert_eq!(
            (a.metadata_access.write, a.metadata_access.read_address),
            (b.metadata_access.write, b.metadata_access.read_address)
        );
        assert_eq!(
            (
                a.snapshot.valid,
                a.snapshot.cohorts,
                a.snapshot.queued,
                a.snapshot.cohort_pointers,
                a.snapshot.ready_pointers
            ),
            (
                b.snapshot.valid,
                b.snapshot.cohorts,
                b.snapshot.queued,
                b.snapshot.cohort_pointers,
                b.snapshot.ready_pointers
            )
        );
        assert_eq!(
            a.snapshot.phase.trailing_zeros(),
            b.snapshot.phase.trailing_zeros() & 1
        );
        if let Some((_, raw)) = a.ready_access.write {
            assert_eq!(raw, packed_weights(golden(inputs[written])));
            written += 1;
        }
        if a.consumed {
            assert_eq!(a.output, Some(golden(inputs[taken])));
            taken += 1;
        }
        if !tick.ce || before.queued == 2 {
            frozen += 1;
            assert_eq!(selected.registers(), registers);
            assert_eq!(a.snapshot.product_registers, before.product_registers);
            assert_eq!(
                (a.snapshot.phase, a.snapshot.valid),
                (before.phase, before.valid)
            );
            if before.queued == 2 && a.consumed {
                same_pop += 1;
                assert!(!a.accepted);
            }
        }
        sent += usize::from(a.accepted);
        if sent == inputs.len() && selected.idle() {
            break;
        }
    }
    assert!(selected.idle() && reference.idle());
    assert_eq!(
        (sent, taken, written),
        (inputs.len(), inputs.len(), inputs.len())
    );
    assert!(frozen > 100 && same_pop > 0);
    assert_eq!(
        (
            NUMERIC_BITS,
            ALLOCATION.phase_valid_bits,
            ALLOCATION.soft_bits
        ),
        (121, 6, 286)
    );
    println!("FIXED inputs={sent} writes={written} consumed={taken} frozen={frozen} old_full_pop={same_pop} wall={}",selected.snapshot().wall);
}

#[test]
fn shared_row_reads_precede_fine_coarse_and_next_cohort_overwrites() {
    let a = input(0, 200, [[128, 192], [64, 128]], false);
    let b = input(1, 510, [[255, 1], [1, 255]], false);
    let mut e = FixedCoefficientEmu::new(100).unwrap();
    for age in 0..=4 {
        let before = e.registers();
        let s = e
            .tick(Tick {
                input: match age {
                    0 => Some(a),
                    2 => Some(b),
                    _ => None,
                },
                output_ready: false,
                ..Tick::default()
            })
            .unwrap();
        if age == 2 {
            assert_eq!(e.registers().row_totals, [50, 150]);
        }
        if age == 3 {
            assert_eq!(before.row_totals, [50, 150]);
            assert_eq!(e.registers().fine_corners, golden(a).weights[0]);
            assert_eq!(e.registers().row_totals, [156, 155]);
        }
        if age == 4 {
            assert_eq!(before.row_totals, [156, 155]);
            assert_eq!(s.ready_access.write.unwrap().1, packed_weights(golden(a)));
            assert_eq!(e.registers().row_totals, [509, 1]);
        }
    }
}

#[test]
fn illegal_offers_wait_for_eligibility_then_fault_without_ports_or_recovery() {
    for case in 0..4 {
        let mut bad = input(0, 200, [[255; 2]; 2], false);
        let mut e = FixedCoefficientEmu::new(100).unwrap();
        match case {
            0 => bad.parents = [512, 0],
            1 => bad.metadata.coordinates[1][3] = 1024,
            2 => bad.metadata.levels[0] = 11,
            _ => bad.metadata.last_fine = true,
        }
        let before = e.registers();
        assert!(
            !e.tick(Tick {
                ce: false,
                input: Some(bad),
                ..Tick::default()
            })
            .unwrap()
            .accepted
        );
        assert_eq!(e.registers(), before);
        assert!(matches!(
            e.tick(Tick {
                input: Some(bad),
                ..Tick::default()
            }),
            Err(Fault::Input)
        ));
        assert_eq!(e.registers(), before);
        assert!(e.faulted());
        assert!(matches!(e.tick(Tick::default()), Err(Fault::Terminal)));
    }
}

#[test]
fn old_full_pop_does_not_validate_or_admit_bad_offer_until_later_edge() {
    let legal = input(0, 200, [[255, 128], [1, 254]], false);
    let mut bad = legal;
    bad.metadata.key = 64;
    let mut e = FixedCoefficientEmu::new(100).unwrap();
    let mut accepted = 0;
    for _ in 0..20 {
        let s = e
            .tick(Tick {
                input: (accepted < 2).then_some(legal),
                output_ready: false,
                ..Tick::default()
            })
            .unwrap();
        accepted += usize::from(s.accepted);
        if e.snapshot().queued == 2 {
            break;
        }
    }
    assert_eq!(e.snapshot().queued, 2);
    let before = e.snapshot();
    let registers = e.registers();
    let s = e
        .tick(Tick {
            input: Some(bad),
            ..Tick::default()
        })
        .unwrap();
    assert!(s.consumed && !s.accepted && !s.input_ready);
    assert!(s.metadata_access.write.is_none() && s.ready_access.write.is_none());
    assert_eq!(e.registers(), registers);
    assert_eq!(e.snapshot().product_registers, before.product_registers);
    assert!(matches!(
        e.tick(Tick {
            input: Some(bad),
            ..Tick::default()
        }),
        Err(Fault::Input)
    ));
    assert_eq!(e.snapshot().queued, 1);
    assert!(e.output().is_none());
    let mut credit = FixedCoefficientEmu::new(10).unwrap();
    assert!(matches!(
        credit.tick(Tick {
            input: Some(legal),
            work_available: 17,
            ..Tick::default()
        }),
        Err(Fault::WorkCredit)
    ));
}
