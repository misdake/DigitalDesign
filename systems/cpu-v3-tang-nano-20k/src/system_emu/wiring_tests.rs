//! CPU boundary tests use an explicitly controlled read peer, not an MC model.
use super::wiring::{CpuSide, DataResponse, Input, InstructionResponse};
use cpu_v3::{device_receive, device_send, halt, load, load_immediate16};
use std::collections::{BTreeMap, VecDeque};

#[test]
fn settling_has_no_edge_and_startup_hold_preserves_wall_clock_and_cache_sweep() {
    let mut cpu = CpuSide::new(7);
    let mut input = Input {
        reset: true,
        startup_hold: true,
        ..Input::default()
    };
    cpu.clock(&input).unwrap();
    cpu.clock(&input).unwrap();
    input.reset = false;
    let before = cpu.settle(&input).unwrap();
    for _ in 0..32 {
        assert_eq!(cpu.settle(&input).unwrap(), before);
    }
    assert_eq!(cpu.read_system_channel(11), 0);
    let retired = cpu.core().retired_words();
    for _ in 0..256 {
        let output = cpu.clock(&input).unwrap();
        assert_eq!(output.core.code_segment, 7);
    }
    let output = cpu.settle(&input).unwrap();
    assert!(
        !output.dcache.valid_sweep,
        "cache must clear while the core is held"
    );
    assert_eq!(cpu.core().retired_words(), retired);
    assert!(
        cpu.read_system_channel(11) >= 4,
        "SystemControl must tick while memory/startup holds CPU"
    );
    let elapsed = cpu.read_system_channel(11);
    for _ in 0..32 {
        cpu.settle(&input).unwrap();
    }
    assert_eq!(cpu.read_system_channel(11), elapsed);
}

struct Burst {
    ready_at: usize,
    beats: VecDeque<u64>,
}
fn burst(memory: &BTreeMap<u32, u16>, address: u32, now: usize) -> Burst {
    assert_eq!(address & 15, 0);
    Burst {
        ready_at: now + 3,
        beats: (0..4)
            .map(|beat| {
                (0..4).fold(0, |v, lane| {
                    v | (u64::from(*memory.get(&(address + beat * 4 + lane)).unwrap_or(&0))
                        << (lane * 16))
                })
            })
            .collect(),
    }
}
fn response(pending: &Option<Burst>, now: usize) -> (bool, u64) {
    pending
        .as_ref()
        .filter(|b| now >= b.ready_at)
        .map_or((false, 0), |b| (true, b.beats[0]))
}
fn consume(pending: &mut Option<Burst>, valid: bool, ready: bool) {
    if valid && ready {
        let b = pending.as_mut().unwrap();
        b.beats.pop_front();
        if b.beats.is_empty() {
            *pending = None;
        }
    }
}
#[test]
fn physical_u64_beats_cseg_and_frozen_mmio_watch_inputs_follow_one_commit() {
    for segment in [0u16, 7] {
        let mut code = load_immediate16(1, 0x108).to_vec();
        code.push(load(0, 1, 7)); // Highest16 bits of the last64-bit fill beat.
        code.extend(load_immediate16(2, 6 | (3 << 3)));
        code.push(device_send(2, 0, 6));
        code.extend(load_immediate16(3, 0));
        code.push(device_send(3, 0, 7));
        code.push(device_receive(4, 6, 2));
        code.push(device_send(4, 7, 1));
        code.push(halt());
        let base = u32::from(segment) << 16;
        let mut memory: BTreeMap<u32, u16> = code
            .into_iter()
            .enumerate()
            .map(|(i, w)| (base + i as u32, w))
            .collect();
        for i in 0..16 {
            memory.insert(0x100 + i, 0xa100 + i as u16);
        }
        let mut cpu = CpuSide::new(segment);
        let (mut instruction, mut data) = (None, None);
        let (mut watch_ticks, mut reads, mut writes, mut stopped) = (0, 0, Vec::new(), false);
        for cycle in 0..2000 {
            let (iv, iw) = response(&instruction, cycle);
            let (dv, dw) = response(&data, cycle);
            let mut input = Input {
                reset: cycle < 2,
                startup_hold: cycle < 20,
                memory_initialized: cycle >= 4,
                instruction: InstructionResponse {
                    request_ready: instruction.is_none() && !cycle.is_multiple_of(4),
                    response_valid: iv,
                    read_data: iw,
                    error: false,
                },
                data: DataResponse {
                    request_ready: data.is_none() && !cycle.is_multiple_of(5),
                    response_valid: dv,
                    read_data: dw,
                    ..DataResponse::default()
                },
                watch_read_data: u16::from(watch_ticks >= 5),
                ..Input::default()
            };
            let probe = cpu.settle(&input).unwrap();
            if probe.core.device_read_enable && probe.core.device_index == 6 {
                input.external_device_read_data = 0xcafe;
            }
            let before = cpu.settle(&input).unwrap();
            assert_eq!(
                cpu.settle(&input).unwrap(),
                before,
                "settle must not consume peer data"
            );
            let output = cpu.clock(&input).unwrap();
            assert_eq!(output, before, "commit returns frozen pre-edge observation");
            assert!(
                !output.core.fault,
                "fault {} at {:x}",
                output.core.fault_code, output.core.fault_pc
            );
            if input.reset {
                instruction = None;
                data = None;
                continue;
            }
            consume(&mut instruction, iv, output.icache.memory_response_ready);
            consume(&mut data, dv, output.dcache.memory_response_ready);
            if input.instruction.request_ready && output.icache.memory_request_valid {
                assert_eq!(output.icache.memory_address >> 16, u64::from(segment));
                instruction = Some(burst(&memory, output.icache.memory_address as u32, cycle));
            }
            if input.data.request_ready && output.dcache.memory_request_valid {
                assert!(!output.dcache.memory_write);
                assert!(output.dcache.memory_line);
                data = Some(burst(&memory, output.dcache.memory_address as u32, cycle));
            }
            if output.system.watch_read_enable {
                assert_eq!(
                    (
                        output.system.watch_device_index,
                        output.system.watch_device_channel
                    ),
                    (6, 3)
                );
                watch_ticks += 1;
            }
            if output.core.device_index == 6 && output.core.device_read_enable {
                reads += 1;
            }
            if output.core.device_index == 7 && output.core.device_write_enable {
                writes.push(output.core.device_write_data);
            }
            if output.core.halted {
                assert_eq!(output.core.halt_signal, 0xa10f);
                stopped = true;
                break;
            }
        }
        assert!(stopped, "bounded controlled-peer CPU run did not halt");
        assert!(watch_ticks >= 5);
        assert_eq!(reads, 1);
        assert_eq!(writes, [0xcafe]);
    }
}
