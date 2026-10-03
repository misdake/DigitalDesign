//! Independent tuple grouping/field packing and actual registered wave checks.
use super::{runtime_membership as member, runtime_packet as packet, transport::Member};
use std::collections::VecDeque;

pub(super) fn groups(v: member::Input) -> Vec<usize> {
    let mut tiles = vec![];
    let mut result = vec![];
    for i in 0..4 {
        let tile = (v.coordinates[i & 1] / 8, v.coordinates[2 + (i >> 1)] / 8);
        if v.weights[i] != 0 && !tiles.contains(&tile) {
            tiles.push(tile);
            result.push(i);
        }
    }
    result
}
fn fields(values: &[(u128, usize)]) -> u128 {
    let mut result = 0;
    let mut offset = 0;
    for &(value, width) in values {
        assert!(value < 1_u128 << width);
        result |= value << offset;
        offset += width;
    }
    result
}
pub(super) fn golden_member_banks(v: member::Input) -> [u128; 7] {
    let mut capture: Vec<_> = v.weights.into_iter().map(|w| (w.into(), 9)).collect();
    capture.extend(v.coordinates.map(|c| (c.into(), 10)));
    capture.extend([
        (v.slot.into(), 4),
        (v.level.into(), 4),
        (v.key.into(), 6),
        (v.fine.into(), 1),
        (v.last_fine.into(), 1),
    ]);
    let mut intermediate: Vec<_> = v.weights.into_iter().map(|w| (w.into(), 9)).collect();
    intermediate.extend(v.coordinates.map(|c| (u128::from(c / 8), 7)));
    intermediate.extend([
        ((v.coordinates[0] % 8).into(), 3),
        ((v.coordinates[2] % 8).into(), 3),
        (v.slot.into(), 4),
        (v.level.into(), 4),
        (v.key.into(), 6),
        (v.fine.into(), 1),
        ((!v.fine || v.last_fine).into(), 1),
    ]);
    intermediate.extend(v.weights.map(|w| (u128::from(w != 0), 1)));
    let slice = fields(&intermediate);
    intermediate.extend([
        (u128::from(v.coordinates[0] / 8 == v.coordinates[1] / 8), 1),
        (u128::from(v.coordinates[2] / 8 == v.coordinates[3] / 8), 1),
    ]);
    let equal = fields(&intermediate);
    for (a, b) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
        let lhs = (v.coordinates[a & 1] / 8, v.coordinates[2 + (a >> 1)] / 8);
        let rhs = (v.coordinates[b & 1] / 8, v.coordinates[2 + (b >> 1)] / 8);
        intermediate.push((u128::from(lhs == rhs), 1));
    }
    let member = golden_member(v);
    [
        fields(&capture),
        slice,
        equal,
        fields(&intermediate),
        member,
        member,
        member,
    ]
}
pub(super) fn golden_packet_banks(v: member::Input, tap: usize) -> [u128; 9] {
    let word = golden_member(v);
    // Independent bit-vector deletion, rather than the DUT compression formula.
    let mut captured = 0;
    let mut next = 0;
    for bit in 0..92 {
        if bit != 36 {
            captured |= ((word >> bit) & 1) << next;
            next += 1;
        }
    }
    captured |= (tap as u128) << next;
    let packed = golden_packet(v, tap);
    let mut selected = vec![(packed & ((1 << 28) - 1), 28)];
    selected.extend(v.weights.map(|w| (w.into(), 9)));
    selected.push((v.key.into(), 6));
    let tile = (
        v.coordinates[tap & 1] / 8,
        v.coordinates[2 + (tap >> 1)] / 8,
    );
    for i in 0..4 {
        selected.push((
            u128::from(tile == (v.coordinates[i & 1] / 8, v.coordinates[2 + (i >> 1)] / 8)),
            1,
        ));
    }
    selected.extend([((packed >> 64) & 1, 1), ((packed >> 65) & 1, 1)]);
    [
        captured,
        fields(&selected),
        packed,
        packed,
        packed,
        packed,
        packed,
        packed,
        packed,
    ]
}
pub(super) fn golden_member(v: member::Input) -> u128 {
    let g = groups(v);
    let mut values: Vec<_> = v.weights.into_iter().map(|w| (w.into(), 9)).collect();
    values.extend((0..4).map(|i| (u128::from(g.contains(&i)), 1)));
    values.extend(v.coordinates.map(|c| (u128::from(c / 8), 7)));
    values.extend([
        (u128::from(v.coordinates[0] % 8), 3),
        (u128::from(v.coordinates[2] % 8), 3),
        (u128::from(v.coordinates[0] / 8 == v.coordinates[1] / 8), 1),
        (u128::from(v.coordinates[2] / 8 == v.coordinates[3] / 8), 1),
        (v.slot.into(), 4),
        (v.level.into(), 4),
        ((v.key / 4).into(), 4),
        ((v.key % 4).into(), 2),
        (v.fine.into(), 1),
        ((!v.fine || v.last_fine).into(), 1),
    ]);
    fields(&values)
}
pub(super) fn golden_packet(v: member::Input, tap: usize) -> u128 {
    let g = groups(v);
    assert!(g.contains(&tap));
    let tile = (
        v.coordinates[tap & 1] / 8,
        v.coordinates[2 + (tap >> 1)] / 8,
    );
    let mut values = vec![
        (v.slot.into(), 4),
        (v.level.into(), 4),
        (tile.0.into(), 7),
        (tile.1.into(), 7),
        ((v.coordinates[0] % 8).into(), 3),
        ((v.coordinates[2] % 8).into(), 3),
    ];
    for i in 0..4 {
        let other = (v.coordinates[i & 1] / 8, v.coordinates[2 + (i >> 1)] / 8);
        values.push((u128::from(if other == tile { v.weights[i] } else { 0 }), 9));
    }
    values.extend([
        (u128::from(v.fine && tap == 0), 1),
        (
            u128::from((!v.fine || v.last_fine) && g.last() == Some(&tap)),
            1,
        ),
        ((v.key / 4).into(), 4),
        ((v.key % 4).into(), 2),
    ]);
    fields(&values)
}
fn input(mask: u8, same: u8, boundary: bool) -> member::Input {
    let (a, b) = if boundary { (1023, 0) } else { (7, 8) };
    member::Input {
        weights: std::array::from_fn(|i| {
            if mask >> i & 1 != 0 {
                [1, 127, 128, 255][i]
            } else {
                0
            }
        }),
        coordinates: [
            a,
            if same & 1 != 0 { a } else { b },
            a,
            if same & 2 != 0 { a } else { b },
        ],
        slot: 15,
        level: 10,
        key: 63,
        fine: true,
        last_fine: true,
    }
}
#[test]
fn actual_scalar_pipelines_tuple_goldens_contiguous_and_bubbles() {
    assert_eq!(
        (
            member::DATA_BITS,
            member::CONTROL_BITS,
            packet::DATA_BITS,
            packet::CONTROL_BITS
        ),
        (648, 8, 673, 10)
    );
    let mut m = member::Pipeline::default();
    let mut p = packet::Pipeline::default();
    let mut expected_m = VecDeque::new();
    let mut packets = VecDeque::new();
    let mut expected_p = VecDeque::new();
    let mut vectors = VecDeque::new();
    for mask in 1..16 {
        for same in 0..4 {
            for boundary in [false, true] {
                for (fine, last_fine) in [(false, false), (true, false), (true, true)] {
                    let mut v = input(mask, same, boundary);
                    v.fine = fine;
                    v.last_fine = last_fine;
                    vectors.push_back(v);
                    v.coordinates = [0, 1, 0, 1];
                    v.key = 0;
                    v.slot = 0;
                    v.level = 0;
                    vectors.push_back(v);
                }
            }
        }
    }
    let mut endpoint = input(1, 3, false);
    endpoint.weights = [511, 0, 0, 0];
    vectors.push_back(endpoint);
    let total = vectors.len();
    let mut member_count = 0;
    let mut packet_count = 0;
    for edge in 0..10_000 {
        let offer = if edge % 11 != 0 {
            vectors.pop_front()
        } else {
            None
        };
        if let Some(v) = offer {
            expected_m.push_back((edge + 7, v));
        }
        if let Some(out) = m.tick(true, offer).unwrap() {
            let (due, v) = expected_m.pop_front().unwrap();
            assert_eq!(due, edge);
            assert_eq!(out.bits(), golden_member(v));
            for tap in groups(v) {
                packets.push_back((v, tap, out));
            }
            member_count += 1;
        }
        let offer = if edge % 13 != 0 {
            packets.pop_front()
        } else {
            None
        };
        if let Some((v, tap, _)) = offer {
            expected_p.push_back((edge + 9, golden_packet(v, tap)));
        }
        let capture = offer.map(|(_, tap, member)| packet::Input {
            member,
            tap: tap as u8,
        });
        if let Some(out) = p.tick(true, capture).unwrap() {
            let (due, word) = expected_p.pop_front().unwrap();
            assert_eq!(due, edge);
            assert_eq!(out as u128, word);
            packet_count += 1;
        }
        for (value, width) in m
            .banks()
            .into_iter()
            .zip(member::BANK_BITS)
            .chain(p.banks().into_iter().zip(packet::BANK_BITS))
        {
            if let Some(value) = value {
                assert!(value < 1_u128 << width);
            }
        }
        if vectors.is_empty() && packets.is_empty() && m.inflight() == 0 && p.inflight() == 0 {
            break;
        }
    }
    assert_eq!(member_count, total);
    assert!(packet_count > total);
    assert!(expected_m.is_empty() && expected_p.is_empty());
}
#[test]
fn actual_scalar_each_cut_ce_freezes_and_ignores_replacement() {
    let v = input(15, 0, false);
    let replacement = input(1, 3, true);
    let mut m = member::Pipeline::default();
    m.tick(true, Some(v)).unwrap();
    for cut in 0..7 {
        assert!(m.banks()[cut].is_some());
        let old = m.clone();
        for _ in 0..3 {
            assert_eq!(m.tick(false, Some(replacement)).unwrap(), None);
            assert_eq!(m, old);
        }
        let out = m.tick(true, None).unwrap();
        if cut == 6 {
            assert_eq!(out.unwrap().bits(), golden_member(v));
        } else {
            assert!(out.is_none());
        }
    }
    let member = old_member(v);
    let mut p = packet::Pipeline::default();
    p.tick(true, Some(packet::Input { member, tap: 3 }))
        .unwrap();
    assert!(p.banks()[0].unwrap() < 1_u128 << 93);
    for cut in 0..9 {
        assert!(p.banks()[cut].is_some());
        let old = p.clone();
        for _ in 0..3 {
            assert!(p
                .tick(false, Some(packet::Input { member, tap: 4 }))
                .unwrap()
                .is_none());
            assert_eq!(p, old);
        }
        let out = p.tick(true, None).unwrap();
        if cut == 8 {
            assert_eq!(out.unwrap() as u128, golden_packet(v, 3));
        } else {
            assert!(out.is_none());
        }
    }
}
fn old_member(v: member::Input) -> Member {
    let mut m = member::Pipeline::default();
    m.tick(true, Some(v)).unwrap();
    for _ in 0..6 {
        m.tick(true, None).unwrap();
    }
    m.output().unwrap()
}
#[test]
fn actual_scalar_terminal_fault_width_empty_and_invalid_tap() {
    for mode in 0..5 {
        let mut m = member::Pipeline::default();
        m.tick(true, Some(input(15, 0, false))).unwrap();
        let mut bad = input(15, 0, false);
        match mode {
            0 => bad.weights[0] = 512,
            1 => bad.coordinates[0] = 1024,
            2 => bad.slot = 16,
            3 => bad.key = 64,
            _ => bad.weights = [0; 4],
        }
        assert!(m.tick(true, Some(bad)).is_err());
        let old = m.clone();
        assert!(m.tick(false, None).is_err());
        assert!(m.tick(true, Some(input(1, 3, false))).is_err());
        assert_eq!(m, old);
    }
    let member = old_member(input(1, 3, false));
    for tap in [1, 3, 4] {
        let mut p = packet::Pipeline::default();
        p.tick(true, Some(packet::Input { member, tap: 0 }))
            .unwrap();
        assert!(p.tick(true, Some(packet::Input { member, tap })).is_err());
        let old = p.clone();
        assert!(p.tick(false, None).is_err());
        assert!(p.tick(true, None).is_err());
        assert_eq!(p, old);
    }
}
#[test]
fn dynamic_counted_guard_rejects_both_helpers_and_restores_compile_scope() {
    for which in 0..2 {
        assert!(std::panic::catch_unwind(|| {
            let _scope = super::counted_call_guard::Scope::enter();
            if which == 0 {
                let _ = super::membership_values([1, 0, 0, 0], [0; 4], [0; 3], 0, [1, 1]);
            } else {
                let _ = super::packet_values(|_| 0, 0);
            }
        })
        .is_err());
    }
    let before = super::counted_call_guard::calls();
    super::membership_values([1, 0, 0, 0], [0; 4], [0; 3], 0, [1, 1]).unwrap();
    let after = super::counted_call_guard::calls();
    assert_eq!(after[0], before[0] + 1);
}
