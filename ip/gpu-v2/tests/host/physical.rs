//! Framework acceptance remains in gpu-v2; no unrelated crate tests are run.
use audited::{flow::*, lifecycle, physical::*, Fixed, FrameReport, Model, Operation, PortShape};
use std::collections::BTreeMap;
const MAX_CYCLE: u64 = 100;
const MAX_EVENTS: usize = 128;
fn port(read: bool, write: bool, latency: u64) -> MemoryPort {
    MemoryPort {
        read,
        write,
        read_latency: latency,
        write_latency: 1,
        initiation_interval: 1,
    }
}
fn bank(name: &str, kind: RamKind, ports: Vec<MemoryPort>) -> MemoryBank {
    MemoryBank {
        name: name.into(),
        kind,
        width: 18,
        depth: 1024,
        ports,
        collision: ReadDuringWrite::Forbidden,
    }
}
fn slice(bank: usize, width: u32) -> MemorySlice {
    MemorySlice {
        bank,
        base_row: 0,
        bit_offset: 0,
        source_low: 0,
        width,
    }
}
fn ram_frame(read_first: bool) -> (FrameReport, Vec<Timing>, Vec<MemoryAccess>) {
    let mut model = Model::numerical();
    let ram = model
        .ram::<8, 0, false>(
            "ram",
            2,
            PortShape {
                read_ports: 2,
                write_ports: 2,
                read_latency: 1,
                max_reads_per_frame: 16,
                max_writes_per_frame: 16,
            },
        )
        .unwrap();
    let f = model.compute("memory semantics", MAX_EVENTS).unwrap();
    f.write(ram.at::<0>(), Fixed::constant::<17>()).unwrap();
    let value;
    if read_first {
        value = f.read(ram.at::<0>()).unwrap();
        f.write(ram.at::<0>(), Fixed::constant::<92>()).unwrap();
    } else {
        f.write(ram.at::<0>(), Fixed::constant::<92>()).unwrap();
        value = f.read(ram.at::<0>()).unwrap();
    }
    f.publish("read", value).unwrap();
    let report = f.finish();
    report.audit().unwrap();
    assert_eq!(report.outputs[0].raw, if read_first { 17 } else { 92 });
    let mut writes = 0;
    let mut accesses = Vec::new();
    let times = report
        .events
        .iter()
        .map(|e| match e.operation {
            Operation::Write { .. } => {
                let issue = if writes == 0 { 0 } else { 3 };
                writes += 1;
                accesses.push(MemoryAccess {
                    event: e.id,
                    copy: 0,
                    ports: vec![1],
                });
                Timing {
                    issue,
                    ready: issue + 1,
                }
            }
            Operation::Read { .. } => {
                accesses.push(MemoryAccess {
                    event: e.id,
                    copy: 0,
                    ports: vec![0],
                });
                Timing { issue: 3, ready: 4 }
            }
            Operation::Publish(_) => Timing {
                issue: 10,
                ready: 10,
            },
            _ => Timing { issue: 0, ready: 0 },
        })
        .collect();
    (report, times, accesses)
}
fn layout() -> MemoryLayout {
    MemoryLayout {
        banks: vec![bank(
            "memory",
            RamKind::Bsram,
            vec![port(true, false, 1), port(false, true, 1)],
        )],
        placements: vec![MemoryPlacement {
            memory: 0,
            copies: vec![MemoryCopy {
                slices: vec![slice(0, 8)],
            }],
        }],
    }
}

#[test]
fn dsp_packing_accounts_half_slots_but_rejects_mixed_kinds() {
    let inventory = DspInventory::pack(
        4,
        &[
            (DspMode::Multiply9, 7, 3, 1),
            (DspMode::Multiply18, 7, 3, 1),
            (DspMode::PairMultiplyAdd, 1, 4, 1),
        ],
    )
    .unwrap();
    assert_eq!(
        inventory.audit().unwrap(),
        DspUsage {
            macros: 7,
            tiles: 4,
            multiplier_half_slots: 25
        }
    );
    let mut mixed = inventory.clone();
    let large = mixed
        .instances
        .iter()
        .position(|d| d.mode == DspMode::Multiply18)
        .unwrap();
    mixed.instances[large].tile = 0;
    mixed.instances[large].macro_index = 0;
    assert!(mixed.audit().is_err());
    assert!(DspInventory::pack(
        3,
        &[
            (DspMode::Multiply9, 7, 3, 1),
            (DspMode::Multiply18, 7, 3, 1),
            (DspMode::PairMultiplyAdd, 1, 4, 1)
        ]
    )
    .is_err());
}
#[test]
fn independent_alu_preadd_and_wide_tile_cannot_share_incompatible_sites() {
    for mode in [DspMode::Alu54, DspMode::PreAdd18, DspMode::Multiply36] {
        let mut d =
            DspInventory::pack(2, &[(mode, 1, 3, 1), (DspMode::Multiply18, 1, 3, 1)]).unwrap();
        d.instances[1].tile = d.instances[0].tile;
        d.instances[1].macro_index = d.instances[0].macro_index;
        assert!(d.audit().is_err());
    }
    let mut d = DspInventory::pack(
        2,
        &[
            (DspMode::Multiply36, 1, 3, 1),
            (DspMode::Multiply18, 1, 3, 1),
        ],
    )
    .unwrap();
    d.instances[1].tile = 0;
    d.instances[1].macro_index = 1;
    assert!(d.audit().is_err());
}
#[test]
fn dsp_checks_operand_width_latency_and_cross_period_spacing() {
    let d = DspInventory::pack(1, &[(DspMode::Multiply9, 1, 3, 2)]).unwrap();
    let mut issues = vec![
        DspIssue {
            instance: 0,
            issue: 0,
            ready: 3,
            work: DspWork::Multiply {
                a_bits: 9,
                b_bits: 8,
            },
        },
        DspIssue {
            instance: 0,
            issue: 7,
            ready: 10,
            work: DspWork::Multiply {
                a_bits: 9,
                b_bits: 8,
            },
        },
    ];
    d.audit_issues(&issues, None, MAX_CYCLE).unwrap();
    assert!(d.audit_issues(&issues, Some(8), MAX_CYCLE).is_err()); // phase 7 -> 0 gap is one
    issues[1].issue = 4;
    issues[1].ready = 7;
    d.audit_issues(&issues, Some(8), MAX_CYCLE).unwrap();
    issues[0].work = DspWork::Multiply {
        a_bits: 10,
        b_bits: 8,
    };
    assert!(d.audit_issues(&issues, None, MAX_CYCLE).is_err());
    issues[0].work = DspWork::Multiply {
        a_bits: 9,
        b_bits: 8,
    };
    issues[0].ready = 4;
    assert!(d.audit_issues(&issues, None, MAX_CYCLE).is_err());
}
#[test]
fn read_during_write_obeys_old_or_new_value_contract() {
    for read_first in [true, false] {
        let (f, t, a) = ram_frame(read_first);
        let mut l = layout();
        assert!(l.audit_accesses(&f, &t, &a, MAX_CYCLE).is_err());
        l.banks[0].collision = if read_first {
            ReadDuringWrite::ReadFirst
        } else {
            ReadDuringWrite::WriteFirst
        };
        l.audit_accesses(&f, &t, &a, MAX_CYCLE).unwrap();
        l.banks[0].collision = if read_first {
            ReadDuringWrite::WriteFirst
        } else {
            ReadDuringWrite::ReadFirst
        };
        assert!(l.audit_accesses(&f, &t, &a, MAX_CYCLE).is_err());
    }
}
#[test]
fn shared_rw_port_and_same_address_writes_are_rejected() {
    let (f, mut t, mut a) = ram_frame(true);
    let mut l = layout();
    l.banks[0].collision = ReadDuringWrite::ReadFirst;
    l.banks[0].ports = vec![port(true, true, 1)];
    for access in &mut a {
        access.ports = vec![0]
    }
    assert!(l.audit_accesses(&f, &t, &a, MAX_CYCLE).is_err());
    l.banks[0].ports = vec![
        port(true, false, 1),
        port(false, true, 1),
        port(false, true, 1),
    ];
    let mut writes = 0;
    for access in &mut a {
        if matches!(f.events[access.event].operation, Operation::Write { .. }) {
            t[access.event] = Timing { issue: 3, ready: 4 };
            access.ports = vec![1 + writes];
            writes += 1
        }
    }
    assert!(l.audit_accesses(&f, &t, &a, MAX_CYCLE).is_err());
}
#[test]
fn mirrored_ram_writes_must_reach_every_copy() {
    let (f, t, mut a) = ram_frame(true);
    let mut l = layout();
    l.banks[0].collision = ReadDuringWrite::ReadFirst;
    let mut other = l.banks[0].clone();
    other.name = "mirror".into();
    l.banks.push(other);
    l.placements[0].copies.push(MemoryCopy {
        slices: vec![slice(1, 8)],
    });
    assert!(l.audit_accesses(&f, &t, &a, MAX_CYCLE).is_err());
    let mirrors: Vec<_> = a
        .iter()
        .filter(|a| matches!(f.events[a.event].operation, Operation::Write { .. }))
        .map(|a| MemoryAccess {
            copy: 1,
            ..a.clone()
        })
        .collect();
    a.extend(mirrors);
    l.audit_accesses(&f, &t, &a, MAX_CYCLE).unwrap();
    let u = l.audit(&f).unwrap();
    assert_eq!(u.logical_bits, 16);
    assert_eq!(u.replica_payload_bits, 32);
}
#[test]
fn layout_rejects_missing_bits_capacity_overlap_and_unplaced_stores() {
    let (f, _, _) = ram_frame(true);
    let mut l = layout();
    l.audit(&f).unwrap();
    l.placements[0].copies[0].slices[0].width = 7;
    assert!(l.audit(&f).is_err());
    l = layout();
    l.placements[0].copies[0].slices[0].base_row = 1023;
    assert!(l.audit(&f).is_err());
    l = layout();
    let duplicate = l.placements[0].copies[0].clone();
    l.placements[0].copies.push(duplicate);
    assert!(l.audit(&f).is_err());
    l = layout();
    l.placements.clear();
    assert!(l.audit(&f).is_err());
}
#[test]
fn ssram_async_read_is_legal_but_bsram_zero_latency_is_not() {
    let (f, mut t, a) = ram_frame(false);
    let mut l = layout();
    l.banks[0].kind = RamKind::Ssram;
    l.banks[0].collision = ReadDuringWrite::WriteFirst;
    l.banks[0].ports[0].read_latency = 0;
    for access in &a {
        if matches!(f.events[access.event].operation, Operation::Read { .. }) {
            t[access.event].ready = t[access.event].issue;
        }
    }
    l.audit_accesses(&f, &t, &a, MAX_CYCLE).unwrap();
    l.banks[0].kind = RamKind::Bsram;
    assert!(l.audit(&f).is_err());
}

#[test]
fn liveness_preserves_wiring_aliases_and_accounts_periodic_overlap() {
    let mut m = Model::numerical();
    let input = m.input::<8, 0, false>("input", &[17]).unwrap();
    let f = m.compute("alias", MAX_EVENTS).unwrap();
    let value = f.read(input.at::<0>()).unwrap();
    let alias = f.resize_exact::<16, 0, false>(value).unwrap();
    f.publish("value", alias).unwrap();
    let frame = f.finish();
    let times: Vec<_> = frame
        .events
        .iter()
        .map(|e| {
            if matches!(e.operation, Operation::Publish(_)) {
                Timing { issue: 4, ready: 4 }
            } else {
                Timing { issue: 0, ready: 0 }
            }
        })
        .collect();
    let finite = lifecycle::analyze(&frame, &times, 4, None, MAX_CYCLE).unwrap();
    assert_eq!(finite.intervals.len(), 1);
    assert_eq!(finite.peak_bits, 8);
    let periodic = lifecycle::analyze(&frame, &times, 4, Some(2), MAX_CYCLE).unwrap();
    assert_eq!(periodic.peak_bits, 24);
    assert!(periodic
        .audit_budget(&lifecycle::RegisterBudget {
            total_bits: 23,
            by_width: BTreeMap::new()
        })
        .is_err());
    periodic
        .audit_budget(&lifecycle::RegisterBudget {
            total_bits: 24,
            by_width: BTreeMap::new(),
        })
        .unwrap();
    assert!(lifecycle::analyze(&frame, &times, 3, None, MAX_CYCLE).is_err());
}
fn config(capacity: usize) -> FlowConfig {
    FlowConfig {
        context_banks: 2,
        fifo_capacity: capacity,
        id_bits: 16,
        epoch_bits: 8,
        payload_bits: 28,
        phase_interval: 2,
        max_steps: 32,
    }
}
fn load(machine: &mut FlowMachine, bank: usize) -> ContextLease {
    machine
        .tick(Tick {
            ce: true,
            load_context: Some(bank),
            ..Tick::default()
        })
        .unwrap()
        .lease
        .unwrap()
}
fn advance(machine: &mut FlowMachine) {
    machine
        .tick(Tick {
            ce: true,
            ..Tick::default()
        })
        .unwrap();
}
#[test]
fn flow_freezes_on_ce_and_preserves_context_until_commit() {
    let mut machine = FlowMachine::new(config(2)).unwrap();
    let lease = load(&mut machine, 0);
    advance(&mut machine);
    machine
        .tick(Tick {
            ce: true,
            accept: Some((1, lease)),
            ..Tick::default()
        })
        .unwrap();
    let held = machine.snapshot().clone();
    machine
        .tick(Tick {
            ce: false,
            load_context: Some(0),
            complete: Some(1),
            commit: true,
            ..Tick::default()
        })
        .unwrap();
    assert_eq!(*machine.snapshot(), held);
    assert!(machine
        .tick(Tick {
            ce: true,
            load_context: Some(0),
            ..Tick::default()
        })
        .is_err());
    assert_eq!(*machine.snapshot(), held);
    let record = machine
        .tick(Tick {
            ce: true,
            complete: Some(1),
            commit: true,
            load_context: Some(0),
            ..Tick::default()
        })
        .unwrap();
    assert_eq!(record.committed, Some(1));
    assert_eq!(record.lease.unwrap().epoch, 2);
    let mut trace = machine.finish();
    trace.audit().unwrap();
    trace.final_state.references[0] = 1;
    assert!(trace.audit().is_err());
}
#[test]
fn flow_rejects_credit_overrun_stale_lease_duplicate_id_and_out_of_order_commit() {
    let mut machine = FlowMachine::new(config(1)).unwrap();
    let lease = load(&mut machine, 0);
    advance(&mut machine);
    machine
        .tick(Tick {
            ce: true,
            accept: Some((1, lease)),
            ..Tick::default()
        })
        .unwrap();
    advance(&mut machine);
    let held = machine.snapshot().clone();
    assert!(machine
        .tick(Tick {
            ce: true,
            accept: Some((2, lease)),
            ..Tick::default()
        })
        .is_err());
    assert_eq!(*machine.snapshot(), held);
    assert!(machine
        .tick(Tick {
            ce: true,
            commit: true,
            ..Tick::default()
        })
        .is_err());
    machine
        .tick(Tick {
            ce: true,
            complete: Some(1),
            commit: true,
            accept: Some((2, lease)),
            ..Tick::default()
        })
        .unwrap();
    assert_eq!(machine.snapshot().queue.len(), 1);
    assert_eq!(machine.snapshot().peak_fifo_bits, 54);
    advance(&mut machine);
    assert!(machine
        .tick(Tick {
            ce: true,
            accept: Some((1, lease)),
            ..Tick::default()
        })
        .is_err());
    machine
        .tick(Tick {
            ce: true,
            complete: Some(2),
            commit: true,
            ..Tick::default()
        })
        .unwrap();
    let fresh = load(&mut machine, 0);
    assert_ne!(fresh, lease);
    assert_eq!(machine.snapshot().phase, 0);
    let fault = machine
        .tick(Tick {
            ce: true,
            accept: Some((3, lease)),
            ..Tick::default()
        })
        .unwrap_err();
    assert!(fault.debug_message().contains("stale context lease"));
}
#[test]
fn flow_step_limit_also_bounds_stalled_runs() {
    let mut c = config(1);
    c.max_steps = 2;
    let mut machine = FlowMachine::new(c).unwrap();
    for _ in 0..2 {
        machine.tick(Tick::default()).unwrap();
    }
    assert!(machine.tick(Tick::default()).is_err());
    machine.finish().audit().unwrap();
}

#[test]
fn flow_commits_only_complete_head_and_checks_width_limits() {
    let mut machine = FlowMachine::new(config(2)).unwrap();
    let lease = load(&mut machine, 0);
    advance(&mut machine);
    machine
        .tick(Tick {
            ce: true,
            accept: Some((1, lease)),
            ..Tick::default()
        })
        .unwrap();
    advance(&mut machine);
    machine
        .tick(Tick {
            ce: true,
            accept: Some((2, lease)),
            ..Tick::default()
        })
        .unwrap();
    machine
        .tick(Tick {
            ce: true,
            complete: Some(2),
            ..Tick::default()
        })
        .unwrap();
    let held = machine.snapshot().clone();
    assert!(machine
        .tick(Tick {
            ce: true,
            commit: true,
            ..Tick::default()
        })
        .is_err());
    assert_eq!(*machine.snapshot(), held);
    machine
        .tick(Tick {
            ce: true,
            complete: Some(1),
            commit: true,
            ..Tick::default()
        })
        .unwrap();
    machine
        .tick(Tick {
            ce: true,
            commit: true,
            ..Tick::default()
        })
        .unwrap();
    assert_eq!(machine.snapshot().committed, [1, 2]);
    machine.finish().audit().unwrap();
    let mut c = config(2);
    c.id_bits = 1;
    c.epoch_bits = 1;
    let mut machine = FlowMachine::new(c).unwrap();
    let lease = load(&mut machine, 0);
    advance(&mut machine);
    assert!(machine
        .tick(Tick {
            ce: true,
            accept: Some((2, lease)),
            ..Tick::default()
        })
        .unwrap_err()
        .debug_message()
        .contains("token identity width"));
    assert!(machine
        .tick(Tick {
            ce: true,
            load_context: Some(0),
            ..Tick::default()
        })
        .unwrap_err()
        .debug_message()
        .contains("context epoch width"));
}

#[test]
fn late_input_reads_still_require_capture_and_context_is_shared_across_pixels() {
    let mut m = Model::numerical();
    let input = m.input::<8, 0, false>("context", &[17]).unwrap();
    let f = m.compute("capture", MAX_EVENTS).unwrap();
    let value = f.read(input.at::<0>()).unwrap();
    f.publish("port", value).unwrap();
    f.publish("golden", value).unwrap();
    let frame = f.finish();
    let times: Vec<_> = frame
        .events
        .iter()
        .map(|e| {
            if matches!(e.operation, Operation::Read { .. }) {
                Timing { issue: 4, ready: 4 }
            } else {
                Timing { issue: 5, ready: 5 }
            }
        })
        .collect();
    let mut policy = lifecycle::LifetimePolicy {
        commit_cycle: 5,
        period: Some(2),
        max_cycle: MAX_CYCLE,
        invariant_inputs: vec![],
        retained_outputs: Some(vec!["port".into()]),
    };
    let per_pixel = lifecycle::analyze_bound_policy(&frame, &times, &[], &policy).unwrap();
    assert_eq!(per_pixel.intervals[0].start, 0);
    assert_eq!(per_pixel.peak_bits, 24);
    policy.invariant_inputs = vec![0];
    assert_eq!(
        lifecycle::analyze_bound_policy(&frame, &times, &[], &policy)
            .unwrap()
            .peak_bits,
        8
    );
    policy.retained_outputs = Some(vec!["missing".into()]);
    assert!(lifecycle::analyze_bound_policy(&frame, &times, &[], &policy).is_err());
}

#[test]
fn memory_hazards_include_all_prior_reads_even_when_read_order_is_changed() {
    let mut m = Model::numerical();
    let ram = m
        .ram::<8, 0, false>(
            "ram",
            2,
            PortShape {
                read_ports: 2,
                write_ports: 1,
                read_latency: 1,
                max_reads_per_frame: 16,
                max_writes_per_frame: 16,
            },
        )
        .unwrap();
    let f = m.compute("read hazards", MAX_EVENTS).unwrap();
    f.write(ram.at::<0>(), Fixed::constant::<17>()).unwrap();
    let a = f.read(ram.at::<0>()).unwrap();
    let b = f.read(ram.at::<0>()).unwrap();
    f.write(ram.at::<0>(), Fixed::constant::<92>()).unwrap();
    f.publish("a", a).unwrap();
    f.publish("b", b).unwrap();
    let frame = f.finish();
    let mut accesses = Vec::new();
    let mut reads = 0;
    let mut writes = 0;
    let times: Vec<_> = frame
        .events
        .iter()
        .map(|e| match e.operation {
            Operation::Read { .. } => {
                let issue = if reads == 0 { 9 } else { 1 };
                reads += 1;
                accesses.push(MemoryAccess {
                    event: e.id,
                    copy: 0,
                    ports: vec![0],
                });
                Timing {
                    issue,
                    ready: issue + 1,
                }
            }
            Operation::Write { .. } => {
                accesses.push(MemoryAccess {
                    event: e.id,
                    copy: 0,
                    ports: vec![1],
                });
                let issue = if writes == 0 { 0 } else { 5 };
                writes += 1;
                Timing {
                    issue,
                    ready: issue + 1,
                }
            }
            Operation::Publish(_) => Timing {
                issue: 12,
                ready: 12,
            },
            _ => Timing { issue: 0, ready: 0 },
        })
        .collect();
    assert!(layout()
        .audit_accesses(&frame, &times, &accesses, MAX_CYCLE)
        .unwrap_err()
        .debug_message()
        .contains("logical memory hazard reordered"));
}

trait FaultMessage {
    fn debug_message(&self) -> String;
}
impl FaultMessage for audited::Fault {
    fn debug_message(&self) -> String {
        format!("{self:?}")
    }
}

#[test]
fn split_rom_checks_each_slice_port_latency_and_periodic_self_spacing() {
    let mut m = Model::numerical();
    let rom = m
        .rom::<28, 0, false>(
            "split",
            &[Fixed::constant::<0x0abcdef>()],
            PortShape {
                read_ports: 1,
                write_ports: 0,
                read_latency: 1,
                max_reads_per_frame: 4,
                max_writes_per_frame: 0,
            },
        )
        .unwrap();
    let f = m.compute("split", MAX_EVENTS).unwrap();
    let value = f.read(rom.at::<0>()).unwrap();
    f.publish("out", value).unwrap();
    let frame = f.finish();
    let times: Vec<_> = frame
        .events
        .iter()
        .map(|e| {
            if matches!(e.operation, Operation::Read { .. }) {
                Timing { issue: 0, ready: 1 }
            } else {
                Timing { issue: 1, ready: 1 }
            }
        })
        .collect();
    let event = frame
        .events
        .iter()
        .find(|e| matches!(e.operation, Operation::Read { .. }))
        .unwrap()
        .id;
    let mut l = MemoryLayout {
        banks: vec![
            bank("lo", RamKind::Bsram, vec![port(true, false, 1)]),
            bank("hi", RamKind::Bsram, vec![port(true, false, 1)]),
        ],
        placements: vec![MemoryPlacement {
            memory: 0,
            copies: vec![MemoryCopy {
                slices: vec![
                    slice(0, 16),
                    MemorySlice {
                        source_low: 16,
                        ..slice(1, 12)
                    },
                ],
            }],
        }],
    };
    let a = vec![MemoryAccess {
        event,
        copy: 0,
        ports: vec![0, 0],
    }];
    l.audit_periodic_accesses(&frame, &times, &[], &a, 2, MAX_CYCLE)
        .unwrap();
    l.banks[1].ports[0].read_latency = 2;
    assert!(l.audit_accesses(&frame, &times, &a, MAX_CYCLE).is_err());
    l.banks[1].ports[0].read_latency = 1;
    l.banks[1].ports[0].initiation_interval = 3;
    l.audit_accesses(&frame, &times, &a, MAX_CYCLE).unwrap();
    assert!(l
        .audit_periodic_accesses(&frame, &times, &[], &a, 2, MAX_CYCLE)
        .is_err());
    let bad = vec![MemoryAccess {
        event,
        copy: 0,
        ports: vec![0],
    }];
    assert!(l.audit_accesses(&frame, &times, &bad, MAX_CYCLE).is_err());
}

#[test]
fn fused_certificate_preserves_provenance_and_rejects_internal_escapes() {
    let make_frame = |escape: bool| {
        let mut m = Model::numerical();
        let input = m.input::<8, 0, true>("operands", &[2, 3, 4, 5, 6]).unwrap();
        let f = m.compute("fusion", MAX_EVENTS).unwrap();
        let a = f.read(input.at::<0>()).unwrap();
        let b = f.read(input.at::<1>()).unwrap();
        let c = f.read(input.at::<2>()).unwrap();
        let d = f.read(input.at::<3>()).unwrap();
        let acc = f.read(input.at::<4>()).unwrap();
        let p = f
            .mul::<16, 0, true>(a, b, audited::ProductRoute::Native18)
            .unwrap();
        let q = f
            .mul::<16, 0, true>(c, d, audited::ProductRoute::Native18)
            .unwrap();
        let xy = f.add::<17, 0, true>(p, q).unwrap();
        let sum = f.add::<18, 0, true>(xy, acc).unwrap();
        f.publish("sum", sum).unwrap();
        if escape {
            f.publish("escape", p).unwrap();
        }
        f.finish()
    };
    let frame = make_frame(false);
    let root = frame
        .events
        .iter()
        .rev()
        .find(|e| matches!(e.operation, Operation::Add))
        .unwrap();
    let xy_id = frame.values[root.inputs[0]].producer;
    let products: Vec<_> = frame.events[xy_id]
        .inputs
        .iter()
        .map(|&v| frame.values[v].producer)
        .collect();
    let mut operands = frame.events[products[0]].inputs.clone();
    operands.extend(&frame.events[products[1]].inputs);
    operands.push(root.inputs[1]);
    let group = FusedGroup {
        result_event: root.id,
        absorbed_events: vec![products[0], products[1], xy_id],
        operands,
    };
    group.audit(&frame).unwrap();
    let deps = bound_dependencies(&frame, std::slice::from_ref(&group)).unwrap();
    for &event in &group.absorbed_events {
        assert_eq!(deps[event], vec![group.result_event]);
    }
    let mut forged = group.clone();
    forged.operands.swap(0, 1);
    assert!(forged.audit(&frame).is_err());
    let frame = make_frame(true);
    frame.audit().unwrap();
    assert!(group
        .audit(&frame)
        .unwrap_err()
        .debug_message()
        .contains("escapes"));
}
