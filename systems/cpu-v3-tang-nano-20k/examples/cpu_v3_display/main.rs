use cpu_v3::{CpuV3CacheImage, CpuV3Core, CpuV3DirectMappedCache, CpuV3DirectMappedCacheWithImage};
use cpu_v3_tang_nano_20k::{CpuV3MemoryArbiter, DisplaySdramPort, FramebufferHdmi};
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::ResetController;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, BsramImage, GowinCliError, GowinDspMode, GowinModuleProject,
    ResourceCountExpectation, TangNano20K, TangNano20KSdramHdmiInputs, TangNano20KSdramHdmiOutputs,
    BSRAM_1024_DEPTH,
};

include!(concat!(env!("OUT_DIR"), "/display_image.rs"));

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(project(), "target/cpu_v3_display_gowin")
}

struct DisplayInstructionImage;
const fn boot_words() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut i = 0;
    while i < DISPLAY_DEMO_PROGRAM.len() {
        words[i] = DISPLAY_DEMO_PROGRAM[i] as u64;
        i += 1;
    }
    words
}
impl BsramImage<16> for DisplayInstructionImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = boot_words();
}
impl CpuV3CacheImage for DisplayInstructionImage {
    const INITIAL_VALID: u64 = {
        let lines = DISPLAY_DEMO_PROGRAM.len().div_ceil(16);
        if lines >= 64 {
            u64::MAX
        } else {
            (1u64 << lines) - 1
        }
    };
}
type InstructionCache = CpuV3DirectMappedCacheWithImage<DisplayInstructionImage>;
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
                .replace("__CPU__", &CpuV3Core::verilog_identity().module_name())
                .replace(
                    "__ICACHE__",
                    &InstructionCache::verilog_identity().module_name(),
                )
                .replace(
                    "__DCACHE__",
                    &CpuV3DirectMappedCache::verilog_identity().module_name(),
                )
                .replace(
                    "__ARBITER__",
                    &CpuV3MemoryArbiter::verilog_identity().module_name(),
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
            VerilogDependency::new::<CpuV3Core>("u_cpu"),
            VerilogDependency::new::<InstructionCache>("u_icache"),
            VerilogDependency::new::<CpuV3DirectMappedCache>("u_dcache"),
            VerilogDependency::new::<CpuV3MemoryArbiter>("u_memory_arbiter"),
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
        .expect_bsram_blocks(ResourceCountExpectation::Exact(4))
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Exact(2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v3_tang_nano_20k::{
        framebuffer_word, framebuffer_word_at, DisplayDevice, DISPLAY_DEVICE,
        FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD,
    };
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn combined_project_claims_framebuffer_hardware() {
        let verilog = VerilogProject::generate::<CpuV3Display>().unwrap();
        assert!(!verilog.files.is_empty());
        let project = project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 2);
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::HdmiOutput], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 4);
    }

    #[test]
    fn animation_program_initializes_both_framebuffers() {
        use cpu_v3::{Machine, RunOutcome};
        use digital_design_ip_common::PhysicalWordAddress;

        assert!(DISPLAY_DEMO_PROGRAM.len() <= BSRAM_1024_DEPTH);
        let mut machine = Machine::default();
        machine.load_program(0, DISPLAY_DEMO_PROGRAM).unwrap();
        assert!(matches!(
            machine.run(12_000_000).unwrap(),
            RunOutcome::StepLimit { .. }
        ));
        assert_ne!(
            machine.physical_memory(PhysicalWordAddress::new(0x0020_0100)),
            0
        );
        assert_ne!(
            machine.physical_memory(PhysicalWordAddress::new(framebuffer_word(319, 239))),
            0,
            "the final row in framebuffer A was not initialized"
        );
        assert_ne!(
            machine.physical_memory(PhysicalWordAddress::new(framebuffer_word_at(
                FRAMEBUFFER_B_BASE_WORD,
                319,
                239
            ))),
            0,
            "the final row in framebuffer B was not initialized"
        );
        assert_eq!(
            machine.data_segment(),
            0,
            "DSEG must be restored after every pixel store"
        );
    }

    #[test]
    fn framebuffer_is_completely_written() {
        use cpu_v3::{Machine, RunOutcome};
        use digital_design_ip_common::PhysicalWordAddress;

        let mut machine = Machine::default();
        machine.load_program(0, DISPLAY_DEMO_PROGRAM).unwrap();
        assert!(matches!(
            machine.run(12_000_000).unwrap(),
            RunOutcome::StepLimit { .. }
        ));
        let mut unwritten = Vec::new();
        let mut unwritten_count = 0usize;
        for base in [FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD] {
            for y in 0..240u32 {
                for x in 0..320u32 {
                    let address = framebuffer_word_at(base, x, y);
                    if machine.physical_memory(PhysicalWordAddress::new(address)) == 0 {
                        unwritten_count += 1;
                        if unwritten.len() < 16 {
                            unwritten.push((base, y, x));
                        }
                    }
                }
            }
        }
        assert!(
            unwritten_count == 0,
            "{unwritten_count} unwritten framebuffer pixels; first: {unwritten:?}"
        );
    }

    #[test]
    fn animation_publishes_alternating_back_buffers_on_host_vblank() {
        use cpu_v3::{Machine, RunOutcome};

        let mut machine = Machine::default();
        machine.load_program(0, DISPLAY_DEMO_PROGRAM).unwrap();
        machine.attach_device(DISPLAY_DEVICE, Box::<DisplayDevice>::default());
        assert!(matches!(
            machine.run(12_000_000).unwrap(),
            RunOutcome::StepLimit { .. }
        ));
        let display = machine.device::<DisplayDevice>(DISPLAY_DEVICE).unwrap();
        assert!(display.swap_pending());
        display.advance_frame();
        assert_eq!(display.active_base(), FRAMEBUFFER_B_BASE_WORD);

        assert!(matches!(
            machine.run(1_000_000).unwrap(),
            RunOutcome::StepLimit { .. }
        ));
        let display = machine.device::<DisplayDevice>(DISPLAY_DEVICE).unwrap();
        assert!(display.swap_pending());
        display.advance_frame();
        assert_eq!(display.active_base(), FRAMEBUFFER_A_BASE_WORD);
    }

    #[test]
    #[ignore = "explicit full-system Icarus simulation"]
    fn cpu_writes_while_display_reads() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Display>().unwrap();
    }
}
