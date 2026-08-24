use digital_design_code::CircuitWires;
use digital_design_hardware::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, G16Core, G16MmioBridge, GowinCliError,
    GowinDspMode, GowinModuleProject, Hardware, HardwareIdentity, Module, ResourceCountExpectation,
    SystemControlDevice, TangNano20K, TangNano20KDebugOutputs, TangNano20KInputs,
    VerilogDependency, BSRAM_1024_DEPTH,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/g16_mmio_gowin")
}

/// MMIO diagnostic compiled from the `SOURCE` below with `--g16 --code-base 0`;
/// kept in sync by `program_is_the_current_compiler_output`.
const PROGRAM: [u16; 93] = [
    0xfff0, 0xafd0, 0xfff0, 0xaff0, 0xf001, 0xaf05, 0x90f2, 0xaf01, 0x81f3, 0x3110, 0xaf20, 0xe1c1,
    0xa9c0, 0xf004, 0xb0cb, 0xf004, 0xaf14, 0x91f3, 0x83f3, 0x3330, 0xe1c3, 0xa9c0, 0xf004, 0xb0c0,
    0x91f3, 0x83f3, 0x3330, 0xe1c3, 0xa9c0, 0xf003, 0xb0c7, 0xf004, 0xaf38, 0x93f3, 0x83f3, 0x3330,
    0xe1c3, 0xa9c0, 0xf002, 0xb0cc, 0xf005, 0xaf34, 0x93f3, 0x83f3, 0x3330, 0xe1c3, 0xa9c0, 0xf002,
    0xb0c1, 0x90f3, 0x83f3, 0x3330, 0xe1c3, 0xa9c0, 0xf001, 0xb0c8, 0xaf39, 0x93f3, 0x83f3, 0x3330,
    0xe1c3, 0xa9c0, 0xf000, 0xb0ce, 0x92f3, 0x83f3, 0x3330, 0xe1c3, 0xa9c0, 0xf000, 0xb0c5, 0xf001,
    0xaf34, 0x93f3, 0xf0ff, 0xcfbb, 0xf0ff, 0xcff3, 0xf0ff, 0xcfea, 0xf0ff, 0xcfe0, 0xf0ff, 0xcfd7,
    0xf0ff, 0xcfcc, 0xf0ff, 0xcfc1, 0xf0ff, 0xcfb8, 0xf0ff, 0xcfac, 0xe800,
];

struct ProgramImage;

const fn program_image() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut index = 0;
    // Keep the complete boot memory materialized as BSRAM (see g16_cpu).
    while index < words.len() {
        words[index] = (((index as u64) * 0x9e37) ^ 0x5aa5) & 0xffff;
        index += 1;
    }
    index = 0;
    while index < PROGRAM.len() {
        words[index] = PROGRAM[index] as u64;
        index += 1;
    }
    words
}

impl BsramImage<16> for ProgramImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = program_image();
}

type ProgramMemory = Bsram1R1Rw1024<16, ProgramImage>;
type SystemControl = SystemControlDevice<234>;

#[derive(Hardware)]
#[hardware(namespace = "examples/g16_mmio")]
struct G16MmioBoardTest;

impl Module for G16MmioBoardTest {
    type Input = TangNano20KInputs;
    type Output = TangNano20KDebugOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("G16MmioBoardTest is a Verilog-only hardware test harness")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("self_test.v")
                .replace(
                    "__PROGRAM_MEMORY__",
                    &ProgramMemory::verilog_identity().module_name(),
                )
                .replace("__G16_CORE__", &G16Core::verilog_identity().module_name())
                .replace(
                    "__MMIO_BRIDGE__",
                    &G16MmioBridge::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<ProgramMemory>("u_program"),
            VerilogDependency::new::<G16Core>("u_core"),
            VerilogDependency::new::<G16MmioBridge>("u_mmio_bridge"),
            VerilogDependency::new::<SystemControl>("u_sysctl"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, G16MmioBoardTest> {
    TangNano20K::debug_uart_project::<G16MmioBoardTest>("g16_mmio_self_test")
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Exact(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v2::{frontend::parse_source_with, Compiler, CompilerOptions};
    use digital_design_hardware::{ResourceKind, VerilogProject};

    /// Alive LED pattern, then the DDHT test ID `0x09` frame forever, all
    /// through device 0. Leaf and register-only: every non-MMIO data access
    /// faults at the bridge.
    const SOURCE: &str = r#"
        fn main() {
            dev_send(0, 2, 0b010101);
            while 1 == 1 {
                while dev_recv(0, 3) & 1 != 0 { }
                dev_send(0, 3, 0x44);
                while dev_recv(0, 3) & 1 != 0 { }
                dev_send(0, 3, 0x44);
                while dev_recv(0, 3) & 1 != 0 { }
                dev_send(0, 3, 0x48);
                while dev_recv(0, 3) & 1 != 0 { }
                dev_send(0, 3, 0x54);
                while dev_recv(0, 3) & 1 != 0 { }
                dev_send(0, 3, 0x01);
                while dev_recv(0, 3) & 1 != 0 { }
                dev_send(0, 3, 0x09);
                while dev_recv(0, 3) & 1 != 0 { }
                dev_send(0, 3, 0x00);
                while dev_recv(0, 3) & 1 != 0 { }
                dev_send(0, 3, 0x14);
            }
        }
    "#;

    fn compile() -> Vec<u16> {
        let options = CompilerOptions::g16();
        let frontend = parse_source_with(SOURCE, options.data_base).unwrap();
        let mut compiler = Compiler::new();
        compiler.opts = options;
        for function in frontend.funcs {
            compiler.add_func(function);
        }
        compiler.finish_g16("main").words
    }

    #[test]
    fn program_is_the_current_compiler_output() {
        let compiled = compile();
        assert!(
            compiled.len() < BSRAM_1024_DEPTH,
            "program uses {} words; the boot memory holds {BSRAM_1024_DEPTH}",
            compiled.len()
        );
        if compiled != PROGRAM {
            let items = compiled
                .iter()
                .map(|word| format!("0x{word:04x}"))
                .collect::<Vec<_>>();
            panic!("program changed; new PROGRAM = &[{}]", items.join(", "));
        }
        assert_eq!(
            ProgramImage::WORDS[..compiled.len()],
            compiled.iter().copied().map(u64::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_contains_program_memory_core_bridge_and_sysctl() {
        let verilog = VerilogProject::generate::<G16MmioBoardTest>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 2);
        let project = gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Multiplier18x18], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn mmio_diagnostic_executes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<G16MmioBoardTest>().unwrap();
    }
}
