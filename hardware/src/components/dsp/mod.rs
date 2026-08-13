//! Registered Gowin-inferred DSP datapaths.
//!
//! These target leaves intentionally mirror RTL shapes measured on the
//! GW2AR-18. Add a specialization only after its inference, resource count,
//! timing, and arithmetic latency have been verified.

use crate::resources::components::DspMultipliers;
use crate::{
    Hardware, HardwareIdentity, Module, ModuleIo, ModuleTest, TargetResourceRequest, TestStep,
};
use askama::Template;
use digital_design_code::{CircuitWires, Wires};

const INPUT_WIDTH: usize = 18;
const PRODUCT_WIDTH: usize = 36;
const ACCUMULATOR_WIDTH: usize = 54;

fn signed(value: u64, width: usize) -> i64 {
    debug_assert!((1..64).contains(&width));
    ((value << (64 - width)) as i64) >> (64 - width)
}

fn bits(value: i128, width: usize) -> u64 {
    debug_assert!((1..64).contains(&width));
    (value & ((1i128 << width) - 1)) as u64
}

fn wrapped_signed(value: i128, width: usize) -> i64 {
    signed(bits(value, width), width)
}

#[derive(Clone, Copy, Debug)]
enum DspShape {
    Multiply,
    MultiplyAdd,
    MultiplyAccumulate,
}

fn verified_multiplier_lanes(shape: DspShape) -> u64 {
    match shape {
        DspShape::Multiply => 1,
        DspShape::MultiplyAdd | DspShape::MultiplyAccumulate => 2,
    }
}

fn dsp_resource(shape: DspShape) -> Vec<TargetResourceRequest> {
    // The inventory unit is one 18x18 multiplier lane. A plain multiplier
    // occupies half a GW2AR DSP macro; a wide-ALU operation occupies the full
    // macro and therefore reserves two lanes. PnR remains authoritative for
    // heterogeneous packing across larger groups of instances.
    vec![TargetResourceRequest::new(DspMultipliers::new(
        verified_multiplier_lanes(shape),
    ))]
}

#[derive(Template)]
#[template(path = "components/dsp/mul_s18.v", escape = "none")]
struct MulTemplate<'a> {
    module_name: &'a str,
}

/// Two-stage registered signed 18 x 18 multiply.
///
/// Inputs sampled on one rising edge appear at `product` after the following
/// rising edge. Gowin maps this measured RTL shape to one `MULT18X18`.
#[derive(Hardware)]
#[hardware(namespace = "components/arithmetic/dsp", target_leaf)]
pub struct DspMulS18;

#[derive(Clone, ModuleIo)]
pub struct DspMulS18Input {
    pub a: Wires<INPUT_WIDTH>,
    pub b: Wires<INPUT_WIDTH>,
}

#[derive(Clone, ModuleIo)]
pub struct DspMulS18Output {
    pub product: Wires<PRODUCT_WIDTH>,
}

#[derive(Default)]
pub struct DspMulS18State {
    a: i64,
    b: i64,
    product: i64,
}

impl Module for DspMulS18 {
    type Input = DspMulS18Input;
    type Output = DspMulS18Output;
    type EmuState = DspMulS18State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        dsp_resource(DspShape::Multiply)
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        DspMulS18State::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &DspMulS18OutputValue {
                product: bits(i128::from(state.product), PRODUCT_WIDTH),
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
        state.product = state.a * state.b;
        state.a = signed(input.a, INPUT_WIDTH);
        state.b = signed(input.b, INPUT_WIDTH);
    }

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            MulTemplate {
                module_name: &module_name,
            }
            .render()
            .expect("DSP multiply Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(mul_test().verilog_testbench())
    }
}

#[derive(Template)]
#[template(path = "components/dsp/mul_add_s18.v", escape = "none")]
struct MulAddTemplate<'a> {
    module_name: &'a str,
}

/// Two-stage signed 18 x 18 multiply plus a registered signed 36-bit addend.
/// Gowin maps this measured RTL shape to one `MULTADDALU18X18`.
#[derive(Hardware)]
#[hardware(namespace = "components/arithmetic/dsp", target_leaf)]
pub struct DspMulAddS18;

#[derive(Clone, ModuleIo)]
pub struct DspMulAddS18Input {
    pub a: Wires<INPUT_WIDTH>,
    pub b: Wires<INPUT_WIDTH>,
    pub addend: Wires<PRODUCT_WIDTH>,
}

#[derive(Clone, ModuleIo)]
pub struct DspMulAddS18Output {
    pub result: Wires<ACCUMULATOR_WIDTH>,
}

#[derive(Default)]
pub struct DspMulAddS18State {
    a: i64,
    b: i64,
    addend: i64,
    result: i64,
}

impl Module for DspMulAddS18 {
    type Input = DspMulAddS18Input;
    type Output = DspMulAddS18Output;
    type EmuState = DspMulAddS18State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        dsp_resource(DspShape::MultiplyAdd)
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        DspMulAddS18State::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &DspMulAddS18OutputValue {
                result: bits(i128::from(state.result), ACCUMULATOR_WIDTH),
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
        state.result = state.a * state.b + state.addend;
        state.a = signed(input.a, INPUT_WIDTH);
        state.b = signed(input.b, INPUT_WIDTH);
        state.addend = signed(input.addend, PRODUCT_WIDTH);
    }

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            MulAddTemplate {
                module_name: &module_name,
            }
            .render()
            .expect("DSP multiply-add Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(mul_add_test().verilog_testbench())
    }
}

#[derive(Template)]
#[template(path = "components/dsp/mac_s18.v", escape = "none")]
struct MacTemplate<'a> {
    module_name: &'a str,
}

/// Registered signed 18 x 18 multiply-accumulate with active-low reset.
/// Gowin maps this measured RTL shape to one `MULTADDALU18X18`.
#[derive(Hardware)]
#[hardware(namespace = "components/arithmetic/dsp", target_leaf)]
pub struct DspMacS18;

#[derive(Clone, ModuleIo)]
pub struct DspMacS18Input {
    pub reset_n: digital_design_code::Wire,
    pub a: Wires<INPUT_WIDTH>,
    pub b: Wires<INPUT_WIDTH>,
}

#[derive(Clone, ModuleIo)]
pub struct DspMacS18Output {
    pub accumulator: Wires<ACCUMULATOR_WIDTH>,
}

#[derive(Default)]
pub struct DspMacS18State {
    a: i64,
    b: i64,
    accumulator: i64,
}

impl Module for DspMacS18 {
    type Input = DspMacS18Input;
    type Output = DspMacS18Output;
    type EmuState = DspMacS18State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        dsp_resource(DspShape::MultiplyAccumulate)
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        DspMacS18State::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        if !input.sample(circuit).reset_n {
            *state = DspMacS18State::default();
        }
        output.drive(
            circuit,
            &DspMacS18OutputValue {
                accumulator: bits(i128::from(state.accumulator), ACCUMULATOR_WIDTH),
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
        if !input.reset_n {
            *state = DspMacS18State::default();
        } else {
            state.accumulator = wrapped_signed(
                i128::from(state.accumulator) + i128::from(state.a * state.b),
                ACCUMULATOR_WIDTH,
            );
            state.a = signed(input.a, INPUT_WIDTH);
            state.b = signed(input.b, INPUT_WIDTH);
        }
    }

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            MacTemplate {
                module_name: &module_name,
            }
            .render()
            .expect("DSP MAC Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(mac_test().verilog_testbench())
    }
}

fn encoded(value: i64, width: usize) -> u64 {
    bits(i128::from(value), width)
}

fn mul_test() -> ModuleTest<DspMulS18> {
    ModuleTest::new([
        TestStep::new(
            DspMulS18InputValue {
                a: encoded(-3, INPUT_WIDTH),
                b: encoded(7, INPUT_WIDTH),
            },
            DspMulS18OutputValue { product: 0 },
        ),
        TestStep::new(
            DspMulS18InputValue {
                a: encoded(5, INPUT_WIDTH),
                b: encoded(-9, INPUT_WIDTH),
            },
            DspMulS18OutputValue {
                product: encoded(-21, PRODUCT_WIDTH),
            },
        ),
        TestStep::new(
            DspMulS18InputValue {
                a: encoded(-131_072, INPUT_WIDTH),
                b: encoded(-1, INPUT_WIDTH),
            },
            DspMulS18OutputValue {
                product: encoded(-45, PRODUCT_WIDTH),
            },
        ),
        TestStep::new(
            DspMulS18InputValue { a: 0, b: 0 },
            DspMulS18OutputValue { product: 131_072 },
        ),
    ])
}

fn mul_add_test() -> ModuleTest<DspMulAddS18> {
    ModuleTest::new([
        TestStep::new(
            DspMulAddS18InputValue {
                a: encoded(-12, INPUT_WIDTH),
                b: encoded(9, INPUT_WIDTH),
                addend: encoded(40, PRODUCT_WIDTH),
            },
            DspMulAddS18OutputValue { result: 0 },
        ),
        TestStep::new(
            DspMulAddS18InputValue {
                a: encoded(131_071, INPUT_WIDTH),
                b: encoded(-2, INPUT_WIDTH),
                addend: encoded(-17, PRODUCT_WIDTH),
            },
            DspMulAddS18OutputValue {
                result: encoded(-68, ACCUMULATOR_WIDTH),
            },
        ),
        TestStep::new(
            DspMulAddS18InputValue {
                a: 0,
                b: 0,
                addend: 0,
            },
            DspMulAddS18OutputValue {
                result: encoded(-262_159, ACCUMULATOR_WIDTH),
            },
        ),
    ])
}

fn mac_test() -> ModuleTest<DspMacS18> {
    ModuleTest::new([
        TestStep::new(
            DspMacS18InputValue {
                reset_n: false,
                a: encoded(99, INPUT_WIDTH),
                b: encoded(99, INPUT_WIDTH),
            },
            DspMacS18OutputValue { accumulator: 0 },
        ),
        TestStep::new(
            DspMacS18InputValue {
                reset_n: true,
                a: encoded(-3, INPUT_WIDTH),
                b: encoded(7, INPUT_WIDTH),
            },
            DspMacS18OutputValue { accumulator: 0 },
        ),
        TestStep::new(
            DspMacS18InputValue {
                reset_n: true,
                a: encoded(5, INPUT_WIDTH),
                b: encoded(-9, INPUT_WIDTH),
            },
            DspMacS18OutputValue {
                accumulator: encoded(-21, ACCUMULATOR_WIDTH),
            },
        ),
        TestStep::new(
            DspMacS18InputValue {
                reset_n: true,
                a: 0,
                b: 0,
            },
            DspMacS18OutputValue {
                accumulator: encoded(-66, ACCUMULATOR_WIDTH),
            },
        ),
        TestStep::new(
            DspMacS18InputValue {
                reset_n: false,
                a: 0,
                b: 0,
            },
            DspMacS18OutputValue { accumulator: 0 },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{verify_verilog_with_iverilog, ResourceAmount, ResourceKind};

    #[test]
    fn measured_shapes_match_their_emulators() {
        mul_test().run_emu();
        mul_add_test().run_emu();
        mac_test().run_emu();
    }

    #[test]
    fn measured_shapes_claim_their_pnr_multiplier_lane_cost() {
        for (resources, lanes) in [
            (DspMulS18::target_resources(), 1),
            (DspMulAddS18::target_resources(), 2),
            (DspMacS18::target_resources(), 2),
        ] {
            assert_eq!(resources.len(), 1);
            assert_eq!(
                resources[0].resources,
                [ResourceAmount::new(ResourceKind::Multiplier18x18, lanes)]
            );
        }
    }

    #[test]
    #[ignore = "explicit external simulator validation of inferred DSP shapes"]
    fn verify_measured_shapes_with_iverilog() {
        verify_verilog_with_iverilog::<DspMulS18>().unwrap();
        verify_verilog_with_iverilog::<DspMulAddS18>().unwrap();
        verify_verilog_with_iverilog::<DspMacS18>().unwrap();
    }
}
