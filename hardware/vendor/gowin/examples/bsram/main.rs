use digital_design_circuit::CircuitWires;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, Bsram1R1Rw1024, Bsram1Rw1024, BsramImage, BsramTrueDualPort1024,
    GowinCliError, GowinModuleProject, Hardware, Module, ModuleTest, TangNano20K,
    TangNano20KDebugOutputs, TangNano20KDebugOutputsValue, TangNano20KInputs,
    TangNano20KInputsValue, TestStep, VerilogDependency, ZeroBsramImage, BSRAM_1024_DEPTH,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(bsram_gowin_project(), "target/bsram_gowin")
}

#[derive(Hardware)]
#[hardware(namespace = "examples/bsram")]
struct BsramBoardSelfTest;

struct BoardImage;

const fn board_init_image() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut address = 0;
    while address < words.len() {
        words[address] = (((address as u64) << 6) | ((address as u64) >> 4)) ^ 0xa55a;
        address += 1;
    }
    words
}

impl BsramImage<16> for BoardImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = board_init_image();
}

type Sp16 = Bsram1Rw1024<16, ZeroBsramImage>;
type Sp18 = Bsram1Rw1024<18, ZeroBsramImage>;
type OneReadRw16 = Bsram1R1Rw1024<16, ZeroBsramImage>;
type OneReadRw18 = Bsram1R1Rw1024<18, ZeroBsramImage>;
type TrueDualPort16 = BsramTrueDualPort1024<16, ZeroBsramImage>;
type TrueDualPort18 = BsramTrueDualPort1024<18, ZeroBsramImage>;
type PatternSp16 = Bsram1Rw1024<16, BoardImage>;

impl Module for BsramBoardSelfTest {
    type Input = TangNano20KInputs;
    type Output = TangNano20KDebugOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        panic!("BsramBoardSelfTest is a Verilog-only hardware test harness")
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        let _ = circuit;
        panic!("BsramBoardSelfTest is a Verilog-only hardware test harness")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<Sp16>("u_Bsram1Rw1024_WIDTH16"),
            VerilogDependency::new::<Sp18>("u_Bsram1Rw1024_WIDTH18"),
            VerilogDependency::new::<OneReadRw16>("u_Bsram1R1Rw1024_WIDTH16"),
            VerilogDependency::new::<OneReadRw18>("u_Bsram1R1Rw1024_WIDTH18"),
            VerilogDependency::new::<TrueDualPort16>("u_BsramTrueDualPort1024_WIDTH16"),
            VerilogDependency::new::<TrueDualPort18>("u_BsramTrueDualPort1024_WIDTH18"),
            VerilogDependency::new::<PatternSp16>("u_pattern_bsram"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(board_test().verilog_testbench())
    }
}

fn board_test() -> ModuleTest<BsramBoardSelfTest> {
    ModuleTest::new([
        TestStep::drive(TangNano20KInputsValue { buttons: 1 }).after_cycles(2),
        TestStep::new(
            TangNano20KInputsValue { buttons: 0 },
            TangNano20KDebugOutputsValue {
                leds: 1,
                uart_tx: true,
            },
        )
        .after_cycles(3_200),
    ])
}

fn bsram_gowin_project() -> GowinModuleProject<TangNano20K, BsramBoardSelfTest> {
    TangNano20K::debug_uart_project::<BsramBoardSelfTest>("bsram_self_test")
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware_gowin::{HardwareIdentity, ResourceKind, VerilogProject};

    #[test]
    fn project_contains_bsram_shapes_and_images() {
        let verilog = VerilogProject::generate::<BsramBoardSelfTest>().unwrap();
        for (module, instance) in [
            (
                Sp16::verilog_identity().module_name(),
                "u_Bsram1Rw1024_WIDTH16",
            ),
            (
                Sp18::verilog_identity().module_name(),
                "u_Bsram1Rw1024_WIDTH18",
            ),
            (
                OneReadRw16::verilog_identity().module_name(),
                "u_Bsram1R1Rw1024_WIDTH16",
            ),
            (
                OneReadRw18::verilog_identity().module_name(),
                "u_Bsram1R1Rw1024_WIDTH18",
            ),
            (
                TrueDualPort16::verilog_identity().module_name(),
                "u_BsramTrueDualPort1024_WIDTH16",
            ),
            (
                TrueDualPort18::verilog_identity().module_name(),
                "u_BsramTrueDualPort1024_WIDTH18",
            ),
        ] {
            assert!(verilog
                .files
                .values()
                .any(|source| source.contains(&format!("module {module}("))));
            assert!(verilog
                .files
                .values()
                .any(|source| source.contains(&format!("{module} {instance}"))));
        }
        let pattern_module = PatternSp16::verilog_identity().module_name();
        assert!(verilog
            .files
            .values()
            .any(|source| source.contains(&format!("module {pattern_module}("))));
        assert!(verilog
            .files
            .values()
            .any(|source| source.contains(&format!("{pattern_module} u_pattern_bsram"))));
        assert_eq!(verilog.resource_claims.len(), 7);
        assert!(verilog.resource_claims.iter().all(|claim| {
            claim.resources
                == [digital_design_hardware::ResourceAmount::new(
                    ResourceKind::Bsram18K,
                    1,
                )]
        }));

        let project = bsram_gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 7);
        assert_eq!(project.resources.claimed[&ResourceKind::DebugUartTx], 1);
    }

    #[test]
    #[should_panic(expected = "emulator implementation is not available")]
    fn verilog_only_board_harness_rejects_emu_execution() {
        board_test().run_emu();
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn verify_handwritten_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<BsramBoardSelfTest>().unwrap();
    }
}
