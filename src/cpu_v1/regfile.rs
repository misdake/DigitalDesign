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

    pub alu_out: Wires<4>,
    pub mem_out: Wires<4>,
    // TODO bus_out
}
#[derive(Clone)]
pub struct CpuRegWriteOutput {
    // TODO written data or 0, to be used by branch
}

pub struct CpuRegWrite;
impl CpuComponent for CpuRegWrite {
    type Input = CpuRegWriteInput;
    type Output = CpuRegWriteOutput;
    fn build(input: &Self::Input) -> Self::Output {
        let regs = input.regs;

        let select_alu = input.reg0_write_select.wires[Reg0WriteSelect::AluOut as usize];
        let select_mem = input.reg0_write_select.wires[Reg0WriteSelect::MemOut as usize];
        // TODO BusOut
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

#[test]
fn test_reg() {
    use crate::*;
    clear_all();

    let regs = [0u8; 4].map(|_| reg_w());

    let reg0_addr = input_w();
    let reg1_addr = input_w();
    let reg0_write_enable = input();
    let reg0_write_select = input_w();
    let alu_out = input_w();
    let mem_out = input_w();

    let (reg0_data, reg1_data) = {
        let read_input = CpuRegReadInput {
            regs,
            reg0_addr,
            reg1_addr,
        };
        let CpuRegReadOutput {
            reg0_data,
            reg1_data,
            reg0_select,
        } = CpuRegRead::build(&read_input);
        let write_input = CpuRegWriteInput {
            regs,
            reg0_select,
            reg0_write_enable,
            reg0_write_select,
            alu_out,
            mem_out,
        };
        let CpuRegWriteOutput {} = CpuRegWrite::build(&write_input);
        (reg0_data, reg1_data)
    };

    let mut regs_sw = [0u8; 4];

    let mut test =
        |reg0: u8, reg1: u8, write: bool, src: Reg0WriteSelect, alu_value: u8, mem_value: u8| {
            // input
            reg0_addr.set_u8(reg0);
            reg1_addr.set_u8(reg1);
            reg0_write_select.set_u8(1 << src as u8);
            reg0_write_enable.set(write as u8);
            alu_out.set_u8(alu_value);
            mem_out.set_u8(mem_value);

            simulate();

            let (reg0_data_ref, reg1_data_ref) = {
                let reg0_data = regs_sw[reg0 as usize];
                let reg1_data = regs_sw[reg1 as usize];
                if write {
                    let write_data = match src {
                        Reg0WriteSelect::AluOut => alu_value,
                        Reg0WriteSelect::MemOut => mem_value,
                        // TODO BusOut
                    };
                    regs_sw[reg0 as usize] = write_data;
                }
                (reg0_data, reg1_data)
            };
            //         println!("test reg0 {reg0}, reg1 {reg1}, write {write}, src {src:?}, alu {alu_value}, mem {mem_value} => ref {}|{}, ref {}|{}", reg0_data_ref, reg0_data.get_u8(),
            // reg1_data_ref, reg1_data.get_u8());
            //         println!(
            //             "regs {} {} {} {}",
            //             regs[0].out.get_u8(),
            //             regs[1].out.get_u8(),
            //             regs[2].out.get_u8(),
            //             regs[3].out.get_u8()
            //         );
            //         println!(
            //             "regs_sw {} {} {} {}",
            //             regs_sw[0], regs_sw[1], regs_sw[2], regs_sw[3]
            //         );

            assert_eq!(reg0_data_ref, reg0_data.get_u8());
            assert_eq!(reg1_data_ref, reg1_data.get_u8());
            assert_eq!(regs_sw[0], regs[0].out.get_u8());
            assert_eq!(regs_sw[1], regs[1].out.get_u8());
            assert_eq!(regs_sw[2], regs[2].out.get_u8());
            assert_eq!(regs_sw[3], regs[3].out.get_u8());
        };

    let testcases = shuffled_list(1 << 9, 0.123);
    for i in testcases {
        let reg0 = ((i >> 0) % (1 << 2)) as u8;
        let reg1 = ((i >> 2) % (1 << 2)) as u8;
        let write = (i >> 4) % (1 << 1) > 0;
        let src = if (i >> 5) % (1 << 1) > 0 {
            Reg0WriteSelect::AluOut
        } else {
            Reg0WriteSelect::MemOut
        };
        let alu_value = ((i >> 1) % (1 << 4)) as u8;
        let mem_value = ((i >> 3) % (1 << 4)) as u8;

        test(reg0, reg1, write, src, alu_value, mem_value);
    }
}
