mod verilog_module;

use crate::{ExportGateReg, Wire, Wires};

#[derive(Default)]
pub struct ExportModuleInterface {
    module_name: &'static str,
    input_wires: Vec<(String, Wire)>,
    output_wires: Vec<(String, Wire)>,
}
impl ExportModuleInterface {
    pub fn module_name(&mut self, module_name: &'static str) -> &mut Self {
        self.module_name = module_name;
        self
    }
    pub fn input_wire(&mut self, name: &'static str, wire: Wire) -> &mut Self {
        self.input_wires.push((name.to_string(), wire));
        self
    }
    pub fn input_wires<const T: usize>(
        &mut self,
        name: &'static str,
        wires: Wires<T>,
    ) -> &mut Self {
        for i in 0..T {
            self.input_wires
                .push((format!("{name}_{i}"), wires.wires[i]));
        }
        self
    }
    pub fn output_wire(&mut self, name: &'static str, wire: Wire) -> &mut Self {
        self.output_wires.push((name.to_string(), wire));
        self
    }
    pub fn output_wires<const T: usize>(
        &mut self,
        name: &'static str,
        wires: Wires<T>,
    ) -> &mut Self {
        for i in 0..T {
            self.output_wires
                .push((format!("{name}_{i}"), wires.wires[i]));
        }
        self
    }
}

trait Exporter {
    fn exporter_name() -> &'static str;
    fn export(&self, interface: &ExportModuleInterface, content: &ExportGateReg) -> String;
}
