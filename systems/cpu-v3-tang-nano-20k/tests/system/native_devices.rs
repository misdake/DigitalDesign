//! Bounded real CPU/native-memory plus shared functional device acceptance.
use cpu_v3::{
    device_receive, device_send, halt, load, load_immediate16, store, write_data_segment_page,
    Device,
};
use cpu_v3_tang_nano_20k::{system_emu::CpuV3SystemEmu, system_sim::VBlankMode};
#[derive(Default)]
struct Probe {
    reads: usize,
    writes: usize,
    observed: u16,
    backing: usize,
}
impl Device for Probe {
    fn read(&mut self, memory: &mut [u16], _: u8) -> u16 {
        assert_eq!(memory.len(), 1 << 22);
        self.reads += 1;
        self.backing = memory.as_ptr() as usize;
        self.observed = memory[0x4000];
        self.observed
    }
    fn write(&mut self, memory: &mut [u16], _: u8, value: u16) {
        self.writes += 1;
        assert_eq!(self.backing, memory.as_ptr() as usize);
        memory[0x4000] = value;
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
fn run_recorded(machine: &mut CpuV3SystemEmu, addresses: &mut Vec<(u32, bool)>) -> u16 {
    for _ in 0..20000 {
        let o = machine.step().unwrap();
        if o.data_request_valid {
            addresses.push((o.data_address as u32, o.data_write));
        }
        assert!(!o.fault, "fault {} at {:x}", o.fault_code, o.fault_pc);
        if o.halted {
            return o.halt_signal as u16;
        }
    }
    panic!("native device test exceeded20000 logic cycles")
}
fn run(machine: &mut CpuV3SystemEmu) -> u16 {
    run_recorded(machine, &mut Vec::new())
}

#[test]
fn device_access_is_once_and_respects_dirty_and_stale_cache() {
    for invalidate in [false, true] {
        let mut words = Vec::new();
        words.extend(load_immediate16(1, 0x4000));
        words.extend(load_immediate16(0, 0x1234));
        words.push(store(0, 1, 0));
        words.push(device_receive(4, 6, 0));
        words.push(device_send(0, 0, 4));
        words.extend(load_immediate16(0, 0x5678));
        words.push(device_send(0, 6, 0));
        if invalidate {
            words.push(device_send(0, 0, 1));
        }
        words.push(load(0, 1, 0));
        words.push(halt());
        let mut backing = vec![0; 1 << 22];
        backing[..words.len()].copy_from_slice(&words);
        backing[0x4000] = 0xbeef;
        let mut machine = CpuV3SystemEmu::new(backing);
        machine.attach_device(6, Box::<Probe>::default());
        assert_eq!(run(&mut machine), if invalidate { 0x5678 } else { 0x1234 });
        let probe = machine.device::<Probe>(6).unwrap();
        assert_eq!((probe.reads, probe.writes, probe.observed), (1, 1, 0xbeef));
        assert_eq!(machine.physical_memory()[0x4000], 0x5678);
        assert_eq!(machine.core().architectural_gprs()[4], 0xbeef);
    }
}
#[test]
fn cb06_cseg7_hardware_watch_and_shared_input_start_together() {
    use cpu_v3_tang_nano_20k::{
        boot::pack_single_application,
        input_device::{ControllerSnapshot, InputMode},
    };
    let mut words = Vec::new();
    words.extend(load_immediate16(0, 11 << 3));
    words.push(device_send(0, 0, 6));
    words.extend(load_immediate16(0, 1));
    words.push(device_send(0, 0, 11));
    words.extend(load_immediate16(0, 0));
    words.push(device_send(0, 0, 7));
    words.push(device_receive(4, 0, 11));
    words.push(device_receive(0, 5, 8));
    words.push(device_send(0, 7, 1));
    words.push(device_receive(5, 7, 1));
    words.push(halt());
    let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let package = pack_single_application(&bytes, 2).unwrap();
    let mut machine =
        CpuV3SystemEmu::from_single_flash(&package, 0xa55a, VBlankMode::Manual).unwrap();
    machine.publish_input(Some(ControllerSnapshot {
        mode: InputMode::Analog,
        buttons_active_low: 0xffff,
        right: [128, 128],
        left: [128, 128],
    }));
    let (mut halted, mut armed, mut retired, mut held) = (false, false, 0, 0);
    for _ in 0..20000 {
        let o = machine.step().unwrap();
        assert!(!o.fault);
        assert_eq!(o.code_segment, 7);
        if o.device_write_enable && o.device_index == 0 && o.device_channel == 7 {
            armed = true;
            retired = machine.core().retired_words();
        } else if armed {
            if o.retired_words == u64::from(retired) {
                held += 1;
            } else {
                armed = false;
            }
        }
        if o.halted {
            assert_eq!(o.halt_signal, 3);
            halted = true;
            break;
        }
    }
    assert!(
        halted && held >= 8,
        "real hardware timer must release watch while CPU is held: held={held}"
    );
    assert!(machine.core().architectural_gprs()[4] >= 1);
    let markers = machine
        .device::<cpu_v3_tang_nano_20k::ObservationDevice>(7)
        .unwrap();
    assert_eq!(markers.last(1), Some(3));
    assert_eq!(markers.writes(1), 1);
    assert_eq!(machine.core().architectural_gprs()[5], 0);
    assert_eq!(&machine.physical_memory()[..3], &[0, 0, 0xa55a]);
}
#[test]
fn actual_functional_gpu_preserves_publication_stale_cache_and_partial_error() {
    use cpu_v3_tang_nano_20k::{GpuDevice, GPU_DEVICE};
    use gpu_v2::{framebuffer::ports::MaterializedSurface, system::vliw_command as abi};
    for (clean, invalidate, fail) in [
        (false, true, false),
        (true, false, false),
        (true, true, false),
        (true, true, true),
    ] {
        let mut memory = vec![0xa55a; 1 << 22];
        let target = abi::target_params(MaterializedSurface {
            color_base_bytes: 0x400000,
            depth_base_bytes: 0x480000,
            width: 400,
            height: 240,
        })
        .unwrap();
        for (out, b) in memory[0x48000 / 2..]
            .iter_mut()
            .zip(target.as_chunks::<2>().0)
        {
            *out = u16::from_le_bytes(*b);
        }
        let mut commands = vec![
            abi::Command::wait(false, true),
            abi::Command::FLUSH,
            abi::Command::dma_local(0x48000, 264, 32).unwrap(),
            abi::Command::wait(true, false),
            abi::Command::set_target(264).unwrap(),
            abi::Command::clear(false, 0x1357),
        ];
        if fail {
            commands.push(abi::Command(0xd0000000));
        } else {
            commands.push(abi::Command::wait(true, true));
            commands.push(abi::Command::FLUSH);
        }
        commands.push(abi::Command::END);
        for (out, cmd) in memory[0x40000 / 2..]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&commands)
        {
            out[0] = cmd.0 as u16;
            out[1] = (cmd.0 >> 16) as u16;
        }
        let mut words = Vec::new();
        words.extend(load_immediate16(2, ((0x400000u32 / 2) >> 14) as u16));
        words.push(write_data_segment_page(0, 2));
        words.extend(load_immediate16(1, 0));
        words.push(load(5, 1, 0)); // warm color cache
        words.extend(load_immediate16(2, ((0x40000u32 / 2) >> 14) as u16));
        words.push(write_data_segment_page(0, 2));
        words.extend(load_immediate16(1, 10));
        words.extend(load_immediate16(
            3,
            abi::Command::clear(false, 0x2468).0 as u16,
        ));
        words.push(store(3, 1, 0));
        if clean {
            words.push(device_send(0, 0, 4));
        }
        words.extend(load_immediate16(0, (commands.len() - 1) as u16));
        words.push(device_send(0, 4, 0));
        words.extend(load_immediate16(0, (0x40000u32 / 128) as u16));
        words.push(device_send(0, 4, 1));
        words.push(device_receive(6, 4, 2));
        if invalidate {
            words.push(device_send(0, 0, 1));
        }
        words.extend(load_immediate16(2, ((0x400000u32 / 2) >> 14) as u16));
        words.push(write_data_segment_page(0, 2));
        words.extend(load_immediate16(1, 0));
        words.push(load(0, 1, 0));
        words.push(halt());
        memory[..words.len()].copy_from_slice(&words);
        let mut machine = CpuV3SystemEmu::new(memory);
        machine.attach_device(GPU_DEVICE, Box::new(GpuDevice::functional()));
        let physical = if clean { 0x2468 } else { 0x1357 };
        let mut addresses = Vec::new();
        let signal = run_recorded(&mut machine, &mut addresses);
        assert!(
            addresses.contains(&(0x200000, false)),
            "CPU actually touches GPU color words"
        );
        assert!(
            addresses.contains(&(0x2000a, true)),
            "CPU actually patches the shared CB clear word"
        );
        eprintln!("GPU clean={clean} invalidate={invalidate} fail={fail}: signal={signal:x} pixel={:x} status={} effects={:?} error={:?} segments={:?}",machine.physical_memory()[0x200000],machine.core().architectural_gprs()[6],machine.gpu().last_effects(),machine.gpu().last_error(),machine.core().data_segments());
        assert_eq!(signal, if invalidate { physical } else { 0xa55a });
        assert_eq!(machine.core().architectural_gprs()[5], 0xa55a);
        assert_eq!(
            machine.core().architectural_gprs()[6],
            if fail { 2 } else { 0 }
        );
        assert!(machine.physical_memory()[0x200000..0x219000]
            .iter()
            .all(|&v| v == physical));
        assert_eq!(machine.physical_memory()[0x240000], 0xa55a);
        assert!(!machine.gpu().busy());
        assert_eq!(machine.gpu().last_error().is_some(), fail);
        if fail {
            assert_eq!(
                machine.gpu().last_error().unwrap().effects,
                machine.gpu().last_effects()
            );
            assert_eq!(machine.gpu().last_error().unwrap().byte_pc, 24);
        }
    }
}

#[test]
fn data_pages_use_16k_words_and_preserve_high_physical_address_bits() {
    // The formerly mistaken page0x20 really addresses word0x80000, not word0x200000.
    let mut words = Vec::new();
    words.extend(load_immediate16(1, 0));
    words.extend(load_immediate16(2, 0x20));
    words.push(write_data_segment_page(0, 2));
    words.push(load(4, 1, 0));
    words.extend(load_immediate16(2, 0x80));
    words.push(write_data_segment_page(0, 2));
    words.push(load(0, 1, 0));
    words.push(halt());
    let mut backing = vec![0; 1 << 22];
    backing[..words.len()].copy_from_slice(&words);
    backing[0x80000] = 0x1111;
    backing[0x200000] = 0x2222;
    let mut machine = CpuV3SystemEmu::new(backing);
    let mut addresses = Vec::new();
    assert_eq!(run_recorded(&mut machine, &mut addresses), 0x2222);
    assert_eq!(machine.core().architectural_gprs()[4], 0x1111);
    assert!(addresses.contains(&(0x80000, false)) && addresses.contains(&(0x200000, false)));
}

fn prefix_memory() -> Vec<u16> {
    use gpu_v2::{framebuffer::ports::MaterializedSurface, system::vliw_command as abi};
    let mut memory = vec![0xa55a; 1 << 22];
    let target = abi::target_params(MaterializedSurface {
        color_base_bytes: 0x400000,
        depth_base_bytes: 0x480000,
        width: 400,
        height: 240,
    })
    .unwrap();
    let copy2 = abi::memcpy_params(0x400000, 0x500000, 1024).unwrap();
    let copy3 = abi::memcpy_params(0x500000, 0x510000, 1024).unwrap();
    for (address, data) in [(0x48000, target), (0x48100, copy2), (0x48200, copy3)] {
        for (out, b) in memory[address / 2..]
            .iter_mut()
            .zip(data.as_chunks::<2>().0)
        {
            *out = u16::from_le_bytes(*b);
        }
    }
    let mut program = Vec::new();
    for (index, param) in [0x48000, 0x48100, 0x48200].into_iter().enumerate() {
        let action = if index == 0 {
            abi::Command::clear(false, 0x1357)
        } else {
            abi::Command::memcpy(264).unwrap()
        };
        let commands = [
            abi::Command::wait(true, true),
            abi::Command::FLUSH,
            abi::Command::dma_local(param, 264, 32).unwrap(),
            abi::Command::wait(true, false),
            if index == 0 {
                abi::Command::set_target(264).unwrap()
            } else {
                abi::Command::NOP
            },
            action,
            abi::Command::wait(true, true),
            abi::Command::FLUSH,
            abi::Command::END,
        ];
        let base = 0x40000 + index * 0x100;
        for (out, cmd) in memory[base / 2..]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(commands)
        {
            out[0] = cmd.0 as u16;
            out[1] = (cmd.0 >> 16) as u16;
        }
        program.extend(load_immediate16(0, (commands.len() - 1) as u16));
        program.push(device_send(0, 4, 0));
        program.extend(load_immediate16(0, (base / 128) as u16));
        // A DEVSEND expires this unused prefix only when the write advances.
        program.push(0xf123);
        program.push(device_send(0, 4, 1));
    }
    program.extend(load_immediate16(0, 0x600d));
    program.push(halt());
    memory[..program.len()].copy_from_slice(&program);
    memory
}
fn assert_prefix_preimage(memory: &[u16]) {
    let mut expected = prefix_memory();
    expected[0x200000..0x219000].fill(0x1357);
    expected[0x280000..0x280200].fill(0x1357);
    assert!(
        memory == expected,
        "only the full CLEAR plane and first memcpy destination may change before target START"
    );
}
#[test]
fn functional_prefix_snapshot_pauses_before_target_and_can_resume_once() {
    use cpu_v3_tang_nano_20k::{GpuDevice, GPU_DEVICE};
    let memory = prefix_memory();
    let mut machine = CpuV3SystemEmu::new(memory);
    machine.attach_device(GPU_DEVICE, Box::new(GpuDevice::functional()));
    let (mut starts, mut snapshot, mut pending, mut paused_cycle) = (0, None, None, None);
    for _ in 0..20000 {
        let cycles = machine.cycles();
        let native = machine.native_cycles();
        let retired = machine.core().retired_words();
        let advanced = machine
            .step_observing(|write, memory| {
                if write.device == 4 && write.channel == 1 {
                    starts += 1;
                    if starts == 3 {
                        assert_eq!(memory[0x280000], 0x1357);
                        assert_eq!(memory[0x288000], 0xa55a);
                        snapshot = Some(memory.to_vec());
                        pending = Some(write);
                        paused_cycle = Some(cycles);
                        return false;
                    }
                }
                true
            })
            .unwrap();
        if advanced.is_none() {
            assert_eq!(machine.cycles(), cycles);
            assert_eq!(machine.native_cycles(), native);
            assert_eq!(machine.core().retired_words(), retired);
            break;
        }
    }
    assert_eq!(starts, 3);
    let snapshot = snapshot.expect("target submission reached within bound");
    assert_prefix_preimage(&snapshot);
    let pending = pending.unwrap();
    assert!(!machine.gpu().busy());
    assert!(machine.gpu().last_error().is_none());
    assert_eq!(&*machine.physical_memory(), &snapshot);
    assert!(machine
        .step_observing(|write, _| {
            assert_eq!(write, pending);
            false
        })
        .unwrap()
        .is_none());
    assert_eq!(machine.cycles(), paused_cycle.unwrap());
    assert_eq!(&*machine.physical_memory(), &snapshot);
    assert_eq!(run(&mut machine), 0x600d);
    assert!(machine.physical_memory()[0x288000..0x288200]
        .iter()
        .all(|&v| v == 0x1357));
    assert_eq!(machine.physical_memory()[0x288200], 0xa55a);
    assert!(machine.gpu().last_error().is_none());
}

#[test]
fn display_uses_shared_pixels_only_after_explicit_vblank() {
    use cpu_v3_tang_nano_20k::{
        DisplayDevice, DISPLAY_DEVICE, FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_HEIGHT,
        FRAMEBUFFER_WIDTH,
    };
    use cpu_v3_tang_nano_20k::{
        DISPLAY_COLOR_FORMAT, DISPLAY_CONTROL, DISPLAY_FRAMEBUFFER_HIGH, DISPLAY_FRAMEBUFFER_LOW,
        DISPLAY_LINEAR_RGB565, DISPLAY_NEXT_SWAP,
    };
    let mut words = Vec::new();
    for (channel, value) in [
        (DISPLAY_COLOR_FORMAT, DISPLAY_LINEAR_RGB565),
        (DISPLAY_FRAMEBUFFER_LOW, FRAMEBUFFER_A_BASE_WORD as u16),
        (
            DISPLAY_FRAMEBUFFER_HIGH,
            (FRAMEBUFFER_A_BASE_WORD >> 16) as u16,
        ),
        (DISPLAY_CONTROL, DISPLAY_NEXT_SWAP),
    ] {
        words.extend(load_immediate16(0, value));
        words.push(device_send(0, DISPLAY_DEVICE, channel));
    }
    words.push(halt());
    let mut backing = vec![0xf800; 1 << 22];
    backing[..words.len()].copy_from_slice(&words);
    let mut machine = CpuV3SystemEmu::new(backing);
    machine.attach_device(DISPLAY_DEVICE, Box::<DisplayDevice>::default());
    run(&mut machine);
    assert!(machine.display_state().swap_pending);
    assert_eq!(machine.display_state().frame_index, 0);
    assert!(machine.render_active_framebuffer().iter().all(|&p| p == 0));
    assert!(machine.advance_vblank());
    assert_eq!(machine.display_state().frame_index, 1);
    assert!(machine
        .device::<DisplayDevice>(DISPLAY_DEVICE)
        .unwrap()
        .linear_rgb565());
    let pixels = machine.render_active_framebuffer();
    assert_eq!(
        pixels.len(),
        FRAMEBUFFER_WIDTH as usize * FRAMEBUFFER_HEIGHT as usize
    );
    assert!(pixels.iter().all(|&p| p == 0xff0000));
}

#[test]
fn instruction_prefix_pause_keeps_pc_and_retirement_then_executes_once() {
    use cpu_v3::{CpuV3Sim, StepOutcome};
    use cpu_v3_tang_nano_20k::{GpuDevice, GPU_DEVICE};
    let mut cpu = CpuV3Sim::with_physical_memory_words(1 << 22);
    cpu.physical_memory_mut().copy_from_slice(&prefix_memory());
    cpu.attach_device(GPU_DEVICE, Box::new(GpuDevice::functional()));
    let (mut starts, mut snapshot, mut pending) = (0, None, None);
    for _ in 0..2000 {
        let pc = cpu.pc();
        let registers = *cpu.registers();
        let retired = cpu.retired_words();
        if cpu
            .step_observing(|write, memory| {
                if write.device == 4 && write.channel == 1 {
                    starts += 1;
                    if starts == 3 {
                        assert_eq!(memory[0x280000], 0x1357);
                        assert_eq!(memory[0x288000], 0xa55a);
                        snapshot = Some(memory.to_vec());
                        pending = Some(write);
                        return false;
                    }
                }
                true
            })
            .unwrap()
            .is_none()
        {
            assert_eq!(cpu.pc(), pc);
            assert_eq!(cpu.registers(), &registers);
            assert_eq!(cpu.retired_words(), retired);
            break;
        }
    }
    assert_eq!(starts, 3);
    let snapshot = snapshot.unwrap();
    assert_prefix_preimage(&snapshot);
    let pending = pending.unwrap();
    assert_eq!(cpu.physical_memory_slice(), &snapshot);
    assert!(!cpu.device::<GpuDevice>(GPU_DEVICE).unwrap().busy());
    assert!(cpu
        .step_observing(|write, _| {
            assert_eq!(write, pending);
            false
        })
        .unwrap()
        .is_none());
    assert_eq!(cpu.physical_memory_slice(), &snapshot);
    let retired = cpu.retired_words();
    assert_eq!(cpu.step().unwrap(), StepOutcome::Running);
    assert_eq!(cpu.retired_words(), retired + 2);
    let mut halted = false;
    for _ in 0..2000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted);
    assert!(cpu.physical_memory_slice()[0x288000..0x288200]
        .iter()
        .all(|&v| v == 0x1357));
    assert_eq!(cpu.physical_memory_slice()[0x288200], 0xa55a);
    assert!(cpu
        .device::<GpuDevice>(GPU_DEVICE)
        .unwrap()
        .last_error()
        .is_none());
}
