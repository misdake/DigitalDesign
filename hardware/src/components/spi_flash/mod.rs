//! Read-only streaming access to a board-fitted SPI NOR Flash.

use crate::resources::components::SpiFlash;
use crate::{HardwareIdentity, Module, ModuleIo, TargetResourceRequest, VerilogIdentity};
use askama::Template;
use digital_design_code::{CircuitWires, Wire, Wires};
use std::marker::PhantomData;

/// Host-side contents used by the emulator backend.
///
/// Bytes outside the supplied image read as `FF`, matching erased NOR Flash.
/// The image affects emulation only; generated Verilog reads the fitted device.
pub trait SpiFlashImage: 'static {
    const BYTES: &'static [u8];
}

/// An entirely erased emulated Flash image.
pub struct ErasedSpiFlashImage;

impl SpiFlashImage for ErasedSpiFlashImage {
    const BYTES: &'static [u8] = &[];
}

/// A read-only standard-SPI burst reader.
///
/// A command is accepted when `start && ready`. `length == 0` completes
/// without touching the physical device. For a non-empty command, response
/// bytes are held with `data_valid` until `data_ready`; the SPI clock pauses
/// low while backpressured. `done` pulses after the final byte is accepted.
///
/// This is a target-resource leaf because it owns the complete fitted Flash
/// device. The physical `flash_*` signals must be connected by the selected
/// board binding. The implementation emits only the read-data command `03h`.
pub struct SpiFlashReader<
    I,
    const CAPACITY_BYTES: u32 = 8_388_608,
    const HALF_PERIOD_CYCLES: u64 = 2,
>(PhantomData<I>);

impl<I, const CAPACITY_BYTES: u32, const HALF_PERIOD_CYCLES: u64> HardwareIdentity
    for SpiFlashReader<I, CAPACITY_BYTES, HALF_PERIOD_CYCLES>
where
    I: SpiFlashImage,
{
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        assert!(
            HALF_PERIOD_CYCLES > 0,
            "SPI Flash half-period must contain at least one main-clock cycle"
        );
        assert!(
            CAPACITY_BYTES > 0 && CAPACITY_BYTES <= 1 << 24,
            "SPI Flash capacity must be from 1 through 16777216 bytes, found {CAPACITY_BYTES}"
        );
        VerilogIdentity::new("SpiFlashReader")
            .namespace(["components", "memory", "spi_flash"])
            .constant("CAPACITY_BYTES", CAPACITY_BYTES)
            .constant("HALF_PERIOD_CYCLES", HALF_PERIOD_CYCLES)
    }
}

#[derive(Clone, ModuleIo)]
pub struct SpiFlashReaderInput {
    pub start: Wire,
    pub address: Wires<24>,
    pub length: Wires<24>,
    pub data_ready: Wire,
    /// Physical Flash IO1/DO input.
    pub flash_miso: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct SpiFlashReaderOutput {
    pub ready: Wire,
    pub data_valid: Wire,
    pub data: Wires<8>,
    pub done: Wire,
    /// Pulses with `done` when a command exceeds this specialization's capacity.
    pub error: Wire,
    /// Physical Flash clock output.
    pub flash_clk: Wire,
    /// Physical active-low chip select output.
    pub flash_cs_n: Wire,
    /// Physical Flash IO0/DI output.
    pub flash_mosi: Wire,
}

pub struct SpiFlashReaderState {
    ready: bool,
    data_valid: bool,
    data: u8,
    done: bool,
    error: bool,
    address: u32,
    remaining: u32,
    delay_cycles: u64,
}

#[derive(Template)]
#[template(path = "components/spi_flash/spi_flash_reader.v", escape = "none")]
struct SpiFlashReaderTemplate<'a> {
    module_name: &'a str,
    half_period_cycles: u64,
    capacity_bytes: u32,
}

#[derive(Template)]
#[template(path = "components/spi_flash/spi_flash_reader_tb.v", escape = "none")]
struct SpiFlashReaderTestbenchTemplate<'a> {
    module_name: &'a str,
    expected_0: u8,
    expected_1: u8,
    expected_2: u8,
}

fn image_byte<I: SpiFlashImage>(address: usize) -> u8 {
    I::BYTES.get(address).copied().unwrap_or(0xff)
}

impl<I, const CAPACITY_BYTES: u32, const HALF_PERIOD_CYCLES: u64> Module
    for SpiFlashReader<I, CAPACITY_BYTES, HALF_PERIOD_CYCLES>
where
    I: SpiFlashImage,
{
    type Input = SpiFlashReaderInput;
    type Output = SpiFlashReaderOutput;
    type EmuState = SpiFlashReaderState;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SpiFlash)]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        assert!(
            HALF_PERIOD_CYCLES > 0,
            "SPI Flash half-period must contain at least one main-clock cycle"
        );
        assert!(
            CAPACITY_BYTES > 0 && CAPACITY_BYTES <= 1 << 24,
            "SPI Flash capacity must be from 1 through 16777216 bytes, found {CAPACITY_BYTES}"
        );
        SpiFlashReaderState {
            ready: true,
            data_valid: false,
            data: 0,
            done: false,
            error: false,
            address: 0,
            remaining: 0,
            delay_cycles: 0,
        }
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &SpiFlashReaderOutputValue {
                ready: state.ready,
                data_valid: state.data_valid,
                data: u64::from(state.data),
                done: state.done,
                error: state.error,
                flash_clk: false,
                flash_cs_n: state.ready || (state.data_valid && state.remaining == 0),
                flash_mosi: false,
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        state.done = false;
        state.error = false;

        if state.data_valid {
            if input.data_ready {
                state.data_valid = false;
                if state.remaining == 0 {
                    state.ready = true;
                    state.done = true;
                } else {
                    state.delay_cycles = 16 * HALF_PERIOD_CYCLES;
                }
            }
            return;
        }

        if !state.ready {
            if state.delay_cycles > 1 {
                state.delay_cycles -= 1;
            } else {
                state.data = I::BYTES
                    .get(state.address as usize)
                    .copied()
                    .unwrap_or(0xff);
                state.address = (state.address + 1) & 0x00ff_ffff;
                state.remaining -= 1;
                state.data_valid = true;
                state.delay_cycles = 0;
            }
            return;
        }

        if input.start {
            if input.length == 0 {
                state.done = true;
            } else if input.address + input.length > u64::from(CAPACITY_BYTES) {
                state.done = true;
                state.error = true;
            } else {
                state.ready = false;
                state.address = input.address as u32;
                state.remaining = input.length as u32;
                // Command/address (32 SPI bits) followed by the first byte.
                state.delay_cycles = 80 * HALF_PERIOD_CYCLES;
            }
        }
    }

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            SpiFlashReaderTemplate {
                module_name: &module_name,
                half_period_cycles: HALF_PERIOD_CYCLES,
                capacity_bytes: CAPACITY_BYTES,
            }
            .render()
            .expect("SPI Flash reader Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            SpiFlashReaderTestbenchTemplate {
                module_name: &module_name,
                expected_0: image_byte::<I>(1),
                expected_1: image_byte::<I>(2),
                expected_2: image_byte::<I>(3),
            }
            .render()
            .expect("SPI Flash reader testbench template must render"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceAmount, ResourceKind, VerilogProject};
    use digital_design_code::build_circuit;

    struct TestImage;

    impl SpiFlashImage for TestImage {
        const BYTES: &'static [u8] = &[0x12, 0x34, 0x56, 0x78];
    }

    type Reader = SpiFlashReader<TestImage, 8_388_608, 1>;

    #[test]
    fn emulator_streams_and_holds_bytes_under_backpressure() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = SpiFlashReaderInput::allocate();
            let output = Reader::hardware(&input);
            (input, output)
        });

        input.drive(
            &mut circuit,
            &SpiFlashReaderInputValue {
                start: true,
                address: 1,
                length: 3,
                data_ready: false,
                flash_miso: false,
            },
        );
        circuit.execute_gates();
        circuit.clock_tick();
        input.drive(
            &mut circuit,
            &SpiFlashReaderInputValue {
                start: false,
                address: 0,
                length: 0,
                data_ready: false,
                flash_miso: false,
            },
        );

        let mut received = Vec::new();
        for cycle in 0..300 {
            circuit.execute_gates();
            let value = output.sample(&circuit);
            if value.data_valid {
                assert!(!value.ready);
                if cycle % 3 == 0 {
                    received.push(value.data as u8);
                    input.drive(
                        &mut circuit,
                        &SpiFlashReaderInputValue {
                            start: false,
                            address: 0,
                            length: 0,
                            data_ready: true,
                            flash_miso: false,
                        },
                    );
                }
            } else {
                input.drive(
                    &mut circuit,
                    &SpiFlashReaderInputValue {
                        start: false,
                        address: 0,
                        length: 0,
                        data_ready: false,
                        flash_miso: false,
                    },
                );
            }
            circuit.clock_tick();
            if received == [0x34, 0x56, 0x78] {
                break;
            }
        }
        assert_eq!(received, [0x34, 0x56, 0x78]);
    }

    #[test]
    fn export_is_a_single_flash_resource_leaf() {
        let project = VerilogProject::generate::<Reader>().unwrap();
        assert_eq!(project.resource_claims.len(), 1);
        assert_eq!(
            project.resource_claims[0].resources,
            [ResourceAmount::new(ResourceKind::SpiFlashDevice, 1)]
        );
        let source = project.files.values().next().unwrap();
        assert!(source.contains("8'h03"));
        assert!(!source.contains("8'h06"));
        assert!(!source.contains("8'h02"));
    }

    #[test]
    fn emulator_rejects_a_request_past_the_fitted_capacity() {
        type TinyReader = SpiFlashReader<TestImage, 4, 1>;
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = SpiFlashReaderInput::allocate();
            let output = TinyReader::hardware(&input);
            (input, output)
        });
        input.drive(
            &mut circuit,
            &SpiFlashReaderInputValue {
                start: true,
                address: 3,
                length: 2,
                data_ready: true,
                flash_miso: false,
            },
        );
        circuit.execute_gates();
        circuit.clock_tick();
        circuit.execute_gates();
        let value = output.sample(&circuit);
        assert!(value.ready && value.done && value.error && value.flash_cs_n);
        assert!(!value.data_valid);
    }

    #[test]
    #[ignore = "explicit external simulator validation of the SPI device protocol"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<Reader>().unwrap();
    }
}
