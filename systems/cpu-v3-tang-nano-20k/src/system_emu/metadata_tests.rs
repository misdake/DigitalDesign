//! Read-only producer metadata and physical GPU data deliberately differ.
use super::*;
use gpu_v2::system::vliw_command as abi;
fn pause_at(sim: &mut CpuV3SystemEmu, channel: u8) {
    for _ in 0..20000 {
        if let Some(out) = sim
            .step_observing(|w, _| !(w.device == 7 && w.channel == channel))
            .unwrap()
        {
            assert!(!out.fault);
            assert!(!out.halted);
        } else {
            return;
        }
    }
    panic!("metadata observation marker not reached within cycle bound");
}
#[test]
fn metadata_observation_preserves_pending_dirty_missed_and_gpu_stale_views() {
    let mut memory = vec![0xa55a; 1 << 22];
    memory[0x6000] = 0xbeef;
    memory[0x5800..0x5900].fill(0x5678);
    memory[0x4000..0x4040].fill(0);
    let params = abi::memcpy_params(0xb000, 0xa000, 512).unwrap();
    for (out, bytes) in memory[0x24000..].iter_mut().zip(params.as_chunks::<2>().0) {
        *out = u16::from_le_bytes(*bytes);
    }
    let commands = [
        abi::Command::wait(true, true),
        abi::Command::FLUSH,
        abi::Command::dma_local(0x48000, 264, 32).unwrap(),
        abi::Command::wait(true, false),
        abi::Command::memcpy(264).unwrap(),
        abi::Command::wait(true, true),
        abi::Command::FLUSH,
        abi::Command::END,
    ];
    for (out, command) in memory[0x20000..]
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(commands)
    {
        out[0] = command.0 as u16;
        out[1] = (command.0 >> 16) as u16;
    }
    let mut code = Vec::new();
    code.extend(load_immediate16(1, 1));
    code.extend(load_immediate16(2, 0x4002));
    code.extend(load_immediate16(0, 0x1234));
    code.push(store(0, 2, 0));
    code.push(device_send(1, 7, 1));
    code.extend(load_immediate16(2, 0x5000));
    code.push(load(4, 2, 0));
    code.extend(load_immediate16(0, (commands.len() - 1) as u16));
    code.push(device_send(0, 4, 0));
    code.extend(load_immediate16(0, (0x40000u32 / 128) as u16));
    code.push(device_send(0, 4, 1));
    code.push(device_send(1, 7, 2));
    code.push(device_send(1, 0, 4));
    code.push(load(5, 2, 0));
    code.push(device_send(1, 0, 1));
    code.push(load(6, 2, 0));
    code.push(halt());
    memory[..code.len()].copy_from_slice(&code);
    let mut sim = CpuV3SystemEmu::new(memory);
    sim.attach_device(4, Box::new(GpuDevice::functional()));
    sim.attach_device(7, Box::<crate::ObservationDevice>::default());
    pause_at(&mut sim, 1);
    assert_eq!(
        sim.core()
            .pending_scalar_store_word(PhysicalWordAddress::new(0x4002)),
        Some(0x1234)
    );
    assert_eq!(sim.physical_memory()[0x4002], 0);
    assert_eq!(sim.metadata_words(0x4002, 1).unwrap(), [0x1234]);
    sim.step().unwrap();
    pause_at(&mut sim, 2);
    assert!(sim.gpu().last_error().is_none());
    assert_eq!(sim.physical_memory()[0x4002], 0);
    assert_eq!(sim.physical_memory()[0x5000], 0x5678);
    let cycles = sim.cycles();
    let native = sim.native_cycles();
    let retired = sim.core().retired_words();
    let regs = sim.core().architectural_gprs();
    let physical = sim.physical_memory().to_vec();
    for _ in 0..16 {
        assert_eq!(sim.metadata_words(0x4002, 1).unwrap(), [0x1234]);
        assert_eq!(sim.metadata_words(0x6000, 1).unwrap(), [0xbeef]);
        assert_eq!(sim.metadata_words(0x5000, 1).unwrap(), [0xa55a]);
        assert_eq!(sim.physical_memory()[0x5000], 0x5678);
        assert_eq!(
            sim.physical_memory()[0x4002],
            0,
            "metadata observation never publishes the dirty header"
        );
    }
    assert!(sim.metadata_words(1 << 22, 1).is_err());
    assert!(sim.metadata_words(usize::MAX, 2).is_err());
    assert_eq!(sim.cycles(), cycles);
    assert_eq!(sim.native_cycles(), native);
    assert_eq!(sim.core().retired_words(), retired);
    assert_eq!(sim.core().architectural_gprs(), regs);
    assert!(sim.physical_memory().as_ref() == physical.as_slice());
    sim.step().unwrap();
    let mut halted = false;
    for _ in 0..20000 {
        let out = sim.step().unwrap();
        assert!(!out.fault);
        if out.halted {
            halted = true;
            break;
        }
    }
    assert!(halted);
    let gpr = sim.core().architectural_gprs();
    assert_eq!((gpr[4], gpr[5], gpr[6]), (0xa55a, 0xa55a, 0x5678));
    assert_eq!(sim.physical_memory()[0x4002], 0x1234);
    assert_eq!(sim.physical_memory()[0x5000], 0x5678);
    let marker = sim.device::<crate::ObservationDevice>(7).unwrap();
    assert_eq!((marker.writes(1), marker.writes(2)), (1, 1));
}
