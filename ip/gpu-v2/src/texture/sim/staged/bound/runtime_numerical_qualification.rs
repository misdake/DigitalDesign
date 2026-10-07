//! Actual public Runtime registers, source captures, pool ownership and MC.
use super::super::{runtime_membership as member, runtime_numerical_tests as reference};
use super::qualification as q;
use super::*;
use std::{collections::BTreeMap, io::Write};

const BOUND: u64 = 20_000;
#[test]
fn dedicated_actual_d_lod_selector_bill_matches_static_certificate_and_old_basis() {
    use super::super::{control, inventory, runtime_inventory};
    let b = super::super::Binding::build().unwrap();
    assert_eq!(
        (
            b.derivative.packed.read_selector_tree_bits,
            b.lod.packed.read_selector_tree_bits
        ),
        (1140, 229)
    );
    assert_eq!(
        (
            b.derivative.packed.write_selector_tree_bits,
            b.lod.packed.write_selector_tree_bits
        ),
        (469, 316)
    );
    assert_eq!(
        (
            b.derivative.packed.control_boolean_gates,
            b.lod.packed.control_boolean_gates
        ),
        (2574, 1792)
    );
    for storage in [control::Storage::Dedicated, control::Storage::Packed] {
        let p = control::Hardware {
            storage,
            ..Default::default()
        };
        let c = timed::Hardware {
            prefetch: false,
            preparation: timed::PreparationMode::BoundStages,
            packet_storage: timed::PacketStorage::Pool64,
            ..Default::default()
        };
        let old = inventory::describe(&b, p, &c).unwrap();
        let actual = runtime_inventory::describe(&b, p, &c).unwrap();
        let removed_read: u64 = [&b.coefficient, &b.plane, &b.packet]
            .into_iter()
            .map(|stage| {
                if storage == control::Storage::Dedicated {
                    stage
                        .ff_banks
                        .iter()
                        .map(|f| u64::from(f.width) * f.slots.saturating_sub(1))
                        .sum()
                } else {
                    stage.packed.read_selector_tree_bits
                }
            })
            .sum();
        let old_runtime_read = old.rotating_read_mux_bits - removed_read;
        if storage == control::Storage::Dedicated {
            // The Work two-head selector adds payload92 + cursor2 mux nodes.
            assert_eq!(actual.rotating_read_mux_bits, old_runtime_read + 769 + 94);
            assert_eq!(
                actual.rotating_write_mux_bits,
                old.rotating_write_mux_bits + 785
            );
            assert_eq!(
                actual.storage_control_boolean_gates,
                old.storage_control_boolean_gates + 4366
            );
            let ceilings = [1251, 727];
            let actual_used = [908, 325];
            for ((name, ceiling), used) in ["derivative FF", "LOD FF"]
                .into_iter()
                .zip(ceilings)
                .zip(actual_used)
            {
                assert_eq!(
                    actual.rows.iter().find(|r| r.name == name).unwrap().ff_bits,
                    ceiling
                );
                assert!(used <= ceiling);
            }
        } else {
            assert_eq!(actual.rotating_read_mux_bits, old_runtime_read + 94);
            assert_eq!(
                actual.rotating_write_mux_bits,
                old.rotating_write_mux_bits
                    - [&b.coefficient, &b.plane, &b.packet]
                        .into_iter()
                        .map(|s| s.packed.write_selector_tree_bits)
                        .sum::<u64>()
            );
            assert_eq!(
                actual.storage_control_boolean_gates,
                old.storage_control_boolean_gates
                    - [&b.coefficient, &b.plane, &b.packet]
                        .into_iter()
                        .map(|s| s.packed.control_boolean_gates)
                        .sum::<u64>()
            );
        }
        println!("DLOD STEERING {storage:?} oldRuntimeRead={old_runtime_read} actualRead={} actualWrite={} actualBoolean={} FF={}",actual.rotating_read_mux_bits,actual.rotating_write_mux_bits,actual.storage_control_boolean_gates,actual.ff_bits);
    }
}
#[test]
fn actual_d_lod_partial_helper_and_scalar_coordinate_cuts() {
    let (slots, bytes) = q::fixture();
    let mut input = q::input(3, 1, [0.13, 0.07], crate::texture::ports::Filter::Bilinear);
    input.uv = [[0.13, 0.07]; 4];
    input.uv[3][0] += 0.25;
    let original = super::super::prepare(&input, &slots).unwrap();
    let mut r = q::runtime(&slots, super::super::control::Hardware::default());
    let mut mc = q::physical::Physical::new(4096, bytes, true, false);
    for wall in 0..BOUND {
        r.step(
            &mut mc,
            (wall == 32).then_some(&input),
            timed::Control::default(),
        )
        .unwrap();
        let trace = &r.preparation.trace;
        for calc in &trace.l_edge.as_ref().unwrap().calculations {
            assert_eq!(
                calc.raw, original.lod.frame.values[calc.value].raw,
                "LOD wall{wall} age{} value{} operands{:?}",
                calc.age, calc.value, calc.operands
            );
        }
        if let Some(d) = trace.d_capture {
            for (name, raw) in [("slope", i128::from(d.slope)), ("bias", i128::from(d.bias))] {
                assert_eq!(
                    raw,
                    original
                        .derivative
                        .frame
                        .outputs
                        .iter()
                        .find(|v| v.name == name)
                        .unwrap()
                        .raw,
                    "D return {name}"
                );
            }
        }
        if let Some(l) = trace.l_capture {
            let direct = super::super::super::coordinate_values(
                [
                    original
                        .derivative
                        .frame
                        .outputs
                        .iter()
                        .find(|v| v.name == "uv0")
                        .unwrap()
                        .raw as u32,
                    original
                        .derivative
                        .frame
                        .outputs
                        .iter()
                        .find(|v| v.name == "uv1")
                        .unwrap()
                        .raw as u32,
                ],
                l.context.shift,
                l.context.nearest,
                l.context.halve,
                l.context.side,
            )
            .unwrap();
            r.preparation
                .binding
                .coordinate
                .audit(&direct.frame)
                .unwrap();
            for (new, old) in direct
                .frame
                .outputs
                .iter()
                .zip(&original.lanes[0].coordinate.frame.outputs)
            {
                assert_eq!(
                    (&new.name, new.format, new.raw),
                    (&old.name, old.format, old.raw)
                );
            }
            for (name, raw) in [
                ("shift0", i128::from(l.context.shift)),
                ("side0", i128::from(l.context.side[0])),
                ("side1", i128::from(l.context.side[1])),
                ("parent0", i128::from(l.context.parents[0])),
                ("parent1", i128::from(l.context.parents[1])),
            ] {
                assert_eq!(
                    raw,
                    original
                        .lod
                        .frame
                        .outputs
                        .iter()
                        .find(|v| v.name == name)
                        .unwrap()
                        .raw,
                    "LOD return {name}"
                );
            }
        }
        if let Some(c) = trace.coordinate {
            let old = &original.lanes[0].coordinate.frame;
            for w in 0..2 {
                for a in 0..2 {
                    assert_eq!(
                        i128::from(c.fractions[w][a]),
                        old.outputs
                            .iter()
                            .find(|v| v.name == format!("f{w}.{a}"))
                            .unwrap()
                            .raw,
                        "coordinate fraction {w}.{a}"
                    );
                }
            }
        }
        if wall > 32 && r.idle() {
            return;
        }
    }
    panic!("partial helper bounded drain");
}
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
    let originals: Vec<_> = qs
        .iter()
        .map(|v| super::super::prepare(v, &slots).unwrap())
        .collect();
    let mut d_owners = [None; 4];
    let mut l_owners = [None; 4];
    let mut d_calcs = 0;
    let mut l_calcs = 0;
    let mut d_captures = 0;
    let mut l_captures = 0;
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
        let before_d_lod = r.preparation.d_lod_state();
        let calls = super::super::counted_call_guard::calls();
        // Pause every phase of each data path, and block only new captures.
        r.preparation.membership_ready = wall % 19 != 7;
        r.preparation.packet_ready = wall % 17 != 3;
        let raw_input = qs
            .get(accepted)
            .map(RawQuadInput::capture)
            .transpose()
            .unwrap();
        let step = r
            .step_raw(
                &mut mc,
                raw_input.as_ref(),
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
        // Admission is raw boundary capture, including on accepted edges.
        assert_eq!(super::super::counted_call_guard::calls(), calls);
        if !step.effective_ce {
            assert_eq!(r.preparation.numerical_banks(), before);
            assert_eq!(r.preparation.d_lod_state(), before_d_lod);
            frozen += 1;
        } else {
            if let Some(input) = trace.d_input {
                d_owners[usize::from(before_d_lod.2) / 8] = Some(usize::from(input.header.quad));
            }
            if let Some(input) = trace.l_input {
                l_owners[usize::from(before_d_lod.3) / 8] = Some(usize::from(input.header.quad));
            }
            for (stage, edge, owners) in [
                (0, trace.d_edge.as_ref().unwrap(), &d_owners),
                (1, trace.l_edge.as_ref().unwrap(), &l_owners),
            ] {
                for calc in &edge.calculations {
                    let id = owners[calc.iteration].unwrap();
                    let p = &originals[id];
                    let frame = if stage == 0 {
                        &p.derivative.frame
                    } else {
                        &p.lod.frame
                    };
                    assert_eq!(
                        calc.raw, frame.values[calc.value].raw,
                        "connected D/LOD quad{id} stage{stage} cut{}",
                        calc.age
                    );
                    if stage == 0 {
                        d_calcs += 1;
                    } else {
                        l_calcs += 1;
                    }
                }
            }
            d_captures += usize::from(trace.d_capture.is_some());
            l_captures += usize::from(trace.l_capture.is_some());
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
    assert_eq!((d_captures, l_captures), (qs.len(), qs.len()));
    assert!(d_calcs > 0 && l_calcs > 0);
    println!("NUMERICAL actual Runtime: Dcalcs={d_calcs} LODcalcs={l_calcs} captures={d_captures}/{l_captures} member_cuts={hits_m:?} packet_cuts={hits_p:?} frozen={frozen} PW={written} refused={refused} partial={partial} compilations={}",r.stats.compilations);
}
#[test]
fn connected_actual_coordinate_cuts_match_live_frames_and_freeze_under_ce() {
    let (slots, bytes) = q::fixture();
    let mut r = q::runtime(
        &slots,
        super::super::control::Hardware {
            max_cycles: BOUND,
            ..Default::default()
        },
    );
    let mut mc = q::physical::Physical::new(4096, bytes, true, true);
    let inputs: Vec<_> = (0..4)
        .map(|id| {
            q::input(
                id,
                15,
                [0.13 + f64::from(id) / 8.0, 0.07],
                [
                    crate::texture::ports::Filter::Nearest,
                    crate::texture::ports::Filter::Bilinear,
                    crate::texture::ports::Filter::Trilinear,
                ][usize::from(id) % 3],
            )
        })
        .collect();
    let originals: Vec<_> = inputs
        .iter()
        .map(|v| super::super::prepare(v, &slots).unwrap())
        .collect();
    let mut active: Vec<(usize, usize)> = vec![];
    let mut c_calcs = 0;
    let mut frozen = 0;
    let mut accepted = 0;
    for wall in 0..BOUND {
        let before = r.preparation.coordinate_state();
        let step = r
            .step(
                &mut mc,
                inputs.get(accepted),
                timed::Control {
                    ce: wall % 11 != 3,
                    result_ready: true,
                },
            )
            .unwrap();
        if step.accepted {
            accepted += 1;
        }
        let trace = &r.preparation.trace;
        if !step.effective_ce {
            assert_eq!(r.preparation.coordinate_state(), before);
            frozen += 1;
        } else {
            if let Some((quad, ordinal)) = trace.c_owner {
                active.push((usize::from(quad), ordinal));
            }
            if trace.coordinate.is_some() {
                active.remove(0);
            }
            for calc in &trace.c_edge.as_ref().unwrap().calculations {
                // Every live registered cut must equal a currently-owned lane's
                // independently prepared coordinate value at the same index.
                assert!(
                    active.iter().any(|(q, o)| {
                        originals[*q].lanes[*o].coordinate.frame.values[calc.value].raw == calc.raw
                    }),
                    "coordinate cut age{} value{} raw{} has no live owner",
                    calc.age,
                    calc.value,
                    calc.raw
                );
                c_calcs += 1;
            }
        }
        if accepted == inputs.len() && r.idle() {
            break;
        }
    }
    assert!(
        c_calcs > 0 && frozen > 0,
        "c_calcs={c_calcs} frozen={frozen}"
    );
    assert!(r.idle());
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
