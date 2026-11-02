//! Bounded functional bring-up before the independent full-closure RTL replay.
use digital_design_hardware_gowin::sdram_memory_controller::cycle::{
    gear::{Input, LogicOutput},
    memory::Memory,
};
#[derive(Clone, Copy)]
struct Job {
    address: u32,
    words: u8,
    write: bool,
    mask: u8,
}
fn word(job: usize, index: u32) -> u32 {
    0x7788_1122u32
        .wrapping_add((job as u32) << 16)
        .wrapping_add(index * 0x101)
}
fn get(memory: &[u16], address: u32) -> u32 {
    let a = address as usize * 2;
    u32::from(memory[a]) | u32::from(memory[a + 1]) << 16
}
pub struct Run {
    pub trace: Vec<(Input, LogicOutput)>,
    pub writes: Vec<(u32, u8, u32)>,
    pub final_words: Vec<u32>,
}
pub fn run() -> Run {
    let mut jobs = vec![
        Job {
            address: 0x40,
            words: 8,
            write: true,
            mask: 0,
        },
        Job {
            address: 0x40,
            words: 8,
            write: false,
            mask: 0,
        },
        Job {
            address: 0x41,
            words: 1,
            write: true,
            mask: 10,
        },
        Job {
            address: 0x40,
            words: 8,
            write: false,
            mask: 0,
        },
    ];
    for mask in 0..16 {
        jobs.push(Job {
            address: 0x44 + u32::from(mask % 4) * 32 + u32::from(mask / 4) * 1024,
            words: 1,
            write: true,
            mask,
        });
        jobs.push(Job {
            address: 0x44 + u32::from(mask % 4) * 32 + u32::from(mask / 4) * 1024,
            words: 1,
            write: false,
            mask: 0,
        });
    }
    let mut backing = vec![0xbad0; 1 << 22];
    let mut reference = vec![0xbad0_bad0u32; 1 << 21];
    for address in 0..4096 {
        let value = 0x1000_0000 | address as u32;
        backing[address * 2] = value as u16;
        backing[address * 2 + 1] = (value >> 16) as u16;
        reference[address] = value;
    }
    let mut memory = Memory::new(true, 32);
    let mut trace = Vec::new();
    let mut writes = Vec::new();
    let mut idle = 0;
    let mut job = 0;
    let mut accepted = false;
    let mut sent = 0u8;
    let mut received = 0u8;
    for cycle in 0..3000 {
        if job == jobs.len() {
            let i = Input::default();
            memory.clock_logic(&i, &mut backing).unwrap();
            trace.push((i, memory.output(&i)));
            idle += 1;
            if idle == 1200 {
                for (a, v) in reference.iter().enumerate() {
                    assert_eq!(get(&backing, a as u32), *v, "final backing guard {a}");
                }
                return Run {
                    trace,
                    writes,
                    final_words: reference,
                };
            }
            continue;
        }
        let j = jobs[job];
        let i = Input {
            reset: cycle < 2,
            request_valid: !accepted,
            writing: j.write,
            address: j.address,
            words: j.words,
            write_mask: j.mask,
            write_data_valid: accepted && j.write && sent < j.words.div_ceil(2),
            write_data: u64::from(word(job, u32::from(sent) * 2))
                | u64::from(word(job, u32::from(sent) * 2 + 1)) << 32,
            ..Default::default()
        };
        let o = memory.output(&i);
        if i.request_valid && o.request_ready {
            accepted = true;
            if j.write {
                for index in 0..j.words {
                    let a = (j.address + u32::from(index)) as usize;
                    let mut mask = 0;
                    for byte in 0..4 {
                        if j.mask & (1 << byte) == 0 {
                            mask |= 255u32 << (8 * byte);
                        }
                    }
                    reference[a] = (reference[a] & !mask) | (word(job, u32::from(index)) & mask);
                    writes.push((a as u32, j.mask, reference[a]));
                }
            }
        }
        if i.write_data_valid && o.write_data_ready {
            sent += 1;
        }
        let complete = if j.write {
            if o.write_done {
                for index in 0..j.words {
                    let a = j.address + u32::from(index);
                    assert_eq!(
                        get(&backing, a),
                        reference[a as usize],
                        "completion before commit: job {job} word {index}"
                    );
                }
                true
            } else {
                false
            }
        } else if o.read_valid {
            let a = j.address + u32::from(received) * 2;
            let expected = u64::from(reference[a as usize])
                | if j.words == 1 {
                    0
                } else {
                    u64::from(reference[a as usize + 1]) << 32
                };
            assert_eq!(o.read_data, expected, "read job {job} pair {received}");
            received += 1;
            received == j.words.div_ceil(2)
        } else {
            false
        };
        memory
            .clock_logic(&i, &mut backing)
            .unwrap_or_else(|e| panic!("cycle {cycle}, job {job}: {e:?}"));
        trace.push((i, memory.output(&i)));
        if complete {
            job += 1;
            accepted = false;
            sent = 0;
            received = 0;
        }
    }
    panic!("native closure exceeded 3000 logic cycles at job {job}");
}
