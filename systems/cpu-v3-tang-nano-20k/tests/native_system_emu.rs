//! Actual core/FPU/cache/arbiter/SystemControl against native SDRAM edges.
use cpu_v3::{device_send, halt, load, load_immediate16, store};
use cpu_v3_tang_nano_20k::system_emu::CpuV3SystemEmu;
#[test]
fn fpu_store_remains_dirty_until_explicit_ordered_clean() {
    for clean in [false, true] {
        let mut words = Vec::new();
        words.extend(load_immediate16(1, 0x4000));
        words.extend(load_immediate16(2, 0x4002));
        words.extend(load_immediate16(3, 0x4010));
        words.extend(load_immediate16(0, 1));
        words.push(store(0, 1, 1));
        words.extend(load_immediate16(0, 2));
        words.push(store(0, 2, 1));
        // Same scalar FLD/ADD/FST encodings as the independent system co-sim.
        words.extend([
            0xe100, 0x0800, 0xe200, 0x0c00, 0xd083, 0x1000, 0xe310, 0x0010,
        ]);
        // Adjacent maintenance must drain both accepted FST halves first.
        if clean {
            words.push(device_send(0, 0, 4));
        }
        words.push(load(0, 3, 1));
        words.push(halt());
        let mut backing = vec![0u16; 1 << 22];
        backing[..words.len()].copy_from_slice(&words);
        backing[0x4010] = 0xbad0;
        backing[0x4011] = 0xbad0;
        let start = std::time::Instant::now();
        let mut machine = CpuV3SystemEmu::new(backing);
        let (mut stopped, mut commands) = (false, 0);
        for _ in 0..15000 {
            let o = machine.step().unwrap();
            assert!(!o.fault, "fault {} at {:x}", o.fault_code, o.fault_pc);
            if o.device_write_enable && o.device_index == 0 && o.device_channel == 4 {
                commands += 1;
            }
            if o.halted {
                assert_eq!(o.halt_signal, 3);
                stopped = true;
                break;
            }
        }
        assert!(stopped, "CPU native run exceeded15000 logic cycles");
        assert_eq!(
            commands,
            usize::from(clean),
            "maintenance event executes once"
        );
        assert_eq!(
            &machine.physical_memory()[0x4010..0x4012],
            if clean { &[0, 3] } else { &[0xbad0, 0xbad0] },
            "no implicit cache publication"
        );
        assert_eq!(machine.native_cycles(), machine.cycles() * 2);
        assert!(
            machine.cycles() > 10800,
            "default native initialization is real"
        );
        eprintln!(
            "native CPU clean={clean}: {} logic / {} native, host {:?}",
            machine.cycles(),
            machine.native_cycles(),
            start.elapsed()
        );
    }
}

#[test]
fn instruction_invalidate_clears_btc_and_queued_old_code() {
    use cpu_v3::jump_register;
    fn call(words: &mut Vec<u16>) {
        let return_pc = (words.len() + load_immediate16(2, 0).len() + 1) as u16;
        words.extend(load_immediate16(2, return_pc));
        words.push(jump_register(1));
    }
    let target = 0x100;
    let mut words = Vec::new();
    words.extend(load_immediate16(1, target));
    call(&mut words);
    call(&mut words);
    let patched = load_immediate16(0, 2);
    words.extend(load_immediate16(3, patched[1]));
    words.push(store(3, 1, 1));
    words.push(device_send(0, 0, 4)); // publish instruction bytes to DRAM
    words.push(device_send(0, 0, 0)); // discard I-cache, fetch payload and BTC
    call(&mut words);
    words.push(halt());
    let mut backing = vec![0u16; 1 << 22];
    backing[..words.len()].copy_from_slice(&words);
    let function = load_immediate16(0, 1);
    backing[target as usize..target as usize + 2].copy_from_slice(&function);
    backing[target as usize + 2] = jump_register(2);
    let mut machine = CpuV3SystemEmu::new(backing);
    let mut stopped = false;
    for _ in 0..15000 {
        let o = machine.step().unwrap();
        assert!(!o.fault, "fault at {:x}", o.fault_pc);
        if o.halted {
            assert_eq!(
                machine.physical_memory()[target as usize + 1],
                patched[1],
                "explicit clean publishes new opcode"
            );
            assert_eq!(
                o.halt_signal, 2,
                "old BTC/payload must not replay after instruction invalidation"
            );
            stopped = true;
            break;
        }
    }
    assert!(stopped, "self-modifying code exceeded15000 logic cycles");
}
