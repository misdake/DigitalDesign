//! Independent single-active read DMA. The completion token is emitted on an
//! edge after the last scratchpad write, never on the last bus response edge.

use std::collections::VecDeque;

use crate::scratchpad::SCRATCH_BYTES;
use crate::timing::{MemoryTiming, ReadBurst, ReadMemory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaDesc {
    pub physical_addr: usize,
    pub scratchpad_addr: usize,
    pub byte_count: usize,
    pub completion_token: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaError {
    BadDescriptor,
    QueueFull,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DmaCycle {
    pub scratch_write: Option<(usize, u64)>,
    pub completed_token: Option<u8>,
    pub request_accepted: bool,
    pub accepted_burst: Option<ReadBurst>,
    pub response_accepted: bool,
}

#[derive(Clone, Copy, Debug)]
struct Active {
    desc: DmaDesc,
    bytes_done: usize,
    burst_beats_remaining: usize,
    request_pending: bool,
}

#[derive(Clone, Debug)]
pub struct DmaEngine {
    pub memory: ReadMemory,
    memory_bytes: usize,
    queue: VecDeque<DmaDesc>,
    active: Option<Active>,
    completion_next: Option<u8>,
}

impl DmaEngine {
    pub fn new(words: Vec<u64>, timing: MemoryTiming, seed: u64) -> Self {
        let memory_bytes = words.len() * 8;
        Self {
            memory: ReadMemory::new(words, timing, seed),
            memory_bytes,
            queue: VecDeque::new(),
            active: None,
            completion_next: None,
        }
    }

    pub fn enqueue(&mut self, desc: DmaDesc) -> Result<(), DmaError> {
        if desc.completion_token > 3
            || desc.byte_count == 0
            || !desc.byte_count.is_multiple_of(32)
            || !desc.physical_addr.is_multiple_of(32)
            || desc
                .physical_addr
                .checked_add(desc.byte_count)
                .is_none_or(|end| end > self.memory_bytes)
            || !desc.scratchpad_addr.is_multiple_of(32)
            || desc
                .scratchpad_addr
                .checked_add(desc.byte_count)
                .is_none_or(|end| end > SCRATCH_BYTES)
        {
            return Err(DmaError::BadDescriptor);
        }
        if self.queue.len() == 4 {
            return Err(DmaError::QueueFull);
        }
        self.queue.push_back(desc);
        Ok(())
    }

    pub fn is_idle(&self) -> bool {
        self.queue.is_empty()
            && self.active.is_none()
            && self.completion_next.is_none()
            && self.memory.is_idle()
    }

    fn next_burst(active: Active) -> ReadBurst {
        let address = active.desc.physical_addr + active.bytes_done;
        let remaining = active.desc.byte_count - active.bytes_done;
        let row_remaining = 1024 - address % 1024;
        let bytes = [128, 64, 32]
            .into_iter()
            .find(|&length| {
                length <= remaining && length <= row_remaining && address.is_multiple_of(length)
            })
            .expect("32-byte aligned DMA has a legal burst");
        ReadBurst {
            address,
            beats: bytes / 8,
        }
    }

    pub fn tick(&mut self) -> DmaCycle {
        let mut out = DmaCycle {
            completed_token: self.completion_next.take(),
            ..DmaCycle::default()
        };
        if self.active.is_none() {
            if let Some(desc) = self.queue.pop_front() {
                self.active = Some(Active {
                    desc,
                    bytes_done: 0,
                    burst_beats_remaining: 0,
                    request_pending: true,
                });
            }
        }
        let request = self
            .active
            .filter(|active| active.request_pending)
            .map(Self::next_burst);
        let cycle = self.memory.tick(request, true);
        out.request_accepted = cycle.request_accepted;
        out.accepted_burst = cycle.request_accepted.then(|| request.unwrap());
        out.response_accepted = cycle.response_accepted;
        if let Some(active) = self.active.as_mut() {
            if cycle.request_accepted {
                active.request_pending = false;
                active.burst_beats_remaining = request.unwrap().beats;
            }
            if let Some(beat) = cycle.response.filter(|_| cycle.response_accepted) {
                assert!(active.burst_beats_remaining > 0);
                let address = (active.desc.scratchpad_addr + active.bytes_done) / 8;
                out.scratch_write = Some((address, beat.data));
                active.bytes_done += 8;
                active.burst_beats_remaining -= 1;
                assert_eq!(beat.last, active.burst_beats_remaining == 0);
                if active.burst_beats_remaining == 0 {
                    if active.bytes_done == active.desc.byte_count {
                        self.completion_next = Some(active.desc.completion_token);
                        self.active = None;
                    } else {
                        active.request_pending = true;
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
    use crate::scratchpad::Scratchpad;

    #[test]
    fn split_burst_completion_follows_last_scratch_write() {
        let words = (0..64).map(|index| index as u64 ^ 0xabcde).collect();
        let mut dma = DmaEngine::new(
            words,
            MemoryTiming {
                grant_wait: 2,
                first_beat: 3,
                beat_gap: 1,
                jitter: 2,
            },
            19,
        );
        dma.enqueue(DmaDesc {
            physical_addr: 0,
            scratchpad_addr: 256,
            byte_count: 160,
            completion_token: 1,
        })
        .unwrap();
        let mut scratch = Scratchpad::new(0xffff);
        let mut last_write = 0;
        let mut completion = 0;
        for edge in 1..=200 {
            let cycle = dma.tick();
            scratch.tick(None, None, None, cycle.scratch_write);
            if cycle.scratch_write.is_some() {
                last_write = edge;
            }
            if cycle.completed_token == Some(1) {
                completion = edge;
                break;
            }
        }
        assert_eq!(completion, last_write + 1);
        for index in 0..20 {
            assert_eq!(scratch.inspect_word(32 + index), index as u64 ^ 0xabcde);
        }
        assert_eq!(scratch.inspect_word(52), u64::MAX);
    }

    #[test]
    fn burst_planner_respects_one_kibibyte_source_row() {
        let words = (0..256).map(|index| index as u64).collect();
        let mut dma = DmaEngine::new(
            words,
            MemoryTiming {
                grant_wait: 0,
                first_beat: 1,
                beat_gap: 0,
                jitter: 0,
            },
            1,
        );
        dma.enqueue(DmaDesc {
            physical_addr: 992,
            scratchpad_addr: 0,
            byte_count: 160,
            completion_token: 2,
        })
        .unwrap();
        let mut scratch = Scratchpad::new(0xffff);
        let mut accepted = Vec::new();
        let mut done = false;
        for _ in 0..80 {
            let cycle = dma.tick();
            if let Some(burst) = cycle.accepted_burst {
                accepted.push(burst);
            }
            scratch.tick(None, None, None, cycle.scratch_write);
            if cycle.completed_token == Some(2) {
                done = true;
                break;
            }
        }
        assert!(done);
        assert_eq!(
            accepted,
            [
                ReadBurst {
                    address: 992,
                    beats: 4
                },
                ReadBurst {
                    address: 1024,
                    beats: 16
                }
            ]
        );
        for index in 0..20 {
            assert_eq!(scratch.inspect_word(index), (124 + index) as u64);
        }
    }
}
