#[path = "support/sdram/mod.rs"]
mod sdram;
use digital_design_hardware_gowin::sdram_memory_controller::{
    emu::service,
    ports::*,
    sim::{average, oracle, traffic::*},
};
use gpu_v2::{
    command_processor::ports::Command,
    frontend::{ports::*, sim::oracle as frontend},
    scratchpad::ports::DmaDescriptor,
    vertex::ports::*,
};
fn program() -> Input {
    let vertices = [
        PackedVertex::encode([12, 34, 56], [-127, 0, 127], [0, 4095], 0x1234).unwrap(),
        PackedVertex::encode([1023, 1, 500], [1, -128, 90], [2000, 1200], 0xabcd).unwrap(),
        PackedVertex::encode([0, 0, 0], [0, 0, 0], [4095, 0], 0xf81f).unwrap(),
    ];
    let mut memory = vec![0x5a; 8];
    for vertex in vertices {
        for cell in vertex.0 {
            memory.extend(cell.to_le_bytes());
        }
    }
    memory.resize(64, 0xa5);
    Input {
        commands: vec![
            Command::Dma(DmaDescriptor {
                physical_addr: 0x1008,
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
fn image(input: &Input) -> OracleImage {
    // SAFETY: this is the independent packed input dataset supplied to the test.
    unsafe {
        OracleImage::from_host(
            input.memory_base,
            input.memory.clone(),
            "frontend integration input",
        )
        .unwrap()
    }
}
#[test]
fn frontend_oracle_consumes_real_service_beats_in_both_modes() {
    let input = program();
    let golden = frontend::run(&input).unwrap();
    let stable = average::Memory::new(
        image(&input),
        average::Profile::gpu_default().unwrap(),
        Default::default(),
    )
    .unwrap();
    let mut fixed = sdram::Adapter {
        service: stable,
        max_cycles: 10000,
        events: vec![],
    };
    let got = frontend::run_with_memory(&input, &mut fixed).unwrap();
    assert_eq!(got.outputs, golden.outputs);
    assert!(got.fence);
    assert_eq!(fixed.service.bytes(), input.memory);
    assert_eq!(
        fixed
            .events
            .iter()
            .filter(|e| matches!(e, Event::ReadBeat { .. }))
            .count(),
        8
    ); // 40 B payload, 64 B native cover
    let mut cycles = vec![];
    for batch in [1, 50] {
        let model = oracle::Memory::new(
            image(&input),
            oracle::Config {
                load: Load::display_and_cpu(batch),
                chain: ChainPolicy::ChainedCandidate,
                ..Default::default()
            },
        )
        .unwrap();
        let mut source = sdram::Adapter {
            service: model,
            max_cycles: 10000,
            events: vec![],
        };
        let got = frontend::run_with_memory(&input, &mut source).unwrap();
        assert_eq!(got.outputs, golden.outputs);
        assert!(got.fence);
        assert_eq!(source.service.bytes(), input.memory);
        assert_eq!(
            source
                .events
                .iter()
                .filter(|e| matches!(e, Event::Complete { .. }))
                .count(),
            1
        );
        cycles.push(source.service.cycle());
    }
    assert!(cycles[1] > cycles[0]); // same average display bandwidth, different short-batch wait
}

#[test]
fn frontend_oracle_runs_on_independent_cycle_combination() {
    let input = program();
    let golden = frontend::run(&input).unwrap();
    let memory = service::Memory::new(
        image(&input),
        service::Config {
            init_cycles: 32,
            ..Default::default()
        },
    )
    .unwrap();
    let mut adapter = sdram::Adapter {
        service: memory,
        max_cycles: 20000,
        events: Vec::new(),
    };
    let observed = frontend::run_with_memory(&input, &mut adapter).unwrap();
    assert_eq!(observed.outputs, golden.outputs);
    assert_eq!(observed.fence, golden.fence);
    assert!(adapter.service.idle());
    assert_eq!(adapter.service.bytes(), input.memory);
    assert!(adapter
        .events
        .iter()
        .any(|e| matches!(e, Event::ReadBeat { .. })));
}
#[test]
fn insufficient_tail_cover_is_a_real_source_error_not_zero_padding() {
    let input = program();
    let short = OracleImage::filled::<0>(0x1000, 48).unwrap();
    let model = average::Memory::new(
        short,
        average::Profile::gpu_default().unwrap(),
        Default::default(),
    )
    .unwrap();
    let mut source = sdram::Adapter {
        service: model,
        max_cycles: 10000,
        events: vec![],
    };
    assert!(frontend::run_with_memory(&input, &mut source).is_err());
}

#[test]
fn early_average_oracle_is_calibrated_deterministic_and_matches_functional_data() {
    use digital_design_hardware_gowin::sdram_memory_controller::sim::{
        calibration, cycle_calibration,
    };
    let trace = calibration::representative_trace(8).unwrap();
    let config = cycle_calibration::Config {
        service: service::Config {
            init_cycles: 32,
            early_grant: true,
            max_queued: 512,
            ..Default::default()
        },
        load: Load::display_and_cpu(50),
        ..Default::default()
    };
    let first = cycle_calibration::analyze(&trace, config.clone()).unwrap();
    let repeat = cycle_calibration::analyze(&trace, config).unwrap();
    assert_eq!(first.classes, repeat.classes);
    let profile = first.profile().unwrap();
    assert_eq!(profile, repeat.profile().unwrap());
    assert!(first.background_submitted > 50 && first.background_completed > 0);
    assert!(first.classes.iter().all(|s| s.samples == 8));
    let input = program();
    let golden = frontend::run(&input).unwrap();
    let mut source = sdram::Adapter {
        service: average::Memory::new(image(&input), profile, Default::default()).unwrap(),
        max_cycles: 20000,
        events: vec![],
    };
    let got = frontend::run_with_memory(&input, &mut source).unwrap();
    assert_eq!(got.outputs, golden.outputs);
    assert_eq!(source.service.bytes(), input.memory);
    // An incomplete calibration cannot silently invent timing for missing sizes.
    assert!(cycle_calibration::analyze(&trace[..1], Default::default())
        .unwrap()
        .profile()
        .is_err());
    let mut overlap = cycle_calibration::Config {
        load: Load::display(1),
        ..Default::default()
    };
    overlap.load.streams[0].base = 0;
    assert!(cycle_calibration::analyze(&trace, overlap)
        .unwrap_err()
        .contains("isolated"));
}
#[test]
fn early_cycle_service_matches_independent_oracle_for_read_write_and_guards() {
    for chained_groups in [false, true] {
        let mut observed = service::Memory::new(
            OracleImage::filled::<0xa5>(0, 8192).unwrap(),
            service::Config {
                init_cycles: 32,
                early_grant: true,
                chained_groups,
                ..Default::default()
            },
        )
        .unwrap();
        let mut expected = oracle::Memory::new(
            OracleImage::filled::<0xa5>(0, 8192).unwrap(),
            Default::default(),
        )
        .unwrap();
        for bytes in [32, 64, 128, 512] {
            // SAFETY: independent external stimulus, not an audited intermediate.
            let data = (0..bytes / 8)
                .map(|n| unsafe {
                    OracleWord::from_host(
                        0xc7e1357902468abdu64 ^ (n as u64 * 0x01010101),
                        "oracle differential stimulus",
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            for request in [
                Request::Write {
                    address: 4096,
                    data,
                    enables: vec![255; bytes / 8],
                },
                Request::Read {
                    address: 4096,
                    bytes,
                },
            ] {
                let mut results = Vec::new();
                for model in [
                    &mut expected as &mut dyn Service,
                    &mut observed as &mut dyn Service,
                ] {
                    model.submit(Client::GpuReadOnly, request.clone()).unwrap();
                    let mut read = Vec::new();
                    let mut complete = 0;
                    for _ in 0..20000 {
                        for event in model.step().unwrap() {
                            match event {
                                Event::ReadBeat {
                                    index, data, last, ..
                                } => read.push((index, data.bits(), last)),
                                Event::Complete { .. } => complete += 1,
                                _ => {}
                            }
                        }
                        if model.idle() {
                            break;
                        }
                    }
                    assert!(model.idle());
                    assert_eq!(complete, 1);
                    results.push(read);
                }
                assert_eq!(results[0], results[1]);
                assert_eq!(expected.bytes(), observed.bytes());
                assert!(observed.bytes()[..4096].iter().all(|&b| b == 0xa5));
                assert!(observed.bytes()[4608..].iter().all(|&b| b == 0xa5));
            }
        }
    }
}

#[test]
fn chained_group_average_profile_is_repeatable_and_reduces_group_latency() {
    use digital_design_hardware_gowin::sdram_memory_controller::sim::{
        calibration, cycle_calibration,
    };
    let trace = calibration::representative_trace(8).unwrap();
    let mut config = cycle_calibration::Config {
        service: service::Config {
            init_cycles: 32,
            early_grant: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let baseline = cycle_calibration::analyze(&trace, config.clone()).unwrap();
    config.service.chained_groups = true;
    let grouped = cycle_calibration::analyze(&trace, config.clone()).unwrap();
    let repeat = cycle_calibration::analyze(&trace, config).unwrap();
    assert_eq!(grouped.classes, repeat.classes);
    let profile = grouped.profile().unwrap();
    assert!(grouped.classes[3].mean_complete() < baseline.classes[3].mean_complete());
    assert!(grouped.classes[7].mean_complete() < baseline.classes[7].mean_complete());
    assert!(profile
        .read_sector_first
        .windows(2)
        .all(|p| p[1] >= p[0] + 16));
    let input = program();
    let golden = frontend::run(&input).unwrap();
    let mut source = sdram::Adapter {
        service: average::Memory::new(image(&input), profile, Default::default()).unwrap(),
        max_cycles: 20000,
        events: vec![],
    };
    assert_eq!(
        frontend::run_with_memory(&input, &mut source)
            .unwrap()
            .outputs,
        golden.outputs
    );
}
