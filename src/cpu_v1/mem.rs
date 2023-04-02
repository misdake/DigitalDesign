use crate::cpu_v1::decoder::MemAddrSelect;
use crate::cpu_v1::CpuComponent;
use crate::{decode8, flatten2, mux2_w, reduce256, Regs, Wire, Wires};

#[derive(Clone)]
pub struct CpuMemInput {
    pub mem: [Regs<4>; 256],
    // bank
    pub mem_bank: Regs<4>,
    // TODO set mem_bank_write_enable
    // write
    pub reg0: Wires<4>,
    pub mem_write_enable: Wire,
    // addr
    pub imm: Wires<4>,
    pub reg1: Wires<4>,
    pub mem_addr_select: Wires<2>, // MemAddrSelect: imm, reg1
}
#[derive(Clone)]
pub struct CpuMemOutput {
    pub mem_out: Wires<4>,
}

pub struct CpuMem;
impl CpuComponent for CpuMem {
    type Input = CpuMemInput;
    type Output = CpuMemOutput;

    fn build(input: &CpuMemInput) -> CpuMemOutput {
        let mem_array = input.mem;
        let mem_data = mem_array.map(|i| i.out);
        let mem_data = mem_data.as_slice();

        let use_imm = input.mem_addr_select.wires[MemAddrSelect::Imm as usize];
        let use_reg1 = input.mem_addr_select.wires[MemAddrSelect::Reg1 as usize];
        let addr_low = (use_imm.expand() & input.imm) | (use_reg1.expand() & input.reg1);
        let addr_high = input.mem_bank.out;
        let addr: Wires<8> = flatten2(addr_low, addr_high);

        let enable_line = decode8(addr);
        let mut lines: [Wires<4>; 256] = [Wires::uninitialized(); 256];
        for i in 0..256 {
            lines[i] = enable_line[i].expand() & mem_data[i];

            let write_enable = enable_line[i] & input.mem_write_enable;
            let write_data = mux2_w(mem_data[i], input.reg0, write_enable);
            mem_array[i].set_in(write_data);
        }
        let output = reduce256(lines.as_slice(), &|a, b| a | b);

        CpuMemOutput { mem_out: output }
    }
}

#[test]
fn test_mem() {
    use crate::*;
    clear_all();

    let mem = [0u8; 256].map(|_| reg_w());
    let mem_bank = reg_w();

    let imm = input_w();
    let reg0 = input_w();
    let reg1 = input_w();
    let mem_addr_select = input_w();
    let mem_write_enable = input();

    let input = CpuMemInput {
        mem,
        mem_bank,
        reg0,
        mem_write_enable,
        imm,
        reg1,
        mem_addr_select,
    };
    let CpuMemOutput { mem_out } = CpuMem::build(&input);

    // TODO test mem_bank
    let mut mem_sw = [0u8; 256];
    let load_mem_imm = |mem_sw: &mut [u8; 256], i: u8| {
        // println!("load_mem_imm {i}");
        imm.set_u8(i);
        mem_addr_select.set_u8(1 << MemAddrSelect::Imm as u8);
        mem_write_enable.set(0);
        execute_gates();
        mem_sw[i as usize]
    };
    let load_mem_reg = |mem_sw: &mut [u8; 256], a: u8| {
        // println!("load_mem_reg {a}");
        reg1.set_u8(a);
        mem_addr_select.set_u8(1 << MemAddrSelect::Reg1 as u8);
        mem_write_enable.set(0);
        execute_gates();
        mem_sw[a as usize]
    };
    let store_mem_imm = |mem_sw: &mut [u8; 256], i: u8, d: u8| {
        // println!("store_mem_imm {i} {d}");
        reg0.set_u8(d);
        imm.set_u8(i);
        mem_addr_select.set_u8(1 << MemAddrSelect::Imm as u8);
        mem_write_enable.set(1);
        execute_gates();
        let r = mem_sw[i as usize];
        mem_sw[i as usize] = d;
        r
    };
    let store_mem_reg = |mem_sw: &mut [u8; 256], a: u8, d: u8| {
        // println!("store_mem_reg {a} {d}");
        reg0.set_u8(d);
        reg1.set_u8(a);
        mem_addr_select.set_u8(1 << MemAddrSelect::Reg1 as u8);
        mem_write_enable.set(1);
        execute_gates();
        let r = mem_sw[a as usize];
        mem_sw[a as usize] = d;
        r
    };

    for i in 0..16 {
        store_mem_imm(&mut mem_sw, i, i);
        clock_tick();
    }

    let testcases = shuffled_list(1 << 8, 1.1);
    for t in testcases {
        let v0 = (t % 16) as u8;
        let v1 = ((t >> 4) % 16) as u8;
        let v2 = ((t >> 3) % 4) as u8;
        let mem_out_sw = match v2 {
            0 => load_mem_imm(&mut mem_sw, v0),
            1 => load_mem_reg(&mut mem_sw, v0),
            2 => store_mem_imm(&mut mem_sw, v0, v1),
            3 => store_mem_reg(&mut mem_sw, v0, v1),
            _ => unreachable!(),
        };
        // println!("ref {}, out {}", mem_out_sw, mem_out.get_u8());
        assert_eq!(mem_out_sw, mem_out.get_u8());
        clock_tick();

        for i in 0..16 {
            assert_eq!(mem_sw[i], mem[i].out.get_u8());
        }
    }
}
