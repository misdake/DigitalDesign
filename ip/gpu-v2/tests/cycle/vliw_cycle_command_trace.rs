//! Finite real CP transitions with explicit source/backend peers, no replay GPU.
use gpu_v2::system::vliw_cycle::command::{
    CommandEdge, DeviceBus, Phase, Processor, ProcessorInput,
};

fn bus(channel: u8, data: u16) -> DeviceBus {
    DeviceBus {
        index: 4,
        channel,
        data,
        write: true,
        ..Default::default()
    }
}

#[test]
fn committed_trace_distinguishes_prefetch_latch_retry_issue_and_drain() {
    let mut words = vec![0; 70]; // Cross the physical 64-word instruction RAM ring.
    words.extend([0x20000000, 0x40000003, 0x70000000, 0xf0000000]);
    let mut observed = Processor::with_draws();
    let mut plain = observed.clone();
    let mut index = 0;
    let mut latched = Vec::new();
    let mut issued = Vec::new();
    let mut released = Vec::new();
    let mut retries = 0;
    let mut end_wait = 0;
    let mut drained = false;
    for tick in 0..2000 {
        let phase = observed.phase();
        if phase == Phase::Ending {
            end_wait += 1;
            if end_wait == 1 {
                let mut failed = observed.clone();
                let edge = failed
                    .clock_observing(ProcessorInput {
                        ce: true,
                        abort: true,
                        render_idle: true,
                        dma_idle: true,
                        ..Default::default()
                    })
                    .unwrap();
                assert!(edge.aborted && !edge.drained);
                assert!(!failed.outputs(ProcessorInput::default()).busy);
                // An abort coincident with physical END drain still needs
                // a reset boundary before another observer submission epoch.
                assert!(
                    failed
                        .clock_observing(ProcessorInput {
                            reset: true,
                            ..Default::default()
                        })
                        .unwrap()
                        .reset
                );
            }
        }
        let pair = index & !1;
        let mut i = ProcessorInput {
            ce: tick % 5 != 0,
            ram_ready: tick % 3 != 0,
            ram_q: u64::from(*words.get(pair).unwrap_or(&0))
                | (u64::from(*words.get(pair + 1).unwrap_or(&0)) << 32),
            cache_done: true,
            cache_done_bank: tick % 2 != 0,
            cache_ready: tick % 4 != 0,
            dma_idle: true,
            render_idle: phase != Phase::Ending || end_wait >= 7,
            flush_complete: tick % 7 == 0,
            ..Default::default()
        };
        i.draw.work_ready = tick % 11 == 0;
        if tick == 0 {
            i.bus = bus(0, words.len() as u16 - 1);
        }
        if tick == 1 {
            i.ce = false;
            i.bus = bus(1, 0x3210);
        }
        // Repeated old-state reads cannot create events or change either model.
        for _ in 0..3 {
            assert_eq!(observed.outputs(i), plain.outputs(i));
        }
        plain.clock(i).unwrap();
        let edge = observed.clock_observing(i).unwrap();
        assert_eq!(format!("{observed:?}"), format!("{plain:?}"), "edge {tick}");
        if tick == 1 {
            assert_eq!(edge.submit, Some((0x190800, 296)));
        }
        if !i.ce {
            assert!(edge.latched.is_none() && edge.execute.is_none() && edge.issued.is_none());
        }
        if let Some(command) = edge.latched {
            latched.push(command);
        }
        if edge.execute.is_some() && edge.issued.is_none() {
            retries += 1;
        }
        if let Some(command) = edge.issued {
            issued.push(command);
        }
        if let Some(command) = edge.released {
            released.push(command);
            index += 1;
        }
        if phase == Phase::Ending {
            assert!(
                edge.released.is_none(),
                "END release must not repeat while draining"
            );
        }
        if edge.drained {
            drained = true;
            break;
        }
    }
    assert!(drained && end_wait >= 7 && retries > 0);
    let expected: Vec<_> = words
        .iter()
        .enumerate()
        .map(|(pc, &word)| (pc as u32 * 4, word))
        .collect();
    assert_eq!(latched, expected);
    assert_eq!(issued, expected);
    assert_eq!(released, expected.iter().map(|v| v.0).collect::<Vec<_>>());
    // A successor submit resets diagnostic PC without releasing the previous END again.
    observed
        .clock(ProcessorInput {
            bus: bus(0, 0),
            ..Default::default()
        })
        .unwrap();
    let edge = observed
        .clock_observing(ProcessorInput {
            ce: true,
            bus: bus(1, 1),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(edge.submit, Some((128, 4)));
    assert_eq!(edge.released, None);
}

#[test]
fn abort_reset_and_invalid_input_do_not_invent_a_command_end() {
    let mut cp = Processor::with_draws();
    cp.clock(ProcessorInput {
        bus: bus(0, 1),
        ..Default::default()
    })
    .unwrap();
    cp.clock(ProcessorInput {
        bus: bus(1, 0),
        ..Default::default()
    })
    .unwrap();
    for _ in 0..8 {
        if cp.phase() == Phase::Exec {
            break;
        }
        cp.clock(ProcessorInput {
            ce: true,
            cache_done: true,
            ram_ready: true,
            ram_q: 0x20000000,
            ..Default::default()
        })
        .unwrap();
    }
    assert_eq!(cp.phase(), Phase::Exec);
    let e = cp
        .clock_observing(ProcessorInput {
            ce: true,
            abort: true,
            ..Default::default()
        })
        .unwrap();
    assert!(e.aborted && e.released.is_none() && e.issued.is_none() && !e.drained);
    let reset = cp
        .clock_observing(ProcessorInput {
            reset: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        reset,
        CommandEdge {
            reset: true,
            ..Default::default()
        }
    );
    assert!(!cp.outputs(ProcessorInput::default()).busy);
    assert!(
        !cp.clock_observing(ProcessorInput {
            reset: true,
            ..Default::default()
        })
        .unwrap()
        .reset
    );
    assert!(cp
        .clock_observing(ProcessorInput {
            bus: bus(1, 4),
            ..Default::default()
        })
        .is_err());
}
