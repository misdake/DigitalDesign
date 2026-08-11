mod tang_console_138k;
mod tang_nano_20k;

pub use tang_console_138k::*;
pub use tang_nano_20k::*;

use crate::{TargetComponent, TargetInventory};

pub trait HardwareBackend: 'static {
    const NAME: &'static str;
}

pub trait HardwareTarget: 'static {
    type Backend: HardwareBackend;

    const NAME: &'static str;

    /// Complete inventory for one concrete purchasable hardware variant.
    ///
    /// Keeping this on the target avoids a second device/model hierarchy. If
    /// two boards happen to use the same FPGA, repeating these facts is cheap
    /// and keeps board revisions and fitted memories unambiguous.
    fn inventory() -> TargetInventory;
}

/// Compile-time declaration that a target supports a component family.
/// Resource quantities are still checked by `TargetResources::take`.
pub trait Supports<C: TargetComponent>: HardwareTarget {}
