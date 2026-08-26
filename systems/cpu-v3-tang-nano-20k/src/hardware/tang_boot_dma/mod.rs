//! Tang Nano 20K wiring for the reusable boot DMA protocol engine.

use crate::{
    BootDmaEngine, ErasedSpiFlashImage, HardwareIdentity, Module, ModuleIo, SpiFlashReader,
    TargetResourceRequest, VerilogDependency, VerilogIdentity,
};
use digital_design_circuit::{CircuitWires, Wire, Wires};

type FittedFlashReader = SpiFlashReader<ErasedSpiFlashImage, 8_388_608, 2>;

#[derive(Clone, ModuleIo)]
pub struct TangNano20KBootDmaInput {
    pub reset: Wire,
    pub start: Wire,
    pub flash_offset: Wires<24>,
    pub destination: Wires<22>,
    pub file_size_bytes: Wires<32>,
    pub memory_size_bytes: Wires<32>,
    pub flash_miso: Wire,
    pub sdram_read_data: Wires<32>,
    pub sdram_read_valid: Wire,
    pub sdram_init_done: Wire,
    pub sdram_command_ack: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct TangNano20KBootDmaOutput {
    pub busy: Wire,
    pub done: Wire,
    pub error: Wire,
    pub error_code: Wires<8>,
    pub completed_words: Wires<32>,
    pub flash_clk: Wire,
    pub flash_cs_n: Wire,
    pub flash_mosi: Wire,
    pub sdram_command_valid: Wire,
    pub sdram_command: Wires<3>,
    pub sdram_precharge: Wire,
    pub sdram_address: Wires<21>,
    pub sdram_write_mask: Wires<4>,
    pub sdram_write_data: Wires<32>,
    pub sdram_burst_length: Wires<8>,
}

/// Target-specific structural wrapper around the reusable DMA engine.
///
/// The fitted Flash reader claims the complete Flash device. SDRAM and its PLL
/// remain claimed by `TangNano20K::boot_memory_project` at the board boundary.
pub struct TangNano20KBootDma;

impl HardwareIdentity for TangNano20KBootDma {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("TangNano20KBootDma").namespace(["target", "tang_nano_20k", "boot"])
    }
}

impl Module for TangNano20KBootDma {
    type Input = TangNano20KBootDmaInput;
    type Output = TangNano20KBootDmaOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        Vec::new()
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("TangNano20KBootDma is a target-specific structural wrapper")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("boot_dma.v")
                .replace(
                    "__BOOT_DMA_ENGINE__",
                    &BootDmaEngine::verilog_identity().module_name(),
                )
                .replace(
                    "__FLASH_READER__",
                    &FittedFlashReader::verilog_identity().module_name(),
                )
                .replace(
                    "__SDRAM_WORD_PORT__",
                    &crate::TangNano20KSdramWordPort::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<BootDmaEngine>("u_engine"),
            VerilogDependency::new::<FittedFlashReader>("u_flash"),
            VerilogDependency::new::<crate::TangNano20KSdramWordPort>("u_memory"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("boot_dma_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceAmount, ResourceKind, VerilogProject};

    #[test]
    fn wrapper_claims_only_the_fitted_flash_leaf() {
        let verilog = VerilogProject::generate::<TangNano20KBootDma>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 1);
        assert_eq!(
            verilog.resource_claims[0].resources,
            [ResourceAmount::new(ResourceKind::SpiFlashDevice, 1)]
        );
    }
}
