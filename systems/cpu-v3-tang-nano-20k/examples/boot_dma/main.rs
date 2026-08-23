use cpu_v3_tang_nano_20k::TangNano20KBootDma;
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_gowin::{
    run_gowin_project_cli, GowinCliError, TangNano20K, TangNano20KBootInputs,
    TangNano20KBootOutputs,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        TangNano20K::boot_memory_project::<BootDmaSelfTest>("boot_dma_self_test"),
        "target/boot_dma_gowin",
    )
}

#[derive(Hardware)]
#[hardware(namespace = "examples/boot_dma")]
struct BootDmaSelfTest;

impl Module for BootDmaSelfTest {
    type Input = TangNano20KBootInputs;
    type Output = TangNano20KBootOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("BootDmaSelfTest is a Verilog-only board test")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").replace(
            "__TANG_NANO_20K_BOOT_DMA__",
            &TangNano20KBootDma::verilog_identity().module_name(),
        ))
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<TangNano20KBootDma>("u_dma")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn project_claims_each_fitted_memory_once() {
        let verilog = VerilogProject::generate::<BootDmaSelfTest>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 1);
        let project = TangNano20K::boot_memory_project::<BootDmaSelfTest>("test")
            .generate()
            .unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::SpiFlashDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
    }
}
