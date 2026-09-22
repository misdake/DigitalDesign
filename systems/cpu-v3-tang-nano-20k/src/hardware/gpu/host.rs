//! Cycle model of the arbiter plus `SharedSdramPort` line path for the two GPU
//! masters.
//!
//! It is used by the host transaction model in `crate::gpu_device` so a
//! polled submission reaches completion without a CPU or an RTL simulator. It
//! reproduces the real handshake shape: a request is accepted in one cycle, a
//! line read streams its 4/8/12/16 ordered 64-bit beats, and a line write
//! captures those beats before committing. The request length comes from the
//! GPU master's `line_count_minus_1[1:0]`, so the model already exercises the
//! variable-length path even though the bring-up GPU always asks for one line.

use super::{GpuMemoryBus, GpuOutputs};

const RECOVERY_CYCLES: u8 = 3;
/// Largest request: four lines = sixteen 64-bit beats.
const MAX_BEATS: usize = 16;

/// Number of 64-bit beats for a `line_count_minus_1` encoding.
pub(crate) const fn line_beat_count(line_count_minus_1: u8) -> usize {
    ((line_count_minus_1 as usize & 0x3) + 1) * 4
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum State {
    #[default]
    Idle,
    ReadBeats(u8),
    WriteCapture(u8),
    WriteResponse,
    Recovery(u8),
}

/// Direct-memory line responder for `gpu_ro` and `gpu_fb_w`.
#[derive(Clone)]
pub(crate) struct HostGpuMemory {
    state: State,
    address: usize,
    /// Beats of the in-flight request; four to sixteen.
    beats: u8,
    write_buffer: [u64; MAX_BEATS],
}

impl Default for HostGpuMemory {
    fn default() -> Self {
        Self {
            state: State::Idle,
            address: 0,
            beats: 4,
            write_buffer: [0; MAX_BEATS],
        }
    }
}

impl HostGpuMemory {
    fn line_beat(memory: &[u16], address: usize, beat: usize) -> u64 {
        let base = address + 4 * beat;
        let mut value = 0u64;
        for offset in 0..4 {
            let word = memory.get(base + offset).copied().unwrap_or(0);
            value |= u64::from(word) << (16 * offset);
        }
        value
    }

    /// The handshake inputs presented to the GPU core this cycle.
    pub(crate) fn inputs(&self, memory: &[u16]) -> GpuMemoryBus {
        match self.state {
            State::Idle => GpuMemoryBus {
                ro_request_ready: true,
                fb_w_request_ready: true,
                ..GpuMemoryBus::default()
            },
            State::ReadBeats(beat) => GpuMemoryBus {
                ro_response_valid: true,
                ro_read_data: Self::line_beat(memory, self.address, beat as usize),
                ro_response_last: beat + 1 == self.beats,
                ..GpuMemoryBus::default()
            },
            State::WriteResponse => GpuMemoryBus {
                fb_w_response_valid: true,
                fb_w_response_last: true,
                ..GpuMemoryBus::default()
            },
            State::WriteCapture(_) => GpuMemoryBus {
                fb_w_write_data_ready: true,
                ..GpuMemoryBus::default()
            },
            State::Recovery(_) => GpuMemoryBus::default(),
        }
    }

    /// Applies the clock edge using this cycle's GPU outputs.
    pub(crate) fn advance(&mut self, outputs: &GpuOutputs, memory: &mut [u16]) {
        match self.state {
            State::Idle => {
                if outputs.ro_request_valid {
                    // A host model accepts what the RTL cannot: assert the same
                    // row contract the fitted adapter assumes, so a crossing
                    // request fails the test instead of silently truncating.
                    crate::hardware::assert_line_request_fits_row(
                        outputs.ro_address,
                        u32::from(outputs.ro_line_count_minus_1),
                    );
                    self.address = outputs.ro_address as usize;
                    self.beats = line_beat_count(outputs.ro_line_count_minus_1) as u8;
                    self.state = State::ReadBeats(0);
                } else if outputs.fb_w_request_valid {
                    crate::hardware::assert_line_request_fits_row(
                        outputs.fb_w_address,
                        u32::from(outputs.fb_w_line_count_minus_1),
                    );
                    self.address = outputs.fb_w_address as usize;
                    self.beats = line_beat_count(outputs.fb_w_line_count_minus_1) as u8;
                    self.write_buffer[0] = outputs.fb_w_write_data;
                    self.state = State::WriteCapture(1);
                }
            }
            State::ReadBeats(beat) => {
                if beat + 1 == self.beats {
                    self.state = State::Recovery(0);
                } else {
                    self.state = State::ReadBeats(beat + 1);
                }
            }
            State::WriteCapture(beat) => {
                self.write_buffer[beat as usize] = outputs.fb_w_write_data;
                if beat + 1 == self.beats {
                    for index in 0..self.beats as usize {
                        let value = self.write_buffer[index];
                        let base = self.address + 4 * index;
                        for offset in 0..4 {
                            if let Some(slot) = memory.get_mut(base + offset) {
                                *slot = (value >> (16 * offset)) as u16;
                            }
                        }
                    }
                    self.state = State::WriteResponse;
                } else {
                    self.state = State::WriteCapture(beat + 1);
                }
            }
            State::WriteResponse => {
                self.state = State::Recovery(0);
            }
            State::Recovery(count) => {
                if count == RECOVERY_CYCLES {
                    self.state = State::Idle;
                } else {
                    self.state = State::Recovery(count + 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_outputs(address: u32, line_count_minus_1: u8) -> GpuOutputs {
        GpuOutputs {
            ro_request_valid: true,
            ro_address: address,
            ro_line_count_minus_1: line_count_minus_1,
            ..GpuOutputs::default()
        }
    }

    fn beat_word(beat: usize) -> u64 {
        let value = 0x1000u64 + beat as u64;
        value * 0x0001_0001_0001_0001
    }

    #[test]
    fn line_beat_count_spans_one_to_four_lines() {
        assert_eq!(line_beat_count(0), 4);
        assert_eq!(line_beat_count(1), 8);
        assert_eq!(line_beat_count(2), 12);
        assert_eq!(line_beat_count(3), 16);
    }

    #[test]
    fn reads_stream_exactly_the_requested_number_of_beats_with_last_at_the_end() {
        for line_count_minus_1 in 0..4u8 {
            let beats = line_beat_count(line_count_minus_1);
            let mut memory = vec![0u16; 0x1000];
            for (index, word) in memory.iter_mut().enumerate() {
                *word = index as u16;
            }
            let mut model = HostGpuMemory::default();
            model.advance(&read_outputs(0x40, line_count_minus_1), &mut memory);
            let mut seen = 0usize;
            while model.state != State::Idle {
                if let State::ReadBeats(beat) = model.state {
                    let inputs = model.inputs(&memory);
                    assert!(inputs.ro_response_valid);
                    assert_eq!(
                        inputs.ro_read_data,
                        HostGpuMemory::line_beat(&memory, 0x40, beat as usize)
                    );
                    assert_eq!(inputs.ro_response_last, beat as usize + 1 == beats);
                    seen += 1;
                }
                model.advance(&GpuOutputs::default(), &mut memory);
            }
            assert_eq!(seen, beats, "length {line_count_minus_1} beat mismatch");
        }
    }

    #[test]
    fn writes_commit_consecutive_beats_in_order_for_every_length() {
        for line_count_minus_1 in 0..4u8 {
            let beats = line_beat_count(line_count_minus_1);
            let base = 0x80usize;
            let mut memory = vec![0u16; 0x4000];
            let mut model = HostGpuMemory::default();
            // Beat zero is captured on the accepting edge.
            let start = GpuOutputs {
                fb_w_request_valid: true,
                fb_w_write: true,
                fb_w_address: base as u32,
                fb_w_line_count_minus_1: line_count_minus_1,
                fb_w_write_data: beat_word(0),
                ..GpuOutputs::default()
            };
            model.advance(&start, &mut memory);
            while model.state != State::Idle {
                let capture = match model.state {
                    State::WriteCapture(beat) => beat as usize,
                    _ => 0,
                };
                let outputs = GpuOutputs {
                    fb_w_write_data: beat_word(capture),
                    ..GpuOutputs::default()
                };
                model.advance(&outputs, &mut memory);
            }
            for beat in 0..beats {
                assert_eq!(
                    memory[base + 4 * beat],
                    (0x1000 + beat) as u16,
                    "length {line_count_minus_1} beat {beat} out of order"
                );
            }
        }
    }
}
