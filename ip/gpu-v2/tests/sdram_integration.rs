#[path = "support/sdram/mod.rs"]
mod sdram;
use digital_design_hardware_gowin::sdram_memory_controller::{
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
