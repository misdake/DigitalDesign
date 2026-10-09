//! Actual public Runtime registers, source captures, pool ownership and MC.
use super::super::{runtime_membership as member, runtime_numerical_tests as reference};
use super::qualification as q;
use super::*;
use std::{collections::BTreeMap, io::Write};

const BOUND: u64 = 20_000;
fn evidence(name: &str) -> Option<std::fs::File> {
    std::env::var_os("GPU_RUNTIME_COEFFICIENT_OUTPUT").map(|dir| {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::File::create(std::path::PathBuf::from(dir).join(name)).unwrap()
    })
}
#[test]
fn connected_actual_numerical_banks_pool_writes_and_ce_cuts() {
    let (slots, bytes) = q::fixture();
    let mut r = q::runtime(
        &slots,
        super::super::control::Hardware {
            max_cycles: BOUND,
            ..Default::default()
        },
    );
    let mut mc = q::physical::Physical::new(4096, bytes, true, true);
    let qs: Vec<_> = (0..8)
        .map(|id| {
            q::input(
                id,
                if id == 1 { 9 } else { 15 },
                [0.25, 0.25],
                [
                    crate::texture::ports::Filter::Nearest,
                    crate::texture::ports::Filter::Bilinear,
                    crate::texture::ports::Filter::Trilinear,
                ][usize::from(id) % 3],
            )
        })
        .collect();
    let mut accepted = 0;
    let mut got = vec![];
    let mut history_m: [Option<member::Input>; 7] = [None; 7];
    let mut history_p: [Option<(member::Input, usize)>; 9] = [None; 9];
    let mut owner = BTreeMap::new();
    let mut hits_m = [0; 7];
    let mut hits_p = [0; 9];
    let mut frozen = 0;
    let mut refused = 0;
    let mut written = 0;
    let mut partial = 0;
    let mut csv = evidence("actual-numerical-banks.csv");
    if let Some(f) = csv.as_mut() {
        writeln!(
            f,
            "wall,base,CE,member_valid,packet_valid,member_banks_hex,packet_banks_hex,WorkW,ACK,PW"
        )
        .unwrap();
    }
    for wall in 0..BOUND {
        let before = r.preparation.numerical_banks();
        let calls = super::super::counted_call_guard::calls();
        // Pause every phase of each data path, and block only new captures.
        r.preparation.membership_ready = wall % 19 != 7;
        r.preparation.packet_ready = wall % 17 != 3;
        let step = r
            .step(
                &mut mc,
                qs.get(accepted),
                timed::Control {
                    ce: wall % 7 != 2,
                    result_ready: wall > 1000,
                },
            )
            .unwrap();
        if step.accepted {
            accepted += 1;
        }
        let trace = &r.preparation.trace;
        if !step.accepted {
            assert_eq!(super::super::counted_call_guard::calls(), calls);
        } else {
            // Only eligible upstream compile/provenance may use counted helpers.
            let after = super::super::counted_call_guard::calls();
            assert!(after[0] > calls[0] && after[1] > calls[1]);
        }
        if !step.effective_ce {
            assert_eq!(r.preparation.numerical_banks(), before);
            frozen += 1;
        } else {
            if trace.work.write.is_some() {
                let v = history_m[6].take().expect("old membership output owner");
                owner.insert(reference::golden_member(v), v);
            }
            let captured = trace.work.capture.map(|(m, t)| {
                let v = *owner
                    .get(&m.bits())
                    .expect("Work capture from independently checked member");
                if !trace.work.ack {
                    partial += 1;
                }
                (v, usize::from(t))
            });
            if let Some(payload) = trace.packet {
                let (v, tap) = history_p[8].take().expect("old packet output owner");
                assert_eq!(payload as u128, reference::golden_packet(v, tap));
                let writes: Vec<_> = step
                    .cache
                    .packet_events
                    .iter()
                    .filter_map(|e| {
                        if let timed::packet::Event::Write { payload, .. } = e {
                            Some(*payload)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(writes, vec![payload]);
                written += 1;
            }
            for i in (1..7).rev() {
                history_m[i] = history_m[i - 1];
            }
            history_m[0] = trace.member_input;
            for i in (1..9).rev() {
                history_p[i] = history_p[i - 1];
            }
            history_p[0] = captured;
            for (i, v) in history_m.iter().enumerate() {
                assert_eq!(
                    trace.member_banks[i],
                    v.map(|v| reference::golden_member_banks(v)[i])
                );
                hits_m[i] += usize::from(v.is_some());
            }
            for (i, v) in history_p.iter().enumerate() {
                assert_eq!(
                    trace.packet_banks[i],
                    v.map(|(v, t)| reference::golden_packet_banks(v, t)[i])
                );
                hits_p[i] += usize::from(v.is_some());
            }
            if before.1[0].is_some() && !r.preparation.packet_ready {
                assert!(trace.work.capture.is_none());
                refused += 1;
            }
        }
        if let Some(f) = csv.as_mut() {
            writeln!(
                f,
                "{},{},{},{:?},{:?},{:x?},{:x?},{:?},{},{}",
                wall,
                r.preparation_stats().enabled,
                step.effective_ce,
                trace.member_banks.map(|v| v.is_some()),
                trace.packet_banks.map(|v| v.is_some()),
                trace.member_banks,
                trace.packet_banks,
                trace.work.write,
                trace.work.ack,
                trace.packet.is_some()
            )
            .unwrap();
        }
        got.extend(step.results);
        if accepted == qs.len() && r.idle() {
            break;
        }
    }
    assert!(r.idle() && accepted == qs.len());
    assert_eq!(got, qs.iter().flat_map(q::golden).collect::<Vec<_>>());
    assert!(hits_m.iter().all(|&n| n > 0) && hits_p.iter().all(|&n| n > 0));
    assert!(frozen > 0 && written > 0 && refused > 0 && partial > 0);
    println!("NUMERICAL actual Runtime: member_cuts={hits_m:?} packet_cuts={hits_p:?} frozen={frozen} PW={written} refused={refused} partial={partial} poison={}",r.poison_hits);
}
#[test]
fn connected_packet_operand_error_and_partial_watchdog_are_terminal() {
    for watchdog in [false, true] {
        let (slots, bytes) = q::fixture();
        let mut r = q::runtime(
            &slots,
            super::super::control::Hardware {
                max_cycles: BOUND,
                ..Default::default()
            },
        );
        let mut mc = q::physical::Physical::new(4096, bytes, true, false);
        let input = q::input(
            0,
            15,
            [0.25, 0.25],
            crate::texture::ports::Filter::Trilinear,
        );
        let mut accepted = false;
        let mut injected = false;
        for _ in 0..BOUND {
            let banks = r.preparation.numerical_banks();
            if watchdog && banks.1.iter().any(Option::is_some) {
                r.max_wall = r.wall;
                injected = true;
            } else if !watchdog && r.preparation.corrupt_head_tap() {
                injected = true;
            }
            let result = r.step(
                &mut mc,
                (!accepted).then_some(&input),
                timed::Control::default(),
            );
            if injected {
                let error = result.unwrap_err();
                assert!(
                    error.contains(if watchdog { "watchdog" } else { "tap" }),
                    "{error}"
                );
                let old = r.snapshot();
                let numeric = r.preparation.numerical_banks();
                let mc_cycles = mc.cycles;
                assert!(r.step(&mut mc, None, timed::Control::default()).is_err());
                assert_eq!(r.snapshot(), old);
                assert_eq!(r.preparation.numerical_banks(), numeric);
                assert_eq!(mc.cycles, mc_cycles);
                break;
            }
            accepted |= result.unwrap().accepted;
        }
        assert!(injected);
    }
}

#[test]
fn all_supported_configuration_receipts_keep_numeric_owners_and_capacities() {
    use super::super::{control, inventory, runtime_inventory, Binding};
    let binding = Binding::build().unwrap();
    let c = timed::Hardware {
        prefetch: false,
        preparation: timed::PreparationMode::BoundStages,
        packet_storage: timed::PacketStorage::Pool64,
        ..Default::default()
    };
    let mut count = 0;
    for work in 2..=32 {
        for coordinate in 1..=16 {
            for contexts in 1..=8 {
                for release in [false, true] {
                    for storage in [control::Storage::Dedicated, control::Storage::Packed] {
                        let p = control::Hardware {
                            work_credits: work,
                            coordinate_credits: coordinate,
                            contexts,
                            release_after_capture: release,
                            storage,
                            max_cycles: BOUND,
                            ..Default::default()
                        };
                        p.validate().unwrap();
                        let old = inventory::describe(&binding, p, &c).unwrap();
                        let actual = runtime_inventory::describe(&binding, p, &c).unwrap();
                        assert_eq!(
                            (actual.sdp4_cells, actual.ram16x1_cells, actual.bsram),
                            (old.sdp4_cells, old.ram16x1_cells, old.bsram)
                        );
                        let row =
                            |name| actual.rows.iter().find(|r| r.name == name).unwrap().ff_bits;
                        assert_eq!(row("actual membership valid/fault"), 8);
                        assert_eq!(row("actual packet valid/fault"), 10);
                        for name in ["membership FF", "packet FF", "prep phase/queue control"] {
                            assert_eq!(
                                row(name),
                                old.rows.iter().find(|r| r.name == name).unwrap().ff_bits
                            );
                        }
                        if storage == control::Storage::Packed {
                            assert_eq!(row("actual membership retained control allowance"), 7);
                            assert_eq!(row("actual packet retained control allowance"), 6);
                        }
                        count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(count, 15_872);
    println!("NUMERICAL receipts: all {count} W2..32/C1..16/context1..8/storage/release configurations accepted; generic/data ceilings retained");
}
