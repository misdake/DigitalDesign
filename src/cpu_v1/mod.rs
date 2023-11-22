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
mod regfile;
use regfile::*;
mod mem;
use mem::*;
mod bus;
use bus::*;
mod isa;
use isa::*;
mod emu;
// use emu::*;

mod devices;
use devices::*;
mod assembler;
use assembler::*;
#[cfg(test)]
mod programs;

use crate::{clear_all, external, reg, reg_w, External, Reg, Regs, Wires};
use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

#[allow(unused)]
struct CpuV1State {
    inst_src: [Instruction; 256],
    inst: [Wires<8>; 256],
    pc: Regs<8>,       // write in CpuV1
    reg: [Regs<4>; 4], // write in RegWrite
    mem: [Regs<4>; 256],
    mem_page: Regs<4>,
    flag_p: Reg,  // write in CpuV1
    flag_nz: Reg, // write in CpuV1
    flag_n: Reg,  // write in CpuV1
    bus_addr0: Regs<4>,
    bus_addr1: Regs<4>,
    devices: Rc<RefCell<Devices>>,
}
impl CpuV1State {
    fn create(inst_src: [Instruction; 256]) -> Self {
        let inst = inst_src.map(|v| Wires::<8>::parse_u8(v.to_binary()));
        let regs = [0u8; 4].map(|_| reg_w());
        let mem = [0u8; 256].map(|_| reg_w());
        Self {
            inst_src,
            inst,
            pc: reg_w(),
            mem,
            mem_page: reg_w(),
            reg: regs,
            flag_p: reg(),
            flag_nz: reg(),
            flag_n: reg(),
            bus_addr0: reg_w(),
            bus_addr1: reg_w(),
            devices: Rc::new(RefCell::new(Devices::new())),
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
    type Bus: CpuComponent<Input = CpuBusInput, Output = CpuBusOutput>;

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
            mem_page_write_enable,
            jmp_op,
            jmp_src_select,
            bus_enable,
            bus_addr0_write,
            bus_addr1_write,
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

        let bus_in = CpuBusInput {
            bus_addr0_write,
            bus_addr1_write,
            bus_enable,
            bus_addr0: state.bus_addr0.out,
            bus_addr1: state.bus_addr1.out,
            reg0_data,
            reg1_data,
            imm,
            devices: state.devices.clone(),
        };
        let bus_out: CpuBusOutput = Self::Bus::build(&bus_in);
        let CpuBusOutput {
            bus_out,
            bus_addr0_next,
            bus_addr1_next,
        } = bus_out;
        state.bus_addr0.set_in(bus_addr0_next);
        state.bus_addr1.set_in(bus_addr1_next);

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
            mem: state.mem.map(|v| v.out),
            mem_page: state.mem_page.out,
            reg0: reg0_data,
            mem_write_enable,
            mem_page_write_enable,
            imm,
            reg1: reg1_data,
            mem_addr_select,
        };
        let mem_out = Self::Mem::build(&mem_in);
        let CpuMemOutput {
            mem_out,
            mem_next,
            mem_page_next,
        } = mem_out;
        for i in 0..256 {
            state.mem[i].set_in(mem_next[i]);
        }
        state.mem_page.set_in(mem_page_next);

        // RegWrite
        let reg_write_in = CpuRegWriteInput {
            regs: state.reg,
            reg0_select,
            reg0_write_enable,
            reg0_write_select,
            alu_out,
            mem_out,
            bus_out,
        };
        let reg_write_out = Self::RegWrite::build(&reg_write_in);
        let CpuRegWriteOutput { reg0_write_data } = reg_write_out;

        // Branch
        let branch_in = CpuBranchInput {
            imm,
            reg0: reg0_data,
            reg0_write_enable,
            reg0_write_data,
            jmp_op,
            jmp_src_select,
            flag_p: state.flag_p.out(),
            flag_nz: state.flag_nz.out(),
            flag_n: state.flag_n.out(),
        };
        let branch_out: CpuBranchOutput = Self::Branch::build(&branch_in);
        let CpuBranchOutput {
            pc_offset_enable,
            pc_offset,
            jmp_long_enable,
            jmp_long,
            flag_p,
            flag_nz,
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
        state.flag_nz.set_in(flag_nz);
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
struct CpuV1MixInstance;
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
    type Bus = CpuComponentEmuContext<CpuBus, CpuBusEmu>;
}
impl CpuV1 for CpuV1MixInstance {
    type Pc = CpuPc;
    type InstRom = CpuComponentEmuContext<CpuInstRom, CpuInstRomEmu>;
    type Decoder = CpuDecoder;
    type Alu = CpuAlu;
    type Branch = CpuBranch;
    type RegRead = CpuRegRead;
    type RegWrite = CpuRegWrite;
    type Mem = CpuComponentEmuContext<CpuMem, CpuMemEmu>;
    type Bus = CpuComponentEmuContext<CpuBus, CpuBusEmu>;
}
impl CpuV1 for CpuV1EmuInstance {
    type Pc = CpuComponentEmuContext<CpuPc, CpuPcEmu>;
    type InstRom = CpuComponentEmuContext<CpuInstRom, CpuInstRomEmu>;
    type Decoder = CpuComponentEmuContext<CpuDecoder, CpuDecoderEmu>;
    type Alu = CpuAlu;
    type Branch = CpuBranch;
    type RegRead = CpuRegRead;
    type RegWrite = CpuRegWrite;
    type Mem = CpuComponentEmuContext<CpuMem, CpuMemEmu>;
    type Bus = CpuComponentEmuContext<CpuBus, CpuBusEmu>;
}

#[allow(unused)]
fn cpu_v1_build_with_ref(
    inst_rom: [Instruction; 256],
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
    println!("cpu_v1_build_with_ref {:?}", crate::get_statistics());
    (state1, state2, internal1, internal2)
}
#[allow(unused)]
fn cpu_v1_build(inst_rom: [Instruction; 256]) -> (CpuV1State, CpuV1StateInternal) {
    clear_all();
    let mut state1 = CpuV1State::create(inst_rom.clone());
    let internal1 = CpuV1Instance::build(&mut state1);
    println!("cpu_v1_build {:?}", crate::get_statistics());
    (state1, internal1)
}
#[allow(unused)]
fn cpu_v1_build_mix(inst_rom: [Instruction; 256]) -> (CpuV1State, CpuV1StateInternal) {
    clear_all();
    let mut state1 = CpuV1State::create(inst_rom.clone());
    let internal1 = CpuV1MixInstance::build(&mut state1);
    println!("cpu_v1_build_mix {:?}", crate::get_statistics());
    (state1, internal1)
}

pub trait CpuComponent: Any {
    type Input: Clone;
    type Output: Clone;
    fn build(input: &Self::Input) -> Self::Output;
}

pub trait CpuComponentEmu<T: CpuComponent>: Sized + Any {
    fn init_output(input: &T::Input) -> T::Output;
    fn execute(input: &T::Input, output: &T::Output);
    fn build(input: &T::Input) -> T::Output {
        let output = Self::init_output(input);
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
