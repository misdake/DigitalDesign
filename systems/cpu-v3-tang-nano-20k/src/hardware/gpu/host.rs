//! Cycle model of the arbiter plus `SharedSdramPort` line path for the two GPU
//! masters.
//!
//! It is used by the host transaction model in `crate::gpu_device` so a
//! polled submission reaches completion without a CPU or an RTL simulator. It
//! reproduces the real handshake shape: a request is accepted in one cycle, a
//! line read streams four unstallable 64-bit beats, and a line write captures
//! four beats before committing.

use super::{GpuMemoryBus, GpuOutputs};

const RECOVERY_CYCLES: u8 = 3;

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
#[derive(Clone, Default)]
pub(crate) struct HostGpuMemory {
    state: State,
    address: usize,
    write_buffer: [u64; 4],
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
                ro_response_last: beat == 3,
                ..GpuMemoryBus::default()
            },
            State::WriteResponse => GpuMemoryBus {
                fb_w_response_valid: true,
                fb_w_response_last: true,
                ..GpuMemoryBus::default()
            },
            State::WriteCapture(_) | State::Recovery(_) => GpuMemoryBus::default(),
        }
    }

    /// Applies the clock edge using this cycle's GPU outputs.
    pub(crate) fn advance(&mut self, outputs: &GpuOutputs, memory: &mut [u16]) {
        match self.state {
            State::Idle => {
                if outputs.ro_request_valid {
                    self.address = outputs.ro_address as usize;
                    self.state = State::ReadBeats(0);
                } else if outputs.fb_w_request_valid {
                    self.address = outputs.fb_w_address as usize;
                    self.write_buffer[0] = outputs.fb_w_write_data;
                    self.state = State::WriteCapture(1);
                }
            }
            State::ReadBeats(beat) => {
                if beat == 3 {
                    self.state = State::Recovery(0);
                } else {
                    self.state = State::ReadBeats(beat + 1);
                }
            }
            State::WriteCapture(beat) => {
                self.write_buffer[beat as usize] = outputs.fb_w_write_data;
                if beat == 3 {
                    for (index, value) in self.write_buffer.iter().copied().enumerate() {
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
