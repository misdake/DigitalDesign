//! Actual production coefficient/membership/packet conservation, independently
//! checked from emitted Group4 words. No counted Program supplies the answers.
use super::{compact_membership as membership, compact_packet as packet, runtime_membership};
use crate::texture::{
    emu::{coefficient, coefficient::compact_stage::SerialCoefficientEmu, compact},
    ports::Group4,
};

fn groups(input: runtime_membership::Input) -> Vec<Group4> {
    let mut m = membership::Pipeline::with_short_alignment(true);
    m.tick(true, Some(input)).unwrap();
    let mut member = None;
    for edge in 0..16 {
        let ce = edge % 3 != 1;
        if let Some(value) = m.tick(ce, None).unwrap() {
            member = Some(value);
            break;
        }
    }
    let member = member.expect("bounded membership publication");
    let mut output = Vec::new();
    for tap in 0..4 {
        if member.emit() & (1 << tap) == 0 {
            continue;
        }
        let mut p = packet::Pipeline::with_short_alignment(true);
        p.tick(true, Some(packet::Input { member, tap })).unwrap();
        let mut group = None;
        for edge in 0..16 {
            let ce = edge % 3 != 1;
            if let Some(word) = p.tick(ce, None).unwrap() {
                group = Some(Group4::unpack72(word).unwrap());
                break;
            }
        }
        output.push(group.expect("bounded packet publication"));
    }
    output
}

#[test]
fn every_split_is_bounded_and_membership_partitions_each_nonzero_tap_once() {
    // Complete scalar split domain, including the inactive parent0 endpoint.
    for parent in 0_u32..=511 {
        for fraction in 0..=255 {
            let right = parent * fraction / 256;
            let left = parent - right;
            assert!(right <= parent);
            assert_eq!(left + right, parent);
            assert!(parent == 0 || left != 0);
        }
    }
    // All nonzero masks and all same/seam/wrap equivalence classes. Values
    // distinguish each tap; inspect emitted words rather than emitter masks.
    let mut cases = 0;
    for level in 0..=10 {
        let mask = compact::side_mask(level);
        for x in [0, 6 & mask, 7 & mask, mask] {
            for y in [0, 6 & mask, 7 & mask, mask] {
                let coordinates = [x, (x + 1) & mask, y, (y + 1) & mask];
                for active in 1..16 {
                    let weights = std::array::from_fn(|i| {
                        if active & (1 << i) != 0 {
                            [37, 61, 129, 284][i]
                        } else {
                            0
                        }
                    });
                    let output = groups(runtime_membership::Input {
                        weights,
                        coordinates,
                        slot: 7,
                        level,
                        key: 53,
                        fine: true,
                        last_fine: true,
                    });
                    let mut visits = [0; 4];
                    let mut total = 0;
                    for g in output {
                        assert_eq!((g.quad_id, g.lane), (13, 1));
                        assert_eq!(g.key.n, level);
                        for t in 0..4 {
                            if g.coefficients[t] != 0 {
                                visits[t] += 1;
                                assert_eq!(g.coefficients[t], u32::from(weights[t]));
                                assert_eq!(
                                    (g.key.x, g.key.y),
                                    (
                                        (coordinates[t & 1] >> 3) as u8,
                                        (coordinates[2 + (t >> 1)] >> 3) as u8
                                    )
                                );
                                total += g.coefficients[t];
                            }
                        }
                    }
                    assert_eq!(visits, weights.map(|w| u8::from(w != 0)));
                    assert_eq!(total, weights.map(u32::from).iter().sum());
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 2640);
}

#[test]
fn actual_coefficient_to_packet_prefix_and_final_boundary_conserve_511() {
    // Production LOD lambda maps all Q8 fractions to 0..509. Thus the fine
    // parent is positive; Coefficient's top-left remainder is always positive,
    // and fine's first emitted tap is exactly tap0 (the first flag boundary).
    for fraction in 0_u16..=255 {
        let lambda = 2 * fraction - u16::from(fraction > 128);
        assert!(lambda <= 509);
        assert!(511 - lambda >= 2);
    }
    let mut cases = 0;
    let mut seam_groups = 0;
    for n in 0..=10 {
        let mask = compact::side_mask(n);
        for parent in 0..=511 {
            for fraction in [0_u8, 1, 127, 128, 254, 255] {
                let base = if parent % 2 == 0 { mask } else { 7 & mask };
                let raw = u64::from(fraction)
                    | u64::from(255 - fraction) << 8
                    | u64::from(base) << 16
                    | u64::from(base) << 26;
                let coord = compact::expand_result(raw, n);
                let input = coefficient::Input {
                    parents: [parent, 511 - parent],
                    fractions: coord.fractions,
                    nearest: false,
                    metadata: coefficient::Metadata {
                        coordinates: coord.coordinates,
                        levels: [n, n.saturating_sub(1)],
                        slot: 7,
                        key: 53,
                        last_fine: parent == 511,
                    },
                };
                let mut executor = SerialCoefficientEmu::new(64).unwrap();
                assert!(
                    executor
                        .tick(coefficient::Tick {
                            input: Some(input),
                            ..Default::default()
                        })
                        .unwrap()
                        .accepted
                );
                let mut result = None;
                for edge in 0..32 {
                    let s = executor
                        .tick(coefficient::Tick {
                            ce: edge % 4 != 1,
                            ..Default::default()
                        })
                        .unwrap();
                    if s.consumed {
                        result = s.output;
                        break;
                    }
                }
                let out = result.expect("bounded coefficient consumption");
                let mut words = Vec::new();
                for plane in 0..2 {
                    let p = u32::from(input.parents[plane]);
                    let [fu, fv] = input.fractions[plane].map(u32::from);
                    let bottom = p * fv / 256;
                    let top = p - bottom;
                    let br = bottom * fu / 256;
                    let tr = top * fu / 256;
                    assert_eq!(
                        out.weights[plane].map(u32::from),
                        [top - tr, tr, bottom - br, br]
                    );
                    if p != 0 {
                        words.extend(groups(runtime_membership::Input {
                            weights: out.weights[plane],
                            coordinates: out.metadata.coordinates[plane],
                            slot: 7,
                            level: out.metadata.levels[plane],
                            key: 53,
                            fine: plane == 0,
                            last_fine: parent == 511,
                        }));
                    }
                }
                assert_eq!(
                    words.iter().filter(|g| g.first).count(),
                    usize::from(parent != 0)
                );
                assert_eq!(words.iter().filter(|g| g.last).count(), 1);
                let mut prefix = 0;
                for (i, g) in words.iter().enumerate() {
                    assert_eq!(g.first, parent != 0 && i == 0);
                    assert_eq!(g.last, i + 1 == words.len());
                    let group_sum = g.coefficients.iter().sum::<u32>();
                    assert!(group_sum > 0);
                    prefix += group_sum;
                    assert!(prefix <= 511);
                    assert!(prefix * 255 < 1 << 17);
                }
                assert_eq!(prefix, 511);
                seam_groups += usize::from(words.len() > 2);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 33792);
    assert!(seam_groups > 0);
    println!("CONSERVATION scalar=131072 membership=2640 coefficient-to-packet={cases} multi-group={seam_groups}");
}

#[test]
fn numeric_bound_does_not_require_active_fine_or_nonzero_first_corner() {
    // Include inactive fine and zero top-left distributions that a rounded
    // corner splitter can produce. This numeric proof makes no first-flag
    // promise for those arbitrary Membership inputs; protocol guards remain.
    for weights in [
        [[0; 4], [0, 0, 0, 511]],
        [[0, 128, 128, 0], [0, 0, 0, 255]],
        [[0, 0, 0, 511], [0; 4]],
    ] {
        let mut prefix = 0;
        for (plane, row) in weights.into_iter().enumerate() {
            if row == [0; 4] {
                continue;
            }
            for g in groups(runtime_membership::Input {
                weights: row,
                coordinates: [7, 8, 7, 8],
                slot: 0,
                level: 5,
                key: 0,
                fine: plane == 0,
                last_fine: weights[1] == [0; 4],
            }) {
                prefix += g.coefficients.iter().sum::<u32>();
                assert!(prefix <= 511);
                assert!(prefix * 255 < 1 << 17);
            }
        }
        assert_eq!(prefix, 511);
    }
}
