//! 2-slot instruction payload queue with two requests and a two-word BTC.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, Module, ModuleIo, SsramBits, TargetResourceRequest};

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionFetchQueueInput {
    pub reset: Wire,
    pub flush: Wire,
    pub core_request_valid: Wire,
    pub core_address: Wires<32>,
    pub core_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<16>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionFetchQueueOutput {
    pub core_request_ready: Wire,
    pub core_response_valid: Wire,
    pub core_read_data: Wires<16>,
    pub core_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_address: Wires<32>,
    pub memory_response_ready: Wire,
}

/// Keeps 2 payload slots and two request-provenance slots. Current bits invalidate
/// late responses across arbitrarily many redirects, without epoch wraparound.
#[derive(Hardware)]
#[hardware(namespace = "components/cpu/cpu_v3", target_leaf)]
pub struct CpuV3InstructionFetchQueue;

/// Fixed resolved-target capacity: four entries of two instruction words.
pub const CPU_V3_BTC_ENTRIES: usize = 4;
const QUEUE_DEPTH: usize = 2;
const REQUEST_SLOTS: usize = 2;
const BTC_STORAGE: usize = CPU_V3_BTC_ENTRIES;

/// Model-only diagnostics, intentionally absent from the hardware ports and
/// frozen benchmark schema. A lookup/hit counts once when a replay starts,
/// even if the core initially backpressures it.
#[derive(Clone, Copy, Default, Debug)]
pub struct CpuV3BtcStatistics {
    pub lookups: u64,
    pub hits: u64,
    pub installed: u64,
    pub cancelled_fills: u64,
    pub accepted_words: u64,
    pub aborted_replays: u64,
    /// Core-request cycles waiting after both BTC words, until the first
    /// ordinary word is accepted or the stream is redirected/flushed.
    pub continuation_wait_cycles: u64,
    pub queue_cycles: [u64; 5],
    pub outstanding_cycles: [u64; 5],
    pub joint_cycles: [[u64; 5]; 5],
    pub direct_words: u64,
    pub queued_words: u64,
    pub demand_wait_cycles: u64,
    pub credit_blocked_cycles: u64,
    pub requests_accepted: u64,
}

#[derive(Clone, Copy, Default)]
struct BtcEntry {
    valid: bool,
    tag: u32,
    words: [u16; 2],
    rank: u8,
}

/// Cycle-accurate register model of `cpu_v3_instruction_fetch_queue.v`.
#[derive(Clone, Default)]
pub struct CpuV3InstructionFetchQueueState {
    stream_valid: bool,
    expected_core_address: u32,
    next_memory_address: u32,
    queue_data: [u16; QUEUE_DEPTH],
    queue_error: [bool; QUEUE_DEPTH],
    queue_head: u8,
    queue_tail: u8,
    queue_count: u8,
    metadata_current: [bool; REQUEST_SLOTS],
    metadata_head: u8,
    metadata_tail: u8,
    metadata_count: u8,
    btc: [BtcEntry; BTC_STORAGE],
    replay_entry: usize,
    replay_remaining: u8,
    fill_phase: u8,
    fill_tag: u32,
    fill_word: u16,
    statistics: CpuV3BtcStatistics,
    tracking_continuation: bool,
}

fn next_word(address: u32, count: u32) -> u32 {
    (address & 0xffff_0000) | (address.wrapping_add(count) & 0xffff)
}

struct FetchSignals {
    output: CpuV3InstructionFetchQueueOutputValue,
    restart: bool,
    hit: Option<usize>,
    btc_response: bool,
    core_pop: bool,
    queue_pop: bool,
    request_fire: bool,
    response_fire: bool,
    enqueue: bool,
}

impl CpuV3InstructionFetchQueueState {
    pub fn btc_statistics(&self) -> CpuV3BtcStatistics {
        self.statistics
    }

    fn signals(&self, input: &CpuV3InstructionFetchQueueInputValue) -> FetchSignals {
        let address = input.core_address as u32;
        let matches = self.stream_valid && address == self.expected_core_address;
        let restart = input.core_request_valid && !matches;
        let hit = if restart && address >> 22 == 0 && !input.flush && !input.reset {
            self.btc[..CPU_V3_BTC_ENTRIES]
                .iter()
                .position(|e| e.valid && e.tag == address)
        } else {
            None
        };
        let btc_response = !input.reset
            && !input.flush
            && input.core_request_valid
            && (hit.is_some() || (!restart && matches && self.replay_remaining != 0));
        let current =
            self.metadata_count != 0 && self.metadata_current[self.metadata_head as usize];
        let queued = self.queue_count != 0;
        // Logical slots may pop/push together. RTL rotates their payloads
        // through four physical RAM rows, so a full replacement cannot
        // overwrite the asynchronous head consumed on this edge.
        let response_ready = !input.reset
            && self.metadata_count != 0
            && (input.flush
                || restart
                || !current
                || self.queue_count < QUEUE_DEPTH as u8
                || (!btc_response
                    && input.core_request_valid
                    && matches
                    && input.core_response_ready));
        let response_fire = input.memory_response_valid && response_ready;
        let bypass = !queued && input.memory_response_valid && current;
        let valid = !input.reset
            && !input.flush
            && input.core_request_valid
            && (btc_response || (!restart && matches && (queued || bypass)));
        let core_pop = valid && input.core_response_ready;
        let queue_pop = core_pop && !btc_response && queued;
        let request_valid = !input.reset
            && !input.flush
            && (restart || self.stream_valid)
            && (self.metadata_count < REQUEST_SLOTS as u8 || response_fire);
        let issue_address = if restart {
            next_word(address, if hit.is_some() { 2 } else { 0 })
        } else {
            self.next_memory_address
        };
        let (data, error) = if btc_response {
            let index = hit.unwrap_or(self.replay_entry);
            (
                self.btc[index].words[usize::from(!(hit.is_some() || self.replay_remaining == 2))],
                false,
            )
        } else if queued {
            (
                self.queue_data[self.queue_head as usize],
                self.queue_error[self.queue_head as usize],
            )
        } else {
            (input.memory_read_data as u16, input.memory_error)
        };
        FetchSignals {
            output: CpuV3InstructionFetchQueueOutputValue {
                core_request_ready: core_pop,
                core_response_valid: valid,
                core_read_data: data.into(),
                core_error: valid && error,
                memory_request_valid: request_valid,
                memory_address: issue_address.into(),
                memory_response_ready: response_ready,
            },
            restart,
            hit,
            btc_response,
            core_pop,
            queue_pop,
            request_fire: request_valid && input.memory_request_ready,
            response_fire,
            enqueue: response_fire
                && current
                && !input.flush
                && !restart
                && !(core_pop && !btc_response && !queued),
        }
    }

    fn touch(&mut self, index: usize, installing: bool) {
        let old_rank = self.btc[index].rank;
        for (i, entry) in self.btc[..CPU_V3_BTC_ENTRIES].iter_mut().enumerate() {
            if i == index {
                entry.rank = 0;
            } else if entry.valid && (installing || entry.rank < old_rank) {
                entry.rank = (entry.rank + 1).min((CPU_V3_BTC_ENTRIES as u8).saturating_sub(1));
            }
        }
    }

    fn clock(&mut self, input: &CpuV3InstructionFetchQueueInputValue) {
        if input.reset {
            *self = Self::default();
            return;
        }
        let sig = self.signals(input);
        self.statistics.queue_cycles[self.queue_count as usize] += 1;
        self.statistics.outstanding_cycles[self.metadata_count as usize] += 1;
        self.statistics.joint_cycles[self.queue_count as usize][self.metadata_count as usize] += 1;
        self.statistics.direct_words +=
            u64::from(sig.core_pop && !sig.btc_response && !sig.queue_pop);
        self.statistics.queued_words += u64::from(sig.queue_pop);
        self.statistics.demand_wait_cycles +=
            u64::from(input.core_request_valid && !sig.output.core_response_valid);
        self.statistics.credit_blocked_cycles += u64::from(
            !input.flush && !sig.restart && self.stream_valid && !sig.output.memory_request_valid,
        );
        self.statistics.requests_accepted += u64::from(sig.request_fire);
        let address = input.core_address as u32;
        let issue_address = sig.output.memory_address as u32;
        if (input.flush || sig.restart) && self.fill_phase != 0 {
            self.statistics.cancelled_fills += 1;
        }
        if (input.flush || sig.restart) && self.replay_remaining != 0 {
            self.statistics.aborted_replays += 1;
        }
        if sig.restart && !input.flush && address >> 22 == 0 {
            self.statistics.lookups += 1;
            self.statistics.hits += u64::from(sig.hit.is_some());
        }
        if !input.flush && !sig.restart && self.tracking_continuation {
            if input.core_request_valid
                && !sig.output.core_response_valid
                && self.replay_remaining == 0
            {
                self.statistics.continuation_wait_cycles += 1;
            }
            if sig.core_pop && !sig.btc_response {
                self.tracking_continuation = false;
            }
        }
        if sig.core_pop && sig.btc_response {
            self.statistics.accepted_words += 1;
        }

        if input.flush {
            for entry in &mut self.btc {
                entry.valid = false;
            }
            self.replay_remaining = 0;
            self.fill_phase = 0;
            self.tracking_continuation = false;
        } else if sig.restart {
            self.replay_remaining = if sig.hit.is_some() {
                2 - u8::from(sig.core_pop)
            } else {
                0
            };
            self.tracking_continuation = sig.hit.is_some();
            self.fill_phase = if address >> 22 == 0 && sig.hit.is_none() {
                1
            } else {
                0
            };
            self.fill_tag = address & 0x3f_ffff;
            if let Some(index) = sig.hit {
                self.replay_entry = index;
                if sig.core_pop {
                    self.touch(index, false);
                }
            }
        } else if sig.core_pop {
            if sig.btc_response {
                if self.replay_remaining == 2 {
                    self.touch(self.replay_entry, false);
                }
                self.replay_remaining -= 1;
            }
            if self.fill_phase != 0 {
                if sig.output.core_error {
                    self.fill_phase = 0;
                    self.statistics.cancelled_fills += 1;
                } else if self.fill_phase == 1 && address == self.fill_tag {
                    self.fill_word = sig.output.core_read_data as u16;
                    self.fill_phase = 2;
                } else if self.fill_phase == 2 && address == next_word(self.fill_tag, 1) {
                    let index = self.btc[..CPU_V3_BTC_ENTRIES]
                        .iter()
                        .position(|e| !e.valid)
                        .unwrap_or_else(|| {
                            self.btc[..CPU_V3_BTC_ENTRIES]
                                .iter()
                                .position(|e| {
                                    e.rank == (CPU_V3_BTC_ENTRIES as u8).saturating_sub(1)
                                })
                                .unwrap()
                        });
                    self.touch(index, true);
                    self.btc[index] = BtcEntry {
                        valid: true,
                        tag: self.fill_tag,
                        words: [self.fill_word, sig.output.core_read_data as u16],
                        rank: 0,
                    };
                    self.fill_phase = 0;
                    self.statistics.installed += 1;
                }
            }
        }

        if input.flush || sig.restart {
            self.metadata_current.fill(false);
            self.queue_head = 0;
            self.queue_tail = 0;
            self.queue_count = 0;
            self.stream_valid = input.core_request_valid;
            if input.core_request_valid {
                self.expected_core_address = next_word(address, u32::from(sig.core_pop));
                self.next_memory_address = if input.flush {
                    address
                } else {
                    next_word(issue_address, u32::from(sig.request_fire))
                };
            }
        } else {
            if sig.core_pop {
                self.expected_core_address = next_word(self.expected_core_address, 1);
            }
            if sig.queue_pop {
                self.queue_head = (self.queue_head + 1) & 1;
            }
            if sig.enqueue {
                self.queue_data[self.queue_tail as usize] = input.memory_read_data as u16;
                self.queue_error[self.queue_tail as usize] = input.memory_error;
                self.queue_tail = (self.queue_tail + 1) & 1;
            }
            self.queue_count = self.queue_count + u8::from(sig.enqueue) - u8::from(sig.queue_pop);
            if sig.request_fire {
                self.next_memory_address = next_word(issue_address, 1);
            }
        }
        // A new request wins if a full FIFO drains and reuses the same slot.
        if sig.response_fire {
            self.metadata_current[self.metadata_head as usize] = false;
            self.metadata_head = (self.metadata_head + 1) & 1;
        }
        if sig.request_fire {
            self.metadata_current[self.metadata_tail as usize] = true;
            self.metadata_tail = (self.metadata_tail + 1) & 1;
        }
        self.metadata_count =
            self.metadata_count + u8::from(sig.request_fire) - u8::from(sig.response_fire);
    }
}

impl Module for CpuV3InstructionFetchQueue {
    type Input = CpuV3InstructionFetchQueueInput;
    type Output = CpuV3InstructionFetchQueueOutput;
    type EmuState = CpuV3InstructionFetchQueueState;
    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        // Match PAYLOAD_WORDS, including the physical 16-word RAM granularity.
        vec![TargetResourceRequest::new(SsramBits::new(16 * 16))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Self::EmuState::default()
    }
    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(circuit, &state.signals(&input.sample(circuit)).output);
    }
    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        state.clock(&input.sample(circuit));
    }
    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_instruction_fetch_queue.v").to_string())
    }
    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_instruction_fetch_queue_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "fetch_btc_tests.rs"]
mod btc_tests;

#[cfg(test)]
#[path = "fetch_cold_tests.rs"]
mod cold_tests;
