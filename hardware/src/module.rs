use digital_design_code::{external, CircuitWires, External, Wire};
use std::any::Any;
use std::fmt::Debug;

#[derive(Clone, Debug)]
pub struct IoBinding {
    pub name: &'static str,
    pub wires: Vec<Wire>,
}

pub trait ModuleIo: Clone + 'static {
    type Value: Clone + Debug + PartialEq + 'static;

    fn allocate() -> Self;
    fn bindings(&self) -> Vec<IoBinding>;
    fn drive(&self, circuit: &mut CircuitWires, value: &Self::Value);
    fn sample(&self, circuit: &CircuitWires) -> Self::Value;
}

/// A hardware module with emulated, NAND, and Verilog construction paths.
///
/// The public construction methods are backend-specific on purpose: callers
/// may mix `emu` and `nand` in a machine model, while `verilog` records a
/// separate hierarchical HDL design.
pub trait Module: Sized + 'static {
    type Input: ModuleIo;
    type Output: ModuleIo;
    type EmuState: 'static;

    const USES_MAIN_CLOCK: bool = false;

    /// Stable Verilog module name. Parameterized modules should override this
    /// and include every parameter that changes the generated circuit.
    fn verilog_name() -> String {
        let rust_name = std::any::type_name::<Self>();
        let base_name = rust_name.split('<').next().unwrap_or(rust_name);
        base_name
            .rsplit("::")
            .next()
            .unwrap_or(base_name)
            .to_string()
    }

    fn create_emu(input: &Self::Input, output: &Self::Output) -> Self::EmuState;

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    );

    fn clock_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
    }

    fn emu(input: &Self::Input) -> Self::Output {
        let output = Self::Output::allocate();
        let state = Self::create_emu(input, &output);
        external(ModuleExternal::<Self> {
            input: input.clone(),
            output: output.clone(),
            state,
        });
        output
    }

    fn nand(_input: &Self::Input) -> Self::Output {
        panic!(
            "NAND implementation is not available for module `{}`",
            std::any::type_name::<Self>()
        )
    }

    /// Build the generated Verilog body. The default converts `nand`.
    /// Hierarchical modules override this and call child `Module::verilog`.
    fn build_verilog(input: &Self::Input) -> Self::Output {
        Self::nand(input)
    }

    /// Return a complete Verilog-2001 module source when hand-written HDL is
    /// available. The source must have the same signature as the Rust IO.
    fn verilog_source() -> Option<&'static str> {
        None
    }

    fn verilog(input: &Self::Input) -> Self::Output {
        crate::project::record_instance::<Self>(input)
    }
}

struct ModuleExternal<M: Module> {
    input: M::Input,
    output: M::Output,
    state: M::EmuState,
}

impl<M: Module> External for ModuleExternal<M> {
    fn execute(&mut self, circuit: &mut CircuitWires) {
        M::execute_emu(&mut self.state, circuit, &self.input, &self.output);
    }

    fn clock(&mut self, circuit: &mut CircuitWires) {
        M::clock_emu(&mut self.state, circuit, &self.input, &self.output);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
