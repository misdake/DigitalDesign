use crate::export::{ExportModuleInterface, Exporter};
use crate::ExportGateReg;

pub struct VerilogModuleExporter {}

impl Exporter for VerilogModuleExporter {
    fn exporter_name() -> &'static str {
        "VerilogModule"
    }

    fn export(&self, interface: &ExportModuleInterface, content: &ExportGateReg) -> String {
        let exporter_name = Self::exporter_name();
        let module_name = interface.module_name;

        // io

        let inputs = interface
            .input_wires
            .iter()
            .map(|(name, _)| format!("    input {name},"))
            .collect::<Vec<_>>()
            .join("\n");
        let outputs = interface
            .output_wires
            .iter()
            .map(|(name, _)| format!("    output {name},"))
            .collect::<Vec<_>>()
            .join("\n");

        // wire/reg declaration

        let wires01 = format!(
            "wire w0 = 1'b{};\nwire w1 = 1'b{};",
            content.wire_0_value, content.wire_1_value
        );
        let wires = (2..content.wire_count)
            .map(|i| format!("wire w{i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let regs_declare = content
            .regs
            .iter()
            .enumerate()
            .map(|(index, _)| format!("reg r{index};"))
            .collect::<Vec<_>>()
            .join("\n");

        // input

        let input_assign = interface
            .input_wires
            .iter()
            .map(|(name, wire)| format!("assign w{} = {name};", wire.0))
            .collect::<Vec<_>>()
            .join("\n");

        let regs_read = content
            .regs
            .iter()
            .enumerate()
            .map(|(index, reg)| format!("    w{} = r{index};", reg.wire_out_index))
            .collect::<Vec<_>>()
            .join("\n");

        // logic

        // assign XXX = !(XXX & XXX);
        let gates = content
            .gates
            .iter()
            .map(|gate| {
                format!(
                    "assign w{} = !(w{} | w{});",
                    gate.wire_out_index, gate.wire_a_index, gate.wire_b_index
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // output

        let regs_write = content
            .regs
            .iter()
            .enumerate()
            .map(|(index, reg)| format!("    r{index} = w{};", reg.wire_in_index))
            .collect::<Vec<_>>()
            .join("\n");

        let output_assign = interface
            .output_wires
            .iter()
            .map(|(name, wire)| format!("assign {name} = w{};", wire.0))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "// exported from {exporter_name}
module {module_name}(
{inputs}
{outputs}
    input clk);

// wires
{wires01}
{wires}
// regs
{regs_declare}

// inputs
{input_assign}
// gates
{gates}

always @(posedge clk) begin
    // regs read
{regs_read}
    // regs write
{regs_write}
end

// outputs
{output_assign}

endmodule
"
        )
    }
}

#[test]
fn test_basic_nand() {
    use crate::{clear_all, export_gate_reg, input};
    clear_all();
    let a = input();
    let b = input();
    let out0 = !a;
    let out1 = !b;
    let out2 = !a & !b;
    let out3 = !a | !b;

    let content = export_gate_reg();
    let mut interface = ExportModuleInterface::default();
    interface
        .module_name("led2")
        .input_wire("Button1", a)
        .input_wire("Button2", b)
        .output_wire("Led0", out0)
        .output_wire("Led1", out1)
        .output_wire("Led2", out2)
        .output_wire("Led3", out3);

    let verilog_output = VerilogModuleExporter {}.export(&interface, &content);
    println!("{verilog_output}");
}

#[test]
fn test_basic_reg() {
    use crate::*;
    clear_all();
    //TODO regs out wires are not supported
    let r = reg_w::<4>();
    let button = input();
    let out = add_naive(r.out, Wires { wires: [button; 1] }.expand_unsigned());
    r.set_in(out.sum);

    let content = export_gate_reg();
    let mut interface = ExportModuleInterface::default();
    interface
        .module_name("led2")
        .input_wire("Button1", button)
        .output_wire("Led0", r.out.wires[0])
        .output_wire("Led1", r.out.wires[1])
        .output_wire("Led2", r.out.wires[2])
        .output_wire("Led3", r.out.wires[3]);

    let verilog_output = VerilogModuleExporter {}.export(&interface, &content);
    println!("{verilog_output}");
}
