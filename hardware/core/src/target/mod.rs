use crate::TargetInventory;

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
