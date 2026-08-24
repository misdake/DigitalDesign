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
    MultiplySum,
    MultiplyDifference,
    PreAddMultiply,
    PreSubtractMultiply,
}

fn verified_multiplier_lanes(shape: DspShape) -> u64 {
    match shape {
        DspShape::Multiply | DspShape::PreAddMultiply | DspShape::PreSubtractMultiply => 1,
        DspShape::MultiplyAdd
        | DspShape::MultiplyAccumulate
        | DspShape::MultiplySum
        | DspShape::MultiplyDifference => 2,
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

#[derive(Clone, ModuleIo)]
pub struct DspMulPairS18Input {
    pub a: Wires<INPUT_WIDTH>,
    pub b: Wires<INPUT_WIDTH>,
    pub c: Wires<INPUT_WIDTH>,
    pub d: Wires<INPUT_WIDTH>,
}

#[derive(Clone, ModuleIo)]
pub struct DspMulPairS18Output {
    pub result: Wires<ACCUMULATOR_WIDTH>,
}

#[derive(Default)]
pub struct DspMulPairS18State {
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    result: i64,
}

fn execute_mul_pair(
    state: &DspMulPairS18State,
    circuit: &mut CircuitWires,
    output: &DspMulPairS18Output,
) {
    output.drive(
        circuit,
        &DspMulPairS18OutputValue {
            result: bits(i128::from(state.result), ACCUMULATOR_WIDTH),
        },
    );
}

fn clock_mul_pair(
    state: &mut DspMulPairS18State,
    circuit: &CircuitWires,
    input: &DspMulPairS18Input,
    subtract: bool,
) {
    let product_ab = i128::from(state.a * state.b);
    let product_cd = i128::from(state.c * state.d);
    let result = if subtract {
        product_ab - product_cd
    } else {
        product_ab + product_cd
    };
    state.result = wrapped_signed(result, ACCUMULATOR_WIDTH);
    let input = input.sample(circuit);
    state.a = signed(input.a, INPUT_WIDTH);
    state.b = signed(input.b, INPUT_WIDTH);
    state.c = signed(input.c, INPUT_WIDTH);
    state.d = signed(input.d, INPUT_WIDTH);
}

#[derive(Template)]
#[template(path = "components/dsp/mul_sum_s18.v", escape = "none")]
struct MulSumTemplate<'a> {
    module_name: &'a str,
}

/// Registered `(a * b) + (c * d)` using two signed 18 x 18 multipliers and
/// one 54-bit DSP ALU.
#[derive(Hardware)]
#[hardware(namespace = "components/arithmetic/dsp", target_leaf)]
pub struct DspMulSumS18;

impl Module for DspMulSumS18 {
    type Input = DspMulPairS18Input;
    type Output = DspMulPairS18Output;
    type EmuState = DspMulPairS18State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        dsp_resource(DspShape::MultiplySum)
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        DspMulPairS18State::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        execute_mul_pair(state, circuit, output);
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        clock_mul_pair(state, circuit, input, false);
    }

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            MulSumTemplate {
                module_name: &module_name,
            }
            .render()
            .expect("DSP multiply-sum Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(mul_pair_test::<Self>(false).verilog_testbench())
    }
}

#[derive(Template)]
#[template(path = "components/dsp/mul_difference_s18.v", escape = "none")]
struct MulDifferenceTemplate<'a> {
    module_name: &'a str,
}

/// Registered `(a * b) - (c * d)` using two signed 18 x 18 multipliers and
/// one 54-bit DSP ALU.
#[derive(Hardware)]
#[hardware(namespace = "components/arithmetic/dsp", target_leaf)]
pub struct DspMulDifferenceS18;

impl Module for DspMulDifferenceS18 {
    type Input = DspMulPairS18Input;
    type Output = DspMulPairS18Output;
    type EmuState = DspMulPairS18State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        dsp_resource(DspShape::MultiplyDifference)
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        DspMulPairS18State::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        execute_mul_pair(state, circuit, output);
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        clock_mul_pair(state, circuit, input, true);
    }

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            MulDifferenceTemplate {
                module_name: &module_name,
            }
            .render()
            .expect("DSP multiply-difference Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(mul_pair_test::<Self>(true).verilog_testbench())
    }
}

#[derive(Clone, ModuleIo)]
pub struct DspPreMulS18Input {
    pub a: Wires<INPUT_WIDTH>,
    pub b: Wires<INPUT_WIDTH>,
    pub c: Wires<INPUT_WIDTH>,
}

#[derive(Clone, ModuleIo)]
pub struct DspPreMulS18Output {
    pub product: Wires<PRODUCT_WIDTH>,
}

#[derive(Default)]
pub struct DspPreMulS18State {
    a: i64,
    b: i64,
    c: i64,
    product: i64,
}

fn execute_pre_mul(
    state: &DspPreMulS18State,
    circuit: &mut CircuitWires,
    output: &DspPreMulS18Output,
) {
    output.drive(
        circuit,
        &DspPreMulS18OutputValue {
            product: bits(i128::from(state.product), PRODUCT_WIDTH),
        },
    );
}

fn clock_pre_mul(
    state: &mut DspPreMulS18State,
    circuit: &CircuitWires,
    input: &DspPreMulS18Input,
    subtract: bool,
) {
    let pre = if subtract {
        state.a - state.b
    } else {
        state.a + state.b
    };
    let pre = wrapped_signed(i128::from(pre), INPUT_WIDTH);
    state.product = pre * state.c;
    let input = input.sample(circuit);
    state.a = signed(input.a, INPUT_WIDTH);
    state.b = signed(input.b, INPUT_WIDTH);
    state.c = signed(input.c, INPUT_WIDTH);
}

#[derive(Template)]
#[template(path = "components/dsp/pre_add_mul_s18.v", escape = "none")]
struct PreAddMulTemplate<'a> {
    module_name: &'a str,
}

/// Registered `(a + b) * c`; the pre-add wraps to signed 18 bits before the
/// multiply, matching the physical pre-adder data path.
#[derive(Hardware)]
#[hardware(namespace = "components/arithmetic/dsp", target_leaf)]
pub struct DspPreAddMulS18;

impl Module for DspPreAddMulS18 {
    type Input = DspPreMulS18Input;
    type Output = DspPreMulS18Output;
    type EmuState = DspPreMulS18State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        dsp_resource(DspShape::PreAddMultiply)
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        DspPreMulS18State::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        execute_pre_mul(state, circuit, output);
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        clock_pre_mul(state, circuit, input, false);
    }

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            PreAddMulTemplate {
                module_name: &module_name,
            }
            .render()
            .expect("DSP pre-add multiply Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(pre_mul_test::<Self>(false).verilog_testbench())
    }
}

#[derive(Template)]
#[template(path = "components/dsp/pre_sub_mul_s18.v", escape = "none")]
struct PreSubMulTemplate<'a> {
    module_name: &'a str,
}

/// Registered `(a - b) * c`; the pre-subtract wraps to signed 18 bits before
/// the multiply, matching the physical pre-adder data path.
#[derive(Hardware)]
#[hardware(namespace = "components/arithmetic/dsp", target_leaf)]
pub struct DspPreSubMulS18;

impl Module for DspPreSubMulS18 {
    type Input = DspPreMulS18Input;
    type Output = DspPreMulS18Output;
    type EmuState = DspPreMulS18State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        dsp_resource(DspShape::PreSubtractMultiply)
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        DspPreMulS18State::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        execute_pre_mul(state, circuit, output);
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        clock_pre_mul(state, circuit, input, true);
    }

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            PreSubMulTemplate {
                module_name: &module_name,
            }
            .render()
            .expect("DSP pre-subtract multiply Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(pre_mul_test::<Self>(true).verilog_testbench())
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

fn mul_pair_test<M>(subtract: bool) -> ModuleTest<M>
where
    M: Module<Input = DspMulPairS18Input, Output = DspMulPairS18Output>,
{
    ModuleTest::new([
        TestStep::new(
            DspMulPairS18InputValue {
                a: encoded(-3, INPUT_WIDTH),
                b: encoded(7, INPUT_WIDTH),
                c: encoded(5, INPUT_WIDTH),
                d: encoded(-9, INPUT_WIDTH),
            },
            DspMulPairS18OutputValue { result: 0 },
        ),
        TestStep::new(
            DspMulPairS18InputValue {
                a: encoded(131_071, INPUT_WIDTH),
                b: encoded(131_071, INPUT_WIDTH),
                c: encoded(-131_072, INPUT_WIDTH),
                d: encoded(-131_072, INPUT_WIDTH),
            },
            DspMulPairS18OutputValue {
                result: encoded(if subtract { 24 } else { -66 }, ACCUMULATOR_WIDTH),
            },
        ),
        TestStep::new(
            DspMulPairS18InputValue {
                a: 0,
                b: 0,
                c: 0,
                d: 0,
            },
            DspMulPairS18OutputValue {
                result: encoded(
                    if subtract { -262_143 } else { 34_359_476_225 },
                    ACCUMULATOR_WIDTH,
                ),
            },
        ),
    ])
}

fn pre_mul_test<M>(subtract: bool) -> ModuleTest<M>
where
    M: Module<Input = DspPreMulS18Input, Output = DspPreMulS18Output>,
{
    ModuleTest::new([
        TestStep::new(
            DspPreMulS18InputValue {
                a: encoded(10, INPUT_WIDTH),
                b: encoded(-3, INPUT_WIDTH),
                c: encoded(-8, INPUT_WIDTH),
            },
            DspPreMulS18OutputValue { product: 0 },
        ),
        TestStep::new(
            DspPreMulS18InputValue {
                a: encoded(-131_072, INPUT_WIDTH),
                b: encoded(1, INPUT_WIDTH),
                c: encoded(-1, INPUT_WIDTH),
            },
            DspPreMulS18OutputValue {
                product: encoded(if subtract { -104 } else { -56 }, PRODUCT_WIDTH),
            },
        ),
        TestStep::new(
            DspPreMulS18InputValue { a: 0, b: 0, c: 0 },
            DspPreMulS18OutputValue {
                product: encoded(if subtract { -131_071 } else { 131_071 }, PRODUCT_WIDTH),
            },
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
        mul_pair_test::<DspMulSumS18>(false).run_emu();
        mul_pair_test::<DspMulDifferenceS18>(true).run_emu();
        pre_mul_test::<DspPreAddMulS18>(false).run_emu();
        pre_mul_test::<DspPreSubMulS18>(true).run_emu();
    }

    #[test]
    fn measured_shapes_claim_their_pnr_multiplier_lane_cost() {
        for (resources, lanes) in [
            (DspMulS18::target_resources(), 1),
            (DspMulAddS18::target_resources(), 2),
            (DspMacS18::target_resources(), 2),
            (DspMulSumS18::target_resources(), 2),
            (DspMulDifferenceS18::target_resources(), 2),
            (DspPreAddMulS18::target_resources(), 1),
            (DspPreSubMulS18::target_resources(), 1),
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
        verify_verilog_with_iverilog::<DspMulSumS18>().unwrap();
        verify_verilog_with_iverilog::<DspMulDifferenceS18>().unwrap();
        verify_verilog_with_iverilog::<DspPreAddMulS18>().unwrap();
        verify_verilog_with_iverilog::<DspPreSubMulS18>().unwrap();
    }
}
