mod board;
mod increment;

pub use board::*;
pub use increment::*;

use digital_design_code::{input_w_const, mux2_w, reg_w, CircuitWires, Wire, Wires};
use digital_design_hardware::{GowinModuleProject, Hardware, Module, ModuleIo, TangNano20K};

#[derive(Clone, ModuleIo)]
pub struct BasicAdderInput {
    pub reset: Wire,
    pub enable: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct BasicAdderOutput {
    pub count: Wires<6>,
}

#[derive(Hardware)]
#[hardware(namespace = "examples/basic_adder")]
pub struct BasicAdder;

pub type BasicAdderTarget = TangNano20K;
pub type BasicAdderTangNano20K = BasicAdderBoard<6_750_000>;

pub fn basic_adder_gowin_project() -> GowinModuleProject<BasicAdderTarget, BasicAdderTangNano20K> {
    TangNano20K::user_io_project::<BasicAdderTangNano20K>("basic_adder")
}

impl Module for BasicAdder {
    type Input = BasicAdderInput;
    type Output = BasicAdderOutput;
    type EmuState = u8;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        0
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &BasicAdderOutputValue {
                count: u64::from(*state),
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        if input.reset.is_one(circuit) {
            *state = 0;
        } else if input.enable.is_one(circuit) {
            *state = state.wrapping_add(1) & 0x3f;
        }
    }

    fn nand(input: &Self::Input) -> Self::Output {
        build_counter(input, Increment6::nand)
    }

    fn build_verilog(input: &Self::Input) -> Self::Output {
        build_counter(input, Increment6::verilog)
    }
}

fn build_counter(
    input: &BasicAdderInput,
    increment: fn(&Increment6Input) -> Increment6Output,
) -> BasicAdderOutput {
    let state = reg_w::<6>();
    let incremented = increment(&Increment6Input { value: state.out }).incremented;
    let enabled = mux2_w(state.out, incremented, input.enable);
    let next = mux2_w(enabled, input_w_const(0), input.reset);
    state.set_in(next);
    BasicAdderOutput { count: state.out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ModuleTest, ResourceKind, TestStep, VerilogProject};
    use std::path::Path;

    #[test]
    fn emu_and_nand_follow_the_same_cycles() {
        let mut expected = 0u64;
        let mut steps = Vec::new();
        for cycle in 0..70 {
            let reset = cycle == 0 || cycle == 35;
            let enable = cycle % 4 != 0;
            if reset {
                expected = 0;
            } else if enable {
                expected = (expected + 1) & 0x3f;
            }
            steps.push(TestStep::new(
                BasicAdderInputValue { reset, enable },
                BasicAdderOutputValue { count: expected },
            ));
        }
        ModuleTest::<BasicAdder>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn project_keeps_the_increment_module_boundary() {
        let project = VerilogProject::generate::<BasicAdder>().unwrap();
        assert_eq!(project.top_module, "BasicAdder");
        assert!(project
            .files
            .contains_key(Path::new("examples/basic_adder/basic_adder.v")));
        assert!(project
            .files
            .contains_key(Path::new("examples/basic_adder/increment6.v")));
        let top = &project.files[Path::new("examples/basic_adder/basic_adder.v")];
        assert!(top.contains("Increment6 u_increment6_0"));
        assert!(top.contains("input wire clk"));
    }

    #[test]
    fn tang_nano_project_contains_all_required_files() {
        let project = basic_adder_gowin_project().generate().unwrap();
        assert!(project.files.contains_key(Path::new("build.tcl")));
        assert!(project.files.contains_key(Path::new("basic_adder.gprj")));
        assert!(project.files.contains_key(Path::new("resource-report.txt")));
        assert!(project.files.contains_key(Path::new(
            "src/generated/examples/basic_adder/basic_adder_board/divisor6750000.v"
        )));
        assert!(project.files.contains_key(Path::new(
            "src/generated/components/timing/clock_divider/divisor6750000_width23.v"
        )));
        assert!(project
            .files
            .contains_key(Path::new("src/generated/board_top.v")));
        assert!(project
            .files
            .contains_key(Path::new("src/generated/board.cst")));
        assert!(project
            .files
            .contains_key(Path::new("src/generated/board.sdc")));

        let wrapper = &project.files[Path::new("src/generated/board_top.v")];
        assert!(wrapper.contains("input wire [1:0] buttons"));
        assert!(wrapper.contains("assign leds[0] = ~bound_leds[0]"));
        assert!(wrapper.contains("BasicAdderBoard_DIVISOR6750000 u_logic"));
        let board = &project.files
            [Path::new("src/generated/examples/basic_adder/basic_adder_board/divisor6750000.v")];
        assert!(board.contains("ClockDivider_DIVISOR6750000_WIDTH23 u_clock_divider_0"));
        let cst = &project.files[Path::new("src/generated/board.cst")];
        assert!(cst.contains("IO_LOC \"clk\" 4;"));
        assert!(cst.contains("IO_LOC \"buttons[0]\" 88;"));
        assert!(cst.contains("IO_LOC \"leds[5]\" 20;"));
        let sdc = &project.files[Path::new("src/generated/board.sdc")];
        assert!(sdc.contains("-period 37.037037"));
        let tcl = &project.files[Path::new("build.tcl")];
        assert!(tcl.contains("set_option -top_module tang_nano_20k_top"));
        assert!(tcl.contains("set_option -verilog_std v2001"));
        assert!(tcl.contains("add_file [file join $here {src} {generated} {board.sdc}]"));
        let gprj = &project.files[Path::new("basic_adder.gprj")];
        assert!(gprj.contains(project.device.part_number));
        assert!(gprj.contains("src/generated/board.sdc"));
        assert_eq!(project.target_name, "tang-nano-20k");
        assert_eq!(project.resources.claimed[&ResourceKind::UserLed], 6);
        assert_eq!(project.resources.claimed[&ResourceKind::UserButton], 2);
        let report = &project.files[Path::new("resource-report.txt")];
        assert!(report.contains("SDR SDRAM device: 67108864 bits"));
        assert!(report.contains("SPI flash device: 67108864 bits"));
    }
}
