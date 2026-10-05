//! Pixel lighting ports, three numerical models, cycle emulator and static RTL.

mod datapath;
pub mod emu;
mod format;
pub mod ports;
pub mod rtl;
pub mod sim;

/// Explicit numerical contract, independent of resource count and scheduling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightingQuantization {
    #[default]
    NearestEven,
    /// Floor intermediates, midpoint Q8 factors, and final nearest-even outputs.
    CompensatedFloor,
}

/// Explicit bounded scheduling experiment; numerical kernel and public ports
/// are selected separately. Default preserves the existing hardware program.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LightingRetiming {
    pub measured_functions: bool,
    pub extra_large_multiply: usize,
    pub extra_small_multiply: usize,
    pub extra_normalize_reads: usize,
    pub compact_lifetimes: bool,
}

impl LightingRetiming {
    /// Same existing resource-profile kernel and rates; fill spare Compact DSP9
    /// slots within the already allocated macro, rather than add a DSP tile.
    pub fn resource_candidate(profile: LightingProfile) -> Self {
        Self {
            measured_functions: true,
            compact_lifetimes: true,
            extra_small_multiply: usize::from(profile == LightingProfile::Compact) * 2,
            ..Self::default()
        }
    }
    /// Matched connection-cost alternative: one more MULT18 slot in an already
    /// allocated macro. Physical lane steering is applied by the RTL lowering.
    pub fn steered_resource_candidate(profile: LightingProfile) -> Self {
        Self {
            extra_large_multiply: 1,
            ..Self::resource_candidate(profile)
        }
    }
}

/// Two complete hardware alternatives with shared full/diffuse arithmetic lanes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightingProfile {
    #[default]
    Fast,
    Compact,
    /// Integration candidate with full/diffuse II2; numerical kernel is explicit.
    SystemFast,
    /// Integration candidate with full/diffuse II4; numerical kernel is explicit.
    SystemCompact,
}
impl LightingProfile {
    pub(crate) fn ii(self, full: bool) -> usize {
        match (self, full) {
            (Self::Fast, true) => 2,
            (Self::Fast, false) => 1,
            (Self::Compact, true) => 3,
            (Self::Compact, false) => 2,
            (Self::SystemFast, _) => 2,
            (Self::SystemCompact, _) => 4,
        }
    }
    pub(crate) fn hardware(self) -> sim::timed::Hardware {
        let h = sim::timed::Hardware::lighting_architecture_ii2();
        match self {
            Self::Fast | Self::SystemFast => sim::timed::Hardware {
                small_multiply: 7,
                large_multiply: 7,
                ..h
            },
            Self::Compact | Self::SystemCompact => sim::timed::Hardware {
                small_multiply: 5,
                large_multiply: 5,
                normalize_reads: 4,
                dsp_tiles: 3,
                ..h
            },
        }
    }
    pub(crate) fn system(self) -> bool {
        matches!(self, Self::SystemFast | Self::SystemCompact)
    }
}
