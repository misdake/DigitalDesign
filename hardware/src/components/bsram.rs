use crate::resources::components::BsramBlocks;
use crate::{
    Hardware, HardwareIdentity, Module, ModuleIo, ModuleTest, TargetResourceRequest, TestStep,
    VerilogVerification,
};
use askama::Template;
use digital_design_code::{CircuitWires, Wire, Wires};

const DEPTH: usize = 1024;

fn validate_width<const WIDTH: usize>() {
    assert!(
        WIDTH == 16 || WIDTH == 18,
        "1024-word BSRAM width must be 16 or 18 bits, found {WIDTH}"
    );
}

fn word_mask<const WIDTH: usize>() -> u64 {
    validate_width::<WIDTH>();
    (1u64 << WIDTH) - 1
}

fn word<const WIDTH: usize>(value: u64) -> u64 {
    value & word_mask::<WIDTH>()
}

fn address<const WIDTH: usize>(value: u64) -> usize {
    validate_width::<WIDTH>();
    usize::try_from(value).expect("10-bit BSRAM address did not fit usize")
}

#[derive(Template)]
#[template(path = "components/bsram/1rw_1024.v", escape = "none")]
struct OneReadWriteTemplate<'a> {
    module_name: &'a str,
    high_bit: usize,
}

#[derive(Template)]
#[template(path = "components/bsram/1r1rw_1024.v", escape = "none")]
struct OneReadOneReadWriteTemplate<'a> {
    module_name: &'a str,
    high_bit: usize,
}

#[derive(Template)]
#[template(path = "components/bsram/true_dual_port_1024.v", escape = "none")]
struct TrueDualPortTemplate<'a> {
    module_name: &'a str,
    high_bit: usize,
}

/// One synchronous normal-mode read/write port backed by one 18-Kbit BSRAM.
/// The registered output holds its previous value during a write.
#[derive(Hardware)]
#[hardware(namespace = "components/memory/bsram", target_leaf)]
pub struct Bsram1Rw1024<const WIDTH: usize>;

#[derive(Clone, ModuleIo)]
pub struct Bsram1Rw1024Input<const WIDTH: usize> {
    pub write_enable: Wire,
    pub address: Wires<10>,
    pub write_data: Wires<WIDTH>,
}

#[derive(Clone, ModuleIo)]
pub struct Bsram1Rw1024Output<const WIDTH: usize> {
    pub read_data: Wires<WIDTH>,
}

pub struct Bsram1Rw1024State {
    memory: Box<[u64; DEPTH]>,
    read_data: u64,
}

impl<const WIDTH: usize> Module for Bsram1Rw1024<WIDTH> {
    type Input = Bsram1Rw1024Input<WIDTH>;
    type Output = Bsram1Rw1024Output<WIDTH>;
    type EmuState = Bsram1Rw1024State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(1))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        validate_width::<WIDTH>();
        Bsram1Rw1024State {
            memory: Box::new([0; DEPTH]),
            read_data: 0,
        }
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &Bsram1Rw1024OutputValue {
                read_data: state.read_data,
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let address = address::<WIDTH>(input.address);
        if input.write_enable {
            let write_data = word::<WIDTH>(input.write_data);
            state.memory[address] = write_data;
        } else {
            state.read_data = state.memory[address];
        }
    }

    fn verilog_source() -> Option<String> {
        validate_width::<WIDTH>();
        let module_name = Self::verilog_identity().module_name();
        Some(
            OneReadWriteTemplate {
                module_name: &module_name,
                high_bit: WIDTH - 1,
            }
            .render()
            .expect("static BSRAM Verilog template must render"),
        )
    }

    fn verilog_verification() -> Option<VerilogVerification> {
        Some(single_port_test::<WIDTH>().verilog_verification(include_str!("bsram.verified")))
    }
}

/// One synchronous read-only port plus one synchronous read/write port.
#[derive(Hardware)]
#[hardware(namespace = "components/memory/bsram", target_leaf)]
pub struct Bsram1R1Rw1024<const WIDTH: usize>;

#[derive(Clone, ModuleIo)]
pub struct Bsram1R1Rw1024Input<const WIDTH: usize> {
    pub read_address: Wires<10>,
    pub rw_write_enable: Wire,
    pub rw_address: Wires<10>,
    pub rw_write_data: Wires<WIDTH>,
}

#[derive(Clone, ModuleIo)]
pub struct Bsram1R1Rw1024Output<const WIDTH: usize> {
    pub read_data: Wires<WIDTH>,
    pub rw_read_data: Wires<WIDTH>,
}

pub struct Bsram1R1Rw1024State {
    memory: Box<[u64; DEPTH]>,
    read_data: u64,
    rw_read_data: u64,
}

impl<const WIDTH: usize> Module for Bsram1R1Rw1024<WIDTH> {
    type Input = Bsram1R1Rw1024Input<WIDTH>;
    type Output = Bsram1R1Rw1024Output<WIDTH>;
    type EmuState = Bsram1R1Rw1024State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(1))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        validate_width::<WIDTH>();
        Bsram1R1Rw1024State {
            memory: Box::new([0; DEPTH]),
            read_data: 0,
            rw_read_data: 0,
        }
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &Bsram1R1Rw1024OutputValue {
                read_data: state.read_data,
                rw_read_data: state.rw_read_data,
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let read_address = address::<WIDTH>(input.read_address);
        let rw_address = address::<WIDTH>(input.rw_address);
        state.read_data = state.memory[read_address];
        if input.rw_write_enable {
            let write_data = word::<WIDTH>(input.rw_write_data);
            state.memory[rw_address] = write_data;
        } else {
            state.rw_read_data = state.memory[rw_address];
        }
    }

    fn verilog_source() -> Option<String> {
        validate_width::<WIDTH>();
        let module_name = Self::verilog_identity().module_name();
        Some(
            OneReadOneReadWriteTemplate {
                module_name: &module_name,
                high_bit: WIDTH - 1,
            }
            .render()
            .expect("static BSRAM Verilog template must render"),
        )
    }

    fn verilog_verification() -> Option<VerilogVerification> {
        Some(read_rw_test::<WIDTH>().verilog_verification(include_str!("bsram.verified")))
    }
}

/// Two symmetric synchronous normal-mode read/write ports.
/// Each registered output holds its previous value while that port writes.
///
/// Simultaneous writes to the same address are deliberately unsupported: the
/// physical result is device- and mode-dependent. The emulator panics rather
/// than inventing a portable priority rule.
#[derive(Hardware)]
#[hardware(namespace = "components/memory/bsram", target_leaf)]
pub struct BsramTrueDualPort1024<const WIDTH: usize>;

#[derive(Clone, ModuleIo)]
pub struct BsramTrueDualPort1024Input<const WIDTH: usize> {
    pub a_write_enable: Wire,
    pub a_address: Wires<10>,
    pub a_write_data: Wires<WIDTH>,
    pub b_write_enable: Wire,
    pub b_address: Wires<10>,
    pub b_write_data: Wires<WIDTH>,
}

#[derive(Clone, ModuleIo)]
pub struct BsramTrueDualPort1024Output<const WIDTH: usize> {
    pub a_read_data: Wires<WIDTH>,
    pub b_read_data: Wires<WIDTH>,
}

pub struct BsramTrueDualPort1024State {
    memory: Box<[u64; DEPTH]>,
    a_read_data: u64,
    b_read_data: u64,
}

impl<const WIDTH: usize> Module for BsramTrueDualPort1024<WIDTH> {
    type Input = BsramTrueDualPort1024Input<WIDTH>;
    type Output = BsramTrueDualPort1024Output<WIDTH>;
    type EmuState = BsramTrueDualPort1024State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(1))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        validate_width::<WIDTH>();
        BsramTrueDualPort1024State {
            memory: Box::new([0; DEPTH]),
            a_read_data: 0,
            b_read_data: 0,
        }
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &BsramTrueDualPort1024OutputValue {
                a_read_data: state.a_read_data,
                b_read_data: state.b_read_data,
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let a_address = address::<WIDTH>(input.a_address);
        let b_address = address::<WIDTH>(input.b_address);
        if input.a_write_enable && input.b_write_enable && a_address == b_address {
            panic!(
                "simultaneous BSRAM writes to address {a_address} are undefined; arbitrate them before the target leaf"
            );
        }
        let old_a = state.memory[a_address];
        let old_b = state.memory[b_address];
        if input.a_write_enable {
            let write_data = word::<WIDTH>(input.a_write_data);
            state.memory[a_address] = write_data;
        } else {
            state.a_read_data = old_a;
        }
        if input.b_write_enable {
            let write_data = word::<WIDTH>(input.b_write_data);
            state.memory[b_address] = write_data;
        } else {
            state.b_read_data = old_b;
        }
    }

    fn verilog_source() -> Option<String> {
        validate_width::<WIDTH>();
        let module_name = Self::verilog_identity().module_name();
        Some(
            TrueDualPortTemplate {
                module_name: &module_name,
                high_bit: WIDTH - 1,
            }
            .render()
            .expect("static BSRAM Verilog template must render"),
        )
    }

    fn verilog_verification() -> Option<VerilogVerification> {
        Some(true_dual_port_test::<WIDTH>().verilog_verification(include_str!("bsram.verified")))
    }
}

fn single_port_test<const WIDTH: usize>() -> ModuleTest<Bsram1Rw1024<WIDTH>> {
    let first = word::<WIDTH>(0x2_a55a);
    let second = word::<WIDTH>(0x1_3cc3);
    ModuleTest::new([
        TestStep::drive(Bsram1Rw1024InputValue {
            write_enable: true,
            address: 37,
            write_data: first,
        }),
        TestStep::new(
            Bsram1Rw1024InputValue {
                write_enable: false,
                address: 37,
                write_data: 0,
            },
            Bsram1Rw1024OutputValue { read_data: first },
        ),
        TestStep::new(
            Bsram1Rw1024InputValue {
                write_enable: true,
                address: 37,
                write_data: second,
            },
            Bsram1Rw1024OutputValue { read_data: first },
        ),
        TestStep::new(
            Bsram1Rw1024InputValue {
                write_enable: false,
                address: 37,
                write_data: 0,
            },
            Bsram1Rw1024OutputValue { read_data: second },
        ),
    ])
}

fn read_rw_test<const WIDTH: usize>() -> ModuleTest<Bsram1R1Rw1024<WIDTH>> {
    let first = word::<WIDTH>(0x2_5aa5);
    let second = word::<WIDTH>(0x1_c33c);
    let independent = word::<WIDTH>(0x3_1551);
    ModuleTest::new([
        TestStep::drive(Bsram1R1Rw1024InputValue {
            read_address: 81,
            rw_write_enable: true,
            rw_address: 19,
            rw_write_data: first,
        }),
        TestStep::drive(Bsram1R1Rw1024InputValue {
            read_address: 19,
            rw_write_enable: true,
            rw_address: 81,
            rw_write_data: independent,
        }),
        TestStep::new(
            Bsram1R1Rw1024InputValue {
                read_address: 19,
                rw_write_enable: false,
                rw_address: 19,
                rw_write_data: 0,
            },
            Bsram1R1Rw1024OutputValue {
                read_data: first,
                rw_read_data: first,
            },
        ),
        TestStep::new(
            Bsram1R1Rw1024InputValue {
                read_address: 81,
                rw_write_enable: true,
                rw_address: 19,
                rw_write_data: second,
            },
            Bsram1R1Rw1024OutputValue {
                read_data: independent,
                rw_read_data: first,
            },
        ),
        TestStep::new(
            Bsram1R1Rw1024InputValue {
                read_address: 19,
                rw_write_enable: false,
                rw_address: 19,
                rw_write_data: 0,
            },
            Bsram1R1Rw1024OutputValue {
                read_data: second,
                rw_read_data: second,
            },
        ),
    ])
}

fn true_dual_port_test<const WIDTH: usize>() -> ModuleTest<BsramTrueDualPort1024<WIDTH>> {
    let first = word::<WIDTH>(0x2_1357);
    let second = word::<WIDTH>(0x1_2468);
    let replacement = word::<WIDTH>(0x3_0f0f);
    ModuleTest::new([
        TestStep::drive(BsramTrueDualPort1024InputValue {
            a_write_enable: true,
            a_address: 7,
            a_write_data: first,
            b_write_enable: true,
            b_address: 901,
            b_write_data: second,
        }),
        TestStep::new(
            BsramTrueDualPort1024InputValue {
                a_write_enable: false,
                a_address: 7,
                a_write_data: 0,
                b_write_enable: false,
                b_address: 901,
                b_write_data: 0,
            },
            BsramTrueDualPort1024OutputValue {
                a_read_data: first,
                b_read_data: second,
            },
        ),
        TestStep::new(
            BsramTrueDualPort1024InputValue {
                a_write_enable: true,
                a_address: 901,
                a_write_data: replacement,
                b_write_enable: false,
                b_address: 7,
                b_write_data: 0,
            },
            BsramTrueDualPort1024OutputValue {
                a_read_data: first,
                b_read_data: first,
            },
        ),
        TestStep::new(
            BsramTrueDualPort1024InputValue {
                a_write_enable: false,
                a_address: 901,
                a_write_data: 0,
                b_write_enable: false,
                b_address: 901,
                b_write_data: 0,
            },
            BsramTrueDualPort1024OutputValue {
                a_read_data: replacement,
                b_read_data: replacement,
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_modes_and_widths_match_the_emu_contract() {
        single_port_test::<16>().run_emu();
        single_port_test::<18>().run_emu();
        read_rw_test::<16>().run_emu();
        read_rw_test::<18>().run_emu();
        true_dual_port_test::<16>().run_emu();
        true_dual_port_test::<18>().run_emu();
    }

    #[test]
    #[ignore = "explicit external simulator validation; copy the printed records into bsram.verified"]
    fn verify_handwritten_verilog_with_iverilog() {
        macro_rules! verify {
            ($module:ty) => {
                println!(
                    "{}",
                    crate::verify_verilog_with_iverilog::<$module>().unwrap()
                );
            };
        }
        verify!(Bsram1Rw1024<16>);
        verify!(Bsram1Rw1024<18>);
        verify!(Bsram1R1Rw1024<16>);
        verify!(Bsram1R1Rw1024<18>);
        verify!(BsramTrueDualPort1024<16>);
        verify!(BsramTrueDualPort1024<18>);
    }
}
