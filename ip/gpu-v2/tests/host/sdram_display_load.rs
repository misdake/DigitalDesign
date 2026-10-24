//! Bounded active-phase offered-load check, not production buffer/CDC qualification.
use digital_design_hardware_gowin::sdram_memory_controller::{emu::service, ports::*};
use std::collections::BTreeMap;

struct Job {
    client: Client,
    release: u64,
    deadline: Option<u64>,
    bytes: usize,
    writing: bool,
    beats: usize,
}

fn submit(
    memory: &mut service::Memory,
    jobs: &mut BTreeMap<u64, Job>,
    client: Client,
    burst: Burst,
    release: u64,
    deadline: Option<u64>,
) {
    let Burst {
        address,
        bytes,
        access,
    } = burst;
    let writing = access == Access::Write;
    let request = if writing {
        Request::Write {
            address,
            data: vec![OracleWord::constant::<0x123456789abcdef0>(); bytes / 8],
            enables: vec![255; bytes / 8],
        }
    } else {
        Request::Read { address, bytes }
    };
    let id = memory.submit(client, request).unwrap();
    assert!(jobs
        .insert(
            id,
            Job {
                client,
                release,
                deadline,
                bytes,
                writing,
                beats: 0
            }
        )
        .is_none());
}

#[test]
fn bounded_active_display_batches_meet_assumed_deadlines_under_cpu_and_gpu_load() {
    // Two source rows per release. Floor the rational 2x interval conservatively.
    let active_2x = 4 * 1056 * 54_000_000_u64 / 33_300_000;
    let active_3x = 6 * 1650 * 54_000_000_u64 / 74_250_000;
    assert_eq!((active_2x, active_3x), (6849, 7200));
    for (name, early_grant, chained_groups) in [
        ("serial", false, false),
        ("early", true, false),
        ("group", true, true),
    ] {
        for period in [active_2x, active_3x] {
            for phase in [0, 73] {
                let mut memory = service::Memory::new(
                    OracleImage::filled::<0xa5>(0, 65536).unwrap(),
                    service::Config {
                        init_cycles: 32,
                        early_grant,
                        chained_groups,
                        max_cycles: 100_000,
                        max_requests: 65536,
                        max_queued: 512,
                    },
                )
                .unwrap();
                while !memory.combination.bridge.output(false).initialized {
                    memory.step().unwrap();
                }
                let origin = memory.cycle();
                let window = period * 7 + phase;
                let mut jobs = BTreeMap::new();
                let mut display_index = 0_u64;
                let mut cpu_index = [0_u64; 2];
                let mut gpu_index = [0_u64; 2];
                let mut gpu_pending = [0_usize; 2];
                let mut display_completed = 0;
                let mut max_display_wait = 0;
                let mut max_active_wait = 0;
                let mut max_display_complete = 0;
                let mut max_initial_complete = 0;
                let mut gpu_completed = 0;
                let mut misses = 0;
                for _ in 0..100_000 {
                    let t = memory.cycle() - origin;
                    if t < window {
                        // Initial four-row fill is 100 segments. Its deadline is an
                        // explicit test assumption; real scanout start is not modeled.
                        let count = if t == 0 {
                            100
                        } else if t >= period + phase && (t - phase).is_multiple_of(period) {
                            50
                        } else {
                            0
                        };
                        for _ in 0..count {
                            submit(
                                &mut memory,
                                &mut jobs,
                                Client::Display,
                                Burst {
                                    address: 0x4000 + display_index * 32 % 8192,
                                    bytes: 32,
                                    access: Access::Read,
                                },
                                t,
                                Some(t + period),
                            );
                            display_index += 1;
                        }
                        for (i, (client, cadence, offset, base, stride)) in [
                            (Client::Instruction, 1728, 17, 0xa000, 32),
                            (Client::Data, 288, 43, 0xc000, 4096 + 32),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            if t >= offset && (t - offset).is_multiple_of(cadence) {
                                let index = cpu_index[i];
                                submit(
                                    &mut memory,
                                    &mut jobs,
                                    client,
                                    Burst {
                                        address: base + index * stride % 4096,
                                        bytes: 32,
                                        access: if i == 1 && (index + 1).is_multiple_of(3) {
                                            Access::Write
                                        } else {
                                            Access::Read
                                        },
                                    },
                                    t,
                                    None,
                                );
                                cpu_index[i] += 1;
                            }
                        }
                        for (i, client) in [Client::FramebufferRead, Client::FramebufferWrite]
                            .into_iter()
                            .enumerate()
                        {
                            // At most two complete payloads/sinks outstanding per
                            // direction, kept saturated through actual completions.
                            if gpu_pending[i] < 2 {
                                submit(
                                    &mut memory,
                                    &mut jobs,
                                    client,
                                    Burst {
                                        address: if i == 0 {
                                            gpu_index[i] * 512 % 4096
                                        } else {
                                            0x8000 + gpu_index[i] * 512 % 8192
                                        },
                                        bytes: 512,
                                        access: if i == 1 { Access::Write } else { Access::Read },
                                    },
                                    t,
                                    None,
                                );
                                gpu_index[i] += 1;
                                gpu_pending[i] += 1;
                            }
                        }
                    }
                    for event in memory.step().unwrap() {
                        match event {
                            Event::Started { id, cycle, .. } => {
                                let job = &jobs[&id];
                                if job.client == Client::Display {
                                    max_display_wait =
                                        max_display_wait.max(cycle - origin - job.release);
                                    if job.release != 0 {
                                        max_active_wait =
                                            max_active_wait.max(cycle - origin - job.release);
                                    }
                                }
                            }
                            Event::ReadBeat {
                                id,
                                index,
                                data,
                                last,
                                ..
                            } => {
                                let job = jobs.get_mut(&id).unwrap();
                                assert!(!job.writing);
                                assert_eq!(index, job.beats);
                                job.beats += 1;
                                assert_eq!(last, job.beats == job.bytes / 8);
                                if matches!(job.client, Client::Display | Client::FramebufferRead) {
                                    assert_eq!(data.bits(), 0xa5a5a5a5a5a5a5a5);
                                }
                            }
                            Event::Complete { id, cycle } => {
                                let job = jobs.remove(&id).unwrap();
                                assert_eq!(job.beats, if job.writing { 0 } else { job.bytes / 8 });
                                let end = cycle - origin;
                                if job.client == Client::Display {
                                    display_completed += 1;
                                    let latency = end - job.release;
                                    if job.release == 0 {
                                        max_initial_complete = max_initial_complete.max(latency);
                                    } else {
                                        max_display_complete = max_display_complete.max(latency);
                                    }
                                    misses += u64::from(end >= job.deadline.unwrap());
                                }
                                if matches!(
                                    job.client,
                                    Client::FramebufferRead | Client::FramebufferWrite
                                ) {
                                    gpu_pending
                                        [usize::from(job.client == Client::FramebufferWrite)] -= 1;
                                    gpu_completed += 1;
                                }
                            }
                        }
                    }
                    if t >= window && memory.idle() {
                        break;
                    }
                }
                assert!(memory.idle() && jobs.is_empty());
                assert_eq!(display_completed, 400); // initial 100 + six releases of 50
                assert!(gpu_completed > 100);
                assert_eq!(misses, 0, "{name} period={period} phase={phase}");
                println!("ACTIVE_DISPLAY config={name} period={period} phase={phase} display={display_completed} gpu={gpu_completed} max_queue_to_grant={max_display_wait} max_active_queue_to_grant={max_active_wait} max_initial100_complete={max_initial_complete} max_batch50_complete={max_display_complete} deadline_misses={misses}");
            }
        }
    }
}
