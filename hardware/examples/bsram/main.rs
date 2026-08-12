use digital_design_code::CircuitWires;
use digital_design_hardware::{
    run_gowin_project_cli, Bsram1R1Rw1024, Bsram1Rw1024, BsramTrueDualPort1024, GowinCliError,
    GowinModuleProject, Hardware, Module, ModuleTest, TangNano20K, TangNano20KDebugOutputs,
    TangNano20KDebugOutputsValue, TangNano20KInputs, TangNano20KInputsValue, TestStep,
    VerilogDependency, VerilogVerification,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(bsram_gowin_project(), "target/bsram_gowin")
}

#[derive(Hardware)]
#[hardware(namespace = "examples/bsram")]
struct BsramBoardSelfTest;

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
            VerilogDependency::new::<Bsram1Rw1024<16>>("u_Bsram1Rw1024_WIDTH16"),
            VerilogDependency::new::<Bsram1Rw1024<18>>("u_Bsram1Rw1024_WIDTH18"),
            VerilogDependency::new::<Bsram1R1Rw1024<16>>("u_Bsram1R1Rw1024_WIDTH16"),
            VerilogDependency::new::<Bsram1R1Rw1024<18>>("u_Bsram1R1Rw1024_WIDTH18"),
            VerilogDependency::new::<BsramTrueDualPort1024<16>>("u_BsramTrueDualPort1024_WIDTH16"),
            VerilogDependency::new::<BsramTrueDualPort1024<18>>("u_BsramTrueDualPort1024_WIDTH18"),
        ]
    }

    fn verilog_verification() -> Option<VerilogVerification> {
        Some(board_test().verilog_verification(include_str!("self_test.verified")))
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
        .after_cycles(2_100),
    ])
}

fn bsram_gowin_project() -> GowinModuleProject<TangNano20K, BsramBoardSelfTest> {
    TangNano20K::debug_uart_project::<BsramBoardSelfTest>("bsram_self_test")
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn project_contains_six_bsram_shapes() {
        let verilog = VerilogProject::generate::<BsramBoardSelfTest>().unwrap();
        for module in [
            "Bsram1Rw1024_WIDTH16",
            "Bsram1Rw1024_WIDTH18",
            "Bsram1R1Rw1024_WIDTH16",
            "Bsram1R1Rw1024_WIDTH18",
            "BsramTrueDualPort1024_WIDTH16",
            "BsramTrueDualPort1024_WIDTH18",
        ] {
            assert!(verilog
                .files
                .values()
                .any(|source| source.contains(&format!("module {module}("))));
            assert!(verilog
                .files
                .values()
                .any(|source| source.contains(&format!("{module} u_{module}"))));
        }
        assert_eq!(verilog.resource_claims.len(), 6);
        assert!(verilog.resource_claims.iter().all(|claim| {
            claim.resources
                == [digital_design_hardware::ResourceAmount::new(
                    ResourceKind::Bsram18K,
                    1,
                )]
        }));

        let project = bsram_gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 6);
        assert_eq!(project.resources.claimed[&ResourceKind::DebugUartTx], 1);
    }

    #[test]
    #[should_panic(expected = "emulator implementation is not available")]
    fn verilog_only_board_harness_rejects_emu_execution() {
        board_test().run_emu();
    }

    #[test]
    #[ignore = "explicit external simulator validation; copy the printed record into self_test.verified"]
    fn verify_handwritten_verilog_with_iverilog() {
        let record =
            digital_design_hardware::verify_verilog_with_iverilog::<BsramBoardSelfTest>().unwrap();
        println!("{record}");
    }
}
