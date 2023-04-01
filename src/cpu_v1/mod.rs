pub mod isa;
pub use isa::*;
mod inst_rom;
use inst_rom::*;
mod pc;
use pc::*;
mod branch;
use branch::*;
mod decoder;
use decoder::*;
mod alu;
use alu::*;

use crate::{clear_all, external, input_w, reg, reg_w, simulate, External, Reg, Regs, Wires};
use std::any::Any;
use std::marker::PhantomData;

#[allow(unused)]
struct CpuV1State {
    clock_enable: Reg, // TODO impl
    inst: [Wires<8>; 256],
    pc: Regs<8>,
    mem: [Regs<4>; 64],
    mem_bank: Regs<2>, // TODO impl
    reg: [Regs<4>; 4],
    flag_p: Reg,
    flag_z: Reg,
    flag_n: Reg,
    external_device: Regs<4>, // TODO impl
}
impl CpuV1State {
    fn create(inst: [u8; 256]) -> Self {
        let inst = inst.map(|v| Wires::<8>::parse_u8(v));
        let mem = [0u8; 64].map(|_| reg_w());
        Self {
            clock_enable: reg(),
            inst,
            pc: reg_w(),
            mem,
            mem_bank: reg_w(),
            reg: [reg_w(); 4],
            flag_p: reg(),
            flag_z: reg(),
            flag_n: reg(),
            external_device: reg_w(),
        }
    }
}

#[allow(unused)] // internal struct for debugging only
struct CpuV1StateInternal {
    inst_rom_in: CpuInstInput,
    inst_rom_out: CpuInstOutput,

    next_pc_in: CpuPcInput,
    next_pc_out: CpuPcOutput,
}

trait CpuV1 {
    type Pc: CpuComponent<Input = CpuPcInput, Output = CpuPcOutput>;
    type InstRom: CpuComponent<Input = CpuInstInput, Output = CpuInstOutput>;
    type Decoder: CpuComponent<Input = CpuDecoderInput, Output = CpuDecoderOutput>;
    type Alu: CpuComponent<Input = CpuAluInput, Output = CpuAluOutput>;
    type Branch: CpuComponent<Input = CpuBranchInput, Output = CpuBranchOutput>;

    fn build(state: &mut CpuV1State) -> CpuV1StateInternal {
        // Inst
        let inst_rom_in = CpuInstInput {
            inst: state.inst,
            pc: state.pc.out,
        };
        let inst_rom_out: CpuInstOutput = Self::InstRom::build(&inst_rom_in);
        let CpuInstOutput { inst } = inst_rom_out;

        // Decoder
        let decoder_in = CpuDecoderInput { inst };
        let decoder_out: CpuDecoderOutput = Self::Decoder::build(&decoder_in);
        #[allow(unused)] //TODO
        let CpuDecoderOutput {
            reg0_addr,
            reg1_addr,
            imm,
            reg0_write_enable,
            reg0_write_select,
            alu_op,
            alu0_select,
            alu1_select,
            mem_addr_select,
            mem_write_enable,
            jmp_op,
            jmp_src_select,
        } = decoder_out;

        let alu_in = CpuAluInput {
            reg0_data: input_w(), // TODO from reg
            reg1_data: input_w(), // TODO from reg
            imm,
            alu_op,
            alu0_select,
            alu1_select,
        };
        let alu_out = Self::Alu::build(&alu_in);
        let CpuAluOutput { alu_out } = alu_out;

        // Branch
        let branch_in = CpuBranchInput {
            imm,
            reg0: input_w(), //TODO from reg
            alu_out,
            jmp_op,
            jmp_src_select,
            flag_p: state.flag_p.out(),
            flag_z: state.flag_z.out(),
            flag_n: state.flag_n.out(),
        };
        let branch_out: CpuBranchOutput = Self::Branch::build(&branch_in);
        let CpuBranchOutput {
            pc_offset_enable,
            pc_offset,
            jmp_long_enable,
            jmp_long,
            flag_p,
            flag_z,
            flag_n,
        } = branch_out;

        // Next Pc
        let next_pc_in = CpuPcInput {
            curr_pc: state.pc.out,
            pc_offset_enable,
            pc_offset,
            jmp_long_enable,
            jmp_long,
        };
        let next_pc_out: CpuPcOutput = Self::Pc::build(&next_pc_in);

        // set regs
        state.pc.set_in(next_pc_out.next_pc);
        state.flag_p.set_in(flag_p);
        state.flag_z.set_in(flag_z);
        state.flag_n.set_in(flag_n);

        CpuV1StateInternal {
            inst_rom_in,
            inst_rom_out,
            next_pc_in,
            next_pc_out,
        }
    }
}

struct CpuV1Instance;
struct CpuV1EmuInstance;
impl CpuV1 for CpuV1Instance {
    type Pc = CpuPc;
    type InstRom = CpuInstRom;
    type Decoder = CpuDecoder;
    type Alu = CpuAlu;
    type Branch = CpuBranch;
}
impl CpuV1 for CpuV1EmuInstance {
    type Pc = CpuComponentEmuContext<CpuPc, CpuPcEmu>;
    type InstRom = CpuComponentEmuContext<CpuInstRom, CpuInstRomEmu>;
    type Decoder = CpuComponentEmuContext<CpuDecoder, CpuDecoderEmu>;
    type Alu = CpuAlu;
    type Branch = CpuBranch;
}

#[test]
fn test() {
    cpu_v1_build();
}

#[allow(unused)]
pub fn cpu_v1_build() {
    clear_all();

    let mut inst_rom = [0u8; 256];
    inst_rom[0] = inst_mov(0, 1).binary;
    inst_rom[1] = inst_add(0, 1).binary;
    inst_rom[2] = inst_inv(2).binary;

    let mut state1 = CpuV1State::create(inst_rom.clone());
    let mut state2 = CpuV1State::create(inst_rom.clone());
    let internal1 = CpuV1Instance::build(&mut state1);
    let internal2 = CpuV1EmuInstance::build(&mut state2);
    internal1.next_pc_in.curr_pc.set_u8(0);
    internal2.next_pc_in.curr_pc.set_u8(0);
    internal1.next_pc_in.pc_offset_enable.set(1);
    internal2.next_pc_in.pc_offset_enable.set(1);
    internal1.next_pc_in.pc_offset.set_u8(1);
    internal2.next_pc_in.pc_offset.set_u8(1);

    simulate();
    let inst1 = internal1.inst_rom_out.inst.get_u8();
    let inst2 = internal2.inst_rom_out.inst.get_u8();
    assert_eq!(inst1, inst2);
    let binary = InstDesc::parse(inst1).unwrap();
    println!("inst: {}", binary.desc.name());

    let next_pc1 = internal1.next_pc_out.next_pc.get_u8();
    let next_pc2 = internal2.next_pc_out.next_pc.get_u8();
    assert_eq!(next_pc1, next_pc2);

    simulate();
    let inst1 = internal1.inst_rom_out.inst.get_u8();
    let inst2 = internal2.inst_rom_out.inst.get_u8();
    assert_eq!(inst1, inst2);
    let binary = InstDesc::parse(inst1).unwrap();
    println!("inst: {}", binary.desc.name());

    let next_pc1 = internal1.next_pc_out.next_pc.get_u8();
    let next_pc2 = internal2.next_pc_out.next_pc.get_u8();
    assert_eq!(next_pc1, next_pc2);
}

pub trait CpuComponent: Any {
    type Input: Clone;
    type Output: Clone;
    fn build(input: &Self::Input) -> Self::Output;
}

pub trait CpuComponentEmu<T: CpuComponent>: Sized + Any {
    fn init_output() -> T::Output;
    fn execute(input: &T::Input, output: &T::Output);
    fn build(input: &T::Input) -> T::Output {
        let output = Self::init_output();
        let ctx: CpuComponentEmuContext<T, Self> = CpuComponentEmuContext {
            _phantom: Default::default(),
            input: input.clone(),
            output: output.clone(),
        };
        external(ctx);
        output
    }
}
struct CpuComponentEmuContext<T: CpuComponent, E: CpuComponentEmu<T>> {
    _phantom: PhantomData<E>,
    input: T::Input,
    output: T::Output,
}
impl<T: CpuComponent, E: CpuComponentEmu<T>> External for CpuComponentEmuContext<T, E> {
    fn execute(&mut self) {
        E::execute(&self.input, &mut self.output);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl<T: CpuComponent, E: CpuComponentEmu<T>> CpuComponent for CpuComponentEmuContext<T, E> {
    type Input = T::Input;
    type Output = T::Output;
    fn build(input: &Self::Input) -> Self::Output {
        E::build(input)
    }
}
