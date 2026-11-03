//! Fixed preloaded configuration-state entry, not an arbitrary checkpoint API.
//! Loader bytes are installed before clocks; their SPI/SDRAM transaction history
//! is deliberately not claimed equivalent to a real loader's terminal state.
use crate::boot::{self, SingleBootError};
#[derive(Clone, Debug)]
pub struct ImageReport {
    pub code_words: usize,
    pub copied_bytes: usize,
    pub zeroed_bytes: usize,
    pub memory_seed: u16,
}
pub fn image(package: &[u8], memory_seed: u16) -> Result<(Vec<u16>, ImageReport), SingleBootError> {
    let commands = boot::single_boot_commands(package)?;
    let copied_bytes = commands
        .iter()
        .filter(|c| c.payload.is_some())
        .map(|c| c.length)
        .sum();
    let zeroed_bytes = commands
        .iter()
        .filter(|c| c.payload.is_none())
        .map(|c| c.length)
        .sum();
    let mut memory = vec![memory_seed; 1 << 22];
    let code_words = boot::install_single_application(package, &mut memory)?;
    Ok((
        memory,
        ImageReport {
            code_words,
            copied_bytes,
            zeroed_bytes,
            memory_seed,
        },
    ))
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColdGate {
    pub ready_sync: u8,
    pub cold_ready: bool,
    pub video_sync: u8,
    pub boot_done: bool,
}
impl ColdGate {
    pub fn boot_enable(&self) -> bool {
        self.cold_ready
    }
    pub fn video_locked(&self) -> bool {
        self.video_sync & 4 != 0
    }
    /// Both domains sample these old flags before either commit is visible.
    pub fn clock(
        &mut self,
        logic: bool,
        pixel: bool,
        memory_initialized: bool,
        display_started: bool,
        cache_ready: bool,
    ) {
        let old = *self;
        if logic {
            if !old.cold_ready {
                self.ready_sync = ((old.ready_sync << 1) | u8::from(memory_initialized)) & 3;
                if old.ready_sync & 2 != 0 {
                    self.cold_ready = true;
                }
            }
            // CB06 END and final writes are complete by definition of preload.
            // Mirror DISPLAY -> FINISHED only after real Display/cache readiness.
            if old.boot_enable() && display_started && cache_ready {
                self.boot_done = true;
            }
        }
        // External PLL locks are declared stable from t0 in this fixed entry.
        if pixel && old.video_sync & 4 == 0 {
            self.video_sync = ((old.video_sync << 1) | 1) & 7;
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Release {
    pub time_ticks: u64,
    pub logic_edges: u64,
    pub native_cycles: u64,
    pub pixel_edges: u64,
    pub system_milliseconds: u16,
    pub system_microseconds: u16,
}
pub const HANDOFF_LIMITS:&str="CB06 application ABI/preload; synthetic configuration-state MC/cache/GPU/Display prefix; no SPI cost, no loader bank/refresh/clock/cache-state equivalence";
