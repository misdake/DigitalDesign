mod inst_rom;
mod isa;
use inst_rom::*;
mod pc;
use pc::*;
mod branch;
use branch::*;
mod decoder;
use decoder::*;
mod alu;
use alu::*;
mod regfile;
use regfile::*;
mod mem;
use mem::*;

#[cfg(test)]
mod programs;

use crate::{clear_all, external, reg, reg_w, External, Reg, Regs, Wires};
use std::any::Any;
use std::marker::PhantomData;

#[allow(unused)]
struct CpuV1State {
    clock_enable: Reg, // TODO impl
    inst: [Wires<8>; 256],
    pc: Regs<8>,              // write in CpuV1
    reg: [Regs<4>; 4],        // write in RegWrite
    mem: [Regs<4>; 256],      // write in Mem
    mem_bank: Regs<4>,        // TODO impl write in Mem
    flag_p: Reg,              // write in CpuV1
    flag_z: Reg,              // write in CpuV1
    flag_n: Reg,              // write in CpuV1
    external_device: Regs<4>, // TODO impl
}
impl CpuV1State {
    fn create(inst: [u8; 256]) -> Self {
        let inst = inst.map(|v| Wires::<8>::parse_u8(v));
        let regs = [0u8; 4].map(|_| reg_w());
        let mem = [0u8; 256].map(|_| reg_w());
        Self {
            clock_enable: reg(),
            inst,
            pc: reg_w(),
            mem,
            mem_bank: reg_w(),
            reg: regs,
            flag_p: reg(),
            flag_z: reg(),
            flag_n: reg(),
            external_device: reg_w(),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(unused)] // internal struct for debugging and testing
struct CpuV1StateInternal {
    decoder_in: CpuDecoderInput,
    branch_in: CpuBranchInput,
    next_pc_in: CpuPcInput,
    next_pc_out: CpuPcOutput,
}

trait CpuV1 {
    type Pc: CpuComponent<Input = CpuPcInput, Output = CpuPcOutput>;
    type InstRom: CpuComponent<Input = CpuInstInput, Output = CpuInstOutput>;
    type Decoder: CpuComponent<Input = CpuDecoderInput, Output = CpuDecoderOutput>;
    type Alu: CpuComponent<Input = CpuAluInput, Output = CpuAluOutput>;
    type Branch: CpuComponent<Input = CpuBranchInput, Output = CpuBranchOutput>;
    type RegRead: CpuComponent<Input = CpuRegReadInput, Output = CpuRegReadOutput>;
    type RegWrite: CpuComponent<Input = CpuRegWriteInput, Output = CpuRegWriteOutput>;
    type Mem: CpuComponent<Input = CpuMemInput, Output = CpuMemOutput>;

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

        // RegRead
        let reg_read_in = CpuRegReadInput {
            regs: state.reg,
            reg0_addr,
            reg1_addr,
        };
        let reg_read_out: CpuRegReadOutput = Self::RegRead::build(&reg_read_in);
        let CpuRegReadOutput {
            reg0_data,
            reg1_data,
            reg0_select,
        } = reg_read_out;

        // Alu
        let alu_in = CpuAluInput {
            reg0_data,
            reg1_data,
            imm,
            alu_op,
            alu0_select,
            alu1_select,
        };
        let alu_out = Self::Alu::build(&alu_in);
        let CpuAluOutput { alu_out } = alu_out;

        // Mem
        let mem_in = CpuMemInput {
            mem: state.mem,
            mem_bank: state.mem_bank,
            reg0: reg0_data,
            mem_write_enable,
            imm,
            reg1: reg1_data,
            mem_addr_select,
        };
        let mem_out = Self::Mem::build(&mem_in);
        let CpuMemOutput { mem_out } = mem_out;

        // RegWrite
        let reg_write_in = CpuRegWriteInput {
            regs: state.reg,
            reg0_select,
            reg0_write_enable,
            reg0_write_select,
            alu_out,
            mem_out,
        };
        let reg_write_out = Self::RegWrite::build(&reg_write_in);
        let CpuRegWriteOutput {} = reg_write_out;

        // Branch
        let branch_in = CpuBranchInput {
            imm,
            reg0: reg0_data,
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
            decoder_in,
            branch_in,
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
    type RegRead = CpuRegRead;
    type RegWrite = CpuRegWrite;
    type Mem = CpuMem;
}
impl CpuV1 for CpuV1EmuInstance {
    type Pc = CpuComponentEmuContext<CpuPc, CpuPcEmu>;
    type InstRom = CpuComponentEmuContext<CpuInstRom, CpuInstRomEmu>;
    type Decoder = CpuComponentEmuContext<CpuDecoder, CpuDecoderEmu>;
    type Alu = CpuAlu;
    type Branch = CpuBranch;
    type RegRead = CpuRegRead;
    type RegWrite = CpuRegWrite;
    type Mem = CpuMem;
}

#[allow(unused)]
fn cpu_v1_build(
    inst_rom: [u8; 256],
) -> (
    CpuV1State,
    CpuV1State,
    CpuV1StateInternal,
    CpuV1StateInternal,
) {
    clear_all();
    let mut state1 = CpuV1State::create(inst_rom.clone());
    let mut state2 = CpuV1State::create(inst_rom.clone());
    let internal1 = CpuV1Instance::build(&mut state1);
    let internal2 = CpuV1EmuInstance::build(&mut state2);
    (state1, state2, internal1, internal2)
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
