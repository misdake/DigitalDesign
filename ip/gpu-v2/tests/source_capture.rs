use gpu_v2::frontend::{sim::runtime::ProducerPort, source_capture::SourceProducerEdge};
use gpu_v2::{
    frontend::{
        ports::Slot,
        source_capture::{Controller, Event, Task, TASK_CREDITS},
    },
    vertex::ports::Transformed,
};

fn checked<T>(c: &mut Controller, ce: bool, f: impl FnOnce(&mut SourceProducerEdge<'_>) -> T) -> T {
    let permit = c.prepare_producer_edge(ce);
    c.step(ce, None).unwrap();
    f(&mut c.bind_producer_edge(permit).unwrap())
}
fn empty_checked() -> Controller {
    Controller::new(std::array::from_fn(|_| Slot::default()), 7, 1000, 10).unwrap()
}

#[test]
fn checked_producer_clock_and_issuer_permits_are_single_use() {
    let mut a = empty_checked();
    let permit = a.prepare_producer_edge(true);
    assert!(
        a.bind_producer_edge(permit).is_err(),
        "missing source clock"
    );
    let stale = a.prepare_producer_edge(true);
    a.step(true, None).unwrap();
    a.step(true, None).unwrap();
    assert!(a.bind_producer_edge(stale).is_err());
    let mut b = empty_checked();
    let foreign = a.prepare_producer_edge(true);
    b.step(true, None).unwrap();
    assert!(b.bind_producer_edge(foreign).is_err());
    let one = a.prepare_producer_edge(true);
    let duplicate = a.prepare_producer_edge(true);
    a.step(true, None).unwrap();
    assert!(a.bind_producer_edge(one).is_ok());
    assert!(a.bind_producer_edge(duplicate).is_err());
    let wrong_ce = a.prepare_producer_edge(true);
    a.step(false, None).unwrap();
    assert!(a.bind_producer_edge(wrong_ce).is_err());
    checked(&mut a, false, |p| {
        assert!(p.free_slot().is_none());
        assert!(p.allocate(0, 3).is_err());
    });
}

#[test]
fn checked_rows_reject_before_mutation_and_publish_on_later_edge() {
    let mut c = empty_checked();
    let epoch = checked(&mut c, true, |p| p.allocate(0, 1).unwrap());
    let before = c.slots().clone();
    assert!(c
        .publish_completed_vertex(0, epoch, 0, &vertices()[0])
        .is_err());
    assert!(c.finish_production(0, epoch).is_err());
    assert!(
        c.allocate(1).is_err(),
        "no concurrent atomic fixture writer"
    );
    assert!(c.abort_production(0, epoch).is_err());
    assert!(c.submit(task(epoch)).is_err());
    assert!(c.seal(0, epoch).is_err());
    checked(&mut c, true, |p| {
        assert!(p.allocate(1, 1).is_err(), "one active writer");
        assert!(p.write_row(0, epoch + 1, 0, 0, 0).is_err());
        assert!(p.write_row(0, epoch, 1, 0, 0).is_err());
        assert!(p.write_row(0, epoch, 0, 7, 0).is_err());
        assert!(p.write_row(0, epoch, 0, 0, 1 << 32).is_err());
        for (row, width) in [32, 32, 32, 32, 36, 24, 16].into_iter().enumerate() {
            assert!(p.write_row(0, epoch, 0, row, 1 << width).is_err());
        }
        assert!(p.publish(0, epoch, 0).is_err());
        assert!(p.finish(0, epoch).is_err());
        assert!(p.release(0, epoch).is_err());
    });
    assert_eq!(c.slots(), &before);
    let rows = vertices()[0].rows();
    for row in [6, 2, 4, 0, 5, 1, 3] {
        checked(&mut c, true, |p| {
            p.write_row(0, epoch, 0, row, rows[row]).unwrap();
            assert!(p.write_row(0, epoch, 0, row, rows[row]).is_err());
            assert!(p.publish(0, epoch, 0).is_err());
        });
        checked(&mut c, true, |p| {
            assert!(p.write_row(0, epoch, 0, row, 0).is_err())
        });
    }
    checked(&mut c, true, |p| {
        p.publish(0, epoch, 0).unwrap();
        assert!(p.finish(0, epoch).is_err());
        assert!(
            p.submit_task(Task {
                vertices: [0; 3],
                ..task(epoch)
            })
            .is_err(),
            "new publication not old-ready"
        );
    });
    checked(&mut c, false, |p| assert!(p.finish(0, epoch).is_err()));
    checked(&mut c, true, |p| p.finish(0, epoch).unwrap());
    assert!(c.slots()[0].held && !c.slots()[0].producing);
    checked(&mut c, true, |p| {
        assert!(p.seal_after_admission(0, epoch, 0).is_err());
        let ticket = p
            .submit_task(Task {
                vertices: [0; 3],
                ..task(epoch)
            })
            .unwrap()
            .unwrap();
        assert!(p.seal_after_admission(0, epoch, ticket + 1).is_err());
        p.seal_after_admission(0, epoch, ticket).unwrap();
        assert!(p
            .submit_task(Task {
                vertices: [0; 3],
                ..task(epoch)
            })
            .is_err());
    });
    for _ in 0..24 {
        c.step(true, None).unwrap();
    }
    assert!(!c.slots()[0].held);
    assert!(c.snapshot().is_some(), "release does not ACK snapshot");
    let newer = checked(&mut c, true, |p| p.allocate(0, 1).unwrap());
    assert_eq!(newer, epoch + 1);
    checked(&mut c, true, |p| {
        assert!(p.write_row(0, epoch, 0, 0, 0).is_err())
    });
}

#[test]
fn checked_old_credit_and_release_cannot_be_borrowed_same_edge() {
    let (mut c, epoch) = producer();
    for _ in 0..4 {
        c.submit(task(epoch)).unwrap().unwrap();
    }
    let permit = c.prepare_producer_edge(true);
    c.step(true, None).unwrap();
    assert_eq!(c.queued(), 3);
    assert_eq!(
        c.bind_producer_edge(permit)
            .unwrap()
            .submit_task(task(epoch))
            .unwrap(),
        None
    );
    let mut c = empty_checked();
    let epoch = c.allocate(0).unwrap();
    c.publish_completed_vertex(0, epoch, 0, &vertices()[0])
        .unwrap();
    c.finish_production(0, epoch).unwrap();
    c.seal(0, epoch).unwrap();
    let permit = c.prepare_producer_edge(true);
    assert!(c
        .step(true, None)
        .unwrap()
        .contains(&Event::SourceReleased { slot: 0, epoch }));
    let mut edge = c.bind_producer_edge(permit).unwrap();
    assert_eq!(edge.free_slot(), Some(1));
    assert!(edge.allocate(0, 1).is_err());
    assert_eq!(
        checked(&mut c, true, |p| p.allocate(0, 1).unwrap()),
        epoch + 1
    );
}

#[test]
fn checked_capture_reads_published_rows_while_next_vertex_writes() {
    let mut c = empty_checked();
    let epoch = checked(&mut c, true, |p| p.allocate(0, 2).unwrap());
    for (row, data) in vertices()[0].rows().into_iter().enumerate() {
        checked(&mut c, true, |p| {
            p.write_row(0, epoch, 0, row, data).unwrap()
        });
    }
    checked(&mut c, true, |p| p.publish(0, epoch, 0).unwrap());
    checked(&mut c, true, |p| {
        p.submit_task(Task {
            vertices: [0; 3],
            ..task(epoch)
        })
        .unwrap()
        .unwrap()
    });
    let mut concurrent = 0;
    for (row, data) in vertices()[1].rows().into_iter().enumerate() {
        let permit = c.prepare_producer_edge(true);
        let events = c.step(true, None).unwrap();
        let mut p = c.bind_producer_edge(permit).unwrap();
        assert!(
            p.write_row(0, epoch, 0, row, 0).is_err(),
            "published addresses cannot be overwritten"
        );
        p.write_row(0, epoch, 1, row, data).unwrap();
        for event in events {
            if let Event::ReadIssue { row: read, .. } = event {
                assert!(read < 7 && read != 7 + row);
                concurrent += 1;
            }
        }
    }
    assert_eq!(concurrent, 7);
    checked(&mut c, true, |p| p.publish(0, epoch, 1).unwrap());
    checked(&mut c, true, |p| p.finish(0, epoch).unwrap());
}

fn vertices() -> [Transformed; 3] {
    std::array::from_fn(|i| Transformed {
        clip: [
            i as i32 * 32768 - 65536,
            32768 - i as i32 * 32768,
            -16384,
            65536,
        ],
        normal: [-2048, i as i16 * 512, 2047],
        uv: [i as u16 * 2000, 4095 - i as u16 * 1000],
        rgb565: 0x1234 ^ (i as u16 * 0x1001),
    })
}
fn producer() -> (Controller, u32) {
    let mut c = Controller::new(std::array::from_fn(|_| Slot::default()), 7, 1000, 100).unwrap();
    let epoch = c.allocate(0).unwrap();
    for (i, v) in vertices().iter().enumerate() {
        c.publish_completed_vertex(0, epoch, i, v).unwrap();
    }
    (c, epoch)
}
fn task(epoch: u32) -> Task {
    Task {
        triangle_id: 123,
        slot: 0,
        epoch,
        vertices: [2, 0, 1],
        context: 7,
    }
}

#[test]
fn capture_charges_twenty_one_reads_and_release_does_not_destroy_snapshot() {
    let (mut c, epoch) = producer();
    let ticket = c.submit(task(epoch)).unwrap().unwrap();
    c.finish_production(0, epoch).unwrap();
    c.seal(0, epoch).unwrap();
    let mut trace = Vec::new();
    for edge in 0..23 {
        let events = c.step(true, None).unwrap();
        assert!(
            events
                .iter()
                .filter(|e| matches!(e, Event::ReadIssue { .. }))
                .count()
                <= 1
        );
        if edge < 22 {
            assert!(c.snapshot().is_none());
            assert!(c.slots()[0].held);
        }
        trace.extend(events);
    }
    assert_eq!(
        trace
            .iter()
            .filter(|e| matches!(e, Event::ReadIssue { .. }))
            .count(),
        21
    );
    assert_eq!(
        trace
            .iter()
            .filter(|e| matches!(e, Event::ReadReturn { .. }))
            .count(),
        21
    );
    assert!(trace.contains(&Event::SourceCaptured {
        ticket,
        slot: 0,
        epoch
    }));
    assert!(trace.contains(&Event::SourceReleased { slot: 0, epoch }));
    assert!(!c.drained()); // geometry/fan still owns its snapshot
    let expected = [
        vertices()[2].clone(),
        vertices()[0].clone(),
        vertices()[1].clone(),
    ];
    assert_eq!(c.snapshot().unwrap().input().unwrap().vertices, expected);
    // Reuse the released source immediately, while its old snapshot remains.
    let next_epoch = c.allocate(0).unwrap();
    assert_eq!(next_epoch, epoch + 1);
    c.publish_completed_vertex(0, next_epoch, 0, &vertices()[1])
        .unwrap();
    for _ in 0..40 {
        assert!(c.step(true, None).unwrap().is_empty());
        assert_eq!(c.snapshot().unwrap().input().unwrap().vertices, expected);
    }
    assert!(c.step(true, Some(ticket + 1)).is_err());
    assert!(c.step(false, Some(ticket)).unwrap().is_empty());
    assert!(c.snapshot().is_some());
    assert_eq!(
        c.step(true, Some(ticket)).unwrap(),
        vec![Event::TriangleConsumed { ticket }]
    );
    assert!(c.drained());
    assert!(c.submit(task(epoch)).is_err());
}

#[test]
fn sealing_references_and_producer_completion_are_independent() {
    let (mut c, epoch) = producer();
    let first = c.submit(task(epoch)).unwrap().unwrap();
    c.seal(0, epoch).unwrap();
    assert!(c.submit(task(epoch)).is_err());
    for _ in 0..23 {
        c.step(true, None).unwrap();
    }
    assert!(c.snapshot().is_some() && c.slots()[0].held && c.slots()[0].producing);
    c.finish_production(0, epoch).unwrap();
    assert_eq!(
        c.step(true, None).unwrap(),
        vec![Event::SourceReleased { slot: 0, epoch }]
    );
    assert_eq!(c.snapshot().unwrap().ticket, first);

    let (mut c, epoch) = producer();
    let a = c.submit(task(epoch)).unwrap().unwrap();
    let b = c
        .submit(Task {
            vertices: [1; 3],
            ..task(epoch)
        })
        .unwrap()
        .unwrap();
    c.finish_production(0, epoch).unwrap();
    c.seal(0, epoch).unwrap();
    for _ in 0..23 {
        c.step(true, None).unwrap();
    }
    assert!(c.slots()[0].held); // second descriptor still references the source
    c.step(true, Some(a)).unwrap();
    for _ in 0..22 {
        c.step(true, None).unwrap();
    }
    assert!(!c.slots()[0].held);
    assert_eq!(c.snapshot().unwrap().ticket, b);
    assert_eq!(
        c.snapshot().unwrap().input().unwrap().vertices,
        std::array::from_fn(|_| vertices()[1].clone())
    );
}

#[test]
fn finite_credits_and_ce_stalls_hold_payload_and_order() {
    let (mut c, epoch) = producer();
    assert!(c.step(true, Some(0)).is_err());
    let mut tickets = Vec::new();
    for _ in 0..TASK_CREDITS {
        tickets.push(c.submit(task(epoch)).unwrap().unwrap());
    }
    assert_eq!(c.submit(task(epoch)).unwrap(), None);
    c.step(true, None).unwrap();
    tickets.push(c.submit(task(epoch)).unwrap().unwrap());
    assert_eq!(c.queued(), 4);
    assert_eq!(c.submit(task(epoch)).unwrap(), None);
    for _ in 0..17 {
        assert!(c.step(false, None).unwrap().is_empty());
    }
    c.finish_production(0, epoch).unwrap();
    c.seal(0, epoch).unwrap();
    let mut retired = Vec::new();
    let mut issues = 1; // first edge above
    for wall in 0..300 {
        let ce = wall % 3 != 0;
        let before = c.snapshot().cloned();
        let consumed = before.as_ref().map(|s| s.ticket);
        for event in c.step(ce, consumed).unwrap() {
            if let Event::TriangleConsumed { ticket } = event {
                retired.push(ticket);
            }
            if let Event::ReadIssue { .. } = event {
                issues += 1;
            }
        }
        if !ce {
            assert_eq!(c.snapshot().cloned(), before);
        }
        if c.drained() {
            break;
        }
    }
    assert_eq!(retired, tickets);
    assert_eq!(issues, 5 * 21);
    assert!(c.drained() && !c.slots()[0].held);
}

#[test]
fn cancellation_drains_registered_read_without_success_publication() {
    let (mut c, epoch) = producer();
    c.submit(task(epoch)).unwrap();
    c.submit(task(epoch)).unwrap();
    assert!(matches!(
        c.step(true, None).unwrap().as_slice(),
        [Event::ReadIssue { .. }]
    ));
    c.cancel();
    assert!(c.submit(task(epoch)).is_err());
    assert!(c.step(false, None).unwrap().is_empty());
    assert!(c.slots()[0].held);
    c.seal(0, epoch).unwrap();
    c.finish_production(0, epoch).unwrap();
    let events = c.step(true, None).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::ReadReturn { .. }))
            .count(),
        1
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::SourceCaptured { .. })));
    assert!(events.contains(&Event::Cancelled));
    assert!(events.contains(&Event::SourceReleased { slot: 0, epoch }));
    assert!(c.drained());
    assert!(c.step(true, None).unwrap().is_empty());
    assert!(c.allocate(0).is_err());
}

#[test]
fn malformed_rows_context_and_unpublished_sources_are_rejected() {
    for row in 0..7 {
        let mut rows = vertices()[0].rows();
        rows[row] |= 1 << [32, 32, 32, 32, 36, 24, 16][row];
        assert!(Transformed::from_rows(rows).is_err());
    }
    assert_eq!(
        Transformed::from_rows(vertices()[0].rows()).unwrap(),
        vertices()[0]
    );
    let (mut c, epoch) = producer();
    assert!(c
        .submit(Task {
            context: 8,
            ..task(epoch)
        })
        .is_err());
    assert!(c
        .submit(Task {
            vertices: [0, 1, 3],
            ..task(epoch)
        })
        .is_err());
    assert!(c
        .submit(Task {
            vertices: [64, 1, 2],
            ..task(epoch)
        })
        .is_err());
    let mut slots = c.slots().clone();
    slots[0].rows[0] |= 1 << 35;
    let mut malformed = Controller::new(slots, 7, 100, 10).unwrap();
    malformed.submit(task(epoch)).unwrap();
    malformed.finish_production(0, epoch).unwrap();
    malformed.seal(0, epoch).unwrap();
    for _ in 0..22 {
        malformed.step(true, None).unwrap();
    }
    assert!(malformed.step(true, None).is_err());
    assert!(malformed.slots()[0].held && malformed.snapshot().is_none());
    malformed.cancel();
    malformed.step(true, None).unwrap();
    assert!(malformed.drained() && !malformed.slots()[0].held);
}

#[test]
fn completed_frontend_rows_feed_existing_triangle_oracle_without_a_second_reference() {
    use gpu_v2::{
        command_processor::ports::Command,
        frontend::{ports::Input, sim},
        scratchpad::ports::DmaDescriptor,
        triangle,
        vertex::ports::{Context, PackedVertex},
    };
    let context = Context {
        base: [-32768, -32768, 0],
        grid_shift: 7,
        ..Context::default()
    };
    let mut memory = Vec::new();
    for (i, xyz) in [[0, 0, 512], [512, 0, 512], [0, 512, 512]]
        .into_iter()
        .enumerate()
    {
        let packed = PackedVertex::encode(
            xyz,
            [0, 0, 127],
            [i as u16 * 1000, 4095],
            0xffff - i as u16 * 100,
        )
        .unwrap();
        for word in packed.0 {
            memory.extend(word.to_le_bytes());
        }
    }
    memory.resize(40, 0x5a);
    let input = Input {
        commands: vec![
            Command::Dma(DmaDescriptor {
                physical_addr: 0,
                scratchpad_addr: 0,
                byte_count: 40,
                completion_token: 0,
            }),
            Command::Wait(0),
            Command::Draw {
                region: 0,
                byte_offset: 0,
                vertices: 3,
                context,
            },
            Command::Fence,
        ],
        memory_base: 0,
        memory,
    };
    let frontend = sim::timed::run(&input, sim::timed::Config::default(), &[20, 21, 22]).unwrap();
    assert!(frontend.fault.is_none());
    let output = &frontend.outputs[0];
    let mut c = Controller::new(frontend.slots, 7, 100, 10).unwrap();
    c.submit(Task {
        triangle_id: 456,
        slot: output.slot,
        epoch: output.epoch,
        vertices: [0, 1, 2],
        context: 7,
    })
    .unwrap();
    c.seal(output.slot, output.epoch).unwrap();
    for _ in 0..23 {
        c.step(true, None).unwrap();
    }
    let snapshot = c.snapshot().unwrap();
    let captured = snapshot.input().unwrap();
    let expected: [Transformed; 3] = output.vertices.clone().try_into().unwrap();
    assert_eq!(captured.vertices, expected);
    let config = triangle::ports::Config::default();
    let report = triangle::sim::oracle::run(&captured, config).unwrap();
    assert!(!report.triangles.is_empty());
    assert!(!c.slots()[output.slot].held && !c.drained());
    // Source release is earlier than this final-use consumer acknowledgment.
    let ticket = snapshot.ticket;
    c.step(true, Some(ticket)).unwrap();
    assert!(c.drained());
}

#[test]
fn cycle_and_task_bounds_are_enforced() {
    let mut c = Controller::new(std::array::from_fn(|_| Slot::default()), 7, 2, 1).unwrap();
    let epoch = c.allocate(0).unwrap();
    c.publish_completed_vertex(0, epoch, 0, &vertices()[0])
        .unwrap();
    let task = Task {
        vertices: [0; 3],
        ..task(epoch)
    };
    c.submit(task).unwrap();
    assert!(c.submit(task).is_err());
    c.step(false, None).unwrap();
    c.step(false, None).unwrap();
    assert!(c.step(false, None).is_err());
}

#[test]
fn failed_producer_with_no_published_vertex_requires_explicit_drain_ack() {
    let mut c = Controller::new(std::array::from_fn(|_| Slot::default()), 7, 100, 10).unwrap();
    let epoch = c.allocate(1).unwrap();
    assert!(c.abort_production(1, epoch).is_err());
    c.seal(1, epoch).unwrap();
    c.cancel();
    c.step(true, None).unwrap();
    assert!(c.slots()[1].held && c.slots()[1].producing);
    assert!(c.finish_production(1, epoch).is_err());
    c.abort_production(1, epoch).unwrap();
    assert!(c.abort_production(1, epoch).is_err());
    assert!(c.step(false, None).unwrap().is_empty());
    assert!(c.slots()[1].held);
    assert_eq!(
        c.step(true, None).unwrap(),
        vec![Event::SourceReleased { slot: 1, epoch }]
    );
    assert!(!c.slots()[1].held && c.drained());
}
