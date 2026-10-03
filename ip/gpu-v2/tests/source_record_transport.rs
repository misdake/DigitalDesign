//! Acceptance driver for the production normal-path source/record connection.
//! Setup fan rows remain opaque fixture data, not geometry arithmetic.
//!
//! Scope: normal lifetime ownership only. Cancellation/fault composition is
//! excluded, so `SnapshotAborted` must never be mapped to a successful consumed
//! ack. Cancellation composition and terminal-drain ownership remain excluded.
//!
//! Transport fan rows are opaque finite fixture payloads generated independently
//! of either controller; they are not numeric setup output and do not drive
//! control.

#[path = "support/source_record_lease.rs"]
mod lease;
use gpu_v2::geometry::record_transport as record;

use gpu_v2::frontend::{ports::Slot, source_capture};
use gpu_v2::vertex::ports::Transformed;
use lease::{Lease, Out, CONTEXT};

const MAX_WALL: usize = 512;

fn transport() -> record::Controller {
    transport_with_context(CONTEXT)
}
fn transport_with_context(context: u64) -> record::Controller {
    record::Controller::new(
        context,
        record::Limits {
            wall_edges: 10_000,
            sources: 32,
            records: 128,
        },
    )
    .unwrap()
}

fn original_vertices() -> [Transformed; 3] {
    std::array::from_fn(|i| Transformed {
        clip: [
            i as i32 * 32768 - 65536,
            32768 - i as i32 * 32768,
            -16384,
            65536,
        ],
        normal: [i16::MIN, i as i16 * 8192, i16::MAX],
        uv: [i as u16 * 2000, 4095 - i as u16 * 1000],
        rgb565: 0x1234 ^ (i as u16 * 0x1001),
    })
}

fn replacement_vertices() -> [Transformed; 3] {
    std::array::from_fn(|i| Transformed {
        clip: [12345 + i as i32 * 111, -54321 - i as i32 * 7, 20000, 65536],
        normal: [100 + i as i16, -200 - i as i16 * 3, 300 + i as i16],
        uv: [1000 + i as u16 * 700, 3000 - i as u16 * 500],
        rgb565: 0xabcd ^ (i as u16 * 0x1111),
    })
}

/// Producer fixture boundary: publish, admit one task, finish and seal.
fn source_controller() -> (source_capture::Controller, u32, u64) {
    source_with_context(CONTEXT)
}
fn source_with_context(context: u64) -> (source_capture::Controller, u32, u64) {
    let mut c = source_capture::Controller::new(
        std::array::from_fn(|_| Slot::default()),
        context,
        100_000,
        64,
    )
    .unwrap();
    let epoch = c.allocate(0).unwrap();
    for (i, value) in original_vertices().iter().enumerate() {
        c.publish_completed_vertex(0, epoch, i, value).unwrap();
    }
    let ticket = c
        .submit(source_capture::Task {
            triangle_id: 123,
            slot: 0,
            epoch,
            vertices: [2, 0, 1],
            context,
        })
        .unwrap()
        .unwrap();
    c.finish_production(0, epoch).unwrap();
    c.seal(0, epoch).unwrap();
    (c, epoch, ticket)
}

fn run_capture(
    lease: &mut Lease,
    src: &mut source_capture::Controller,
    tr: &mut record::Controller,
) -> (u64, u32) {
    for _ in 0..MAX_WALL {
        let out = lease.pump(src, tr, true, record::Input::default()).unwrap();
        let captured = out.source.iter().find_map(|event| match event {
            source_capture::Event::SourceCaptured { ticket, epoch, .. } => Some((*ticket, *epoch)),
            _ => None,
        });
        if let Some((ticket, epoch)) = captured {
            assert!(out
                .source
                .iter()
                .any(|event| matches!(event, source_capture::Event::SourceReleased { .. })));
            return (ticket, epoch);
        }
    }
    panic!("source capture watchdog");
}

fn run_accept(
    lease: &mut Lease,
    src: &mut source_capture::Controller,
    tr: &mut record::Controller,
) -> record::SourceOwner {
    for _ in 0..MAX_WALL {
        let out = lease.pump(src, tr, true, record::Input::default()).unwrap();
        let accepted = out.transport.iter().find_map(|event| match event {
            record::Event::SnapshotAccepted(owner) => Some(*owner),
            _ => None,
        });
        if let Some(owner) = accepted {
            return owner;
        }
    }
    panic!("snapshot acceptance watchdog");
}

fn announce_end(
    lease: &mut Lease,
    src: &mut source_capture::Controller,
    tr: &mut record::Controller,
    owner: record::SourceOwner,
    fans: u8,
) {
    let out = lease
        .pump(
            src,
            tr,
            true,
            record::Input {
                source_end: Some(record::SourceEnd {
                    source: owner,
                    fans,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    if fans == 0 {
        assert_eq!(
            out.transport,
            vec![record::Event::SnapshotLastUseAck(owner)]
        );
    } else {
        assert!(out.transport.is_empty());
    }
}

fn reserve(
    lease: &mut Lease,
    src: &mut source_capture::Controller,
    tr: &mut record::Controller,
    owner: record::SourceOwner,
    fan: u8,
    rows: usize,
) -> record::Key {
    for _ in 0..MAX_WALL {
        let out = lease
            .pump(
                src,
                tr,
                true,
                record::Input {
                    reserve: Some(record::Reserve {
                        source: owner,
                        fan,
                        rows,
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        let key = out.transport.iter().find_map(|event| match event {
            record::Event::Reserved(key) => Some(*key),
            _ => None,
        });
        if let Some(key) = key {
            return key;
        }
    }
    panic!("reservation watchdog");
}

/// Independent opaque fixture payload; never read from either controller.
fn word(source: record::SourceOwner, fan: u8, row: usize) -> u64 {
    let mixed = source.ticket.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ u64::from(fan).wrapping_mul(0x1234_5678_9abc)
        ^ (row as u64).wrapping_mul(0x17bd_1111_2222);
    (mixed & ((1_u64 << 36) - 1)) | 1
}

/// Write every row, then publish on the following enabled edge.
fn write_fan(
    lease: &mut Lease,
    src: &mut source_capture::Controller,
    tr: &mut record::Controller,
    key: record::Key,
    rows: usize,
) -> Out {
    for row in 0..rows {
        let out = lease
            .pump(
                src,
                tr,
                true,
                record::Input {
                    write: Some(record::Write {
                        key,
                        row,
                        word: word(key.source, key.fan, row),
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(out.transport, vec![record::Event::RowWritten { key, row }]);
    }
    let out = lease.pump(src, tr, true, record::Input::default()).unwrap();
    assert!(out
        .transport
        .iter()
        .any(|event| matches!(event, record::Event::Published(owner) if *owner == key)));
    out
}

fn read_row(
    lease: &mut Lease,
    src: &mut source_capture::Controller,
    tr: &mut record::Controller,
    key: record::Key,
    row: usize,
    consumer: record::Consumer,
    last: bool,
) -> record::Response {
    let out = lease
        .pump(
            src,
            tr,
            true,
            record::Input {
                read: Some(record::Read {
                    key,
                    row,
                    consumer,
                    last_attribute_capture: last,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(out.transport, vec![record::Event::ReadIssued { key, row }]);

    let expected = record::Response {
        key,
        row,
        word: word(key.source, key.fan, row),
        consumer,
        last_attribute_capture: last,
    };
    let out = lease.pump(src, tr, true, record::Input::default()).unwrap();
    assert_eq!(out.transport, vec![record::Event::ReturnCaptured(expected)]);

    let out = lease
        .pump(
            src,
            tr,
            true,
            record::Input {
                return_ready: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        out.transport,
        vec![record::Event::ConsumerCaptured(expected)]
    );
    assert!(tr.response().is_none());
    expected
}

fn release(
    lease: &mut Lease,
    src: &mut source_capture::Controller,
    tr: &mut record::Controller,
    key: record::Key,
) {
    let out = lease
        .pump(
            src,
            tr,
            true,
            record::Input {
                last_quad_ack: Some(key),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(out.transport, vec![record::Event::RecordReleased(key)]);
    assert_eq!(tr.phase(key.slot), Some(record::Phase::Free));
}

#[test]
fn third_fan_stalls_until_actual_release_then_last_use_consumes_snapshot_once() {
    let (mut src, _epoch, ticket) = source_controller();
    let mut tr = transport();
    let mut lease = Lease::default();

    let (captured, _) = run_capture(&mut lease, &mut src, &mut tr);
    assert_eq!(captured, ticket);
    assert_eq!(lease.offer(), Some(ticket));
    assert_eq!(
        lease.accepted(),
        0,
        "capture alone is not transport admission"
    );

    let owner = run_accept(&mut lease, &mut src, &mut tr);
    assert_eq!(
        owner,
        record::SourceOwner {
            ticket,
            context: CONTEXT
        }
    );
    assert_eq!(lease.accepted(), 1);
    assert_eq!(lease.offer(), None, "offer clears only on SnapshotAccepted");

    announce_end(&mut lease, &mut src, &mut tr, owner, 3);

    let a = reserve(&mut lease, &mut src, &mut tr, owner, 0, 1);
    write_fan(&mut lease, &mut src, &mut tr, a, 1);
    let b = reserve(&mut lease, &mut src, &mut tr, owner, 1, 1);
    write_fan(&mut lease, &mut src, &mut tr, b, 1);
    assert_eq!(tr.free_slots(), 0);

    // The third fan is offered repeatedly but cannot be admitted while both
    // published records are live.
    for _ in 0..6 {
        let out = lease
            .pump(
                &mut src,
                &mut tr,
                true,
                record::Input {
                    reserve: Some(record::Reserve {
                        source: owner,
                        fan: 2,
                        rows: 1,
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!out
            .transport
            .iter()
            .any(|event| matches!(event, record::Event::Reserved(_))));
    }
    assert_eq!(tr.free_slots(), 0);
    assert_eq!(tr.phase(a.slot), Some(record::Phase::Published));
    assert_eq!(tr.phase(b.slot), Some(record::Phase::Published));
    assert!(src.snapshot().is_some());
    assert_eq!(
        lease.feedback(),
        None,
        "no last use before all fans publish"
    );

    // An actual record release is the only thing that frees a slot.
    read_row(
        &mut lease,
        &mut src,
        &mut tr,
        a,
        0,
        record::Consumer::Attribute,
        true,
    );
    release(&mut lease, &mut src, &mut tr, a);
    assert_eq!(tr.free_slots(), 1);

    let c = reserve(&mut lease, &mut src, &mut tr, owner, 2, 1);
    let out = write_fan(&mut lease, &mut src, &mut tr, c, 1);
    assert!(out
        .transport
        .iter()
        .any(|event| matches!(event, record::Event::SnapshotLastUseAck(o) if *o == owner)));
    assert_eq!(lease.last_use(), 1);
    assert_eq!(lease.feedback(), Some(ticket));
    assert!(
        src.snapshot().is_some(),
        "the transport ack is not yet the capture consumed edge"
    );

    // The next enabled edge delivers `consumed` exactly once.
    let out = lease
        .pump(&mut src, &mut tr, true, record::Input::default())
        .unwrap();
    assert!(out.source.iter().any(
        |event| matches!(event, source_capture::Event::TriangleConsumed { ticket: done } if *done == ticket)
    ));
    assert_eq!(lease.delivered(), 1);
    assert!(src.snapshot().is_none());
    assert_eq!(tr.phase(b.slot), Some(record::Phase::Published));
    assert_eq!(tr.phase(c.slot), Some(record::Phase::Published));
    assert!(
        !tr.drained(),
        "published records stay owned after the snapshot last use"
    );

    read_row(
        &mut lease,
        &mut src,
        &mut tr,
        b,
        0,
        record::Consumer::Attribute,
        true,
    );
    release(&mut lease, &mut src, &mut tr, b);
    read_row(
        &mut lease,
        &mut src,
        &mut tr,
        c,
        0,
        record::Consumer::Attribute,
        true,
    );
    release(&mut lease, &mut src, &mut tr, c);
    assert!(tr.drained());
}

#[test]
fn physical_slot_reuse_keeps_held_snapshot_and_record_rows_until_ack() {
    let (mut src, epoch, ticket) = source_controller();
    let mut tr = transport();
    let mut lease = Lease::default();

    let (captured, captured_epoch) = run_capture(&mut lease, &mut src, &mut tr);
    assert_eq!((captured, captured_epoch), (ticket, epoch));
    assert!(
        !src.slots()[0].held,
        "physical source slot releases before snapshot last use"
    );
    let original = original_vertices();
    let expected = [
        original[2].clone(),
        original[0].clone(),
        original[1].clone(),
    ];
    assert_eq!(src.snapshot().unwrap().input().unwrap().vertices, expected);

    let owner = run_accept(&mut lease, &mut src, &mut tr);

    // Reuse the physical slot with different values while the snapshot is held.
    let next_epoch = src.allocate(0).unwrap();
    assert_eq!(next_epoch, epoch + 1);
    for (i, value) in replacement_vertices().iter().enumerate() {
        src.publish_completed_vertex(0, next_epoch, i, value)
            .unwrap();
    }
    let old_rows = original[0].rows();
    let new_rows = &src.slots()[0].rows[..7];
    assert_ne!(
        new_rows,
        &old_rows[..],
        "physical source slot holds new data"
    );
    assert_eq!(
        src.snapshot().unwrap().input().unwrap().vertices,
        expected,
        "held snapshot still supplies the original data"
    );

    announce_end(&mut lease, &mut src, &mut tr, owner, 1);
    let key = reserve(&mut lease, &mut src, &mut tr, owner, 0, 2);
    write_fan(&mut lease, &mut src, &mut tr, key, 2);

    // Snapshot consumption happens on the next enabled edge, not the ack edge.
    assert!(src.snapshot().is_some());
    let out = lease
        .pump(&mut src, &mut tr, true, record::Input::default())
        .unwrap();
    assert!(out
        .source
        .iter()
        .any(|event| matches!(event, source_capture::Event::TriangleConsumed { .. })));
    assert!(src.snapshot().is_none());

    // Every record row still returns its independent fixture word until the
    // final attribute return is captured and last_quad_ack releases the slot.
    let first = read_row(
        &mut lease,
        &mut src,
        &mut tr,
        key,
        0,
        record::Consumer::Coverage,
        false,
    );
    assert_eq!(first.word, word(owner, 0, 0));
    let last = read_row(
        &mut lease,
        &mut src,
        &mut tr,
        key,
        1,
        record::Consumer::Attribute,
        true,
    );
    assert_eq!(last.word, word(owner, 0, 1));
    assert_eq!(tr.phase(key.slot), Some(record::Phase::Published));
    release(&mut lease, &mut src, &mut tr, key);
    assert!(tr.drained());
}

#[test]
fn ce_pause_holds_admission_and_last_use_feedback_without_loss_or_duplication() {
    let (mut src, _epoch, ticket) = source_controller();
    let mut tr = transport();
    let mut lease = Lease::default();

    let (captured, _) = run_capture(&mut lease, &mut src, &mut tr);
    assert_eq!(captured, ticket);
    assert_eq!(lease.offer(), Some(ticket));
    assert_eq!(lease.accepted(), 0);

    // Hold the latched offer through disabled edges: no admission, no loss.
    let offered_before = lease.offered_edges();
    for _ in 0..4 {
        let out = lease
            .pump(&mut src, &mut tr, false, record::Input::default())
            .unwrap();
        assert!(out.source.is_empty());
        assert!(out.transport.is_empty());
    }
    assert_eq!(lease.offer(), Some(ticket));
    assert_eq!(lease.accepted(), 0);
    assert_eq!(lease.offered_edges(), offered_before + 4);

    let owner = run_accept(&mut lease, &mut src, &mut tr);
    assert_eq!(owner.ticket, ticket);
    assert_eq!(lease.accepted(), 1);

    announce_end(&mut lease, &mut src, &mut tr, owner, 1);
    let key = reserve(&mut lease, &mut src, &mut tr, owner, 0, 1);

    // Write the only row, then hold publication through disabled edges.
    let out = lease
        .pump(
            &mut src,
            &mut tr,
            true,
            record::Input {
                write: Some(record::Write {
                    key,
                    row: 0,
                    word: word(owner, 0, 0),
                }),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        out.transport,
        vec![record::Event::RowWritten { key, row: 0 }]
    );
    for _ in 0..3 {
        let out = lease
            .pump(&mut src, &mut tr, false, record::Input::default())
            .unwrap();
        assert!(out.transport.is_empty());
    }
    assert_eq!(tr.phase(key.slot), Some(record::Phase::Writing));
    assert_eq!(lease.last_use(), 0);
    assert!(src.snapshot().is_some());

    // Publishing on an enabled edge emits last use; feedback is latched, not
    // delivered on the same edge.
    let out = lease
        .pump(&mut src, &mut tr, true, record::Input::default())
        .unwrap();
    assert!(out
        .transport
        .iter()
        .any(|event| matches!(event, record::Event::SnapshotLastUseAck(o) if *o == owner)));
    assert_eq!(lease.last_use(), 1);
    assert_eq!(lease.feedback(), Some(ticket));
    assert_eq!(lease.delivered(), 0);
    assert!(src.snapshot().is_some());

    // Hold feedback through disabled edges, then deliver exactly once.
    for _ in 0..4 {
        let out = lease
            .pump(&mut src, &mut tr, false, record::Input::default())
            .unwrap();
        assert!(out.source.is_empty());
        assert_eq!(lease.feedback(), Some(ticket));
        assert_eq!(lease.delivered(), 0);
        assert!(src.snapshot().is_some());
    }
    let out = lease
        .pump(&mut src, &mut tr, true, record::Input::default())
        .unwrap();
    assert!(out.source.iter().any(
        |event| matches!(event, source_capture::Event::TriangleConsumed { ticket: done } if *done == ticket)
    ));
    assert_eq!(lease.delivered(), 1);
    assert_eq!(lease.feedback(), None);
    assert!(src.snapshot().is_none());

    // No duplicate ownership events on subsequent edges.
    for _ in 0..4 {
        let out = lease
            .pump(&mut src, &mut tr, true, record::Input::default())
            .unwrap();
        assert!(out.source.is_empty());
        assert!(out.transport.is_empty());
    }
    assert_eq!(lease.accepted(), 1);
    assert_eq!(lease.last_use(), 1);
    assert_eq!(lease.delivered(), 1);
}

#[test]
fn stale_or_duplicate_last_use_feedback_is_rejected() {
    let (mut src, _epoch, ticket) = source_controller();
    let mut tr = transport();
    let mut lease = Lease::default();

    let (captured, _) = run_capture(&mut lease, &mut src, &mut tr);
    assert_eq!(captured, ticket);
    let owner = run_accept(&mut lease, &mut src, &mut tr);
    announce_end(&mut lease, &mut src, &mut tr, owner, 1);
    let key = reserve(&mut lease, &mut src, &mut tr, owner, 0, 1);
    write_fan(&mut lease, &mut src, &mut tr, key, 1);

    let out = lease
        .pump(&mut src, &mut tr, true, record::Input::default())
        .unwrap();
    assert!(out
        .source
        .iter()
        .any(|event| matches!(event, source_capture::Event::TriangleConsumed { .. })));
    assert_eq!(lease.delivered(), 1);
    assert!(src.snapshot().is_none());

    // Inject at the source boundary, without a production-only test hook.
    let err = src.step(true, Some(ticket)).unwrap_err();
    assert!(
        err.contains("stale or premature"),
        "unexpected error: {err}"
    );
    assert_eq!(lease.delivered(), 1);
}

#[test]
fn wrong_ticket_early_release_and_duplicate_ack_are_observably_rejected() {
    let (mut src, _epoch, ticket) = source_controller();
    let mut tr = transport();
    let mut lease = Lease::default();

    let (captured, _) = run_capture(&mut lease, &mut src, &mut tr);
    assert_eq!(captured, ticket);
    let owner = run_accept(&mut lease, &mut src, &mut tr);
    announce_end(&mut lease, &mut src, &mut tr, owner, 1);
    let key = reserve(&mut lease, &mut src, &mut tr, owner, 0, 1);
    write_fan(&mut lease, &mut src, &mut tr, key, 1);

    // Early release: no final attribute return has been captured yet, so an ACK
    // must fault rather than silently free the slot.
    let err = lease
        .pump(
            &mut src,
            &mut tr,
            true,
            record::Input {
                last_quad_ack: Some(key),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.contains("EarlyAck"), "unexpected error: {err}");
    assert_eq!(tr.phase(key.slot), Some(record::Phase::Published));
    assert_eq!(tr.free_slots(), 1, "only the other slot was ever free");
    assert!(src.snapshot().is_none());

    // Wrong ticket: an ACK key whose source ticket differs is not this record's
    // owner and must fault instead of releasing slot 0.
    let mut wrong = key;
    wrong.source.ticket = ticket.wrapping_add(999);
    let err = lease
        .pump(
            &mut src,
            &mut tr,
            true,
            record::Input {
                last_quad_ack: Some(wrong),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.contains("Owner"), "unexpected error: {err}");
    assert_eq!(tr.phase(key.slot), Some(record::Phase::Published));
    assert_eq!(tr.free_slots(), 1);

    // Legitimate final capture, then release succeeds exactly once.
    read_row(
        &mut lease,
        &mut src,
        &mut tr,
        key,
        0,
        record::Consumer::Attribute,
        true,
    );
    release(&mut lease, &mut src, &mut tr, key);
    assert_eq!(tr.phase(key.slot), Some(record::Phase::Free));
    assert_eq!(tr.free_slots(), 2);
    assert!(tr.drained());

    // Duplicate ACK: the slot is already free and its key stale; a repeat must
    // fault, not double-release an unrelated record.
    let err = lease
        .pump(
            &mut src,
            &mut tr,
            true,
            record::Input {
                last_quad_ack: Some(key),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.contains("Owner"), "unexpected error: {err}");
    assert_eq!(tr.free_slots(), 2);
}

#[test]
fn actual_snapshot_context_and_zero_fan_feedback_are_preserved() {
    let context = 0x5432_1234;
    let (mut src, _, ticket) = source_with_context(context);
    let mut tr = transport_with_context(context);
    let mut lease = Lease::default();
    run_capture(&mut lease, &mut src, &mut tr);
    let owner = run_accept(&mut lease, &mut src, &mut tr);
    assert_eq!(owner, record::SourceOwner { ticket, context });
    announce_end(&mut lease, &mut src, &mut tr, owner, 0);
    assert_eq!(lease.feedback(), Some(ticket));
    assert!(src.snapshot().is_some());
    let disabled = lease
        .pump(&mut src, &mut tr, false, record::Input::default())
        .unwrap();
    assert!(disabled.source.is_empty() && disabled.transport.is_empty());
    assert_eq!(lease.feedback(), Some(ticket));
    let consumed = lease
        .pump(&mut src, &mut tr, true, record::Input::default())
        .unwrap();
    assert!(consumed
        .source
        .contains(&source_capture::Event::TriangleConsumed { ticket }));
    assert!(src.snapshot().is_none());
    assert_eq!(lease.delivered(), 1);
    assert!(tr.drained());
}

#[test]
fn unsupported_cancellation_never_becomes_a_successful_last_use() {
    let (mut src, _, _) = source_controller();
    let mut tr = transport();
    let mut lease = Lease::default();
    run_capture(&mut lease, &mut src, &mut tr);
    run_accept(&mut lease, &mut src, &mut tr);
    src.cancel();
    let err = lease
        .pump(&mut src, &mut tr, true, record::Input::default())
        .unwrap_err();
    assert!(err.contains("cancellation composition"));
    assert_eq!(lease.delivered(), 0);
    assert_eq!(lease.last_use(), 0);
    assert!(lease
        .pump(&mut src, &mut tr, true, record::Input::default())
        .unwrap_err()
        .contains("recreation"));
    assert_eq!(lease.delivered(), 0);
}
