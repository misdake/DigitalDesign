use gpu_v2::frontend::sim::timed;
use gpu_v2::vertex::ports::*;
fn sample_program() -> gpu_v2::frontend::ports::Input {
    use gpu_v2::{command_processor::ports::Command, scratchpad::ports::DmaDescriptor};
    let vertices = [
        PackedVertex::encode([12, 34, 56], [-127, 0, 127], [0, 4095], 0x1234).unwrap(),
        PackedVertex::encode([1023, 1, 500], [1, -128, 90], [2000, 1200], 0xabcd).unwrap(),
        PackedVertex::encode([0, 0, 0], [0, 0, 0], [4095, 0], 0xf81f).unwrap(),
    ];
    let mut memory = Vec::new();
    for v in &vertices {
        for cell in v.0 {
            memory.extend(cell.to_le_bytes());
        }
    }
    memory.resize(40, 0xa5);
    gpu_v2::frontend::ports::Input {
        commands: vec![
            Command::Dma(DmaDescriptor {
                physical_addr: 0x1000,
                scratchpad_addr: 0,
                byte_count: 40,
                completion_token: 0,
            }),
            Command::Wait(0),
            Command::Draw {
                region: 0,
                byte_offset: 0,
                vertices: 3,
                context: Context::default(),
            },
            Command::Fence,
        ],
        memory_base: 0x1000,
        memory,
    }
}

fn digest(r: &timed::Report) -> u64 {
    let text = format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        r.cycles,
        r.core_cycles,
        r.records,
        r.events,
        r.outputs,
        r.slots,
        r.scratchpad,
        r.transfers,
        r.fault,
        r.fence
    );
    let mut hash = 0xcbf29ce484222325_u64;
    for b in text.bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    hash
}
fn cases() -> Vec<(gpu_v2::frontend::ports::Input, timed::Config, Vec<u64>)> {
    use gpu_v2::{
        command_processor::ports::Command,
        scratchpad::ports::{DmaDescriptor, REGION_BYTES},
    };
    let input = sample_program();
    let mut multi = input.clone();
    multi.commands.insert(
        1,
        Command::Dma(DmaDescriptor {
            physical_addr: 0x1000,
            scratchpad_addr: REGION_BYTES,
            byte_count: 40,
            completion_token: 1,
        }),
    );
    multi.commands.pop();
    multi.commands.extend([
        Command::Wait(1),
        Command::Draw {
            region: 1,
            byte_offset: 0,
            vertices: 3,
            context: Context::default(),
        },
        Command::Release { slot: 0, epoch: 1 },
        Command::Release { slot: 1, epoch: 1 },
        Command::Fence,
    ]);
    let mut fault = input.clone();
    fault.memory.truncate(24);
    vec![
        (input.clone(), timed::Config::default(), vec![]),
        (
            input,
            timed::Config {
                dma_first_latency: 3,
                dma_gap: 2,
                ..Default::default()
            },
            (1..14).chain(90..100).collect(),
        ),
        (multi, timed::Config::default(), (45..50).collect()),
        (fault, timed::Config::default(), (1..14).collect()),
    ]
}

#[test]
fn frozen_legacy_baseline_actions_events_payload_and_final_storage() {
    // Original pre-B1 driver replayed with the current S12F10 row contract.
    // The independent source snapshot/replay is retained in target/b1-integration-20261006.
    let expected = [
        0xf695c1656af6d6a3,
        0x82c169b7e8d326e2,
        0x91fc98c8a387fea1,
        0xbbc38eb643afcc98,
    ];
    for ((input, config, pauses), expected) in cases().into_iter().zip(expected) {
        let r = timed::run(&input, config, &pauses).unwrap();
        assert_eq!(digest(&r), expected);
    }
}

use gpu_v2::frontend::{
    ports::Slot,
    sim::runtime::{ProducerPort, Profile, Sequencer},
    source_capture::Controller,
};
#[test]
fn persistent_current_plan_is_dropped_after_each_publication() {
    let input = sample_program();
    let mut engine = Sequencer::new(
        &input,
        timed::Config::default(),
        Profile::ConnectedTriangles,
    )
    .unwrap();
    let mut src = Controller::new(std::array::from_fn(|_| Slot::default()), 7, 20000, 6).unwrap();
    let mut created = 0;
    let mut published = 0;
    let mut finish = None;
    let mut last_publish = None;
    for wall in 0..20000 {
        let ce = wall % 31 > 3;
        let permit = src.prepare_producer_edge(ce);
        src.step(ce, None).unwrap();
        let out = engine
            .step(ce, true, &mut src.bind_producer_edge(permit).unwrap())
            .unwrap();
        assert!(engine.retained_plan_count() <= 1);
        if out.plan_created {
            created += 1;
            assert!(engine.current_plan().is_some());
        }
        for r in out.records {
            match r.action {
                timed::Action::Publish { .. } => {
                    published += 1;
                    last_publish = Some(wall);
                    assert!(engine.current_plan().is_none());
                }
                timed::Action::DrawDone { .. } => {
                    finish = Some(wall);
                    assert!(last_publish.unwrap() < wall);
                }
                _ => {}
            }
        }
        if engine.done() {
            break;
        }
        assert!(wall + 1 < 20000);
    }
    assert_eq!((created, published), (3, 3));
    assert!(finish.is_some() && engine.fence());
    assert!(
        !src.slots()[0].producing && src.slots()[0].held,
        "frontend FENCE does not release source"
    );
}
#[test]
fn connected_profile_and_watchdog_reject_unsupported_topology() {
    use gpu_v2::command_processor::ports::Command;
    let mut input = sample_program();
    for hardware in [
        gpu_v2::vertex::sim::timed::Hardware {
            wide: 0,
            ..Default::default()
        },
        gpu_v2::vertex::sim::timed::Hardware {
            narrow: 5,
            ..Default::default()
        },
        gpu_v2::vertex::sim::timed::Hardware {
            matrix_read_ports: 3,
            ..Default::default()
        },
        gpu_v2::vertex::sim::timed::Hardware {
            max_cycles: 20001,
            ..Default::default()
        },
    ] {
        assert!(Sequencer::new(
            &input,
            timed::Config {
                hardware,
                ..Default::default()
            },
            Profile::ConnectedTriangles
        )
        .is_err());
    }
    input.commands.push(Command::Release { slot: 0, epoch: 1 });
    assert!(Sequencer::new(
        &input,
        timed::Config::default(),
        Profile::ConnectedTriangles
    )
    .is_err());
    input.commands.pop();
    if let Command::Draw { vertices, .. } = &mut input.commands[2] {
        *vertices = 4;
    }
    assert!(Sequencer::new(
        &input,
        timed::Config::default(),
        Profile::ConnectedTriangles
    )
    .is_err());
    input.commands = vec![Command::Wait(0)];
    let mut engine = Sequencer::new(
        &input,
        timed::Config {
            max_cycles: 2,
            ..Default::default()
        },
        Profile::ConnectedTriangles,
    )
    .unwrap();
    let mut c = Controller::new(std::array::from_fn(|_| Slot::default()), 7, 4, 1).unwrap();
    for _ in 0..2 {
        let permit = c.prepare_producer_edge(true);
        c.step(true, None).unwrap();
        engine
            .step(true, true, &mut c.bind_producer_edge(permit).unwrap())
            .unwrap();
    }
    let permit = c.prepare_producer_edge(true);
    c.step(true, None).unwrap();
    assert!(engine
        .step(true, true, &mut c.bind_producer_edge(permit).unwrap())
        .is_err());
}
#[test]
fn failed_producer_edge_is_terminal_and_cannot_repeat_a_row() {
    struct Reject;
    impl ProducerPort for Reject {
        fn free_slot(&self) -> Option<usize> {
            Some(0)
        }
        fn allocate(&mut self, _: usize, _: usize) -> Result<u32, String> {
            Err("injected allocate fault".into())
        }
        fn write_row(
            &mut self,
            _: usize,
            _: u32,
            _: usize,
            _: usize,
            _: u64,
        ) -> Result<(), String> {
            unreachable!()
        }
        fn publish(&mut self, _: usize, _: u32, _: usize) -> Result<(), String> {
            unreachable!()
        }
        fn finish(&mut self, _: usize, _: u32) -> Result<(), String> {
            unreachable!()
        }
        fn release(&mut self, _: usize, _: u32) -> Result<(), String> {
            unreachable!()
        }
    }
    let input = sample_program();
    let mut e = Sequencer::new(
        &input,
        timed::Config::default(),
        Profile::ConnectedTriangles,
    )
    .unwrap();
    let mut rejected = false;
    for _ in 0..100 {
        if e.step(true, true, &mut Reject).is_err() {
            rejected = true;
            break;
        }
    }
    assert!(rejected);
    assert!(e
        .step(true, true, &mut Reject)
        .err()
        .unwrap()
        .contains("poisoned"));
}

#[test]
fn fixture_seeded_five_sources_hold_one_actual_frontend_task_for_retry() {
    use gpu_v2::frontend::source_capture::{Event, Task};
    use gpu_v2::vertex::sim::oracle;
    let input = sample_program();
    let mut c = Controller::new(std::array::from_fn(|_| Slot::default()), 7, 1000, 6).unwrap();
    let seed_epoch = c.allocate(0).unwrap();
    // Explicit atomic fixture: five repeated references to one valid triangle.
    // This is credit-control evidence, not a clean DRAW throughput claim.
    for v in 0..3 {
        let offset = v * 12;
        let packed = PackedVertex(std::array::from_fn(|i| {
            u32::from_le_bytes(
                input.memory[offset + i * 4..offset + i * 4 + 4]
                    .try_into()
                    .unwrap(),
            )
        }));
        c.publish_completed_vertex(
            0,
            seed_epoch,
            v,
            &oracle::run(&Context::default(), packed, &Default::default())
                .unwrap()
                .output,
        )
        .unwrap();
    }
    c.finish_production(0, seed_epoch).unwrap();
    let seeded = Task {
        triangle_id: 123,
        slot: 0,
        epoch: seed_epoch,
        vertices: [0, 1, 2],
        context: 7,
    };
    for ticket in 0..4 {
        assert_eq!(c.submit(seeded).unwrap(), Some(ticket));
    }
    let mut e = Sequencer::new(
        &input,
        timed::Config {
            max_cycles: 1000,
            ..Default::default()
        },
        Profile::ConnectedTriangles,
    )
    .unwrap();
    let mut pending = None;
    let mut retries = 0;
    let mut fifth = false;
    let mut admitted = false;
    let mut peak = 4;
    let mut five_owned = false;
    let mut captured = 0;
    let mut consumed = 0;
    let mut finish_wall = None;
    for wall in 0..1000 {
        let ce = wall % 29 >= 3;
        let permit = c.prepare_producer_edge(ce);
        let consume = if ce && wall >= 200 {
            c.snapshot().map(|s| s.ticket)
        } else {
            None
        };
        let events = c.step(ce, consume).unwrap();
        captured += events
            .iter()
            .filter(|x| matches!(x, Event::SourceCaptured { .. }))
            .count();
        consumed += events
            .iter()
            .filter(|x| matches!(x, Event::TriangleConsumed { .. }))
            .count();
        let epoch = c.slots()[1].epoch;
        let old_pending = pending;
        let mut p = c.bind_producer_edge(permit).unwrap();
        let out = e.step(ce, pending.is_none(), &mut p).unwrap();
        if ce && !fifth && wall > 0 {
            if let Some(ticket) = p.submit_task(seeded).unwrap() {
                assert_eq!(ticket, 4);
                p.seal_after_admission(0, seed_epoch, ticket).unwrap();
                fifth = true;
            }
        } else if ce {
            if let Some(task) = pending {
                if let Some(ticket) = p.submit_task(task).unwrap() {
                    assert_eq!(ticket, 5);
                    p.seal_after_admission(1, epoch, ticket).unwrap();
                    pending = None;
                    admitted = true;
                } else {
                    retries += 1;
                    assert_eq!(pending, old_pending);
                }
            }
        }
        for record in out.records {
            if matches!(record.action, timed::Action::DrawDone { .. }) {
                assert!(pending.is_none());
                finish_wall = Some(wall);
                pending = Some(Task {
                    triangle_id: 7,
                    slot: 1,
                    epoch,
                    vertices: [0, 1, 2],
                    context: 7,
                });
            }
        }
        peak = peak.max(c.queued());
        five_owned |= fifth && c.queued() == 4 && !c.drained();
        if admitted && c.drained() && c.slots().iter().all(|s| !s.held) {
            break;
        }
        assert!(wall + 1 < 1000);
    }
    assert!(fifth && five_owned && admitted && retries > 0 && finish_wall.unwrap() < 200);
    assert_eq!((peak, captured, consumed), (4, 6, 6));
    assert!(e.fence() && e.done() && c.drained());
}
