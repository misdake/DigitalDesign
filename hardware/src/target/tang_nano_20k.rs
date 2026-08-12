use super::HardwareTarget;
use crate::resources::components::{Clock27M, UserButtons, UserLeds};
use crate::{
    GowinBackend, GowinDeviceInfo, GowinProgrammerCable, ResourceAmount, ResourceKind,
    TargetInventory,
};
use crate::{
    GowinBoardBinding, GowinClockPin, GowinModuleProject, GowinPin, GowinPortDirection,
    GowinProject, Module, ModuleIo,
};
use digital_design_code::Wires;

/// Stable application-facing inputs fitted to every Tang Nano 20K board.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KInputs {
    /// Bit 0 is Button1; bit 1 is Button2.
    pub buttons: Wires<2>,
}

/// Stable application-facing outputs fitted to every Tang Nano 20K board.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KOutputs {
    /// Logical LED-on values; physical active-low polarity is handled by the target.
    pub leds: Wires<6>,
}

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

    /// Create a project whose top module directly uses this board's stable IO.
    /// Resource reservation and physical binding are automatic.
    pub fn user_io_project<M>(project_name: impl Into<String>) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KInputs, Output = TangNano20KOutputs>,
    {
        let project = GowinProject::new(project_name);
        GowinModuleProject::new(project.with_board_binding(Self::user_io_binding()))
    }

    fn user_io_binding() -> GowinBoardBinding<Self> {
        GowinBoardBinding::new("tang_nano_20k_top", "clk", "clk", Self::CLOCK_27M)
            .require(Clock27M)
            .require(UserButtons::<2>)
            .require(UserLeds::<6>)
            .bind_port(
                GowinPortDirection::Input,
                "buttons",
                "buttons",
                Self::USER_BUTTONS,
            )
            .bind_port(GowinPortDirection::Output, "leds", "leds", Self::USER_LEDS)
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
            ResourceAmount::new(ResourceKind::HdmiOutput, 1),
        ])
        .with_fitted_device(ResourceKind::SdrSdramDevice, 64 * 1_024 * 1_024)
        .with_fitted_device(ResourceKind::SpiFlashDevice, 64 * 1_024 * 1_024)
    }
}

impl crate::GowinTarget for TangNano20K {
    const DEVICE: GowinDeviceInfo = GowinDeviceInfo {
        device_name: "GW2AR-18C",
        device_version: "C",
        part_number: "GW2AR-LV18QN88C8/I7",
        project_device_id: "gw2ar18c-000",
        programmer_device: "GW2AR-18C",
        programmer_cable: GowinProgrammerCable::UsbDebuggerA,
    };
}
