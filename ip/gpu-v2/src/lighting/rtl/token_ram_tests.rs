use super::token_read_offset;
use std::collections::VecDeque;

#[test]
fn synchronous_ring_matches_shift_fifo_for_every_phase_wrap_and_ce_pause() {
    for ii in 1..=3 {
        for depth in 4_usize..=64 {
            for capture in 0..ii {
                for consume in 0..ii {
                    let words = (depth + 1).next_power_of_two();
                    let mut memory: Vec<_> =
                        (0..words).map(|i| 0x90000000_u64 + i as u64).collect();
                    let mut read = 0_u64;
                    for epoch in 0..2 {
                        // Context/reset invalidates ownership; RAM and read
                        // payload deliberately retain the previous epoch.
                        let mut pointer = 0_usize;
                        let mut reference = VecDeque::from(vec![u64::MAX; depth]);
                        let mut cycles = 0;
                        for wall in 0..4096 {
                            let phase = cycles % ii;
                            if phase == consume && cycles > depth * ii {
                                assert_eq!(
                                    read,
                                    reference[depth - 1],
                                    "ii={ii},D={depth},W={capture},R={consume},cycle={cycles}"
                                );
                            }
                            if wall % 7 == 0 || wall % 19 == 0 {
                                continue;
                            }
                            let mut next_read = read;
                            if phase == (consume + ii - 1) % ii {
                                let offset = token_read_offset(depth, phase == capture);
                                let address = pointer.wrapping_sub(offset) & (words - 1);
                                if phase == capture {
                                    assert_ne!(address, pointer, "no read/write collision");
                                }
                                next_read = memory[address];
                            }
                            if phase == capture {
                                let value =
                                    (cycles as u64).wrapping_mul(0x9e3779b9) ^ (epoch << 48);
                                memory[pointer] = value;
                                pointer = (pointer + 1) & (words - 1);
                                reference.push_front(value);
                                reference.pop_back();
                            }
                            read = next_read;
                            cycles += 1;
                            if cycles > depth * ii + 80 {
                                break;
                            }
                        }
                        assert!(cycles > depth * ii + 80, "cycle watchdog");
                    }
                }
            }
        }
    }
}
