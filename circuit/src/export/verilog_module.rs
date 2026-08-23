use crate::{ExportGateReg, Wire};
use std::collections::HashSet;
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerilogPort {
    pub name: String,
    pub wires: Vec<Wire>,
}

impl VerilogPort {
    pub fn scalar(name: impl Into<String>, wire: Wire) -> Self {
        Self {
            name: name.into(),
            wires: vec![wire],
        }
    }

    pub fn bus(name: impl Into<String>, wires: impl IntoIterator<Item = Wire>) -> Self {
        Self {
            name: name.into(),
            wires: wires.into_iter().collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerilogConnection {
    Wires(Vec<Wire>),
    Signal(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerilogInstance {
    pub module_name: String,
    pub instance_name: String,
    pub connections: Vec<(String, VerilogConnection)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerilogModule {
    pub module_name: String,
    pub clock: Option<String>,
    pub inputs: Vec<VerilogPort>,
    pub outputs: Vec<VerilogPort>,
    pub instances: Vec<VerilogInstance>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerilogRenderError {
    EmptyPort { name: String },
    RegisterWithoutClock,
    InvalidIdentifier { identifier: String },
    DuplicateIdentifier { identifier: String },
    WireOutOfRange { wire: usize, wire_count: usize },
    InvalidConstantValue { value: u8 },
}

impl Display for VerilogRenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPort { name } => write!(formatter, "Verilog port `{name}` has zero width"),
            Self::RegisterWithoutClock => {
                formatter.write_str("a module containing registers requires a clock")
            }
            Self::InvalidIdentifier { identifier } => {
                write!(
                    formatter,
                    "`{identifier}` is not a supported Verilog identifier"
                )
            }
            Self::DuplicateIdentifier { identifier } => {
                write!(formatter, "duplicate Verilog identifier `{identifier}`")
            }
            Self::WireOutOfRange { wire, wire_count } => write!(
                formatter,
                "wire w{wire} is outside the module wire range 0..{wire_count}"
            ),
            Self::InvalidConstantValue { value } => {
                write!(
                    formatter,
                    "wire constant must be zero or one, found {value}"
                )
            }
        }
    }
}

impl std::error::Error for VerilogRenderError {}

pub fn validate_verilog_identifier(identifier: &str) -> Result<(), VerilogRenderError> {
    let mut chars = identifier.chars();
    let valid_first = chars
        .next()
        .is_some_and(|value| value == '_' || value.is_ascii_alphabetic());
    if !valid_first
        || !chars.all(|value| value == '_' || value.is_ascii_alphanumeric())
        || is_verilog_keyword(identifier)
    {
        return Err(VerilogRenderError::InvalidIdentifier {
            identifier: identifier.to_string(),
        });
    }
    Ok(())
}

fn is_verilog_keyword(identifier: &str) -> bool {
    matches!(
        identifier,
        "always"
            | "and"
            | "assign"
            | "automatic"
            | "begin"
            | "buf"
            | "bufif0"
            | "bufif1"
            | "case"
            | "casex"
            | "casez"
            | "cell"
            | "cmos"
            | "config"
            | "deassign"
            | "default"
            | "defparam"
            | "design"
            | "disable"
            | "edge"
            | "else"
            | "end"
            | "endcase"
            | "endconfig"
            | "endfunction"
            | "endgenerate"
            | "endmodule"
            | "endprimitive"
            | "endspecify"
            | "endtable"
            | "endtask"
            | "event"
            | "for"
            | "force"
            | "forever"
            | "fork"
            | "function"
            | "generate"
            | "genvar"
            | "highz0"
            | "highz1"
            | "if"
            | "ifnone"
            | "incdir"
            | "include"
            | "initial"
            | "inout"
            | "input"
            | "instance"
            | "integer"
            | "join"
            | "large"
            | "liblist"
            | "library"
            | "localparam"
            | "macromodule"
            | "medium"
            | "module"
            | "nand"
            | "negedge"
            | "nmos"
            | "nor"
            | "noshowcancelled"
            | "not"
            | "notif0"
            | "notif1"
            | "or"
            | "output"
            | "parameter"
            | "pmos"
            | "posedge"
            | "primitive"
            | "pull0"
            | "pull1"
            | "pulldown"
            | "pullup"
            | "pulsestyle_onevent"
            | "pulsestyle_ondetect"
            | "rcmos"
            | "real"
            | "realtime"
            | "reg"
            | "release"
            | "repeat"
            | "rnmos"
            | "rpmos"
            | "rtran"
            | "rtranif0"
            | "rtranif1"
            | "scalared"
            | "showcancelled"
            | "signed"
            | "small"
            | "specify"
            | "specparam"
            | "strong0"
            | "strong1"
            | "supply0"
            | "supply1"
            | "table"
            | "task"
            | "time"
            | "tran"
            | "tranif0"
            | "tranif1"
            | "tri"
            | "tri0"
            | "tri1"
            | "triand"
            | "trior"
            | "trireg"
            | "unsigned"
            | "use"
            | "vectored"
            | "wait"
            | "wand"
            | "weak0"
            | "weak1"
            | "while"
            | "wire"
            | "wor"
            | "xnor"
            | "xor"
    )
}

fn insert_unique(
    identifiers: &mut HashSet<String>,
    identifier: &str,
) -> Result<(), VerilogRenderError> {
    validate_verilog_identifier(identifier)?;
    if !identifiers.insert(identifier.to_string()) {
        return Err(VerilogRenderError::DuplicateIdentifier {
            identifier: identifier.to_string(),
        });
    }
    Ok(())
}

fn validate_wire(wire: Wire, wire_count: usize) -> Result<(), VerilogRenderError> {
    if wire.0 >= wire_count {
        return Err(VerilogRenderError::WireOutOfRange {
            wire: wire.0,
            wire_count,
        });
    }
    Ok(())
}

fn declaration(direction: &str, port: &VerilogPort) -> Result<String, VerilogRenderError> {
    validate_verilog_identifier(&port.name)?;
    match port.wires.len() {
        0 => Err(VerilogRenderError::EmptyPort {
            name: port.name.clone(),
        }),
        1 => Ok(format!("    {direction} wire {}", port.name)),
        width => Ok(format!(
            "    {direction} wire [{}:0] {}",
            width - 1,
            port.name
        )),
    }
}

fn port_bit(name: &str, width: usize, bit: usize) -> String {
    if width == 1 {
        name.to_string()
    } else {
        format!("{name}[{bit}]")
    }
}

fn connection_value(connection: &VerilogConnection) -> String {
    match connection {
        VerilogConnection::Signal(signal) => signal.clone(),
        VerilogConnection::Wires(wires) if wires.len() == 1 => format!("w{}", wires[0].0),
        VerilogConnection::Wires(wires) => wires
            .iter()
            .rev()
            .map(|wire| format!("w{}", wire.0))
            .collect::<Vec<_>>()
            .join(", ")
            .pipe(|bits| format!("{{{bits}}}")),
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, function: impl FnOnce(Self) -> T) -> T {
        function(self)
    }
}

impl<T> Pipe for T {}

/// Render an isolated NAND/register netlist as a Verilog-2001 module.
///
/// `Wire` arrays are little-endian: the first wire is emitted as bit zero.
pub fn render_verilog_module(
    module: &VerilogModule,
    content: &ExportGateReg,
) -> Result<String, VerilogRenderError> {
    validate_verilog_identifier(&module.module_name)?;
    if content.wire_count < 2 {
        return Err(VerilogRenderError::WireOutOfRange {
            wire: 1,
            wire_count: content.wire_count,
        });
    }
    for value in [content.wire_0_value, content.wire_1_value] {
        if value > 1 {
            return Err(VerilogRenderError::InvalidConstantValue { value });
        }
    }
    if !content.regs.is_empty() && module.clock.is_none() {
        return Err(VerilogRenderError::RegisterWithoutClock);
    }

    let mut declarations = Vec::new();
    let mut port_names = HashSet::new();
    for port in &module.inputs {
        insert_unique(&mut port_names, &port.name)?;
        for &wire in &port.wires {
            validate_wire(wire, content.wire_count)?;
        }
        declarations.push(declaration("input", port)?);
    }
    for port in &module.outputs {
        insert_unique(&mut port_names, &port.name)?;
        for &wire in &port.wires {
            validate_wire(wire, content.wire_count)?;
        }
        declarations.push(declaration("output", port)?);
    }
    if let Some(clock) = &module.clock {
        insert_unique(&mut port_names, clock)?;
        declarations.push(format!("    input wire {clock}"));
    }

    let mut instance_names = HashSet::new();
    for instance in &module.instances {
        validate_verilog_identifier(&instance.module_name)?;
        insert_unique(&mut instance_names, &instance.instance_name)?;
        let mut connection_names = HashSet::new();
        for (name, connection) in &instance.connections {
            insert_unique(&mut connection_names, name)?;
            if let VerilogConnection::Wires(wires) = connection {
                for &wire in wires {
                    validate_wire(wire, content.wire_count)?;
                }
            }
        }
    }

    for gate in &content.gates {
        for wire in [gate.wire_a_index, gate.wire_b_index, gate.wire_out_index] {
            validate_wire(Wire(wire), content.wire_count)?;
        }
    }
    for reg in &content.regs {
        for wire in [reg.wire_in_index, reg.wire_out_index] {
            validate_wire(Wire(wire), content.wire_count)?;
        }
    }

    let mut output = String::new();
    output.push_str("// Generated by digital-design-code.\n");
    output.push_str(&format!("module {}(\n", module.module_name));
    output.push_str(&declarations.join(",\n"));
    output.push_str("\n);\n\n");

    for wire in 0..content.wire_count {
        output.push_str(&format!("wire w{wire};\n"));
    }
    output.push_str(&format!("assign w0 = 1'b{};\n", content.wire_0_value));
    output.push_str(&format!("assign w1 = 1'b{};\n", content.wire_1_value));

    for (index, _) in content.regs.iter().enumerate() {
        output.push_str(&format!("reg r{index} = 1'b0;\n"));
    }
    output.push('\n');

    for port in &module.inputs {
        for (bit, wire) in port.wires.iter().enumerate() {
            output.push_str(&format!(
                "assign w{} = {};\n",
                wire.0,
                port_bit(&port.name, port.wires.len(), bit)
            ));
        }
    }
    for (index, reg) in content.regs.iter().enumerate() {
        output.push_str(&format!("assign w{} = r{index};\n", reg.wire_out_index));
    }
    if !module.inputs.is_empty() || !content.regs.is_empty() {
        output.push('\n');
    }

    for gate in &content.gates {
        output.push_str(&format!(
            "assign w{} = ~(w{} & w{});\n",
            gate.wire_out_index, gate.wire_a_index, gate.wire_b_index
        ));
    }
    if !content.gates.is_empty() {
        output.push('\n');
    }

    for instance in &module.instances {
        output.push_str(&format!(
            "{} {} (\n",
            instance.module_name, instance.instance_name
        ));
        let connections = instance
            .connections
            .iter()
            .map(|(name, connection)| format!("    .{name}({})", connection_value(connection)))
            .collect::<Vec<_>>()
            .join(",\n");
        output.push_str(&connections);
        output.push_str("\n);\n\n");
    }

    if !content.regs.is_empty() {
        let clock = module.clock.as_ref().unwrap();
        output.push_str(&format!("always @(posedge {clock}) begin\n"));
        for (index, reg) in content.regs.iter().enumerate() {
            output.push_str(&format!("    r{index} <= w{};\n", reg.wire_in_index));
        }
        output.push_str("end\n\n");
    }

    for port in &module.outputs {
        for (bit, wire) in port.wires.iter().enumerate() {
            output.push_str(&format!(
                "assign {} = w{};\n",
                port_bit(&port.name, port.wires.len(), bit),
                wire.0
            ));
        }
    }
    output.push_str("\nendmodule\n");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_circuit, input, input_w, reg, Wires};

    #[test]
    fn renders_packed_ports_and_registers() {
        let (circuit, (input, output)) = build_circuit(|| {
            let input = input_w::<2>();
            let state = reg();
            state.set_in(input.wires[0] & input.wires[1]);
            (
                input,
                Wires {
                    wires: [state.out()],
                },
            )
        });
        let module = VerilogModule {
            module_name: "PackedRegister".to_string(),
            clock: Some("clk".to_string()),
            inputs: vec![VerilogPort::bus("value", input.wires)],
            outputs: vec![VerilogPort::bus("result", output.wires)],
            instances: Vec::new(),
        };

        let rendered = render_verilog_module(&module, &circuit.export_gate_reg()).unwrap();
        assert!(rendered.contains("input wire [1:0] value"));
        assert!(rendered.contains("always @(posedge clk)"));
        assert!(rendered.contains("assign result = w"));
    }

    #[test]
    fn registers_require_a_clock() {
        let (circuit, output) = build_circuit(|| {
            let state = reg();
            state.set_in(input());
            state.out()
        });
        let module = VerilogModule {
            module_name: "NoClock".to_string(),
            clock: None,
            inputs: Vec::new(),
            outputs: vec![VerilogPort::scalar("output_value", output)],
            instances: Vec::new(),
        };
        assert_eq!(
            render_verilog_module(&module, &circuit.export_gate_reg()),
            Err(VerilogRenderError::RegisterWithoutClock)
        );
    }

    #[test]
    fn rejects_duplicate_ports_and_keywords() {
        let (circuit, wire) = build_circuit(input);
        let duplicate = VerilogModule {
            module_name: "Duplicate".to_string(),
            clock: Some("clk".to_string()),
            inputs: vec![VerilogPort::scalar("clk", wire)],
            outputs: Vec::new(),
            instances: Vec::new(),
        };
        assert_eq!(
            render_verilog_module(&duplicate, &circuit.export_gate_reg()),
            Err(VerilogRenderError::DuplicateIdentifier {
                identifier: "clk".to_string()
            })
        );

        let keyword = VerilogModule {
            module_name: "module".to_string(),
            clock: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            instances: Vec::new(),
        };
        assert!(matches!(
            render_verilog_module(&keyword, &circuit.export_gate_reg()),
            Err(VerilogRenderError::InvalidIdentifier { .. })
        ));
    }

    #[test]
    fn rejects_wires_outside_the_netlist() {
        let (circuit, _) = build_circuit(input);
        let module = VerilogModule {
            module_name: "BadWire".to_string(),
            clock: None,
            inputs: vec![VerilogPort::scalar("value", Wire(999))],
            outputs: Vec::new(),
            instances: Vec::new(),
        };
        assert!(matches!(
            render_verilog_module(&module, &circuit.export_gate_reg()),
            Err(VerilogRenderError::WireOutOfRange { wire: 999, .. })
        ));
    }
}
