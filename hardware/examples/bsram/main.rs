use digital_design_code::CircuitWires;
use digital_design_hardware::{
    run_gowin_project_cli, Bsram1R1Rw1024, Bsram1Rw1024, BsramBlocks, BsramTrueDualPort1024,
    GowinCliError, GowinModuleProject, Hardware, Module, ModuleIo, ModuleTest, TangNano20K,
    TangNano20KDebugOutputs, TangNano20KDebugOutputsValue, TangNano20KInputs,
    TangNano20KInputsValue, TargetResourceRequest, TestStep, VerilogVerification,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(bsram_gowin_project(), "target/bsram_gowin")
}

#[derive(Hardware)]
#[hardware(namespace = "examples/bsram", target_leaf)]
struct BsramBoardSelfTest;

impl Module for BsramBoardSelfTest {
    type Input = TangNano20KInputs;
    type Output = TangNano20KDebugOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(6))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &TangNano20KDebugOutputsValue {
                leds: 0,
                uart_tx: true,
            },
        );
    }

    fn verilog_source() -> Option<String> {
        let mut source = include_str!("self_test.v").to_string();
        macro_rules! append {
            ($module:ty) => {
                source.push('\n');
                source.push_str(&<$module as Module>::verilog_source().unwrap());
            };
        }
        append!(Bsram1Rw1024<16>);
        append!(Bsram1Rw1024<18>);
        append!(Bsram1R1Rw1024<16>);
        append!(Bsram1R1Rw1024<18>);
        append!(BsramTrueDualPort1024<16>);
        append!(BsramTrueDualPort1024<18>);
        Some(source)
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
        let source = verilog.files.values().next().unwrap();
        for module in [
            "Bsram1Rw1024_WIDTH16",
            "Bsram1Rw1024_WIDTH18",
            "Bsram1R1Rw1024_WIDTH16",
            "Bsram1R1Rw1024_WIDTH18",
            "BsramTrueDualPort1024_WIDTH16",
            "BsramTrueDualPort1024_WIDTH18",
        ] {
            assert!(source.contains(&format!("module {module}(")));
            assert!(source.contains(&format!("{module} u_{module}")));
        }

        let project = bsram_gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 6);
        assert_eq!(project.resources.claimed[&ResourceKind::DebugUartTx], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation; copy the printed record into self_test.verified"]
    fn verify_handwritten_verilog_with_iverilog() {
        let record =
            digital_design_hardware::verify_verilog_with_iverilog::<BsramBoardSelfTest>().unwrap();
        println!("{record}");
    }
}
