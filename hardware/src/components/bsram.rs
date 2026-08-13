use crate::resources::components::BsramBlocks;
use crate::{
    HardwareIdentity, Module, ModuleIo, ModuleTest, TargetResourceRequest, TestStep,
    VerilogIdentity, VerilogVerification,
};
use askama::Template;
use digital_design_code::{CircuitWires, Wire, Wires};
use std::marker::PhantomData;

/// Number of words in the initial BSRAM component family.
pub const BSRAM_1024_DEPTH: usize = 1024;

const DEPTH: usize = BSRAM_1024_DEPTH;

#[derive(Clone, Copy, Debug)]
enum BsramShape {
    OneReadWrite,
    OneReadOneReadWrite,
    TrueDualPort,
}

fn verified_block_count<const WIDTH: usize>(shape: BsramShape) -> u64 {
    // Add a configuration only after its inferred implementation has passed
    // module simulation, Gowin place-and-route, and the board self-test. This
    // planning count must match the BSRAM total in the PnR report.
    match (shape, WIDTH) {
        (BsramShape::OneReadWrite, 16 | 18)
        | (BsramShape::OneReadOneReadWrite, 16 | 18)
        | (BsramShape::TrueDualPort, 16 | 18) => 1,
        _ => panic!(
            "no measured BSRAM resource result for shape {shape:?}, depth {DEPTH}, width {WIDTH}"
        ),
    }
}

fn resource_request<const WIDTH: usize>(shape: BsramShape) -> TargetResourceRequest {
    TargetResourceRequest::new(BsramBlocks::new(verified_block_count::<WIDTH>(shape)))
}

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
    words: &'a [VerilogWord],
}

struct VerilogWord {
    address: usize,
    literal: String,
}

/// Compile-time power-up contents for a BSRAM specialization.
///
/// The same array initializes the host emulator and generates every word
/// embedded in the FPGA configuration.
pub trait BsramImage<const WIDTH: usize>: 'static {
    const WORDS: [u64; BSRAM_1024_DEPTH];
}

/// An explicit all-zero BSRAM image.
pub struct ZeroBsramImage;

impl<const WIDTH: usize> BsramImage<WIDTH> for ZeroBsramImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = [0; BSRAM_1024_DEPTH];
}

fn image_words<I, const WIDTH: usize>() -> Box<[u64; DEPTH]>
where
    I: BsramImage<WIDTH>,
{
    validate_width::<WIDTH>();
    let words = Box::new(I::WORDS);
    for (address, &value) in words.iter().enumerate() {
        assert!(
            value <= word_mask::<WIDTH>(),
            "BSRAM initial word at address {address} ({value:#x}) does not fit width {WIDTH}"
        );
    }
    words
}

fn image_hash<I, const WIDTH: usize>() -> u64
where
    I: BsramImage<WIDTH>,
{
    let words = image_words::<I, WIDTH>();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in b"digital-design-bsram-init-v1"
        .iter()
        .copied()
        .chain([0])
        .chain(WIDTH.to_le_bytes())
        .chain(words.iter().flat_map(|word| word.to_le_bytes()))
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn verilog_words<I, const WIDTH: usize>() -> Vec<VerilogWord>
where
    I: BsramImage<WIDTH>,
{
    image_words::<I, WIDTH>()
        .iter()
        .copied()
        .enumerate()
        .map(|(address, value)| VerilogWord {
            address,
            literal: format!("{WIDTH}'h{value:x}"),
        })
        .collect()
}

#[derive(Template)]
#[template(path = "components/bsram/1r1rw_1024.v", escape = "none")]
struct OneReadOneReadWriteTemplate<'a> {
    module_name: &'a str,
    high_bit: usize,
    words: &'a [VerilogWord],
}

#[derive(Template)]
#[template(path = "components/bsram/true_dual_port_1024.v", escape = "none")]
struct TrueDualPortTemplate<'a> {
    module_name: &'a str,
    high_bit: usize,
    words: &'a [VerilogWord],
}

/// One synchronous normal-mode read/write port backed by one 18-Kbit BSRAM.
/// The registered output holds its previous value during a write.
pub struct Bsram1Rw1024<const WIDTH: usize, I>(PhantomData<I>);

impl<const WIDTH: usize, I> HardwareIdentity for Bsram1Rw1024<WIDTH, I>
where
    I: BsramImage<WIDTH>,
{
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("Bsram1Rw1024")
            .namespace(["components", "memory", "bsram"])
            .constant("WIDTH", WIDTH)
            .symbol("IMAGE", format!("h{:016x}", image_hash::<I, WIDTH>()))
    }
}

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

impl<const WIDTH: usize, I> Module for Bsram1Rw1024<WIDTH, I>
where
    I: BsramImage<WIDTH>,
{
    type Input = Bsram1Rw1024Input<WIDTH>;
    type Output = Bsram1Rw1024Output<WIDTH>;
    type EmuState = Bsram1Rw1024State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![resource_request::<WIDTH>(BsramShape::OneReadWrite)]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Bsram1Rw1024State {
            memory: image_words::<I, WIDTH>(),
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

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        let words = verilog_words::<I, WIDTH>();
        Some(
            OneReadWriteTemplate {
                module_name: &module_name,
                high_bit: WIDTH - 1,
                words: &words,
            }
            .render()
            .expect("BSRAM Verilog template must render"),
        )
    }

    fn verilog_verification() -> Option<VerilogVerification> {
        Some(single_port_test::<I, WIDTH>().verilog_verification(""))
    }
}

/// One synchronous read-only port plus one synchronous read/write port.
pub struct Bsram1R1Rw1024<const WIDTH: usize, I>(PhantomData<I>);

impl<const WIDTH: usize, I> HardwareIdentity for Bsram1R1Rw1024<WIDTH, I>
where
    I: BsramImage<WIDTH>,
{
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("Bsram1R1Rw1024")
            .namespace(["components", "memory", "bsram"])
            .constant("WIDTH", WIDTH)
            .symbol("IMAGE", format!("h{:016x}", image_hash::<I, WIDTH>()))
    }
}

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

impl<const WIDTH: usize, I> Module for Bsram1R1Rw1024<WIDTH, I>
where
    I: BsramImage<WIDTH>,
{
    type Input = Bsram1R1Rw1024Input<WIDTH>;
    type Output = Bsram1R1Rw1024Output<WIDTH>;
    type EmuState = Bsram1R1Rw1024State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![resource_request::<WIDTH>(BsramShape::OneReadOneReadWrite)]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Bsram1R1Rw1024State {
            memory: image_words::<I, WIDTH>(),
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

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        let words = verilog_words::<I, WIDTH>();
        Some(
            OneReadOneReadWriteTemplate {
                module_name: &module_name,
                high_bit: WIDTH - 1,
                words: &words,
            }
            .render()
            .expect("BSRAM Verilog template must render"),
        )
    }

    fn verilog_verification() -> Option<VerilogVerification> {
        Some(read_rw_test::<I, WIDTH>().verilog_verification(""))
    }
}

/// Two symmetric synchronous normal-mode read/write ports.
/// Each registered output holds its previous value while that port writes.
///
/// Simultaneous writes to the same address are deliberately unsupported: the
/// physical result is device- and mode-dependent. The emulator panics rather
/// than inventing a portable priority rule.
pub struct BsramTrueDualPort1024<const WIDTH: usize, I>(PhantomData<I>);

impl<const WIDTH: usize, I> HardwareIdentity for BsramTrueDualPort1024<WIDTH, I>
where
    I: BsramImage<WIDTH>,
{
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("BsramTrueDualPort1024")
            .namespace(["components", "memory", "bsram"])
            .constant("WIDTH", WIDTH)
            .symbol("IMAGE", format!("h{:016x}", image_hash::<I, WIDTH>()))
    }
}

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

impl<const WIDTH: usize, I> Module for BsramTrueDualPort1024<WIDTH, I>
where
    I: BsramImage<WIDTH>,
{
    type Input = BsramTrueDualPort1024Input<WIDTH>;
    type Output = BsramTrueDualPort1024Output<WIDTH>;
    type EmuState = BsramTrueDualPort1024State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![resource_request::<WIDTH>(BsramShape::TrueDualPort)]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        BsramTrueDualPort1024State {
            memory: image_words::<I, WIDTH>(),
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

    fn generated_verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        let words = verilog_words::<I, WIDTH>();
        Some(
            TrueDualPortTemplate {
                module_name: &module_name,
                high_bit: WIDTH - 1,
                words: &words,
            }
            .render()
            .expect("BSRAM Verilog template must render"),
        )
    }

    fn verilog_verification() -> Option<VerilogVerification> {
        Some(true_dual_port_test::<I, WIDTH>().verilog_verification(""))
    }
}

fn single_port_test<I, const WIDTH: usize>() -> ModuleTest<Bsram1Rw1024<WIDTH, I>>
where
    I: BsramImage<WIDTH>,
{
    let first = word::<WIDTH>(0x2_a55a);
    let second = word::<WIDTH>(0x1_3cc3);
    ModuleTest::new([
        TestStep::new(
            Bsram1Rw1024InputValue {
                write_enable: false,
                address: 733,
                write_data: 0,
            },
            Bsram1Rw1024OutputValue {
                read_data: I::WORDS[733],
            },
        ),
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

fn read_rw_test<I, const WIDTH: usize>() -> ModuleTest<Bsram1R1Rw1024<WIDTH, I>>
where
    I: BsramImage<WIDTH>,
{
    let first = word::<WIDTH>(0x2_5aa5);
    let second = word::<WIDTH>(0x1_c33c);
    let independent = word::<WIDTH>(0x3_1551);
    ModuleTest::new([
        TestStep::new(
            Bsram1R1Rw1024InputValue {
                read_address: 733,
                rw_write_enable: false,
                rw_address: 733,
                rw_write_data: 0,
            },
            Bsram1R1Rw1024OutputValue {
                read_data: I::WORDS[733],
                rw_read_data: I::WORDS[733],
            },
        ),
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

fn true_dual_port_test<I, const WIDTH: usize>() -> ModuleTest<BsramTrueDualPort1024<WIDTH, I>>
where
    I: BsramImage<WIDTH>,
{
    let first = word::<WIDTH>(0x2_1357);
    let second = word::<WIDTH>(0x1_2468);
    let replacement = word::<WIDTH>(0x3_0f0f);
    ModuleTest::new([
        TestStep::new(
            BsramTrueDualPort1024InputValue {
                a_write_enable: false,
                a_address: 733,
                a_write_data: 0,
                b_write_enable: false,
                b_address: 733,
                b_write_data: 0,
            },
            BsramTrueDualPort1024OutputValue {
                a_read_data: I::WORDS[733],
                b_read_data: I::WORDS[733],
            },
        ),
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

    struct TestImage;

    const fn test_image<const WIDTH: usize>() -> [u64; DEPTH] {
        let mut words = [0; DEPTH];
        let mut address = 0;
        while address < DEPTH {
            words[address] =
                ((((address as u64) << 8) | address as u64) ^ 0x2_a55a) & ((1u64 << WIDTH) - 1);
            address += 1;
        }
        words
    }

    impl<const WIDTH: usize> BsramImage<WIDTH> for TestImage {
        const WORDS: [u64; DEPTH] = test_image::<WIDTH>();
    }

    #[test]
    fn all_modes_and_widths_match_the_emu_contract() {
        single_port_test::<TestImage, 16>().run_emu();
        single_port_test::<TestImage, 18>().run_emu();
        read_rw_test::<TestImage, 16>().run_emu();
        read_rw_test::<TestImage, 18>().run_emu();
        true_dual_port_test::<TestImage, 16>().run_emu();
        true_dual_port_test::<TestImage, 18>().run_emu();
    }

    #[test]
    fn measured_specializations_report_one_block_each() {
        macro_rules! assert_one_block {
            ($module:ty) => {
                assert_eq!(
                    <$module>::target_resources(),
                    vec![TargetResourceRequest::new(BsramBlocks::new(1))]
                );
            };
        }
        assert_one_block!(Bsram1Rw1024<16, TestImage>);
        assert_one_block!(Bsram1Rw1024<18, TestImage>);
        assert_one_block!(Bsram1R1Rw1024<16, TestImage>);
        assert_one_block!(Bsram1R1Rw1024<18, TestImage>);
        assert_one_block!(BsramTrueDualPort1024<16, TestImage>);
        assert_one_block!(BsramTrueDualPort1024<18, TestImage>);
    }

    #[test]
    fn memory_identity_depends_on_image_contents() {
        struct OtherImage;
        impl BsramImage<16> for OtherImage {
            const WORDS: [u64; DEPTH] = {
                let mut words = [0; DEPTH];
                let mut address = 0;
                while address < DEPTH {
                    words[address] = address as u64;
                    address += 1;
                }
                words
            };
        }

        assert_ne!(
            Bsram1Rw1024::<16, TestImage>::verilog_identity(),
            Bsram1Rw1024::<16, OtherImage>::verilog_identity()
        );
    }

    #[test]
    #[should_panic(expected = "no measured BSRAM resource result")]
    fn unmeasured_specialization_fails_during_resource_reporting() {
        let _ = Bsram1Rw1024::<17, ZeroBsramImage>::target_resources();
    }

    #[test]
    #[ignore = "explicit external simulator validation of generated BSRAM specializations"]
    fn verify_verilog_with_iverilog() {
        macro_rules! verify {
            ($module:ty) => {
                println!(
                    "{}",
                    crate::verify_verilog_with_iverilog::<$module>().unwrap()
                );
            };
        }
        verify!(Bsram1Rw1024<16, TestImage>);
        verify!(Bsram1Rw1024<18, TestImage>);
        verify!(Bsram1R1Rw1024<16, TestImage>);
        verify!(Bsram1R1Rw1024<18, TestImage>);
        verify!(BsramTrueDualPort1024<16, TestImage>);
        verify!(BsramTrueDualPort1024<18, TestImage>);
    }
}
