use gpu_v2::vertex::{
    ports::*,
    sim::{counted, oracle, timed},
};
#[test]
fn vertex_stages_dense_and_signed() {
    let context = Context {
        base: [-32768, 12345, -100000],
        mvp: [
            [50001, -34567, 12289, 8001],
            [45678, 12345, -65536, 14123],
            [72345, -33456, 27411, 23455],
            [10012, -10034, 10111, 65536],
        ],
        normal_matrix: [[11585, -11585, 0], [11585, 11585, 0], [0, 0, 16384]],
        ..Context::default()
    };
    let v = PackedVertex::encode([1000, 200, 600], [-127, 90, 3], [4095, 2000], 0xf81f).unwrap();
    let reference = oracle::run(&context, v, &oracle::Config::default()).unwrap();
    let counted = counted::run(&context, v).unwrap();
    assert_eq!(reference.output, counted.output);
    for (name, golden) in reference.clip_sum.iter().enumerate() {
        assert_eq!(
            counted
                .frame
                .outputs
                .iter()
                .find(|o| o.name == format!("vertex.0.clip.sum.{name}"))
                .unwrap()
                .raw,
            *golden
        );
    }
    for (name, golden) in reference.normal_sum.iter().enumerate() {
        assert_eq!(
            counted
                .frame
                .outputs
                .iter()
                .find(|o| o.name == format!("vertex.0.normal.sum.{name}"))
                .unwrap()
                .raw,
            *golden
        );
    }
    for (wide, narrow) in [(1, 1), (1, 2), (2, 1), (1, 3), (2, 2)] {
        let plan = timed::run(
            &context,
            &[v; 8],
            timed::Hardware {
                wide,
                narrow,
                ..timed::Hardware::default()
            },
        )
        .unwrap();
        plan.audit().unwrap();
        assert!(plan.counted.outputs.iter().all(|v| *v == reference.output));
    }
}
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
#[test]
fn frontend_three_stages_and_ce_freeze() {
    use gpu_v2::frontend::sim::{counted, oracle, timed};
    let input = sample_program();
    let oracle = oracle::run(&input).unwrap();
    let counted = counted::run(&input).unwrap();
    assert_eq!(oracle.outputs, counted.output.outputs);
    let plan = timed::run(
        &input,
        timed::Config::default(),
        &(1..14).collect::<Vec<_>>(),
    )
    .unwrap();
    plan.audit().unwrap();
    assert_eq!(plan.outputs, oracle.outputs);
    assert!(plan.fence);
    assert!(plan.fault.is_none());
    assert!(plan
        .records
        .iter()
        .any(|r| r.cycle < 14 && matches!(r.action, timed::Action::DmaWrite { .. })));
    assert_eq!(plan.slots[0].rows[21], 0);
    assert!(plan.scratchpad.banks.iter().all(|b| b[5] == 0));
}
#[test]
fn bounded_random_vertex_stage_goldens() {
    let mut seed = 0x1234_5678_u64;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for sample in 0..256 {
        let mut context = Context {
            base: std::array::from_fn(|_| (random() % 200001) as i32 - 100000),
            grid_shift: (random() % 9) as u8,
            mvp: std::array::from_fn(|_| {
                std::array::from_fn(|_| (random() % 200001) as i32 - 100000)
            }),
            ..Context::default()
        };
        if sample % 2 == 0 {
            context.normal_matrix = [[0, -16384, 0], [16384, 0, 0], [0, 0, 16384]];
        }
        let packed = PackedVertex::encode(
            std::array::from_fn(|_| (random() % 1024) as u16),
            std::array::from_fn(|_| random() as i8),
            std::array::from_fn(|_| (random() % 4096) as u16),
            random() as u16,
        )
        .unwrap();
        let o = oracle::run(&context, packed, &oracle::Config::default()).unwrap();
        let c = counted::run(&context, packed).unwrap();
        assert_eq!(o.output, c.output);
        for (prefix, goldens) in [
            ("position", o.position.map(i128::from).to_vec()),
            ("normal.input", o.input_normal.map(i128::from).to_vec()),
            ("clip.sum", o.clip_sum.to_vec()),
            ("normal.sum", o.normal_sum.to_vec()),
        ] {
            for (i, golden) in goldens.iter().enumerate() {
                assert_eq!(
                    c.frame
                        .outputs
                        .iter()
                        .find(|o| o.name == format!("vertex.0.{prefix}.{i}"))
                        .unwrap()
                        .raw,
                    *golden
                );
            }
        }
    }
}
#[test]
fn signed_ties_and_output_overflow_contracts() {
    let mut c = Context {
        grid_shift: 0,
        mvp: [
            [32768, 0, 0, 0],
            [98304, 0, 0, 0],
            [-32768, 0, 0, 0],
            [-98304, 0, 0, 0],
        ],
        ..Context::default()
    };
    let v = PackedVertex::encode([1, 0, 0], [127, -128, 0], [0, 4095], 0xffff).unwrap();
    assert_eq!(
        oracle::run(&c, v, &oracle::Config::default())
            .unwrap()
            .output
            .clip,
        [0, 2, 0, -2]
    );
    assert_eq!(counted::run(&c, v).unwrap().output.clip, [0, 2, 0, -2]);
    c.mvp = Context::default().mvp;
    c.base[0] = i32::MAX - 1023;
    let v = PackedVertex::encode([1023, 0, 0], [0; 3], [0; 2], 0).unwrap();
    assert_eq!(counted::run(&c, v).unwrap().output.clip[0], i32::MAX);
    c.mvp[0][0] = 65537;
    assert!(counted::run(&c, v).is_err());
    assert!(oracle::run(&c, v, &oracle::Config::default()).is_err());
}
#[test]
fn invalid_v6_and_matrix_contracts_reject() {
    let mut v = PackedVertex::encode([0; 3], [0; 3], [0; 2], 0).unwrap();
    v.0[0] |= 1;
    assert!(counted::run(&Context::default(), v).is_err());
    let mut c = Context::default();
    c.normal_matrix[0][0] = 20000;
    assert!(c.validate().is_err());
    c = Context::default();
    c.grid_shift = 31;
    assert!(c.validate().is_err());
    assert!(PackedVertex::encode([1024, 0, 0], [0; 3], [0; 2], 0).is_err());
}
#[test]
fn static_plan_tamper_regressions() {
    let v = PackedVertex::encode([100, 200, 300], [1, 20, -80], [4095, 0], 0x13f7).unwrap();
    let mut p = timed::run(&Context::default(), &[v], timed::Hardware::default()).unwrap();
    p.layout.banks[0].width += 1;
    assert!(p.audit().unwrap_err().contains("layout"));
    p.layout.banks[0].width -= 1;
    p.dsp.instances[0].latency += 1;
    assert!(p.audit().unwrap_err().contains("DSP inventory"));
    p.dsp.instances[0].latency -= 1;
    p.retained.peak_bits += 1;
    assert!(p.audit().unwrap_err().contains("retained"));
    p.retained.peak_bits -= 1;
    p.rom[0].cycle += 1;
    assert!(p.audit().unwrap_err().contains("ROM"));
    p.rom[0].cycle -= 1;
    p.publication[0] -= 1;
    assert!(p.audit().unwrap_err().contains("publication"));
    p.publication[0] += 1;
    p.counted.outputs[0].uv[0] ^= 1;
    assert!(p.audit().unwrap_err().contains("attribute"));
}
#[test]
fn scratchpad_ownership_epochs_and_core_write() {
    use gpu_v2::scratchpad::{
        ports::*,
        sim::{
            counted::{self, Transaction},
            oracle::Scratchpad,
            timed::{self, Transfer},
        },
    };
    let mut s = Scratchpad::default();
    let d = DmaDescriptor {
        physical_addr: 0,
        scratchpad_addr: 4088,
        byte_count: 8,
        completion_token: 3,
    };
    let lease = s.reserve(d).unwrap();
    assert!(s.reserve(d).is_err());
    assert!(s.acquire(lease).is_err());
    s.dma_beat(lease, 0x1234_5678_9abc_def0).unwrap();
    assert!(s.read64(lease, 4088).is_err());
    s.complete(lease).unwrap();
    s.acquire(lease).unwrap();
    s.write64(lease, 4088, 0xfedc_ba98_7654_3210).unwrap();
    assert_eq!(s.read64(lease, 4088).unwrap(), 0xfedc_ba98_7654_3210);
    s.release(lease).unwrap();
    let newer = s.reserve(d).unwrap();
    assert_ne!(newer.epoch, lease.epoch);
    assert!(s.dma_beat(lease, 0).is_err());
    assert!(s.complete(newer).is_err());
    let transactions = [
        Transaction::DmaWrite {
            address: 4088,
            data: 0x1234_5678_9abc_def0,
        },
        Transaction::DmaWrite {
            address: 4096,
            data: 0xa55a_91fe_7734_cde0,
        },
        Transaction::CoreWrite {
            address: 4088,
            data: 0xfedc_ba98_7654_3210,
        },
        Transaction::CoreRead { address: 4088 },
        Transaction::CoreRead { address: 4096 },
    ];
    let r = counted::run(&transactions).unwrap();
    assert_eq!(r.reads, vec![0xfedc_ba98_7654_3210, 0xa55a_91fe_7734_cde0]);
    let transfers = (0..5)
        .map(|i| Transfer {
            transaction: i,
            issue: i as u64 * 2,
            ready: i as u64 * 2 + 1,
        })
        .collect::<Vec<_>>();
    timed::audit(&r, &transfers, 32).unwrap();
    let mut bad = transfers.clone();
    bad[1].issue = 0;
    bad[1].ready = 1;
    assert!(timed::audit(&r, &bad, 32).is_err());
}
#[test]
fn command_guards_reject_malformed_and_unsupported() {
    use gpu_v2::{
        command_processor::{ports::Command, sim::counted},
        scratchpad::ports::DmaDescriptor,
    };
    for d in [
        DmaDescriptor {
            physical_addr: 0,
            scratchpad_addr: 4088,
            byte_count: 16,
            completion_token: 0,
        },
        DmaDescriptor {
            physical_addr: u64::MAX - 7,
            scratchpad_addr: 0,
            byte_count: 8,
            completion_token: 0,
        },
        DmaDescriptor {
            physical_addr: 1,
            scratchpad_addr: 0,
            byte_count: 8,
            completion_token: 0,
        },
        DmaDescriptor {
            physical_addr: 0,
            scratchpad_addr: 0,
            byte_count: 0,
            completion_token: 0,
        },
    ] {
        assert!(counted::run(&[Command::Dma(d)], 4).is_err());
    }
    for command in [
        Command::Wait(4),
        Command::Unsupported(99),
        Command::Draw {
            region: 0,
            byte_offset: 4092,
            vertices: 1,
            context: Context::default(),
        },
    ] {
        assert!(counted::run(&[command], 4).is_err());
    }
}
#[test]
fn dma_fault_is_sticky_drains_and_never_fences() {
    use gpu_v2::{
        command_processor::ports::Command,
        frontend::{
            ports::Input,
            sim::timed::{self, Action},
        },
        scratchpad::ports::{DmaDescriptor, Owner},
    };
    let input = Input {
        commands: vec![
            Command::Dma(DmaDescriptor {
                physical_addr: 0x1000,
                scratchpad_addr: 0,
                byte_count: 32,
                completion_token: 0,
            }),
            Command::Wait(0),
            Command::Fence,
        ],
        memory_base: 0x1000,
        memory: vec![0x5a; 8],
    };
    let r = timed::run(&input, timed::Config::default(), &[]).unwrap();
    r.audit().unwrap();
    assert!(r.fault.as_ref().unwrap().contains("source response"));
    assert!(!r.fence);
    assert_eq!(r.scratchpad.regions[0].owner, Owner::Faulted);
    assert!(r
        .records
        .iter()
        .any(|r| matches!(r.action, Action::DmaFaultComplete { .. })));
    assert!(!r
        .records
        .iter()
        .any(|r| matches!(r.action, Action::DmaComplete { .. })));
    assert!(r.scratchpad.banks.iter().all(|b| b[1] == 0));
}
#[test]
fn frontend_trace_tamper_checks_data_publication_and_events() {
    use gpu_v2::frontend::sim::timed::{self, Action};
    let mut r = timed::run(&sample_program(), timed::Config::default(), &[]).unwrap();
    let row = r
        .records
        .iter_mut()
        .find(|r| matches!(r.action, Action::DmaWrite { .. }))
        .unwrap();
    if let Action::DmaWrite { data, .. } = &mut row.action {
        *data ^= 1;
    }
    assert!(r.audit().unwrap_err().contains("provenance"));
    if let Action::DmaWrite { data, .. } = &mut r
        .records
        .iter_mut()
        .find(|r| matches!(r.action, Action::DmaWrite { .. }))
        .unwrap()
        .action
    {
        *data ^= 1;
    }
    let index = r
        .records
        .iter()
        .position(|r| matches!(r.action, Action::Publish { .. }))
        .unwrap();
    r.records[index].core_cycle -= 1;
    assert!(r.audit().is_err());
    r.records[index].core_cycle += 1;
    r.events[0].pending ^= 1;
    assert!(r.audit().unwrap_err().contains("event"));
}
#[test]
fn slot_publication_is_not_release_and_reuse_increments_epoch() {
    use gpu_v2::{command_processor::ports::EventState, frontend::ports::Slot};
    let mut slot = Slot::default();
    assert_eq!(slot.allocate().unwrap(), 1);
    slot.ready[0] = true;
    assert!(slot.allocate().is_err());
    assert!(slot.release(1).is_err());
    slot.finish_production(1).unwrap();
    assert!(slot.release(2).is_err());
    slot.release(1).unwrap();
    assert_eq!(slot.allocate().unwrap(), 2);
    assert!(!slot.ready[0]);
    let mut events = EventState::default();
    events.update(0x91, 0x91);
    assert_eq!(events.pending, 0x91);
    assert_eq!(events.take(false), None);
    let selected = (0..3)
        .map(|_| events.take(true).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(selected, vec![0, 4, 7]);
    assert_eq!(events.pending, 0x91);
}
#[test]
fn maximum_cycles_bounds_indefinite_wait() {
    use gpu_v2::{
        command_processor::ports::Command,
        frontend::{ports::Input, sim::timed},
    };
    let input = Input {
        commands: vec![Command::Wait(0)],
        memory_base: 0,
        memory: Vec::new(),
    };
    assert!(timed::run(
        &input,
        timed::Config {
            max_cycles: 20,
            ..timed::Config::default()
        },
        &[]
    )
    .err()
    .unwrap()
    .contains("maximum cycle"));
}
#[test]
fn bounded_multi_draw_explicit_release_and_slot_reuse() {
    use gpu_v2::{
        command_processor::ports::Command,
        frontend::sim::{counted, oracle, timed},
    };
    let mut input = sample_program();
    let first = input.commands[0].clone();
    let wait = Command::Wait(0);
    let draw = input.commands[2].clone();
    input.commands = vec![
        first.clone(),
        wait.clone(),
        draw.clone(),
        first.clone(),
        wait.clone(),
        draw.clone(),
        Command::Release { slot: 0, epoch: 1 },
        first,
        wait,
        draw,
        Command::Fence,
    ];
    let o = oracle::run(&input).unwrap();
    let c = counted::run(&input).unwrap();
    let t = timed::run(&input, timed::Config::default(), &[66, 67, 68, 100, 101]).unwrap();
    assert_eq!(o.outputs, c.output.outputs);
    assert_eq!(o.outputs, t.outputs);
    t.audit().unwrap();
    assert_eq!(
        t.outputs
            .iter()
            .map(|d| (d.slot, d.epoch))
            .collect::<Vec<_>>(),
        vec![(0, 1), (1, 1), (0, 2)]
    );
    input.commands.remove(6);
    assert!(timed::run(
        &input,
        timed::Config {
            max_cycles: 1000,
            ..timed::Config::default()
        },
        &[]
    )
    .err()
    .unwrap()
    .contains("maximum cycle"));
}
#[test]
fn asynchronous_second_region_dma_runs_during_vertex_microop() {
    use gpu_v2::{
        command_processor::ports::Command,
        frontend::sim::timed::{self, Action},
        scratchpad::ports::DmaDescriptor,
    };
    let mut input = sample_program();
    let second = input.memory.clone();
    input.memory.extend(second);
    input.memory.resize(200, 0xc3);
    let draw = input.commands[2].clone();
    input.commands = vec![
        input.commands[0].clone(),
        Command::Dma(DmaDescriptor {
            physical_addr: 0x1028,
            scratchpad_addr: 4096,
            byte_count: 160,
            completion_token: 1,
        }),
        Command::Wait(0),
        draw,
        Command::Wait(1),
        Command::Draw {
            region: 1,
            byte_offset: 0,
            vertices: 3,
            context: Context::default(),
        },
        Command::Fence,
    ];
    let r = timed::run(&input, timed::Config::default(), &[]).unwrap();
    r.audit().unwrap();
    let first = r
        .records
        .iter()
        .find(|r| matches!(r.action,Action::DrawStart{lease,..} if lease.region==0))
        .unwrap()
        .cycle;
    let end = r
        .records
        .iter()
        .find(|r| matches!(r.action,Action::DrawDone{lease,..} if lease.region==0))
        .unwrap()
        .cycle;
    assert!(r.records.iter().any(|r| r.cycle > first
        && r.cycle < end
        && matches!(r.action,Action::DmaWrite{lease,..} if lease.region==1)));
    assert_eq!(r.outputs[0].vertices, r.outputs[1].vertices);
}
#[test]
fn vertex_overflow_sets_fault_without_partial_publication() {
    use gpu_v2::{
        command_processor::ports::Command,
        frontend::sim::timed::{self, Action},
    };
    let mut input = sample_program();
    if let Command::Draw { context, .. } = &mut input.commands[2] {
        context.base[0] = 1 << 30;
        context.mvp[0][0] = 1 << 17;
    }
    let r = timed::run(&input, timed::Config::default(), &[]).unwrap();
    r.audit().unwrap();
    assert!(r.fault.is_some());
    assert!(r.outputs.is_empty());
    assert!(!r.fence);
    assert!(!r.records.iter().any(|r| matches!(
        r.action,
        Action::Publish { .. } | Action::VertexWrite { .. }
    )));
}
#[test]
fn counted_packets_cross_beats_from_high_half_without_host_payload_math() {
    use gpu_v2::{
        command_processor::ports::Command,
        frontend::sim::{counted, oracle, timed},
    };
    let mut input = sample_program();
    let payload = input.memory[..36].to_vec();
    input.memory = vec![0xde, 0xad, 0xbe, 0xef];
    input.memory.extend(payload);
    assert_eq!(input.memory.len(), 40);
    if let Command::Draw { byte_offset, .. } = &mut input.commands[2] {
        *byte_offset = 4;
    }
    let o = oracle::run(&input).unwrap();
    let c = counted::run(&input).unwrap();
    let t = timed::run(&input, timed::Config::default(), &[]).unwrap();
    assert_eq!(o.outputs, c.output.outputs);
    assert_eq!(o.outputs, t.outputs);
    assert_eq!(
        c.scratchpad.vertices,
        t.scratchpad_counted.as_ref().unwrap().vertices
    );
    let halves = c
        .commands
        .outputs
        .iter()
        .filter(|o| o.name.ends_with("high_half"))
        .map(|o| o.raw)
        .collect::<Vec<_>>();
    assert_eq!(halves, vec![1, 0, 1]);
    assert_eq!(c.scratchpad.vertices.len(), 3);
}
