// Deliberately no public module registration until the integration owner selects it.
#[path = "../src/geometry/record_transport.rs"]
mod transport;
use transport::*;

fn controller() -> Controller {
    Controller::new(
        7,
        Limits {
            wall_edges: 10_000,
            sources: 32,
            records: 128,
        },
    )
    .unwrap()
}
fn enabled() -> Input {
    Input {
        ce: true,
        ..Input::default()
    }
}
fn edge(c: &mut Controller, input: Input) -> Vec<Event> {
    let events = c.step(input).unwrap();
    c.audit().unwrap();
    assert!(
        events
            .iter()
            .filter(|e| matches!(e, Event::RowWritten { .. }))
            .count()
            <= 1
    );
    assert!(
        events
            .iter()
            .filter(|e| matches!(e, Event::ReadIssued { .. }))
            .count()
            <= 1
    );
    assert!(
        events
            .iter()
            .filter(|e| matches!(e, Event::ConsumerCaptured(_)))
            .count()
            <= 1
    );
    assert!(c.free_slots() <= SLOTS);
    events
}
fn captured(c: &mut Controller, ticket: u64) -> SourceOwner {
    let source = SourceOwner { ticket, context: 7 };
    assert_eq!(
        edge(
            c,
            Input {
                source_captured: Some(source),
                ..enabled()
            }
        ),
        vec![Event::SnapshotAccepted(source)]
    );
    source
}
fn reserve(c: &mut Controller, source: SourceOwner, fan: u8, rows: usize) -> Key {
    let events = edge(
        c,
        Input {
            reserve: Some(Reserve { source, fan, rows }),
            ..enabled()
        },
    );
    assert_eq!(events.len(), 1);
    let Event::Reserved(key) = events[0] else {
        panic!("reservation")
    };
    assert_eq!(c.phase(key.slot), Some(Phase::Writing));
    key
}
// Independent fixture data, never copied from the controller's row store.
fn word(source: SourceOwner, fan: u8, row: usize) -> u64 {
    ((source.ticket * 0x9e37 + u64::from(fan) * 0x12345 + row as u64 * 0x17bd) ^ 0xabcdef123)
        & ((1_u64 << 36) - 1)
}
fn write_all(c: &mut Controller, key: Key, rows: usize) -> Vec<Event> {
    assert!((1..=ROWS).contains(&rows));
    for row in 0..rows {
        let events = edge(
            c,
            Input {
                write: Some(Write {
                    key,
                    row,
                    word: word(key.source, key.fan, row),
                }),
                ..enabled()
            },
        );
        assert_eq!(events, vec![Event::RowWritten { key, row }]);
        assert_eq!(c.phase(key.slot), Some(Phase::Writing));
    }
    let events = edge(c, enabled());
    assert!(events.contains(&Event::Published(key)));
    assert_eq!(c.phase(key.slot), Some(Phase::Published));
    events
}
fn read_row(c: &mut Controller, key: Key, row: usize, consumer: Consumer, last: bool) -> Response {
    assert_eq!(
        edge(
            c,
            Input {
                read: Some(Read {
                    key,
                    row,
                    consumer,
                    last_attribute_capture: last
                }),
                ..enabled()
            }
        ),
        vec![Event::ReadIssued { key, row }]
    );
    assert!(c.response().is_none());
    let expected = Response {
        key,
        row,
        word: word(key.source, key.fan, row),
        consumer,
        last_attribute_capture: last,
    };
    // Even return_ready cannot consume on the capture edge.
    assert_eq!(
        edge(
            c,
            Input {
                return_ready: true,
                ..enabled()
            }
        ),
        vec![Event::ReturnCaptured(expected)]
    );
    assert_eq!(c.response(), Some(expected));
    assert_eq!(
        edge(
            c,
            Input {
                return_ready: true,
                ..enabled()
            }
        ),
        vec![Event::ConsumerCaptured(expected)]
    );
    assert!(c.response().is_none());
    expected
}
fn release(c: &mut Controller, key: Key) {
    assert_eq!(
        edge(
            c,
            Input {
                last_quad_ack: Some(key),
                ..enabled()
            }
        ),
        vec![Event::RecordReleased(key)]
    );
    assert_eq!(c.phase(key.slot), Some(Phase::Free));
}

#[test]
fn two_slots_third_fan_backpressure_and_three_completion_boundaries() {
    let mut c = controller();
    let source = captured(&mut c, 10); // upstream SourceCaptured, not a record ACK
    let a = reserve(&mut c, source, 0, 3);
    write_all(&mut c, a, 3);
    let b = reserve(&mut c, source, 1, 2);
    write_all(&mut c, b, 2);
    assert_eq!(c.free_slots(), 0);
    assert!(edge(
        &mut c,
        Input {
            source_end: Some(SourceEnd { source, fans: 3 }),
            ..enabled()
        }
    )
    .is_empty());
    for ce in [true, false, true, false, true] {
        assert!(edge(
            &mut c,
            Input {
                ce,
                reserve: Some(Reserve {
                    source,
                    fan: 2,
                    rows: 3
                }),
                ..Input::default()
            }
        )
        .is_empty());
    }
    assert!(!c.drained());
    read_row(&mut c, a, 0, Consumer::Coverage, false);
    read_row(&mut c, a, 1, Consumer::Attribute, false);
    assert_eq!(
        c.step(Input {
            last_quad_ack: Some(a),
            ..enabled()
        }),
        Err(Fault::EarlyAck)
    );
    let request = Read {
        key: a,
        row: 2,
        consumer: Consumer::Attribute,
        last_attribute_capture: true,
    };
    assert_eq!(
        edge(
            &mut c,
            Input {
                read: Some(request),
                ..enabled()
            }
        ),
        vec![Event::ReadIssued { key: a, row: 2 }]
    );
    assert_eq!(
        c.step(Input {
            last_quad_ack: Some(a),
            ..enabled()
        }),
        Err(Fault::EarlyAck)
    );
    assert!(edge(
        &mut c,
        Input {
            ce: false,
            last_quad_ack: Some(a),
            ..Input::default()
        }
    )
    .is_empty());
    let response = Response {
        key: a,
        row: 2,
        word: word(source, 0, 2),
        consumer: Consumer::Attribute,
        last_attribute_capture: true,
    };
    assert_eq!(
        edge(&mut c, enabled()),
        vec![Event::ReturnCaptured(response)]
    );
    for _ in 0..5 {
        // Shared read credit is held; the other slot gets no free parallel read.
        assert!(edge(
            &mut c,
            Input {
                read: Some(Read {
                    key: b,
                    row: 0,
                    consumer: Consumer::Coverage,
                    last_attribute_capture: false
                }),
                ..enabled()
            }
        )
        .is_empty());
        assert_eq!(c.response(), Some(response));
    }
    assert_eq!(
        c.step(Input {
            last_quad_ack: Some(a),
            ..enabled()
        }),
        Err(Fault::EarlyAck)
    );
    // A final consumer transfer and ACK may coincide. The old credit cannot
    // fund a same-edge reservation: no release/reallocate collision.
    assert_eq!(
        edge(
            &mut c,
            Input {
                return_ready: true,
                last_quad_ack: Some(a),
                reserve: Some(Reserve {
                    source,
                    fan: 2,
                    rows: 3
                }),
                ..enabled()
            }
        ),
        vec![Event::ConsumerCaptured(response), Event::RecordReleased(a)]
    );
    let next = reserve(&mut c, source, 2, 3);
    assert_eq!(next.slot, a.slot);
    assert!(next.generation > a.generation);
    let published = write_all(&mut c, next, 3);
    assert_eq!(
        published,
        vec![Event::Published(next), Event::SnapshotLastUseAck(source)]
    );
    assert_eq!(c.free_slots(), 0); // snapshot last use did not free either record
    assert_eq!(
        c.step(Input {
            read: Some(Read {
                key: a,
                row: 0,
                consumer: Consumer::Coverage,
                last_attribute_capture: false
            }),
            ..enabled()
        }),
        Err(Fault::Owner)
    );
    assert_eq!(
        c.step(Input {
            last_quad_ack: Some(a),
            ..enabled()
        }),
        Err(Fault::Owner)
    );
    assert_eq!(
        c.step(Input {
            source_captured: Some(source),
            ..enabled()
        }),
        Err(Fault::SourceOrder)
    );
    // An empty source can finish while both older immutable records are live.
    let empty = captured(&mut c, 11);
    assert_eq!(
        edge(
            &mut c,
            Input {
                source_end: Some(SourceEnd {
                    source: empty,
                    fans: 0
                }),
                ..enabled()
            }
        ),
        vec![Event::SnapshotLastUseAck(empty)]
    );
    for row in 0..2 {
        read_row(&mut c, b, row, Consumer::Attribute, row == 1);
    }
    release(&mut c, b);
    for row in 0..3 {
        read_row(&mut c, next, row, Consumer::Attribute, row == 2);
    }
    release(&mut c, next);
    assert!(c.drained());
}

#[test]
fn full_64_rows_exact_data_and_ce_hold_no_publish_read_bypass() {
    let mut c = controller();
    let source = captured(&mut c, 22);
    let key = reserve(&mut c, source, 0, ROWS);
    assert!(edge(
        &mut c,
        Input {
            source_end: Some(SourceEnd { source, fans: 1 }),
            ..enabled()
        }
    )
    .is_empty());
    for row in 0..ROWS {
        let write = Write {
            key,
            row,
            word: word(source, 0, row),
        };
        assert!(edge(
            &mut c,
            Input {
                ce: false,
                write: Some(write),
                ..Input::default()
            }
        )
        .is_empty());
        assert_eq!(
            edge(
                &mut c,
                Input {
                    write: Some(write),
                    ..enabled()
                }
            ),
            vec![Event::RowWritten { key, row }]
        );
    }
    assert_eq!(c.phase(key.slot), Some(Phase::Writing));
    assert_eq!(
        c.step(Input {
            read: Some(Read {
                key,
                row: 0,
                consumer: Consumer::Coverage,
                last_attribute_capture: false
            }),
            ..enabled()
        }),
        Err(Fault::State)
    );
    assert!(edge(&mut c, Input::default()).is_empty());
    assert_eq!(
        edge(&mut c, enabled()),
        vec![Event::Published(key), Event::SnapshotLastUseAck(source)]
    );
    for row in 0..ROWS {
        let consumer = if row % 3 == 0 && row != ROWS - 1 {
            Consumer::Coverage
        } else {
            Consumer::Attribute
        };
        let req = Read {
            key,
            row,
            consumer,
            last_attribute_capture: row == ROWS - 1,
        };
        assert_eq!(
            edge(
                &mut c,
                Input {
                    read: Some(req),
                    ..enabled()
                }
            ),
            vec![Event::ReadIssued { key, row }]
        );
        assert!(edge(&mut c, Input::default()).is_empty());
        assert!(c.response().is_none());
        let expected = Response {
            key,
            row,
            word: word(source, 0, row),
            consumer,
            last_attribute_capture: row == ROWS - 1,
        };
        assert_eq!(
            edge(&mut c, enabled()),
            vec![Event::ReturnCaptured(expected)]
        );
        assert!(edge(
            &mut c,
            Input {
                ce: false,
                return_ready: true,
                ..Input::default()
            }
        )
        .is_empty());
        assert_eq!(c.response(), Some(expected));
        assert_eq!(
            edge(
                &mut c,
                Input {
                    return_ready: true,
                    ..enabled()
                }
            ),
            vec![Event::ConsumerCaptured(expected)]
        );
    }
    release(&mut c, key);
    assert!(c.drained());
}

#[test]
fn admission_owner_context_early_ack_and_malformed_rows_are_rejected() {
    let mut c = controller();
    assert_eq!(
        c.step(Input {
            source_captured: Some(SourceOwner {
                ticket: 0,
                context: 8
            }),
            ..enabled()
        }),
        Err(Fault::Context)
    );
    let source = captured(&mut c, 0);
    for rows in [0, 65] {
        assert_eq!(
            c.step(Input {
                reserve: Some(Reserve {
                    source,
                    fan: 0,
                    rows
                }),
                ..enabled()
            }),
            Err(Fault::Row)
        );
    }
    assert_eq!(
        c.step(Input {
            reserve: Some(Reserve {
                source,
                fan: 1,
                rows: 2
            }),
            ..enabled()
        }),
        Err(Fault::SourceOrder)
    );
    assert_eq!(
        c.step(Input {
            source_end: Some(SourceEnd { source, fans: 7 }),
            ..enabled()
        }),
        Err(Fault::SourceOrder)
    );
    // Contradictory same-edge reserve and source end cannot strand the snapshot.
    assert_eq!(
        c.step(Input {
            reserve: Some(Reserve {
                source,
                fan: 0,
                rows: 2
            }),
            source_end: Some(SourceEnd { source, fans: 0 }),
            ..enabled()
        }),
        Err(Fault::SourceOrder)
    );
    assert_eq!(c.free_slots(), 2);
    let key = reserve(&mut c, source, 0, 2);
    let mut wrong = key;
    wrong.source.context = 9;
    assert_eq!(
        c.step(Input {
            write: Some(Write {
                key: wrong,
                row: 0,
                word: 1
            }),
            ..enabled()
        }),
        Err(Fault::Context)
    );
    assert_eq!(
        c.step(Input {
            write: Some(Write {
                key,
                row: 1,
                word: 1
            }),
            ..enabled()
        }),
        Err(Fault::Row)
    );
    assert_eq!(
        c.step(Input {
            write: Some(Write {
                key,
                row: 0,
                word: 1_u64 << 36
            }),
            ..enabled()
        }),
        Err(Fault::Row)
    );
    assert_eq!(
        c.step(Input {
            last_quad_ack: Some(key),
            ..enabled()
        }),
        Err(Fault::EarlyAck)
    );
    write_all(&mut c, key, 2);
    assert_eq!(
        c.step(Input {
            write: Some(Write {
                key,
                row: 0,
                word: 0
            }),
            ..enabled()
        }),
        Err(Fault::State)
    );
    assert_eq!(
        c.step(Input {
            read: Some(Read {
                key,
                row: 2,
                consumer: Consumer::Attribute,
                last_attribute_capture: true
            }),
            ..enabled()
        }),
        Err(Fault::Row)
    );
    assert_eq!(
        c.step(Input {
            read: Some(Read {
                key,
                row: 1,
                consumer: Consumer::Coverage,
                last_attribute_capture: true
            }),
            ..enabled()
        }),
        Err(Fault::Row)
    );
    assert_eq!(
        c.step(Input {
            abort_ack: Some(key),
            ..enabled()
        }),
        Err(Fault::State)
    );
    read_row(&mut c, key, 0, Consumer::Coverage, false);
    assert_eq!(
        c.step(Input {
            last_quad_ack: Some(key),
            ..enabled()
        }),
        Err(Fault::EarlyAck)
    );
    read_row(&mut c, key, 1, Consumer::Attribute, true);
    assert_eq!(
        c.step(Input {
            read: Some(Read {
                key,
                row: 0,
                consumer: Consumer::Coverage,
                last_attribute_capture: false
            }),
            ..enabled()
        }),
        Err(Fault::State)
    );
    release(&mut c, key);
    assert_eq!(
        c.step(Input {
            last_quad_ack: Some(key),
            ..enabled()
        }),
        Err(Fault::Owner)
    );
    assert_eq!(
        edge(
            &mut c,
            Input {
                source_end: Some(SourceEnd { source, fans: 1 }),
                ..enabled()
            }
        ),
        vec![Event::SnapshotLastUseAck(source)]
    );
    assert!(c.drained());
}

#[test]
fn cancel_partial_write_and_pending_read_require_distinct_consumer_abort() {
    let mut c = controller();
    let source = captured(&mut c, 30);
    let a = reserve(&mut c, source, 0, 2);
    write_all(&mut c, a, 2);
    let b = reserve(&mut c, source, 1, 3);
    edge(
        &mut c,
        Input {
            write: Some(Write {
                key: b,
                row: 0,
                word: word(source, 1, 0),
            }),
            read: Some(Read {
                key: a,
                row: 1,
                consumer: Consumer::Attribute,
                last_attribute_capture: true,
            }),
            ..enabled()
        },
    );
    c.cancel();
    assert!(edge(
        &mut c,
        Input {
            ce: false,
            abort_ack: Some(a),
            ..Input::default()
        }
    )
    .is_empty());
    assert_eq!(c.phase(b.slot), Some(Phase::Writing));
    assert_eq!(c.free_slots(), 0);
    let response = Response {
        key: a,
        row: 1,
        word: word(source, 0, 1),
        consumer: Consumer::Attribute,
        last_attribute_capture: true,
    };
    let first = edge(
        &mut c,
        Input {
            abort_ack: Some(a),
            return_ready: true,
            ..enabled()
        },
    );
    assert_eq!(
        first,
        vec![
            Event::CancelStarted,
            Event::PartialWriteDiscarded(b),
            Event::SnapshotAborted(source),
            Event::ReturnCaptured(response),
            Event::AbortAccepted(a)
        ]
    );
    assert_eq!(c.free_slots(), 1);
    assert!(!c.drained());
    assert_eq!(
        edge(&mut c, enabled()),
        vec![Event::ReturnDiscarded(response), Event::RecordAborted(a)]
    );
    assert!(c.drained());
    assert!(edge(
        &mut c,
        Input {
            source_captured: Some(SourceOwner {
                ticket: 31,
                context: 7
            }),
            reserve: Some(Reserve {
                source,
                fan: 2,
                rows: 1
            }),
            ..enabled()
        }
    )
    .is_empty());
    assert_eq!(
        c.step(Input {
            abort_ack: Some(a),
            ..enabled()
        }),
        Err(Fault::Owner)
    );
}

#[test]
fn cancel_does_not_silently_free_published_records_without_consumer_drain() {
    let mut c = controller();
    let source = captured(&mut c, 40);
    let a = reserve(&mut c, source, 0, 1);
    write_all(&mut c, a, 1);
    let b = reserve(&mut c, source, 1, 1);
    write_all(&mut c, b, 1);
    assert_eq!(
        edge(
            &mut c,
            Input {
                source_end: Some(SourceEnd { source, fans: 2 }),
                ..enabled()
            }
        ),
        vec![Event::SnapshotLastUseAck(source)]
    );
    c.cancel();
    assert_eq!(edge(&mut c, enabled()), vec![Event::CancelStarted]);
    for _ in 0..5 {
        assert!(edge(&mut c, enabled()).is_empty());
    }
    assert_eq!(c.free_slots(), 0);
    assert!(!c.drained());
    assert_eq!(
        edge(
            &mut c,
            Input {
                abort_ack: Some(a),
                ..enabled()
            }
        ),
        vec![Event::AbortAccepted(a), Event::RecordAborted(a)]
    );
    assert_eq!(c.free_slots(), 1);
    assert_eq!(
        edge(
            &mut c,
            Input {
                abort_ack: Some(b),
                ..enabled()
            }
        ),
        vec![Event::AbortAccepted(b), Event::RecordAborted(b)]
    );
    assert!(c.drained());
}

#[test]
fn all_six_fans_are_bounded_and_source_ack_is_not_a_record_ack() {
    let mut c = controller();
    let source = captured(&mut c, 50);
    edge(
        &mut c,
        Input {
            source_end: Some(SourceEnd {
                source,
                fans: MAX_FANS,
            }),
            ..enabled()
        },
    );
    for fan in 0..MAX_FANS {
        let key = reserve(&mut c, source, fan, 1);
        let events = write_all(&mut c, key, 1);
        assert_eq!(
            events.contains(&Event::SnapshotLastUseAck(source)),
            fan == MAX_FANS - 1
        );
        assert_eq!(c.free_slots(), 1);
        read_row(&mut c, key, 0, Consumer::Attribute, true);
        release(&mut c, key);
    }
    assert!(c.drained());
}

#[test]
fn shared_return_turnover_and_cancel_before_publication() {
    let mut c = controller();
    let source = captured(&mut c, 60);
    let a = reserve(&mut c, source, 0, 1);
    write_all(&mut c, a, 1);
    let b = reserve(&mut c, source, 1, 1);
    let request = Read {
        key: a,
        row: 0,
        consumer: Consumer::Coverage,
        last_attribute_capture: false,
    };
    assert_eq!(
        edge(
            &mut c,
            Input {
                read: Some(request),
                write: Some(Write {
                    key: b,
                    row: 0,
                    word: word(source, 1, 0)
                }),
                ..enabled()
            }
        ),
        vec![
            Event::RowWritten { key: b, row: 0 },
            Event::ReadIssued { key: a, row: 0 }
        ]
    );
    let response = Response {
        key: a,
        row: 0,
        word: word(source, 0, 0),
        consumer: Consumer::Coverage,
        last_attribute_capture: false,
    };
    assert_eq!(
        edge(&mut c, enabled()),
        vec![Event::ReturnCaptured(response), Event::Published(b)]
    );
    // Captured head transfers while the shared port accepts the other owner.
    let next = Read {
        key: b,
        row: 0,
        consumer: Consumer::Attribute,
        last_attribute_capture: true,
    };
    assert_eq!(
        edge(
            &mut c,
            Input {
                return_ready: true,
                read: Some(next),
                ..enabled()
            }
        ),
        vec![
            Event::ConsumerCaptured(response),
            Event::ReadIssued { key: b, row: 0 }
        ]
    );
    let response = Response {
        key: b,
        row: 0,
        word: word(source, 1, 0),
        consumer: Consumer::Attribute,
        last_attribute_capture: true,
    };
    assert_eq!(
        edge(&mut c, enabled()),
        vec![Event::ReturnCaptured(response)]
    );
    c.cancel();
    let mut wrong = b;
    wrong.source.context = 8;
    assert_eq!(
        c.step(Input {
            abort_ack: Some(wrong),
            ..enabled()
        }),
        Err(Fault::Context)
    );
    assert_eq!(c.response(), Some(response));
    assert_eq!(
        edge(
            &mut c,
            Input {
                abort_ack: Some(b),
                return_ready: true,
                ..enabled()
            }
        ),
        vec![
            Event::CancelStarted,
            Event::SnapshotAborted(source),
            Event::ReturnDiscarded(response),
            Event::AbortAccepted(b),
            Event::RecordAborted(b)
        ]
    );
    assert_eq!(
        edge(
            &mut c,
            Input {
                abort_ack: Some(a),
                ..enabled()
            }
        ),
        vec![Event::AbortAccepted(a), Event::RecordAborted(a)]
    );
    assert!(c.drained());

    let mut c = controller();
    let source = captured(&mut c, 61);
    let key = reserve(&mut c, source, 0, 1);
    edge(
        &mut c,
        Input {
            write: Some(Write {
                key,
                row: 0,
                word: word(source, 0, 0),
            }),
            ..enabled()
        },
    );
    c.cancel(); // Fully written is still unpublished and has no consumer lease.
    assert_eq!(
        c.step(Input {
            abort_ack: Some(key),
            ..enabled()
        }),
        Err(Fault::State)
    );
    assert_eq!(
        edge(&mut c, enabled()),
        vec![
            Event::CancelStarted,
            Event::PartialWriteDiscarded(key),
            Event::SnapshotAborted(source)
        ]
    );
    assert!(c.drained());
}

#[test]
fn cycle_source_record_budgets_include_ce_stalls() {
    assert!(matches!(
        Controller::new(
            7,
            Limits {
                wall_edges: 0,
                sources: 1,
                records: 1
            }
        ),
        Err(Fault::Bound)
    ));
    let mut c = Controller::new(
        7,
        Limits {
            wall_edges: 2,
            sources: 1,
            records: 1,
        },
    )
    .unwrap();
    edge(&mut c, Input::default());
    edge(&mut c, Input::default());
    assert_eq!(c.step(enabled()), Err(Fault::Bound));
    let mut c = Controller::new(
        7,
        Limits {
            wall_edges: 100,
            sources: 1,
            records: 1,
        },
    )
    .unwrap();
    let source = captured(&mut c, 0);
    edge(
        &mut c,
        Input {
            source_end: Some(SourceEnd { source, fans: 0 }),
            ..enabled()
        },
    );
    assert_eq!(
        c.step(Input {
            source_captured: Some(SourceOwner {
                ticket: 1,
                context: 7
            }),
            ..enabled()
        }),
        Err(Fault::Bound)
    );
    let mut c = Controller::new(
        7,
        Limits {
            wall_edges: 100,
            sources: 2,
            records: 1,
        },
    )
    .unwrap();
    let source = captured(&mut c, 0);
    let key = reserve(&mut c, source, 0, 1);
    write_all(&mut c, key, 1);
    read_row(&mut c, key, 0, Consumer::Attribute, true);
    release(&mut c, key);
    assert_eq!(
        c.step(Input {
            reserve: Some(Reserve {
                source,
                fan: 1,
                rows: 1
            }),
            ..enabled()
        }),
        Err(Fault::Bound)
    );
}
