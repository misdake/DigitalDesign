use digital_design_code::{input_const, Wires};
use digital_design_hardware::{
    Bsram1Rw1024, Bsram1Rw1024Input, Bsram1Rw1024InputValue, Bsram1Rw1024Output,
    Bsram1Rw1024OutputValue, Hardware, Module, ModuleTest, TestStep, VerilogProject,
    ZeroBsramImage,
};

const DATA_MASK: u64 = 0xa55a;
type Memory = Bsram1Rw1024<16, ZeroBsramImage>;

fn main() {
    usage_test().run_emu_and_mixed_nand();

    let project = VerilogProject::generate::<MaskedBsram>().expect("Verilog export must succeed");
    println!(
        "masked BSRAM emu/NAND test passed; FPGA export contains {} modules and {} BSRAM claim",
        project.files.len(),
        project.resource_claims.len()
    );
}

/// An upper module with NAND encoding logic around a target BSRAM leaf.
#[derive(Hardware)]
#[hardware(namespace = "examples/bsram_masked")]
struct MaskedBsram;

impl Module for MaskedBsram {
    type Input = Bsram1Rw1024Input<16>;
    type Output = Bsram1Rw1024Output<16>;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;

    // Compositional emulation reuses the same circuit and automatically gets
    // the BSRAM leaf's host emulator through Memory::hardware.
    fn emu(input: &Self::Input) -> Self::Output {
        Self::nand(input)
    }

    // Written once: local construction selects BSRAM emulation, while the
    // default Verilog builder records a BSRAM HDL instance automatically.
    fn nand(input: &Self::Input) -> Self::Output {
        Memory::hardware(&Bsram1Rw1024Input {
            write_enable: input.write_enable,
            address: input.address,
            write_data: input.write_data ^ constant_wires::<16>(DATA_MASK),
        })
    }
}

fn constant_wires<const WIDTH: usize>(value: u64) -> Wires<WIDTH> {
    Wires {
        wires: std::array::from_fn(|bit| input_const(((value >> bit) & 1) as u8)),
    }
}

fn usage_test() -> ModuleTest<MaskedBsram> {
    ModuleTest::new([
        TestStep::new(
            Bsram1Rw1024InputValue {
                write_enable: false,
                address: 0,
                write_data: 0,
            },
            Bsram1Rw1024OutputValue { read_data: 0 },
        ),
        TestStep::drive(Bsram1Rw1024InputValue {
            write_enable: true,
            address: 7,
            write_data: 0x1234,
        }),
        TestStep::new(
            Bsram1Rw1024InputValue {
                write_enable: false,
                address: 7,
                write_data: 0,
            },
            Bsram1Rw1024OutputValue {
                read_data: 0x1234 ^ DATA_MASK,
            },
        ),
        TestStep::new(
            Bsram1Rw1024InputValue {
                write_enable: true,
                address: 7,
                write_data: 0xabcd,
            },
            Bsram1Rw1024OutputValue {
                read_data: 0x1234 ^ DATA_MASK,
            },
        ),
        TestStep::new(
            Bsram1Rw1024InputValue {
                write_enable: false,
                address: 7,
                write_data: 0,
            },
            Bsram1Rw1024OutputValue {
                read_data: 0xabcd ^ DATA_MASK,
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceAmount, ResourceKind};

    #[test]
    fn emu_matches_nand_logic_with_an_emulated_bsram_leaf() {
        usage_test().run_emu_and_mixed_nand();
    }

    #[test]
    fn fpga_export_replaces_the_external_leaf_with_one_bsram_module() {
        let project = VerilogProject::generate::<MaskedBsram>().unwrap();
        assert_eq!(project.files.len(), 2);
        assert_eq!(project.resource_claims.len(), 1);
        assert_eq!(
            project.resource_claims[0].resources,
            [ResourceAmount::new(ResourceKind::Bsram18K, 1)]
        );
        let top = project
            .files
            .values()
            .find(|source| source.contains("module MaskedBsram("))
            .unwrap();
        assert!(top.contains("Bsram1Rw1024_WIDTH16_"));
    }
}
