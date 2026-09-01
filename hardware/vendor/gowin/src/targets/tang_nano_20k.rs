use super::HardwareTarget;
use crate::resources::components::{
    Clock27M, DebugUartTx, HdmiOutput, Pll, SdrSdram, UserButtons, UserLeds,
};
use crate::{
    GowinBackend, GowinBoardExtension, GowinDeviceInfo, GowinLogicConnection, GowinProgrammerCable,
    GowinTopPort, GowinTopPortDirection, ResourceAmount, ResourceKind, TargetInventory,
};
use crate::{
    GowinBoardBinding, GowinClockPin, GowinModuleProject, GowinPin, GowinPortDirection,
    GowinProject, Module, ModuleIo,
};
use digital_design_circuit::Wires;

mod sdram_word_port;
pub use sdram_word_port::*;

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
    pub uart_tx: digital_design_circuit::Wire,
}

/// Tang Nano 20K user controls for a design that owns the onboard HDMI port.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KHdmiInputs {
    pub buttons: Wires<2>,
}

/// Logical status LEDs plus the four physical HDMI differential pairs.
///
/// The design instantiates Gowin differential output buffers, so both sides
/// of every pair are explicit module outputs and their electrical mode comes
/// from the primitive rather than an ordinary single-ended pin constraint.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KHdmiOutputs {
    pub leds: Wires<6>,
    pub tmds_clk_p: digital_design_circuit::Wire,
    pub tmds_clk_n: digital_design_circuit::Wire,
    pub tmds_data_p: Wires<3>,
    pub tmds_data_n: Wires<3>,
}

/// Board signals for logic that instantiates the fitted SPI Flash reader.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KFlashInputs {
    pub buttons: Wires<2>,
    /// Physical configuration-Flash IO1/DO signal.
    pub flash_miso: digital_design_circuit::Wire,
}

/// Normal debug outputs plus the fitted configuration-Flash SPI signals.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KFlashOutputs {
    pub leds: Wires<6>,
    pub uart_tx: digital_design_circuit::Wire,
    pub flash_clk: digital_design_circuit::Wire,
    pub flash_cs_n: digital_design_circuit::Wire,
    pub flash_mosi: digital_design_circuit::Wire,
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
    pub sdram_read_valid: digital_design_circuit::Wire,
    pub sdram_init_done: digital_design_circuit::Wire,
    pub sdram_command_ack: digital_design_circuit::Wire,
}

/// Raw Controller HS command interface plus normal board debug outputs.
///
/// Applications normally place a cache-line transaction controller above this
/// interface. Refresh remains explicit at this boundary so that the shared
/// scheduler can account for refresh stalls.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramOutputs {
    pub leds: Wires<6>,
    pub uart_tx: digital_design_circuit::Wire,
    pub sdram_command_valid: digital_design_circuit::Wire,
    pub sdram_command: Wires<3>,
    pub sdram_precharge: digital_design_circuit::Wire,
    pub sdram_address: Wires<21>,
    pub sdram_write_mask: Wires<4>,
    pub sdram_write_data: Wires<32>,
    pub sdram_burst_length: Wires<8>,
}

/// SDRAM controller signals plus the independent clocks used by 720p HDMI.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramHdmiInputs {
    pub buttons: Wires<2>,
    pub sdram_read_data: Wires<32>,
    pub sdram_read_valid: digital_design_circuit::Wire,
    pub sdram_init_done: digital_design_circuit::Wire,
    pub sdram_command_ack: digital_design_circuit::Wire,
    pub pixel_clock: digital_design_circuit::Wire,
    pub serial_clock: digital_design_circuit::Wire,
    pub video_locked: digital_design_circuit::Wire,
}

#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramHdmiOutputs {
    pub leds: Wires<6>,
    pub uart_tx: digital_design_circuit::Wire,
    pub sdram_command_valid: digital_design_circuit::Wire,
    pub sdram_command: Wires<3>,
    pub sdram_precharge: digital_design_circuit::Wire,
    pub sdram_address: Wires<21>,
    pub sdram_write_mask: Wires<4>,
    pub sdram_write_data: Wires<32>,
    pub sdram_burst_length: Wires<8>,
    pub tmds_clk_p: digital_design_circuit::Wire,
    pub tmds_clk_n: digital_design_circuit::Wire,
    pub tmds_data_p: Wires<3>,
    pub tmds_data_n: Wires<3>,
}

/// Board inputs for boot logic that needs both fitted memories concurrently.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KBootInputs {
    pub buttons: Wires<2>,
    pub flash_miso: digital_design_circuit::Wire,
    pub sdram_read_data: Wires<32>,
    pub sdram_read_valid: digital_design_circuit::Wire,
    pub sdram_init_done: digital_design_circuit::Wire,
    pub sdram_command_ack: digital_design_circuit::Wire,
}

/// Raw Flash-read and Controller HS ports exposed to Tang Nano 20K boot logic.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KBootOutputs {
    pub leds: Wires<6>,
    pub uart_tx: digital_design_circuit::Wire,
    pub flash_clk: digital_design_circuit::Wire,
    pub flash_cs_n: digital_design_circuit::Wire,
    pub flash_mosi: digital_design_circuit::Wire,
    pub sdram_command_valid: digital_design_circuit::Wire,
    pub sdram_command: Wires<3>,
    pub sdram_precharge: digital_design_circuit::Wire,
    pub sdram_address: Wires<21>,
    pub sdram_write_mask: Wires<4>,
    pub sdram_write_data: Wires<32>,
    pub sdram_burst_length: Wires<8>,
}

/// Board inputs for the full CPU V3 system that owns both fitted memories
/// and the onboard HDMI port concurrently.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KBootHdmiInputs {
    pub buttons: Wires<2>,
    pub flash_miso: digital_design_circuit::Wire,
    pub sdram_read_data: Wires<32>,
    pub sdram_read_valid: digital_design_circuit::Wire,
    pub sdram_init_done: digital_design_circuit::Wire,
    pub sdram_command_ack: digital_design_circuit::Wire,
    pub pixel_clock: digital_design_circuit::Wire,
    pub serial_clock: digital_design_circuit::Wire,
    pub video_locked: digital_design_circuit::Wire,
}

/// Raw Flash-read, Controller HS, and HDMI ports for the full CPU V3 system.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KBootHdmiOutputs {
    pub leds: Wires<6>,
    pub uart_tx: digital_design_circuit::Wire,
    pub flash_clk: digital_design_circuit::Wire,
    pub flash_cs_n: digital_design_circuit::Wire,
    pub flash_mosi: digital_design_circuit::Wire,
    pub sdram_command_valid: digital_design_circuit::Wire,
    pub sdram_command: Wires<3>,
    pub sdram_precharge: digital_design_circuit::Wire,
    pub sdram_address: Wires<21>,
    pub sdram_write_mask: Wires<4>,
    pub sdram_write_data: Wires<32>,
    pub sdram_burst_length: Wires<8>,
    pub tmds_clk_p: digital_design_circuit::Wire,
    pub tmds_clk_n: digital_design_circuit::Wire,
    pub tmds_data_p: Wires<3>,
    pub tmds_data_n: Wires<3>,
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

    /// HDMI pairs in clock, blue, green, red order. An empty `io_type` leaves
    /// the electrical standard to the fitted `ELVDS_OBUF` primitive.
    pub const HDMI_CLK_P: GowinPin = Self::differential_pin(33);
    pub const HDMI_CLK_N: GowinPin = Self::differential_pin(34);
    pub const HDMI_DATA_P: [GowinPin; 3] = [
        Self::differential_pin(35),
        Self::differential_pin(37),
        Self::differential_pin(39),
    ];
    pub const HDMI_DATA_N: [GowinPin; 3] = [
        Self::differential_pin(36),
        Self::differential_pin(38),
        Self::differential_pin(40),
    ];

    pub const SPI_FLASH_CLK: GowinPin = GowinPin {
        location: 59,
        io_type: "LVCMOS33",
        pull_mode: None,
        drive: Some(8),
        active_low: false,
    };

    pub const SPI_FLASH_CS_N: GowinPin = GowinPin {
        location: 60,
        io_type: "LVCMOS33",
        pull_mode: Some("UP"),
        drive: Some(8),
        active_low: false,
    };

    pub const SPI_FLASH_MOSI: GowinPin = GowinPin {
        location: 61,
        io_type: "LVCMOS33",
        pull_mode: None,
        drive: Some(8),
        active_low: false,
    };

    pub const SPI_FLASH_MISO: GowinPin = GowinPin {
        location: 62,
        io_type: "LVCMOS33",
        pull_mode: Some("UP"),
        drive: None,
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

    const fn differential_pin(location: u16) -> GowinPin {
        GowinPin {
            location,
            io_type: "",
            pull_mode: None,
            drive: None,
            active_low: false,
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

    /// Create a 27 MHz project that exclusively owns the onboard HDMI port.
    /// The application is responsible for the video PLL, TMDS coding,
    /// serialization, and fitted differential output buffers.
    pub fn hdmi_project<M>(project_name: impl Into<String>) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KHdmiInputs, Output = TangNano20KHdmiOutputs>,
    {
        let binding =
            GowinBoardBinding::new("tang_nano_20k_hdmi_top", "clk", "clk", Self::CLOCK_27M)
                .require(Clock27M)
                .require(UserButtons::<2>)
                .require(UserLeds::<6>)
                .require(Pll)
                .require(HdmiOutput)
                .bind_port(
                    GowinPortDirection::Input,
                    "buttons",
                    "buttons",
                    Self::USER_BUTTONS,
                )
                .bind_port(GowinPortDirection::Output, "leds", "leds", Self::USER_LEDS)
                .bind_port(
                    GowinPortDirection::Output,
                    "tmds_clk_p",
                    "tmds_clk_p",
                    [Self::HDMI_CLK_P],
                )
                .bind_port(
                    GowinPortDirection::Output,
                    "tmds_clk_n",
                    "tmds_clk_n",
                    [Self::HDMI_CLK_N],
                )
                .bind_port(
                    GowinPortDirection::Output,
                    "tmds_data_p",
                    "tmds_data_p",
                    Self::HDMI_DATA_P,
                )
        .bind_port(
            GowinPortDirection::Output,
            "tmds_data_n",
            "tmds_data_n",
            Self::HDMI_DATA_N,
        )
        .with_extension(GowinBoardExtension::new("").add_sdc_constraint(
            "create_clock -name pixel_clk -period 13.468013 -waveform {0 6.734007} [get_pins {u_logic/u_pixel_divider/CLKOUT}]",
        ));
        GowinModuleProject::new(GowinProject::new(project_name).with_board_binding(binding))
    }

    /// Create a 27 MHz project that exposes the fitted configuration Flash to
    /// a `SpiFlashReader::hardware` instance in the application module.
    ///
    /// This binding enables the GW2AR MSPI configuration pins as regular user
    /// IO. The reader module, rather than this binding, claims the indivisible
    /// `SpiFlashDevice` target resource.
    pub fn flash_debug_uart_project<M>(
        project_name: impl Into<String>,
    ) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KFlashInputs, Output = TangNano20KFlashOutputs>,
    {
        let binding =
            GowinBoardBinding::new("tang_nano_20k_flash_top", "clk", "clk", Self::CLOCK_27M)
                .require(Clock27M)
                .require(UserButtons::<2>)
                .require(UserLeds::<6>)
                .require(DebugUartTx)
                .bind_port(
                    GowinPortDirection::Input,
                    "buttons",
                    "buttons",
                    Self::USER_BUTTONS,
                )
                .bind_port(GowinPortDirection::Output, "leds", "leds", Self::USER_LEDS)
                .bind_port(
                    GowinPortDirection::Output,
                    "uart_tx",
                    "uart_tx",
                    [Self::DEBUG_UART_TX],
                )
                .bind_port(
                    GowinPortDirection::Output,
                    "flash_clk",
                    "flash_clk",
                    [Self::SPI_FLASH_CLK],
                )
                .bind_port(
                    GowinPortDirection::Output,
                    "flash_cs_n",
                    "flash_cs_n",
                    [Self::SPI_FLASH_CS_N],
                )
                .bind_port(
                    GowinPortDirection::Output,
                    "flash_mosi",
                    "flash_mosi",
                    [Self::SPI_FLASH_MOSI],
                )
                .bind_port(
                    GowinPortDirection::Input,
                    "flash_miso",
                    "flash_miso",
                    [Self::SPI_FLASH_MISO],
                )
                .with_process_option("-use_mspi_as_gpio", "1");
        GowinModuleProject::new(GowinProject::new(project_name).with_board_binding(binding))
    }

    /// Create a single-clock 54 MHz project with the fitted 64-Mibit SDRAM.
    ///
    /// The board wrapper owns the PLL and Gowin Controller HS instance. User
    /// logic, command scheduling, caches, and the controller use the same
    /// 54 MHz clock. Only the SDRAM physical clock uses the PLL's 180-degree
    /// output.
    fn sdram_debug_uart_binding_with_video(video: bool) -> GowinBoardBinding<Self> {
        let wrapper = if video {
            include_str!("tang_nano_20k/sdram/service_54m_hdmi.v")
        } else {
            include_str!("tang_nano_20k/sdram/service_54m.v")
        };
        let mut extension = GowinBoardExtension::new(wrapper)
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

        if video {
            extension = extension
                .connect_logic(GowinLogicConnection::new(
                    "pixel_clock",
                    GowinPortDirection::Input,
                    1,
                    "video_pixel_clock",
                ))
                .connect_logic(GowinLogicConnection::new(
                    "serial_clock",
                    GowinPortDirection::Input,
                    1,
                    "video_serial_clock",
                ))
                .connect_logic(GowinLogicConnection::new(
                    "video_locked",
                    GowinPortDirection::Input,
                    1,
                    "video_locked",
                ))
                .add_source_file(
                    "src/generated/target/tang_nano_20k/sdram/video_pll_720p.v",
                    include_str!("tang_nano_20k/sdram/video_pll_720p.v"),
                )
                .add_sdc_constraint(
                    "create_clock -name pixel_clk -period 13.468013 -waveform {0 6.734007} [get_pins {u_video_pll/d/CLKOUT}]",
                )
                // The 54 MHz SDRAM/logic domain and the 74.25 MHz pixel domain
                // only meet through double-flop synchronizers (slot publish /
                // release handshake, pixel-domain reset release) and the
                // dual-clock line buffer, so the crossings must not be timed
                // as synchronous paths. The PLL-derived 54 MHz clock gets an
                // explicit generated-clock name so the group can reference it.
                .add_sdc_constraint(
                    "create_generated_clock -name sdram_clk -source [get_ports {clk}] -multiply_by 2 [get_pins {u_sdram_pll/rpll_inst/CLKOUT}]",
                )
                .add_sdc_constraint(
                    "set_clock_groups -asynchronous -group [get_clocks {pixel_clk}] -group [get_clocks {sdram_clk}]",
                );
        }

        let mut binding = Self::user_io_binding()
            .require(DebugUartTx)
            .require(Pll)
            .require(SdrSdram)
            .bind_port(
                GowinPortDirection::Output,
                "uart_tx",
                "uart_tx",
                [Self::DEBUG_UART_TX],
            );
        if video {
            binding = binding
                .require(Pll)
                .require(HdmiOutput)
                .bind_port(
                    GowinPortDirection::Output,
                    "tmds_clk_p",
                    "tmds_clk_p",
                    [Self::HDMI_CLK_P],
                )
                .bind_port(
                    GowinPortDirection::Output,
                    "tmds_clk_n",
                    "tmds_clk_n",
                    [Self::HDMI_CLK_N],
                )
                .bind_port(
                    GowinPortDirection::Output,
                    "tmds_data_p",
                    "tmds_data_p",
                    Self::HDMI_DATA_P,
                )
                .bind_port(
                    GowinPortDirection::Output,
                    "tmds_data_n",
                    "tmds_data_n",
                    Self::HDMI_DATA_N,
                );
        }
        binding.with_extension(extension)
    }

    fn sdram_debug_uart_binding() -> GowinBoardBinding<Self> {
        Self::sdram_debug_uart_binding_with_video(false)
    }

    pub fn sdram_debug_uart_project<M>(
        project_name: impl Into<String>,
    ) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KSdramInputs, Output = TangNano20KSdramOutputs>,
    {
        GowinModuleProject::new(
            GowinProject::new(project_name).with_board_binding(Self::sdram_debug_uart_binding()),
        )
    }

    pub fn sdram_hdmi_debug_uart_project<M>(
        project_name: impl Into<String>,
    ) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KSdramHdmiInputs, Output = TangNano20KSdramHdmiOutputs>,
    {
        GowinModuleProject::new(
            GowinProject::new(project_name)
                .with_board_binding(Self::sdram_debug_uart_binding_with_video(true)),
        )
    }

    /// Create a single-clock 54 MHz boot project with simultaneous access to
    /// the fitted SPI Flash and SDRAM devices.
    ///
    /// The Flash reader leaf owns the indivisible Flash resource. SDRAM and
    /// its PLL/controller are owned here at the lowest target-specific board
    /// boundary. Higher-level Stage0 and Stage1 logic claims neither device.
    pub fn boot_memory_project<M>(project_name: impl Into<String>) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KBootInputs, Output = TangNano20KBootOutputs>,
    {
        let binding = Self::sdram_debug_uart_binding()
            .bind_port(
                GowinPortDirection::Output,
                "flash_clk",
                "flash_clk",
                [Self::SPI_FLASH_CLK],
            )
            .bind_port(
                GowinPortDirection::Output,
                "flash_cs_n",
                "flash_cs_n",
                [Self::SPI_FLASH_CS_N],
            )
            .bind_port(
                GowinPortDirection::Output,
                "flash_mosi",
                "flash_mosi",
                [Self::SPI_FLASH_MOSI],
            )
            .bind_port(
                GowinPortDirection::Input,
                "flash_miso",
                "flash_miso",
                [Self::SPI_FLASH_MISO],
            )
            .with_process_option("-use_mspi_as_gpio", "1");
        GowinModuleProject::new(GowinProject::new(project_name).with_board_binding(binding))
    }

    /// Create a multi-clock project that simultaneously owns the fitted SPI
    /// Flash, the 64-Mibit SDRAM, and the onboard 720p HDMI port.
    ///
    /// This is the full CPU V3 system surface: the board wrapper owns the SDRAM
    /// PLL/Controller HS and the video PLL, while the Flash reader leaf owns the
    /// SPI Flash device. Higher-level logic claims none of these devices.
    pub fn boot_hdmi_memory_project<M>(
        project_name: impl Into<String>,
    ) -> GowinModuleProject<Self, M>
    where
        M: Module<Input = TangNano20KBootHdmiInputs, Output = TangNano20KBootHdmiOutputs>,
    {
        let binding = Self::sdram_debug_uart_binding_with_video(true)
            .bind_port(
                GowinPortDirection::Output,
                "flash_clk",
                "flash_clk",
                [Self::SPI_FLASH_CLK],
            )
            .bind_port(
                GowinPortDirection::Output,
                "flash_cs_n",
                "flash_cs_n",
                [Self::SPI_FLASH_CS_N],
            )
            .bind_port(
                GowinPortDirection::Output,
                "flash_mosi",
                "flash_mosi",
                [Self::SPI_FLASH_MOSI],
            )
            .bind_port(
                GowinPortDirection::Input,
                "flash_miso",
                "flash_miso",
                [Self::SPI_FLASH_MISO],
            )
            .with_process_option("-use_mspi_as_gpio", "1");
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
        external_flash_access: crate::GowinExternalFlashAccess::GaoBridge,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ErasedSpiFlashImage, HardwareIdentity, ResourceKind, SpiFlashReader, SpiFlashReaderInput,
        VerilogIdentity,
    };
    use digital_design_circuit::{input_const, input_w_const};

    struct FlashBoard;

    impl HardwareIdentity for FlashBoard {
        const TARGET_RESOURCE_LEAF: bool = false;

        fn verilog_identity() -> VerilogIdentity {
            VerilogIdentity::new("FlashBoard").namespace(["tests", "target"])
        }
    }

    impl Module for FlashBoard {
        type Input = TangNano20KFlashInputs;
        type Output = TangNano20KFlashOutputs;
        type EmuState = ();

        const USES_MAIN_CLOCK: bool = true;

        fn build_verilog(input: &Self::Input) -> Self::Output {
            let mut length = input_w_const::<24>(0);
            length.wires[0] = input_const(1);
            let flash = SpiFlashReader::<ErasedSpiFlashImage>::hardware(&SpiFlashReaderInput {
                start: input.buttons.wires[0],
                address: input_w_const(0),
                length,
                data_ready: input_const(1),
                flash_miso: input.flash_miso,
            });
            Self::Output {
                leds: Wires {
                    wires: flash.data.wires[..6].try_into().unwrap(),
                },
                uart_tx: input_const(1),
                flash_clk: flash.flash_clk,
                flash_cs_n: flash.flash_cs_n,
                flash_mosi: flash.flash_mosi,
            }
        }
    }

    struct BootBoard;

    impl HardwareIdentity for BootBoard {
        const TARGET_RESOURCE_LEAF: bool = false;

        fn verilog_identity() -> VerilogIdentity {
            VerilogIdentity::new("BootBoard").namespace(["tests", "target"])
        }
    }

    impl Module for BootBoard {
        type Input = TangNano20KBootInputs;
        type Output = TangNano20KBootOutputs;
        type EmuState = ();

        const USES_MAIN_CLOCK: bool = true;

        fn build_verilog(input: &Self::Input) -> Self::Output {
            let flash = SpiFlashReader::<ErasedSpiFlashImage>::hardware(&SpiFlashReaderInput {
                start: input.buttons.wires[0],
                address: input_w_const(0),
                length: input_w_const(1),
                data_ready: input_const(1),
                flash_miso: input.flash_miso,
            });
            Self::Output {
                leds: input_w_const(0),
                uart_tx: input_const(1),
                flash_clk: flash.flash_clk,
                flash_cs_n: flash.flash_cs_n,
                flash_mosi: flash.flash_mosi,
                sdram_command_valid: input_const(0),
                sdram_command: input_w_const(0),
                sdram_precharge: input_const(0),
                sdram_address: input_w_const(0),
                sdram_write_mask: input_w_const(0),
                sdram_write_data: input_w_const(0),
                sdram_burst_length: input_w_const(0),
            }
        }
    }

    #[test]
    fn flash_project_binds_mspi_and_claims_one_fitted_device() {
        let project = TangNano20K::flash_debug_uart_project::<FlashBoard>("flash_board")
            .generate()
            .unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::SpiFlashDevice], 1);
        assert!(project.files[std::path::Path::new("build.tcl")]
            .contains("set_option -use_mspi_as_gpio 1"));
        let constraints = &project.files[std::path::Path::new("src/generated/board.cst")];
        for (signal, pin) in [
            ("flash_clk", 59),
            ("flash_cs_n", 60),
            ("flash_mosi", 61),
            ("flash_miso", 62),
        ] {
            assert!(constraints.contains(&format!("IO_LOC \"{signal}\" {pin};")));
        }
    }

    #[test]
    fn boot_project_binds_both_fitted_memories_once() {
        let project = TangNano20K::boot_memory_project::<BootBoard>("boot_board")
            .generate()
            .unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::SpiFlashDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
        let constraints = &project.files[std::path::Path::new("src/generated/board.cst")];
        for (signal, pin) in [
            ("flash_clk", 59),
            ("flash_cs_n", 60),
            ("flash_mosi", 61),
            ("flash_miso", 62),
        ] {
            assert!(constraints.contains(&format!("IO_LOC \"{signal}\" {pin};")));
        }
    }

    struct HdmiBoard;

    impl HardwareIdentity for HdmiBoard {
        const TARGET_RESOURCE_LEAF: bool = false;

        fn verilog_identity() -> VerilogIdentity {
            VerilogIdentity::new("HdmiBoard").namespace(["tests", "target"])
        }
    }

    impl Module for HdmiBoard {
        type Input = TangNano20KHdmiInputs;
        type Output = TangNano20KHdmiOutputs;
        type EmuState = ();

        const USES_MAIN_CLOCK: bool = true;

        fn build_verilog(input: &Self::Input) -> Self::Output {
            let low = input.buttons.wires[0];
            let high = input.buttons.wires[1];
            Self::Output {
                leds: Wires {
                    wires: [low, low, low, high, high, high],
                },
                tmds_clk_p: low,
                tmds_clk_n: high,
                tmds_data_p: Wires {
                    wires: [low, low, low],
                },
                tmds_data_n: Wires {
                    wires: [high, high, high],
                },
            }
        }
    }

    #[test]
    fn hdmi_project_claims_one_port_and_binds_all_four_pairs() {
        let project = TangNano20K::hdmi_project::<HdmiBoard>("hdmi_board")
            .generate()
            .unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::HdmiOutput], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
        let constraints = &project.files[std::path::Path::new("src/generated/board.cst")];
        for (signal, pin) in [
            ("tmds_clk_p", 33),
            ("tmds_clk_n", 34),
            ("tmds_data_p[0]", 35),
            ("tmds_data_n[0]", 36),
            ("tmds_data_p[1]", 37),
            ("tmds_data_n[1]", 38),
            ("tmds_data_p[2]", 39),
            ("tmds_data_n[2]", 40),
        ] {
            assert!(constraints.contains(&format!("IO_LOC \"{signal}\" {pin};")));
        }
    }
}
