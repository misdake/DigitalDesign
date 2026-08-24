//! Vendor-independent FPGA shell contracts and reusable modules.

pub use digital_design_hardware::*;

mod clock_divider;
pub use clock_divider::*;

mod diagnostic_reporter;
pub use diagnostic_reporter::*;

mod reset_controller;
pub use reset_controller::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetPolarity {
    ActiveHigh,
    ActiveLow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockDomainSpec {
    pub name: &'static str,
    pub frequency_hz: u64,
    pub reset_polarity: ResetPolarity,
}

/// Board-independent contract fulfilled by a vendor/target adapter.
pub trait FpgaShell {
    const INPUT_CLOCK_HZ: u64;
    const CLOCK_DOMAINS: &'static [ClockDomainSpec];
}
