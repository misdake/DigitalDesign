//! Small self-contained DDHT status-frame transmitter.

use crate::{Hardware, HardwareIdentity, Module, ModuleIo};
use askama::Template;
use digital_design_circuit::{CircuitWires, Wire, Wires};

#[derive(Clone, ModuleIo)]
pub struct DiagnosticReporterInput {
    /// Enables reporting. Disabling aborts a frame and restores the UART idle level.
    pub report_enable: Wire,
    /// Status sampled atomically when a frame begins; zero conventionally means success.
    pub status: Wires<8>,
}

#[derive(Clone, ModuleIo)]
pub struct DiagnosticReporterOutput {
    pub uart_tx: Wire,
    pub uart_busy: Wire,
    /// Toggles after each complete frame, useful for an LED or logic analyzer.
    pub frame_toggle: Wire,
}

/// Periodically emits the eight-byte DDHT v1 status frame.
///
/// Timing constants are expressed in main-clock cycles. Keeping this protocol
/// in a leaf module prevents CPU and memory harnesses from each growing their
/// own subtly different UART state machine.
#[derive(Hardware)]
#[hardware(namespace = "components/diagnostics")]
pub struct DiagnosticReporter<
    const TEST_ID: u8,
    const CLOCKS_PER_BIT: u16,
    const FIRST_REPORT_DELAY: u32,
    const REPORT_INTERVAL: u32,
>;

#[derive(Template)]
#[template(path = "diagnostic_reporter/diagnostic_reporter.v", escape = "none")]
struct DiagnosticReporterTemplate<'a> {
    module_name: &'a str,
    test_id: u8,
    checksum_base: u8,
    clocks_per_bit_minus_one: u16,
    uart_counter_width: usize,
    uart_counter_high_bit: usize,
    first_report_delay_minus_one: u32,
    report_interval_minus_one: u32,
    delay_counter_width: usize,
    delay_counter_high_bit: usize,
}

#[derive(Template)]
#[template(path = "diagnostic_reporter/diagnostic_reporter_tb.v", escape = "none")]
struct DiagnosticReporterTestbenchTemplate<'a> {
    module_name: &'a str,
    test_id: u8,
    checksum_base: u8,
    clocks_per_bit: u16,
}

impl<
        const TEST_ID: u8,
        const CLOCKS_PER_BIT: u16,
        const FIRST_REPORT_DELAY: u32,
        const REPORT_INTERVAL: u32,
    > Module for DiagnosticReporter<TEST_ID, CLOCKS_PER_BIT, FIRST_REPORT_DELAY, REPORT_INTERVAL>
{
    type Input = DiagnosticReporterInput;
    type Output = DiagnosticReporterOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("DiagnosticReporter is a generated-Verilog leaf")
    }

    fn generated_verilog_source() -> Option<String> {
        validate::<CLOCKS_PER_BIT, FIRST_REPORT_DELAY, REPORT_INTERVAL>();
        let module_name = Self::verilog_identity().module_name();
        let uart_counter_width = counter_width(u32::from(CLOCKS_PER_BIT - 1));
        let maximum_delay = FIRST_REPORT_DELAY.max(REPORT_INTERVAL) - 1;
        let delay_counter_width = counter_width(maximum_delay);
        Some(
            DiagnosticReporterTemplate {
                module_name: &module_name,
                test_id: TEST_ID,
                checksum_base: 0x1d ^ TEST_ID,
                clocks_per_bit_minus_one: CLOCKS_PER_BIT - 1,
                uart_counter_width,
                uart_counter_high_bit: uart_counter_width - 1,
                first_report_delay_minus_one: FIRST_REPORT_DELAY - 1,
                report_interval_minus_one: REPORT_INTERVAL - 1,
                delay_counter_width,
                delay_counter_high_bit: delay_counter_width - 1,
            }
            .render()
            .expect("diagnostic reporter Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        validate::<CLOCKS_PER_BIT, FIRST_REPORT_DELAY, REPORT_INTERVAL>();
        let module_name = Self::verilog_identity().module_name();
        Some(
            DiagnosticReporterTestbenchTemplate {
                module_name: &module_name,
                test_id: TEST_ID,
                checksum_base: 0x1d ^ TEST_ID,
                clocks_per_bit: CLOCKS_PER_BIT,
            }
            .render()
            .expect("diagnostic reporter testbench template must render"),
        )
    }
}

fn validate<
    const CLOCKS_PER_BIT: u16,
    const FIRST_REPORT_DELAY: u32,
    const REPORT_INTERVAL: u32,
>() {
    assert!(CLOCKS_PER_BIT > 0, "UART bit period must be non-zero");
    assert!(
        FIRST_REPORT_DELAY > 0,
        "first diagnostic report delay must be non-zero"
    );
    assert!(
        REPORT_INTERVAL > 0,
        "diagnostic report interval must be non-zero"
    );
}

fn counter_width(maximum: u32) -> usize {
    usize::try_from(u32::BITS - maximum.leading_zeros())
        .unwrap()
        .max(1)
}

#[cfg(test)]
mod tests {
    type TestReporter = super::DiagnosticReporter<0x2a, 4, 3, 5>;

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn frames_latch_status_and_repeat_in_iverilog() {
        crate::verify_verilog_with_iverilog::<TestReporter>().unwrap();
    }
}
