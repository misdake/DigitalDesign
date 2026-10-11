//! Adjacent production tuples checked with independent generic tuple goldens.
use super::{
    compact_membership as m, compact_packet as p, runtime_membership::Input,
    runtime_numerical_tests as golden,
};
use std::collections::VecDeque;

#[test]
fn selected_leaves_cover_levels_seams_sparse_weights_and_ce() {
    for short in [false, true] {
        let mut vectors = VecDeque::new();
        for n in 0..=10 {
            let mask = (1u16 << n.max(1)) - 1;
            for base in [0, 6 & mask, 7 & mask, mask] {
                for weights in 1..16 {
                    vectors.push_back(Input {
                        coordinates: [base, (base + 1) & mask, base, (base + 1) & mask],
                        weights: std::array::from_fn(|i| {
                            if weights >> i & 1 != 0 {
                                [1, 127, 128, 255][i]
                            } else {
                                0
                            }
                        }),
                        level: n,
                        slot: 15,
                        key: 63,
                        fine: weights & 1 != 0,
                        last_fine: weights & 2 != 0,
                    });
                }
            }
        }
        let total = vectors.len();
        let mut member = m::Pipeline::with_short_alignment(short);
        let mut packet = p::Pipeline::with_short_alignment(short);
        let mut mh = VecDeque::new();
        let mut ph = VecDeque::new();
        let mut offers = VecDeque::new();
        let mut enabled = 0;
        let mut mc = 0;
        let mut pc = 0;
        let mut pauses = 0;
        for wall in 0..30_000 {
            let ce = wall % 11 != 3 && wall % 11 != 4;
            let offer = if ce && wall % 7 != 0 {
                vectors.pop_front()
            } else {
                None
            };
            let pin = if ce { offers.pop_front() } else { None };
            let oldm = member.banks();
            let oldp = packet.banks();
            let mout = member.tick(ce, offer).unwrap();
            let pout = packet.tick(ce, pin).unwrap();
            if !ce {
                assert_eq!(member.banks(), oldm);
                assert_eq!(packet.banks(), oldp);
                pauses += 1;
                continue;
            }
            if let Some(v) = offer {
                mh.push_back((enabled + if short { 2 } else { 7 }, v));
            }
            if let Some(mout) = mout {
                let (at, v) = mh.pop_front().unwrap();
                assert_eq!(at, enabled);
                assert_eq!(mout.bits(), golden::compact_member(v));
                mc += 1;
                for t in golden::groups(v) {
                    offers.push_back(p::Input {
                        member: mout,
                        tap: t as u8,
                    });
                }
            }
            if let Some(pin) = pin {
                // Reconstruct tuple independently from semantic queue key: all vectors are distinct
                // only by fields, so expected canonical packet uses the generic Member layout.
                let raw = pin.member;
                let n = raw.raw("n") as u8;
                let mask = (1u16 << n.max(1)) - 1;
                let x = (raw.raw("tx0") * 8 + raw.raw("lx")) as u16;
                let y = (raw.raw("ty0") * 8 + raw.raw("ly")) as u16;
                let v = Input {
                    coordinates: [x, (x + 1) & mask, y, (y + 1) & mask],
                    weights: std::array::from_fn(|i| raw.raw(&format!("w{i}")) as u16),
                    level: n,
                    slot: raw.raw("slot") as u8,
                    key: raw.key(),
                    fine: raw.raw("fine") != 0,
                    last_fine: raw.raw("final") != 0,
                };
                ph.push_back((
                    enabled + if short { 2 } else { 9 },
                    golden::golden_packet(v, pin.tap as usize),
                ));
            }
            if let Some(out) = pout {
                let (at, w) = ph.pop_front().unwrap();
                assert_eq!(at, enabled);
                assert_eq!(out as u128, w);
                pc += 1;
            }
            enabled += 1;
            if vectors.is_empty() && mh.is_empty() && offers.is_empty() && ph.is_empty() {
                break;
            }
        }
        assert_eq!(mc, total);
        assert!(pc > total && pauses > 0);
        println!("COMPACT leaves short={short} members={mc} packets={pc} CE0={pauses}");
    }
}

#[test]
fn selected_membership_rejects_nonadjacent_generic_tuple() {
    let v = Input {
        weights: [511, 0, 0, 0],
        coordinates: [0, 4, 0, 1],
        slot: 0,
        level: 4,
        key: 0,
        fine: true,
        last_fine: true,
    };
    let mut generic = super::runtime_membership::Pipeline::default();
    assert!(generic.tick(true, Some(v)).is_ok());
    let mut selected = m::Pipeline::with_short_alignment(true);
    assert!(selected
        .tick(true, Some(v))
        .unwrap_err()
        .contains("adjacent domain"));
    assert!(selected.tick(true, None).unwrap_err().contains("terminal"));
}
