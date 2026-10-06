//! Pixel lighting ports, three numerical models, cycle emulator and static RTL.

pub mod calendars;
mod datapath;
pub mod emu;
mod format;
pub mod ports;
pub mod rsqrt;
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
/// are selected separately. Default uses the Hardware latency declarations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LightingRetiming {
    /// Ordinary MULT9X9/MULT18X18 advancing-edge latency; None uses Hardware.
    /// Matched primitive configurations support one, two or three stages.
    pub multiply_latency: Option<u64>,
    /// Generic fabric-cone latency; measured typed functions stay one edge.
    /// One-edge candidates require a matched whole-module timing qualification.
    pub cone_latency: Option<u64>,
    /// Dual-product MAC: one combinational macro plus a fabric output register,
    /// or the established four-edge registered implementation.
    pub paired_latency: Option<u64>,
    /// Candidate multi-output square-sum plus reciprocal-address boundary.
    pub sum_address: bool,
    pub measured_functions: bool,
    pub extra_large_multiply: usize,
    pub extra_small_multiply: usize,
    pub extra_normalize_reads: usize,
    pub compact_lifetimes: bool,
}

impl LightingRetiming {
    /// Qualified 60 MHz Fast lit-queue fabric/DSP boundaries.
    pub fn lit_queue_60mhz(profile: LightingProfile) -> (usize, Self) {
        if profile != LightingProfile::Fast {
            // System candidates keep their independently selected boundaries.
            return (0, Self::steered_resource_candidate(profile));
        }
        (
            6,
            Self {
                multiply_latency: Some(1),
                cone_latency: Some(1),
                paired_latency: Some(1),
                sum_address: true,
                ..Self::steered_resource_candidate(profile)
            },
        )
    }
    /// Preserve the Fast resource kernel and rates with measured function binding.
    pub fn resource_candidate(_profile: LightingProfile) -> Self {
        Self {
            measured_functions: true,
            compact_lifetimes: true,
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

/// Fast hardware with shared full/diffuse lanes and a legacy system candidate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightingProfile {
    #[default]
    Fast,
    /// Integration candidate with full/diffuse II2; numerical kernel is explicit.
    SystemFast,
}
impl LightingProfile {
    pub(crate) fn ii(self, full: bool) -> usize {
        match (self, full) {
            (Self::Fast, true) => 2,
            (Self::Fast, false) => 1,
            (Self::SystemFast, _) => 2,
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
        }
    }
    pub(crate) fn system(self) -> bool {
        matches!(self, Self::SystemFast)
    }
}
