//! Actual native32 tap: masks, held pads, native phase and on/off equivalence.
#[path = "native_support/traffic.rs"]
mod traffic;
use digital_design_hardware_gowin::sdram_memory_controller::cycle::memory::Memory;
#[test]
fn native_activity_counts_real_slots_once_and_keeps_execution_identical() {
    let run = traffic::run();
    let mut observed = Memory::new(true, 32);
    let mut plain = observed.clone();
    let mut a = vec![0xbad0; 1 << 22];
    for address in 0..4096 {
        let value = 0x1000_0000 | address as u32;
        a[address * 2] = value as u16;
        a[address * 2 + 1] = (value >> 16) as u16;
    }
    let mut b = a.clone();
    let (mut reads, mut writes, mut unmasked) = (0u64, 0u64, 0u64);
    assert!(run.trace.len() < 3000);
    assert!(!run.writes.is_empty() && !run.final_words.is_empty());
    for (i, _) in run.trace {
        assert_eq!(observed.output(&i), plain.output(&i));
        for half in 0..4 {
            plain.clock_half(&i, &mut b).unwrap();
            let n = observed.clock_half_observing(&i, &mut a).unwrap();
            assert!(n.read_bytes == 0 || half % 2 == 0);
            assert!(n.write_slot_bytes == 0 || half % 2 == 1);
            assert!(n.write_unmasked_bytes <= n.write_slot_bytes);
            reads += u64::from(n.read_bytes);
            writes += u64::from(n.write_slot_bytes);
            unmasked += u64::from(n.write_unmasked_bytes);
            assert_eq!(observed.output(&i), plain.output(&i));
        }
    }
    assert_eq!((reads, writes, unmasked), (128, 100, 66));
    assert_eq!(a, b);
    assert_eq!(observed.native_cycle(), plain.native_cycle());
}
