//! Clocked resources for the GPU development model.
//!
//! BSRAM and DSP edge counts reproduce the measured legacy primitive path.
//! External memory is a seeded service scenario: arbitration, SDRAM rows and
//! refresh are not reduced to a universal constant.

use crate::fixed::{mul_signed_36, NumericFault};

pub const BSRAM_READ_EDGES: u64 = 1;
pub const MULT36_PRODUCT_EDGES: u64 = 3;
pub const MULT18_PRODUCT_EDGES: u64 = 2;

/// The two-register signed 18x18 trial path. Tags retire in issue order.
#[derive(Clone, Debug, Default)]
pub struct Mult18Pipeline {
    stages: [Option<(u64, i64)>; MULT18_PRODUCT_EDGES as usize],
}

impl Mult18Pipeline {
    pub fn tick(
        &mut self,
        issue: Option<(u64, i32, i32)>,
    ) -> Result<Option<(u64, i64)>, NumericFault> {
        let product = issue
            .map(|(tag, a, b)| {
                let bounds = -(1 << 17)..=(1 << 17) - 1;
                if !bounds.contains(&a) || !bounds.contains(&b) {
                    return Err(NumericFault::OutOfRange);
                }
                Ok((tag, i64::from(a) * i64::from(b)))
            })
            .transpose()?;
        self.stages.rotate_right(1);
        self.stages[0] = product;
        Ok(self.stages[1].take())
    }

    pub fn is_empty(&self) -> bool {
        self.stages.iter().all(Option::is_none)
    }
}

// Xorshift has an absorbing zero state. Keep zero a valid, reproducible user
// seed without silently disabling latency variation.
fn rng_state(seed: u64) -> u64 {
    if seed == 0 {
        0x6a09_e667_f3bc_c909
    } else {
        seed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RamRead {
    Data(u64),
    /// Same-address mixed-port read/write data is deliberately unspecified.
    Collision,
}

/// One synchronous read port and one write port. The result changes on the
/// accepting edge; with no read enable the output register holds its value.
#[derive(Clone, Debug)]
pub struct SyncRam64 {
    words: Vec<u64>,
    output: RamRead,
}

impl SyncRam64 {
    pub fn new(words: Vec<u64>) -> Self {
        Self {
            words,
            output: RamRead::Data(0),
        }
    }

    pub fn output(&self) -> RamRead {
        self.output
    }

    pub fn tick(&mut self, read: Option<usize>, write: Option<(usize, u64)>) -> RamRead {
        if let Some(address) = read {
            assert!(
                address < self.words.len(),
                "BSRAM read address out of range"
            );
            self.output = if write.is_some_and(|(written, _)| written == address) {
                RamRead::Collision
            } else {
                RamRead::Data(self.words[address])
            };
        }
        if let Some((address, value)) = write {
            assert!(
                address < self.words.len(),
                "BSRAM write address out of range"
            );
            self.words[address] = value;
        }
        self.output
    }
}

/// A signed 36x36 multiplier with A/B, pipeline, and output registers.
/// It accepts one tagged product on each edge and retires it on edge three.
#[derive(Clone, Debug, Default)]
pub struct Mult36Pipeline {
    stages: [Option<(u64, i128)>; MULT36_PRODUCT_EDGES as usize],
}

impl Mult36Pipeline {
    pub fn tick(
        &mut self,
        issue: Option<(u64, i64, i64)>,
    ) -> Result<Option<(u64, i128)>, NumericFault> {
        let product = issue
            .map(|(tag, a, b)| mul_signed_36(a, b).map(|value| (tag, value)))
            .transpose()?;
        self.stages.rotate_right(1);
        self.stages[0] = product;
        Ok(self.stages[2].take())
    }

    pub fn is_empty(&self) -> bool {
        self.stages.iter().all(Option::is_none)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryTiming {
    /// Idle edges before a request is accepted.
    pub grant_wait: u32,
    /// Edges after acceptance before the first 64-bit beat is offered.
    pub first_beat: u32,
    /// Idle edges inserted between accepted beats.
    pub beat_gap: u32,
    /// Uniform bounded additional wait, selected from the supplied seed.
    pub jitter: u32,
}

impl MemoryTiming {
    pub fn validate(self) {
        assert!(self.first_beat >= 1);
        assert!(self.jitter <= 1024);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadBurst {
    /// Byte address. Requests are 32, 64, or 128 bytes and aligned to size.
    pub address: usize,
    pub beats: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadBeat {
    pub data: u64,
    pub last: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemoryCycle {
    pub request_accepted: bool,
    pub response: Option<ReadBeat>,
    pub response_accepted: bool,
}

#[derive(Clone, Debug)]
struct ActiveRead {
    burst: ReadBurst,
    next: usize,
    wait: u32,
    offered: Option<ReadBeat>,
}

/// Single-outstanding 64-bit read service. A stalled beat remains stable.
#[derive(Clone, Debug)]
pub struct ReadMemory {
    words: Vec<u64>,
    timing: MemoryTiming,
    rng: u64,
    active: Option<ActiveRead>,
    grant_wait: u32,
    pub cycle: u64,
}

impl ReadMemory {
    pub fn new(words: Vec<u64>, timing: MemoryTiming, seed: u64) -> Self {
        timing.validate();
        Self {
            words,
            timing,
            rng: rng_state(seed),
            active: None,
            grant_wait: timing.grant_wait,
            cycle: 0,
        }
    }

    fn variation(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        if self.timing.jitter == 0 {
            0
        } else {
            (self.rng % (u64::from(self.timing.jitter) + 1)) as u32
        }
    }

    pub fn is_idle(&self) -> bool {
        self.active.is_none()
    }

    pub fn tick(&mut self, request: Option<ReadBurst>, response_ready: bool) -> MemoryCycle {
        self.cycle += 1;
        let mut out = MemoryCycle::default();
        if let Some(active) = self.active.as_mut() {
            if active.wait > 0 {
                active.wait -= 1;
            } else {
                if active.offered.is_none() {
                    active.offered = Some(ReadBeat {
                        data: self.words[active.burst.address / 8 + active.next],
                        last: active.next + 1 == active.burst.beats,
                    });
                }
                out.response = active.offered;
                if response_ready {
                    out.response_accepted = true;
                    active.next += 1;
                    if active.next == active.burst.beats {
                        self.active = None;
                        self.grant_wait = self.timing.grant_wait + self.variation();
                    } else {
                        let gap = self.timing.beat_gap;
                        let variation = self.variation();
                        let active = self.active.as_mut().unwrap();
                        active.offered = None;
                        active.wait = gap + variation;
                    }
                }
            }
        } else if self.grant_wait > 0 {
            self.grant_wait -= 1;
        } else if let Some(burst) = request {
            assert!([4, 8, 16].contains(&burst.beats));
            assert_eq!(burst.address % (burst.beats * 8), 0);
            assert!(burst.address / 8 + burst.beats <= self.words.len());
            self.active = Some(ActiveRead {
                burst,
                next: 0,
                wait: self.timing.first_beat - 1 + self.variation(),
                offered: None,
            });
            out.request_accepted = true;
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteTiming {
    pub grant_wait: u32,
    /// Idle edges before each data beat may be accepted.
    pub beat_gap: u32,
    /// Idle edges after the last beat before completion is offered.
    pub completion_wait: u32,
    pub jitter: u32,
}

impl WriteTiming {
    pub fn validate(self) {
        assert!(self.jitter <= 1024);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WriteCycle {
    pub request_accepted: bool,
    pub data_ready: bool,
    pub data_accepted: bool,
    pub response_valid: bool,
    pub response_accepted: bool,
}

#[derive(Clone, Debug)]
enum WriteState {
    Idle,
    Data {
        burst: ReadBurst,
        next: usize,
        wait: u32,
    },
    Response {
        wait: u32,
    },
}

/// Single-outstanding 64-bit write service. Descriptor acceptance moves no
/// data. Each source beat is committed only when ready and valid coincide.
#[derive(Clone, Debug)]
pub struct WriteMemory {
    words: Vec<u64>,
    timing: WriteTiming,
    rng: u64,
    grant_wait: u32,
    state: WriteState,
    pub cycle: u64,
}

impl WriteMemory {
    pub fn new(words: Vec<u64>, timing: WriteTiming, seed: u64) -> Self {
        timing.validate();
        Self {
            words,
            timing,
            rng: rng_state(seed),
            grant_wait: timing.grant_wait,
            state: WriteState::Idle,
            cycle: 0,
        }
    }

    fn variation(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        if self.timing.jitter == 0 {
            0
        } else {
            (self.rng % (u64::from(self.timing.jitter) + 1)) as u32
        }
    }

    pub fn words(&self) -> &[u64] {
        &self.words
    }
    pub fn is_idle(&self) -> bool {
        matches!(self.state, WriteState::Idle)
    }

    pub fn tick(
        &mut self,
        request: Option<ReadBurst>,
        data: Option<u64>,
        response_ready: bool,
    ) -> WriteCycle {
        self.cycle += 1;
        let mut out = WriteCycle::default();
        match &mut self.state {
            WriteState::Idle => {
                if self.grant_wait > 0 {
                    self.grant_wait -= 1;
                } else if let Some(burst) = request {
                    assert!([4, 8, 16].contains(&burst.beats));
                    assert_eq!(burst.address % (burst.beats * 8), 0);
                    assert!(burst.address / 8 + burst.beats <= self.words.len());
                    self.state = WriteState::Data {
                        burst,
                        next: 0,
                        wait: self.timing.beat_gap,
                    };
                    out.request_accepted = true;
                }
            }
            WriteState::Data { burst, next, wait } => {
                if *wait > 0 {
                    *wait -= 1;
                } else {
                    out.data_ready = true;
                    if let Some(word) = data {
                        self.words[burst.address / 8 + *next] = word;
                        out.data_accepted = true;
                        *next += 1;
                        if *next == burst.beats {
                            self.state = WriteState::Response {
                                wait: self.timing.completion_wait,
                            };
                        } else {
                            let variation = self.variation();
                            if let WriteState::Data { wait, .. } = &mut self.state {
                                *wait = self.timing.beat_gap + variation;
                            }
                        }
                    }
                }
            }
            WriteState::Response { wait } => {
                if *wait > 0 {
                    *wait -= 1;
                } else {
                    out.response_valid = true;
                    if response_ready {
                        out.response_accepted = true;
                        self.state = WriteState::Idle;
                        self.grant_wait = self.timing.grant_wait + self.variation();
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bsram_edge_hold_and_collision() {
        let mut ram = SyncRam64::new(vec![0x12, 0x34]);
        assert_eq!(ram.tick(Some(1), None), RamRead::Data(0x34));
        assert_eq!(ram.tick(None, Some((1, 0x56))), RamRead::Data(0x34));
        assert_eq!(ram.tick(Some(1), Some((1, 0x78))), RamRead::Collision);
        assert_eq!(ram.tick(Some(1), None), RamRead::Data(0x78));
    }

    #[test]
    fn dsp_exact_three_edge_retirement_and_full_rate() {
        let mut dsp = Mult36Pipeline::default();
        for edge in 1..=5 {
            let issue = (edge <= 3).then_some((edge, -(edge as i64), 7));
            let retired = dsp.tick(issue).unwrap();
            let expected = if edge >= 3 {
                Some((edge - 2, -((edge - 2) as i128) * 7))
            } else {
                None
            };
            assert_eq!(retired, expected, "edge {edge}");
        }
        assert!(dsp.is_empty());
    }

    #[test]
    fn dsp18_exact_two_edge_retirement_and_full_rate() {
        let mut dsp = Mult18Pipeline::default();
        for edge in 1..=5 {
            let issue = (edge <= 3).then_some((edge, -(edge as i32), 9));
            let retired = dsp.tick(issue).unwrap();
            let expected = if (2..=4).contains(&edge) {
                Some((edge - 1, -((edge - 1) as i64) * 9))
            } else {
                None
            };
            assert_eq!(retired, expected, "edge {edge}");
        }
        assert!(dsp.is_empty());
        assert_eq!(
            dsp.tick(Some((10, 1 << 17, 1))),
            Err(NumericFault::OutOfRange)
        );
    }

    #[test]
    fn memory_delay_jitter_and_stall_are_bounded_and_repeatable() {
        let timing = MemoryTiming {
            grant_wait: 2,
            first_beat: 3,
            beat_gap: 1,
            jitter: 2,
        };
        let burst = ReadBurst {
            address: 0,
            beats: 4,
        };
        let mut a = ReadMemory::new(vec![11, 22, 33, 44], timing, 19);
        let mut b = a.clone();
        let mut beats = Vec::new();
        for edge in 1..=40 {
            let request = (edge <= 3).then_some(burst);
            let ready = edge != 10 && edge != 11;
            let left = a.tick(request, ready);
            assert_eq!(left, b.tick(request, ready));
            if left.response_accepted {
                beats.push(left.response.unwrap());
            }
            if beats.len() == 4 {
                break;
            }
        }
        assert_eq!(
            beats.iter().map(|beat| beat.data).collect::<Vec<_>>(),
            [11, 22, 33, 44]
        );
        assert!(beats.last().unwrap().last);
        assert!(a.is_idle());
    }

    #[test]
    fn write_descriptor_moves_no_data_and_completion_holds() {
        let mut memory = WriteMemory::new(
            vec![0xdead; 4],
            WriteTiming {
                grant_wait: 0,
                beat_gap: 1,
                completion_wait: 2,
                jitter: 0,
            },
            1,
        );
        let descriptor = ReadBurst {
            address: 0,
            beats: 4,
        };
        assert!(
            memory
                .tick(Some(descriptor), Some(0), true)
                .request_accepted
        );
        assert_eq!(memory.words(), &[0xdead; 4]);
        let mut accepted = 0;
        for _ in 0..20 {
            let out = memory.tick(None, Some(100 + accepted), false);
            if out.data_accepted {
                accepted += 1;
            }
            if accepted == 4 {
                break;
            }
        }
        assert_eq!(accepted, 4);
        assert_eq!(memory.words(), &[100, 101, 102, 103]);
        let mut completion = None;
        for edge in 1..=10 {
            let out = memory.tick(None, None, false);
            if out.response_valid {
                completion = Some(edge);
                break;
            }
        }
        assert!(completion.is_some());
        for _ in 0..3 {
            let out = memory.tick(None, None, false);
            assert!(out.response_valid);
            assert!(!out.response_accepted);
        }
        assert!(memory.tick(None, None, true).response_accepted);
        assert!(memory.is_idle());
    }

    #[test]
    fn zero_seed_keeps_read_and_write_jitter_active() {
        let mut read = ReadMemory::new(
            vec![0; 4],
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 3,
            },
            0,
        );
        let mut write = WriteMemory::new(
            vec![0; 4],
            WriteTiming {
                grant_wait: 0,
                beat_gap: 0,
                completion_wait: 0,
                jitter: 3,
            },
            0,
        );
        let read_variation = (0..16).map(|_| read.variation()).collect::<Vec<_>>();
        let write_variation = (0..16).map(|_| write.variation()).collect::<Vec<_>>();
        assert_eq!(read_variation, write_variation);
        assert!(read_variation.iter().any(|&value| value != 0));
        assert!(read_variation.contains(&0));
    }
}
