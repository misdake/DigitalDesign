use super::{HardwareTarget, Supports};
use crate::resources::components::{
    BsramBlocks, Ddr3Bits, DspMultipliers, HdmiOutput, Pll, SpiFlashBits, UserButtons, UserLeds,
    MIBIT,
};
use crate::{GowinBackend, GowinDeviceInfo, ResourceAmount, ResourceKind, TargetInventory};

/// Tang Console fitted with the current C-step, 128-Mbit-flash Mega 138K SOM.
///
/// This deliberately names the complete fitted variant. An older B-step or
/// 64-Mbit-flash SOM should be represented by a separate target type.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TangConsole138KC128M;

impl HardwareTarget for TangConsole138KC128M {
    type Backend = GowinBackend;

    const NAME: &'static str = "tang-console-138k-c-128m";

    fn inventory() -> TargetInventory {
        TargetInventory::new([
            ResourceAmount::new(ResourceKind::Lut4, 138_240),
            ResourceAmount::new(ResourceKind::FlipFlop, 138_240),
            ResourceAmount::new(ResourceKind::SsramBit, 1_080 * 1_024),
            ResourceAmount::new(ResourceKind::Bsram18K, 340),
            ResourceAmount::new(ResourceKind::Multiplier18x18, 298),
            ResourceAmount::new(ResourceKind::Pll, 12),
            ResourceAmount::new(ResourceKind::Ddr3Bit, 8_192 * MIBIT),
            ResourceAmount::new(ResourceKind::SpiFlashBit, 128 * MIBIT),
            // Console dock only: three user-controlled LED channels and two
            // user keys. Power indicators and the reconfiguration key are not
            // application resources.
            ResourceAmount::new(ResourceKind::UserLed, 3),
            ResourceAmount::new(ResourceKind::UserButton, 2),
            ResourceAmount::new(ResourceKind::HdmiOutput, 1),
        ])
    }
}

impl crate::GowinTarget for TangConsole138KC128M {
    const DEVICE: GowinDeviceInfo = GowinDeviceInfo {
        device_name: "GW5AST-138C",
        device_version: "C",
        part_number: "GW5AST-LV138PG484AC1/I0",
        // Verified against Gowin Education 1.9.11.03 device_info.csv.
        project_device_id: "gw5ast138c-007",
        programmer_device: "GW5AST-138C",
    };
}

impl Supports<Pll> for TangConsole138KC128M {}
impl Supports<BsramBlocks> for TangConsole138KC128M {}
impl Supports<DspMultipliers> for TangConsole138KC128M {}
impl Supports<Ddr3Bits> for TangConsole138KC128M {}
impl Supports<SpiFlashBits> for TangConsole138KC128M {}
impl Supports<HdmiOutput> for TangConsole138KC128M {}
impl<const COUNT: u32> Supports<UserLeds<COUNT>> for TangConsole138KC128M {}
impl<const COUNT: u32> Supports<UserButtons<COUNT>> for TangConsole138KC128M {}
