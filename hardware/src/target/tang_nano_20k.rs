use super::HardwareTarget;
use crate::resources::components::{Clock27M, DebugUartTx, Pll, SdrSdram, UserButtons, UserLeds};
use crate::{
    GowinBackend, GowinBoardExtension, GowinDeviceInfo, GowinLogicConnection, GowinProgrammerCable,
    GowinTopPort, GowinTopPortDirection, ResourceAmount, ResourceKind, TargetInventory,
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

/// Board outputs used by automated tests that report through the onboard
/// debugger's UART bridge.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KDebugOutputs {
    pub leds: Wires<6>,
    pub uart_tx: digital_design_code::Wire,
}

/// Inputs supplied to logic in a Tang Nano 20K project using the fitted SDRAM.
///
/// The project main clock and Controller HS both run at 54 MHz. The physical
/// SDRAM clock is a private 180-degree PLL output and is not exposed here.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramInputs {
    pub buttons: Wires<2>,
    pub sdram_read_data: Wires<32>,
    /// High for each valid read beat produced by this board's fitted
    /// Controller HS configuration.
    pub sdram_read_valid: digital_design_code::Wire,
    pub sdram_init_done: digital_design_code::Wire,
    pub sdram_command_ack: digital_design_code::Wire,
}

/// Raw Controller HS command interface plus normal board debug outputs.
///
/// Applications normally place a cache-line transaction controller above this
/// interface. Refresh remains explicit at this boundary so that the shared
/// scheduler can account for refresh stalls.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramOutputs {
    pub leds: Wires<6>,
    pub uart_tx: digital_design_code::Wire,
    pub sdram_command_valid: digital_design_code::Wire,
    pub sdram_command: Wires<3>,
    pub sdram_precharge: digital_design_code::Wire,
    pub sdram_address: Wires<21>,
    pub sdram_write_mask: Wires<4>,
    pub sdram_write_data: Wires<32>,
    pub sdram_burst_length: Wires<8>,
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

    pub const DEBUG_UART_TX: GowinPin = GowinPin {
        location: 69,
        io_type: "LVCMOS33",
        pull_mode: None,
        drive: Some(8),
        active_low: false,
    };

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

    pub fn debug_uart_project<M>(project_name: impl Into<String>) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KInputs, Output = TangNano20KDebugOutputs>,
    {
        let binding = Self::user_io_binding().require(DebugUartTx).bind_port(
            GowinPortDirection::Output,
            "uart_tx",
            "uart_tx",
            [Self::DEBUG_UART_TX],
        );
        GowinModuleProject::new(GowinProject::new(project_name).with_board_binding(binding))
    }

    /// Create a single-clock 54 MHz project with the fitted 64-Mibit SDRAM.
    ///
    /// The board wrapper owns the PLL and Gowin Controller HS instance. User
    /// logic, command scheduling, caches, and the controller use the same
    /// 54 MHz clock. Only the SDRAM physical clock uses the PLL's 180-degree
    /// output.
    pub fn sdram_debug_uart_project<M>(
        project_name: impl Into<String>,
    ) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KSdramInputs, Output = TangNano20KSdramOutputs>,
    {
        let extension = GowinBoardExtension::new(include_str!("tang_nano_20k/sdram/service_54m.v"))
            .with_logic_clock("logic_clk")
            .add_top_port(GowinTopPort::new(
                "O_sdram_clk",
                GowinTopPortDirection::Output,
                1,
            ))
            .add_top_port(GowinTopPort::new(
                "O_sdram_cke",
                GowinTopPortDirection::Output,
                1,
            ))
            .add_top_port(GowinTopPort::new(
                "O_sdram_cs_n",
                GowinTopPortDirection::Output,
                1,
            ))
            .add_top_port(GowinTopPort::new(
                "O_sdram_cas_n",
                GowinTopPortDirection::Output,
                1,
            ))
            .add_top_port(GowinTopPort::new(
                "O_sdram_ras_n",
                GowinTopPortDirection::Output,
                1,
            ))
            .add_top_port(GowinTopPort::new(
                "O_sdram_wen_n",
                GowinTopPortDirection::Output,
                1,
            ))
            .add_top_port(GowinTopPort::new(
                "O_sdram_dqm",
                GowinTopPortDirection::Output,
                4,
            ))
            .add_top_port(GowinTopPort::new(
                "O_sdram_addr",
                GowinTopPortDirection::Output,
                11,
            ))
            .add_top_port(GowinTopPort::new(
                "O_sdram_ba",
                GowinTopPortDirection::Output,
                2,
            ))
            .add_top_port(GowinTopPort::new(
                "IO_sdram_dq",
                GowinTopPortDirection::InOut,
                32,
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_read_data",
                GowinPortDirection::Input,
                32,
                "sdram_read_data",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_read_valid",
                GowinPortDirection::Input,
                1,
                "sdram_read_valid",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_init_done",
                GowinPortDirection::Input,
                1,
                "sdram_init_done",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_command_ack",
                GowinPortDirection::Input,
                1,
                "sdram_command_ack",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_command_valid",
                GowinPortDirection::Output,
                1,
                "sdram_command_valid",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_command",
                GowinPortDirection::Output,
                3,
                "sdram_command",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_precharge",
                GowinPortDirection::Output,
                1,
                "sdram_precharge",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_address",
                GowinPortDirection::Output,
                21,
                "sdram_address",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_write_mask",
                GowinPortDirection::Output,
                4,
                "sdram_write_mask",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_write_data",
                GowinPortDirection::Output,
                32,
                "sdram_write_data",
            ))
            .connect_logic(GowinLogicConnection::new(
                "sdram_burst_length",
                GowinPortDirection::Output,
                8,
                "sdram_burst_length",
            ))
            .add_source_file(
                "src/generated/target/tang_nano_20k/sdram/controller_qn88.v",
                include_str!("tang_nano_20k/sdram/controller_qn88.v"),
            )
            .add_source_file(
                "src/generated/target/tang_nano_20k/sdram/sdrc_hs_defines.v",
                include_str!("tang_nano_20k/sdram/sdrc_hs_defines.v"),
            )
            .add_source_file(
                "src/generated/target/tang_nano_20k/sdram/sdrc_hs_name.v",
                include_str!("tang_nano_20k/sdram/sdrc_hs_name.v"),
            )
            .add_source_file(
                "src/generated/target/tang_nano_20k/sdram/pll_54m.v",
                include_str!("tang_nano_20k/sdram/pll_54m.v"),
            )
            .require_installed_ide_file(
                "ipcore/SDRC_HS/data/sdrc_hs_top.vp",
                [
                    "src/generated/target/tang_nano_20k/sdram/sdrc_hs_defines.v".into(),
                    "src/generated/target/tang_nano_20k/sdram/sdrc_hs_name.v".into(),
                ],
            );

        let binding = Self::user_io_binding()
            .require(DebugUartTx)
            .require(Pll)
            .require(SdrSdram)
            .bind_port(
                GowinPortDirection::Output,
                "uart_tx",
                "uart_tx",
                [Self::DEBUG_UART_TX],
            )
            .with_extension(extension);
        GowinModuleProject::new(GowinProject::new(project_name).with_board_binding(binding))
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
            ResourceAmount::new(ResourceKind::DebugUartTx, 1),
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
