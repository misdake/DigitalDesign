use cpu_v3::{CpuV3Core, CpuV3DirectMappedCache};
use cpu_v3_tang_nano_20k::{DisplaySdramPort, FramebufferHdmi};
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::ResetController;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, GowinCliError, GowinDspMode,
    GowinModuleProject, ResourceCountExpectation, TangNano20K, TangNano20KSdramHdmiInputs,
    TangNano20KSdramHdmiOutputs, BSRAM_1024_DEPTH,
};

include!(concat!(env!("OUT_DIR"), "/display_image.rs"));

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(project(), "target/cpu_v3_display_gowin")
}

struct DisplayBootImage;
const fn boot_words() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut i = 0;
    while i < DISPLAY_DEMO_PROGRAM.len() {
        words[i] = DISPLAY_DEMO_PROGRAM[i] as u64;
        i += 1;
    }
    words
}
impl BsramImage<16> for DisplayBootImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = boot_words();
}
type BootMemory = Bsram1R1Rw1024<16, DisplayBootImage>;
type BoardReset = ResetController<8>;

#[derive(Hardware)]
#[hardware(namespace = "examples/cpu_v3_display")]
struct CpuV3Display;

impl Module for CpuV3Display {
    type Input = TangNano20KSdramHdmiInputs;
    type Output = TangNano20KSdramHdmiOutputs;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("the complete display board is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("system.v")
                .replace(
                    "__BOOT_MEMORY__",
                    &BootMemory::verilog_identity().module_name(),
                )
                .replace("__CPU__", &CpuV3Core::verilog_identity().module_name())
                .replace(
                    "__CACHE__",
                    &CpuV3DirectMappedCache::verilog_identity().module_name(),
                )
                .replace(
                    "__SDRAM__",
                    &DisplaySdramPort::verilog_identity().module_name(),
                )
                .replace(
                    "__DISPLAY__",
                    &FramebufferHdmi::verilog_identity().module_name(),
                )
                .replace("__RESET__", &BoardReset::verilog_identity().module_name()),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<BootMemory>("u_boot"),
            VerilogDependency::new::<CpuV3Core>("u_cpu"),
            VerilogDependency::new::<CpuV3DirectMappedCache>("u_dcache"),
            VerilogDependency::new::<DisplaySdramPort>("u_sdram"),
            VerilogDependency::new::<FramebufferHdmi>("u_display"),
            VerilogDependency::new::<BoardReset>("u_reset"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("system_tb.v").to_string())
    }
}

fn project() -> GowinModuleProject<TangNano20K, CpuV3Display> {
    TangNano20K::sdram_hdmi_debug_uart_project::<CpuV3Display>("cpu_v3_display")
        .expect_bsram_blocks(ResourceCountExpectation::Exact(3))
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Exact(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn combined_project_claims_framebuffer_hardware() {
        let verilog = VerilogProject::generate::<CpuV3Display>().unwrap();
        assert!(!verilog.files.is_empty());
        let project = project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 2);
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::HdmiOutput], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 3);
    }

    #[test]
    fn animation_program_initializes_both_framebuffer_segments() {
        use cpu_v3::{Machine, RunOutcome};
        use digital_design_ip_common::PhysicalWordAddress;

        assert!(DISPLAY_DEMO_PROGRAM.len() <= BSRAM_1024_DEPTH);
        let mut machine = Machine::default();
        machine.load_program(0, DISPLAY_DEMO_PROGRAM).unwrap();
        assert!(matches!(
            machine.run(5_000_000).unwrap(),
            RunOutcome::StepLimit { .. }
        ));
        assert_ne!(
            machine.physical_memory(PhysicalWordAddress::new(0x0020_0100)),
            0
        );
        assert_ne!(
            machine.physical_memory(PhysicalWordAddress::new(0x0021_2cff)),
            0,
            "the final row in the second framebuffer segment was not initialized"
        );
        assert_eq!(
            machine.data_segment(),
            0,
            "DSEG must be restored after every pixel store"
        );
    }

    #[test]
    #[ignore = "explicit full-system Icarus simulation"]
    fn cpu_writes_while_display_reads() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Display>().unwrap();
    }
}
