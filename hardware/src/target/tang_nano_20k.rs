use super::{HardwareTarget, Supports};
use crate::resources::components::{
    BsramBlocks, Clock27M, DspMultipliers, HdmiOutput, Pll, SdrSdramBits, SpiFlashBits,
    UserButtons, UserLeds, MIBIT,
};
use crate::{GowinBackend, GowinDeviceInfo, ResourceAmount, ResourceKind, TargetInventory};
use crate::{GowinBoardBinding, GowinClockPin, GowinPin, GowinPortDirection, ResourceLease};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TangNano20K;

impl TangNano20K {
    pub const CLOCK_27M: GowinClockPin = GowinClockPin {
        pin: GowinPin {
            location: 4,
            io_type: "LVCMOS33",
            pull_mode: Some("UP"),
            drive: None,
            active_low: false,
        },
        frequency_hz: 27_000_000,
    };

    pub const USER_BUTTONS: [GowinPin; 2] = [
        GowinPin {
            location: 88,
            io_type: "LVCMOS33",
            pull_mode: Some("DOWN"),
            drive: None,
            active_low: false,
        },
        GowinPin {
            location: 87,
            io_type: "LVCMOS33",
            pull_mode: Some("DOWN"),
            drive: None,
            active_low: false,
        },
    ];

    pub const USER_LEDS: [GowinPin; 6] = [
        Self::active_low_led(15),
        Self::active_low_led(16),
        Self::active_low_led(17),
        Self::active_low_led(18),
        Self::active_low_led(19),
        Self::active_low_led(20),
    ];

    const fn active_low_led(location: u16) -> GowinPin {
        GowinPin {
            location,
            io_type: "LVCMOS33",
            pull_mode: None,
            drive: Some(8),
            active_low: true,
        }
    }

    /// Bind the standard clock, two buttons, and six LEDs after reserving
    /// those resources from this target.
    pub fn bind_user_io(
        _clock: ResourceLease<Self, Clock27M>,
        _buttons: ResourceLease<Self, UserButtons<2>>,
        _leds: ResourceLease<Self, UserLeds<6>>,
        logic_button_port: impl Into<String>,
        logic_led_port: impl Into<String>,
    ) -> GowinBoardBinding<Self> {
        GowinBoardBinding::new("tang_nano_20k_top", "clk", "clk", Self::CLOCK_27M)
            .bind_port(
                GowinPortDirection::Input,
                "buttons",
                logic_button_port,
                Self::USER_BUTTONS,
            )
            .bind_port(
                GowinPortDirection::Output,
                "leds",
                logic_led_port,
                Self::USER_LEDS,
            )
    }
}

impl HardwareTarget for TangNano20K {
    type Backend = GowinBackend;

    const NAME: &'static str = "tang-nano-20k";

    fn inventory() -> TargetInventory {
        // Counts are sufficient for the first allocator. TODO: represent pin
        // identities and resource bundles so overlapping board functions
        // (for example LEDs versus expansion signals) conflict by atom.
        TargetInventory::new([
            ResourceAmount::new(ResourceKind::Lut4, 20_736),
            ResourceAmount::new(ResourceKind::FlipFlop, 15_552),
            ResourceAmount::new(ResourceKind::SsramBit, 41_472),
            ResourceAmount::new(ResourceKind::Bsram18K, 46),
            ResourceAmount::new(ResourceKind::Multiplier18x18, 48),
            ResourceAmount::new(ResourceKind::Pll, 2),
            ResourceAmount::new(ResourceKind::BoardClock27M, 1),
            ResourceAmount::new(ResourceKind::UserLed, 6),
            ResourceAmount::new(ResourceKind::UserButton, 2),
            ResourceAmount::new(ResourceKind::SdrSdramBit, 64 * MIBIT),
            ResourceAmount::new(ResourceKind::SpiFlashBit, 64 * MIBIT),
            ResourceAmount::new(ResourceKind::HdmiOutput, 1),
        ])
    }
}

impl crate::GowinTarget for TangNano20K {
    const DEVICE: GowinDeviceInfo = GowinDeviceInfo {
        device_name: "GW2AR-18C",
        device_version: "C",
        part_number: "GW2AR-LV18QN88C8/I7",
        project_device_id: "gw2ar18c-000",
        programmer_device: "GW2AR-18C",
    };
}

impl Supports<Pll> for TangNano20K {}
impl Supports<BsramBlocks> for TangNano20K {}
impl Supports<DspMultipliers> for TangNano20K {}
impl Supports<Clock27M> for TangNano20K {}
impl Supports<SdrSdramBits> for TangNano20K {}
impl Supports<SpiFlashBits> for TangNano20K {}
impl Supports<HdmiOutput> for TangNano20K {}
impl<const COUNT: u32> Supports<UserLeds<COUNT>> for TangNano20K {}
impl<const COUNT: u32> Supports<UserButtons<COUNT>> for TangNano20K {}
