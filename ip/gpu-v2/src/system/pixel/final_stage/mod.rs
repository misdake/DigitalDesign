//! Independently qualified final-color leaf for GPU v2 pixel composition.
//!
//! This leaf implements the exact [`crate::system::pixel::final_rgb`] contract
//! for one keyed pixel as a bounded, finite-resource pipeline:
//!
//! ```text
//! base  = RNE(tint * texture / 255)          // bounded multiply/add, no divider
//! color = saturate_u8(RNE((base * g + specular * h) / 256))
//! ```
//!
//! `math` holds the independent integer semantics. `sim::counted` is a closed
//! audited graph, `sim::timed` binds finite resource lanes and certifies II=1
//! with an independent modulo schedule, and `emu` is an independent old-state
//! register model with finite result credits. `rtl` emits one synthesizable
//! Verilog module that the ignored Icarus test drives every edge.
//!
//! This leaf deliberately owns no queue, context, material, dispatcher, ROP
//! or memory port; composition owns the quad/status head and common context.
//! The numeric authority is the existing `final_rgb`; this leaf
//! must reproduce it bit for bit, including ties.

pub mod emu;
pub mod math;
pub mod rtl;
pub mod sim;

/// Key width in bits. The key travels unchanged with its pixel.
pub const KEY_BITS: u32 = 6;
pub const KEY_MASK: u8 = (1 << KEY_BITS) - 1;
/// Result credits reserved at acceptance; also the output FIFO depth. Six are
/// needed across the ten-edge acceptance-to-consumption lifetime at II2; eight
/// is the finite power-of-two implementation, with no same-edge credit reuse.
pub const RESULT_CAPACITY: usize = 8;
/// One arithmetic operation per enabled edge; the FIFO tail is LATENCY stages
/// after acceptance.
pub const PIPELINE_STAGES: usize = 9;
pub const PIPELINE_LATENCY: usize = PIPELINE_STAGES;
/// `CE` gates every numerical stage and both FIFO transfers.
#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub input: Option<Input>,
    pub output_ready: bool,
}
impl Default for Tick {
    fn default() -> Self {
        Self {
            reset: false,
            ce: true,
            input: None,
            output_ready: true,
        }
    }
}

/// One pixel: a 6-bit key plus the final-color operands. `g`/`h` are the
/// already-captured integer lighting intensities (legal `0..=511` and
/// `0..=256`); this leaf does not own or reinterpret that context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub key: u8,
    pub tint: [u8; 3],
    pub texture: [u8; 3],
    pub g: u16,
    pub h: u16,
    pub specular: [u8; 3],
}
impl Input {
    pub fn validate(&self) -> Result<(), String> {
        if self.key > KEY_MASK {
            return Err("final key outside 6 bits".into());
        }
        if self.g > 511 {
            return Err("final g outside 0..=511".into());
        }
        if self.h > 256 {
            return Err("final h outside 0..=256".into());
        }
        Ok(())
    }
    /// Independent integer semantics; equal to `final_rgb` on this input.
    pub fn reference_rgb(&self) -> [u8; 3] {
        math::reference_rgb(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    pub key: u8,
    pub rgb: [u8; 3],
}
impl Output {
    pub fn new(key: u8, rgb: [u8; 3]) -> Self {
        Self { key, rgb }
    }
}

/// Pre-edge observations plus the edge outcome. `output` is the FIFO head
/// before this edge; `consumed` is the actual output transfer on this edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub input_ready: bool,
    pub accepted: bool,
    pub consumed: bool,
    pub output: Option<Output>,
    pub snapshot: Snapshot,
}

/// Bounded diagnostic state: one key per pipeline stage, retired credits and
/// the number of queued results. Not a hardware tag or fitted storage claim.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub wall: u64,
    pub enabled: u64,
    pub pipeline_keys: [Option<u8>; PIPELINE_STAGES],
    pub credits: usize,
    pub queued: usize,
}
