use crate::{ResourceAmount, TargetComponent};
use digital_design_code::{external, CircuitWires, External, Wire};
use std::any::Any;
use std::fmt::Debug;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum VerilogConstValue {
    Unsigned(u128),
    Signed(i128),
    Bool(bool),
    Symbol(String),
}

impl VerilogConstValue {
    fn suffix(&self) -> String {
        match self {
            Self::Unsigned(value) => value.to_string(),
            Self::Signed(value) if *value < 0 => format!("n{}", value.unsigned_abs()),
            Self::Signed(value) => format!("p{value}"),
            Self::Bool(true) => "b1".to_string(),
            Self::Bool(false) => "b0".to_string(),
            Self::Symbol(value) => format!("s{value}"),
        }
    }
}

macro_rules! unsigned_verilog_const {
    ($($type:ty),* $(,)?) => {
        $(impl From<$type> for VerilogConstValue {
            fn from(value: $type) -> Self {
                Self::Unsigned(value as u128)
            }
        })*
    };
}

macro_rules! signed_verilog_const {
    ($($type:ty),* $(,)?) => {
        $(impl From<$type> for VerilogConstValue {
            fn from(value: $type) -> Self {
                Self::Signed(value as i128)
            }
        })*
    };
}

unsigned_verilog_const!(u8, u16, u32, u64, u128, usize);
signed_verilog_const!(i8, i16, i32, i64, i128, isize);

impl From<bool> for VerilogConstValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VerilogIdentity {
    base_name: String,
    namespace: Vec<String>,
    parameters: Vec<(String, VerilogConstValue)>,
}

impl VerilogIdentity {
    pub fn new(base_name: impl Into<String>) -> Self {
        let base_name = base_name.into();
        assert_identifier(&base_name, "Verilog module base name");
        Self {
            base_name,
            namespace: Vec::new(),
            parameters: Vec::new(),
        }
    }

    pub fn namespace<I, S>(mut self, segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.namespace = segments
            .into_iter()
            .map(|segment| {
                let segment = segment.into();
                assert_identifier(&segment, "Verilog namespace segment");
                to_snake_case(&segment)
            })
            .collect();
        self
    }

    pub fn constant(mut self, name: impl Into<String>, value: impl IntoVerilogConst) -> Self {
        let name = name.into();
        assert_identifier(&name, "Verilog specialization constant name");
        assert!(
            !self
                .parameters
                .iter()
                .any(|(existing, _)| existing == &name),
            "duplicate Verilog specialization constant `{name}`"
        );
        let value = value.into_verilog_const();
        if let VerilogConstValue::Symbol(symbol) = &value {
            assert_identifier(symbol, "Verilog specialization symbol");
        }
        self.parameters.push((name, value));
        self
    }

    pub fn symbol(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.constant_value(name, VerilogConstValue::Symbol(value.into()))
    }

    fn constant_value(mut self, name: impl Into<String>, value: VerilogConstValue) -> Self {
        let name = name.into();
        assert_identifier(&name, "Verilog specialization constant name");
        assert!(
            !self
                .parameters
                .iter()
                .any(|(existing, _)| existing == &name),
            "duplicate Verilog specialization constant `{name}`"
        );
        if let VerilogConstValue::Symbol(symbol) = &value {
            assert_identifier(symbol, "Verilog specialization symbol");
        }
        self.parameters.push((name, value));
        self
    }

    pub fn module_name(&self) -> String {
        let mut name = self.base_name.clone();
        for (parameter, value) in &self.parameters {
            name.push('_');
            name.push_str(parameter);
            name.push_str(&value.suffix());
        }
        name
    }

    pub fn relative_path(&self) -> PathBuf {
        let mut path = self.namespace.iter().collect::<PathBuf>();
        let base = to_snake_case(&self.base_name);
        if self.parameters.is_empty() {
            path.push(format!("{base}.v"));
        } else {
            path.push(base);
            let specialization = self
                .parameters
                .iter()
                .map(|(name, value)| format!("{}{}", name.to_ascii_lowercase(), value.suffix()))
                .collect::<Vec<_>>()
                .join("_");
            path.push(format!("{specialization}.v"));
        }
        path
    }

    pub(crate) fn instance_stem(&self) -> String {
        to_snake_case(&self.base_name)
    }
}

pub trait IntoVerilogConst {
    fn into_verilog_const(self) -> VerilogConstValue;
}

impl<T> IntoVerilogConst for T
where
    VerilogConstValue: From<T>,
{
    fn into_verilog_const(self) -> VerilogConstValue {
        VerilogConstValue::from(self)
    }
}

fn assert_identifier(value: &str, description: &str) {
    let mut characters = value.chars();
    let valid = characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric());
    assert!(valid, "{description} `{value}` is not a valid identifier");
}

fn to_snake_case(value: &str) -> String {
    let mut result = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() && index != 0 {
            result.push('_');
        }
        result.push(character.to_ascii_lowercase());
    }
    result
}

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
    fn verilog_values(value: &Self::Value) -> Vec<VerilogIoValue>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerilogIoValue {
    pub name: &'static str,
    pub width: usize,
    pub value: u64,
}

/// A hardware module with emulated, NAND, and Verilog construction paths.
///
/// The public construction methods are backend-specific on purpose: callers
/// may mix `emu` and `nand` in a machine model, while `verilog` records a
/// separate hierarchical HDL design.
pub trait HardwareIdentity: 'static {
    const TARGET_RESOURCE_LEAF: bool;

    fn verilog_identity() -> VerilogIdentity;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetResourceRequest {
    pub component: &'static str,
    pub resources: Vec<ResourceAmount>,
}

#[derive(Clone, Debug)]
pub struct VerilogVerification {
    pub testbench: String,
    pub verified_hashes: &'static str,
}

impl TargetResourceRequest {
    pub fn new<C: TargetComponent>(component: C) -> Self {
        Self {
            component: component.component_name(),
            resources: component.resource_requirements(),
        }
    }
}

pub trait Module: HardwareIdentity + Sized + 'static {
    type Input: ModuleIo;
    type Output: ModuleIo;
    type EmuState: 'static;

    const USES_MAIN_CLOCK: bool = false;

    /// Resources used by this module itself, excluding child modules.
    ///
    /// Only modules derived with `#[hardware(..., target_leaf)]` may override
    /// this, and they must not instantiate child modules. Hierarchical modules
    /// acquire resources exclusively by instantiating those leaves.
    fn target_resources() -> Vec<TargetResourceRequest> {
        Vec::new()
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
    fn verilog_source() -> Option<String> {
        None
    }

    /// Explicit simulation recipe and previously successful source hash for
    /// a hand-written Verilog implementation. Ordinary Rust tests do not run
    /// the external simulator; export only checks this stored attestation.
    fn verilog_verification() -> Option<VerilogVerification> {
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
