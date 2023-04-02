use crate::cpu_v1::decoder::Reg0WriteSelect;
use crate::cpu_v1::CpuComponent;
use crate::{decode2, mux2_w, reduce4, Regs, Wire, Wires};

#[derive(Clone)]
pub struct CpuRegReadInput {
    pub regs: [Regs<4>; 4],

    pub reg0_addr: Wires<2>, // RegAddr
    pub reg1_addr: Wires<2>, // RegAddr
}
#[derive(Clone)]
pub struct CpuRegReadOutput {
    pub reg0_data: Wires<4>,
    pub reg1_data: Wires<4>,

    pub reg0_select: Wires<4>,
}

pub struct CpuRegRead;
impl CpuComponent for CpuRegRead {
    type Input = CpuRegReadInput;
    type Output = CpuRegReadOutput;
    fn build(input: &Self::Input) -> Self::Output {
        let regs = input.regs;

        let port0_enable_each = decode2(input.reg0_addr);
        let port1_enable_each = decode2(input.reg1_addr);
        let port0_read_each = [0, 1, 2, 3].map(|i| port0_enable_each[i].expand() & regs[i].out);
        let port1_read_each = [0, 1, 2, 3].map(|i| port1_enable_each[i].expand() & regs[i].out);
        let port0_read = reduce4(port0_read_each.as_slice(), &|a, b| a | b);
        let port1_read = reduce4(port1_read_each.as_slice(), &|a, b| a | b);

        CpuRegReadOutput {
            reg0_data: port0_read,
            reg1_data: port1_read,
            reg0_select: Wires {
                wires: port0_enable_each,
            },
        }
    }
}

#[derive(Clone)]
pub struct CpuRegWriteInput {
    pub regs: [Regs<4>; 4],

    pub reg0_select: Wires<4>, // from CpuRegReadOutput
    pub reg0_write_enable: Wire,
    pub reg0_write_select: Wires<2>, // Reg0WriteSelect: alu out, mem out

    pub mem_out: Wires<4>,
    pub alu_out: Wires<4>,
}
#[derive(Clone)]
pub struct CpuRegWriteOutput {
    // ?
}

pub struct CpuRegWrite;
impl CpuComponent for CpuRegWrite {
    type Input = CpuRegWriteInput;
    type Output = CpuRegWriteOutput;
    fn build(input: &Self::Input) -> Self::Output {
        let regs = input.regs;

        let select_alu = input.reg0_write_select.wires[Reg0WriteSelect::AluOut as usize];
        let select_mem = input.reg0_write_select.wires[Reg0WriteSelect::MemOut as usize];
        let write_data_alu = select_alu.expand() & input.alu_out;
        let write_data_mem = select_mem.expand() & input.mem_out;
        let write_data = write_data_mem | write_data_alu;

        for i in 0..4 {
            let reg = regs[i];
            let prev = reg.out;
            let write_enable = input.reg0_select.wires[i] & input.reg0_write_enable;
            let write_data = mux2_w(prev, write_data, write_enable);
            regs[i].set_in(write_data);
        }

        CpuRegWriteOutput {}
    }
}
