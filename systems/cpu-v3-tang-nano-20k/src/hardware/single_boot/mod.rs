//! Fixed cold-start loader. Runtime reset and programmable DMA are absent.
use crate::{Hardware, Module, ModuleIo};
use digital_design_circuit::{CircuitWires, Wire, Wires};

#[derive(Clone, ModuleIo)]
pub struct SingleBootLoaderInput {
    pub ready_to_start: Wire,
    pub flash_ready: Wire,
    pub flash_data_valid: Wire,
    pub flash_data: Wires<8>,
    pub flash_done: Wire,
    pub flash_error: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_error: Wire,
    pub display_started: Wire,
    pub display_error: Wire,
    pub cache_ready: Wire,
}
#[derive(Clone, ModuleIo)]
pub struct SingleBootLoaderOutput {
    pub flash_start: Wire,
    pub flash_address: Wires<24>,
    pub flash_length: Wires<24>,
    pub flash_data_ready: Wire,
    pub memory_request_valid: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<16>,
    pub memory_response_ready: Wire,
    pub load_done: Wire,
    pub done: Wire,
    pub error: Wire,
    pub error_code: Wires<4>,
}
#[derive(Hardware)]
#[hardware(namespace = "components/boot")]
pub struct SingleBootLoader;
impl Module for SingleBootLoader {
    type Input = SingleBootLoaderInput;
    type Output = SingleBootLoaderOutput;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
        panic!("SingleBootLoader uses its independent functional oracle and RTL regression")
    }
    fn verilog_source() -> Option<String> {
        Some(include_str!("single_boot.v").to_string())
    }
    fn verilog_testbench() -> Option<String> {
        Some(include_str!("single_boot_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "bounded cold-start loader success, limits, final-write failures and timeout"]
    fn single_boot_contract_in_iverilog() {
        use std::{path::PathBuf, process::Command};
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/hardware/single_boot");
        let evidence = std::env::var_os("SINGLE_BOOT_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("single-boot-{}", std::process::id()))
            });
        std::fs::create_dir_all(&evidence).unwrap();
        for case in 0..28 {
            let executable = evidence.join(format!("case-{case}.vvp"));
            let compile =
                Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()))
                    .args(["-g2012", "-s", "tb", &format!("-Ptb.CASE={case}"), "-o"])
                    .arg(&executable)
                    .arg(source.join("single_boot.v"))
                    .arg(source.join("single_boot_tb.v"))
                    .output()
                    .unwrap();
            assert!(
                compile.status.success(),
                "{}",
                String::from_utf8_lossy(&compile.stderr)
            );
            let run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
                .arg(&executable)
                .output()
                .unwrap();
            std::fs::write(evidence.join(format!("case-{case}.log")), &run.stdout).unwrap();
            assert!(
                run.status.success()
                    && String::from_utf8_lossy(&run.stdout).contains("PASS single boot"),
                "case {case}: {} {}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            );
        }
    }
}
