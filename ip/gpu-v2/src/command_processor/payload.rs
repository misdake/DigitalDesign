//! The physical 90-bit scalar payload. Context matrices remain on their own
//! checked 26-transfer Vertex port, with no matrix copy in the command owner.
use super::{
    control::{Inputs, Outputs, PreparedCommand},
    emu::CommandEmu,
    ports::Command,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetainedPayload(u128);
impl RetainedPayload {
    pub const BITS: u32 = 90;
    pub fn bits(self) -> u128 {
        self.0
    }
    /// Canonical physical bus value. This checks width only; semantic validity
    /// still comes from PreparedCommand and its unchanged preflight contract.
    pub fn from_bits(bits: u128) -> Option<Self> {
        (bits < (1u128 << Self::BITS)).then_some(Self(bits))
    }
    pub fn from_prepared(command: &PreparedCommand) -> Self {
        let bits = match command.payload() {
            Command::Dma(d) => {
                u128::from(d.physical_addr)
                    | ((d.scratchpad_addr as u128) << 64)
                    | ((d.byte_count as u128) << 77)
            }
            Command::Draw {
                region,
                byte_offset,
                vertices,
                ..
            } => (*region as u128) | ((*byte_offset as u128) << 1) | ((*vertices as u128) << 13),
            Command::Release { slot, epoch } => (*slot as u128) | (u128::from(*epoch) << 1),
            Command::Wait(_) | Command::Fence => 0,
            Command::Unsupported(_) => unreachable!("prepared command validates kind"),
        };
        assert!(bits < (1u128 << Self::BITS));
        Self(bits)
    }
    pub fn dma(self) -> (u64, usize, usize) {
        (
            self.0 as u64,
            ((self.0 >> 64) & 8191) as usize,
            ((self.0 >> 77) & 8191) as usize,
        )
    }
    pub fn draw(self) -> (usize, usize, usize) {
        (
            (self.0 & 1) as usize,
            ((self.0 >> 1) & 4095) as usize,
            ((self.0 >> 13) & 127) as usize,
        )
    }
    pub fn release(self) -> (usize, u32) {
        ((self.0 & 1) as usize, (self.0 >> 1) as u32)
    }
}
#[derive(Clone, Debug, Default)]
pub struct CommandProcessorEmu {
    pub control: CommandEmu,
    payload: RetainedPayload,
    owned: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayloadOutputs {
    pub control: Outputs,
    pub payload: RetainedPayload,
    pub owned: bool,
}
impl CommandProcessorEmu {
    pub fn outputs(&self, input: Inputs) -> PayloadOutputs {
        PayloadOutputs {
            control: self.control.outputs(input),
            payload: self.payload,
            owned: self.owned && !input.reset,
        }
    }
    pub fn step(&mut self, input: Inputs, payload: RetainedPayload) -> PayloadOutputs {
        let result = self.outputs(input);
        self.control.step(input);
        if input.reset {
            self.payload = RetainedPayload::default();
            self.owned = false;
        } else if result.control.command_capture {
            self.payload = payload;
            self.owned = true;
        } else if result.control.command_done {
            self.owned = false;
        }
        result
    }
    pub fn held(&self) -> RetainedPayload {
        self.payload
    }
    pub fn owned(&self) -> bool {
        self.owned
    }
}
