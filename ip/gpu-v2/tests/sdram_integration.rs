#[path = "support/sdram/mod.rs"]
mod sdram;
use digital_design_hardware_gowin::sdram_memory_controller::{ports::*, sim::oracle, sim::service};
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
fn frontend_oracle_runs_on_functional_stream_reference() {
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
    assert!(
        adapter
            .events
            .iter()
            .filter(|e| matches!(e, Event::ReadBeat { .. }))
            .count()
            == 8
    );
}
#[test]
fn insufficient_tail_cover_is_a_real_source_error_not_zero_padding() {
    let input = program();
    let short = OracleImage::filled::<0>(0x1000, 48).unwrap();
    let model = oracle::Memory::new(short, Default::default()).unwrap();
    let mut source = sdram::Adapter {
        service: model,
        max_cycles: 10000,
        events: vec![],
    };
    assert!(frontend::run_with_memory(&input, &mut source).is_err());
}

#[test]
fn functional_stream_service_matches_independent_data_oracle() {
    {
        let mut observed = service::Memory::new(
            OracleImage::filled::<0xa5>(0, 8192).unwrap(),
            service::Config {
                init_cycles: 32,
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
                let first_difference = expected
                    .bytes()
                    .iter()
                    .zip(observed.bytes())
                    .position(|(a, b)| a != b);
                assert!(
                    first_difference.is_none(),
                    "memory mismatch at {first_difference:?}"
                );
                assert!(observed.bytes()[..4096].iter().all(|&b| b == 0xa5));
                assert!(observed.bytes()[4608..].iter().all(|&b| b == 0xa5));
            }
        }
    }
}
