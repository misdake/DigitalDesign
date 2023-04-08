use crate::cpu_v1::decoder::MemAddrSelect;
use crate::cpu_v1::CpuComponent;
use crate::{decode8, flatten2, mux2_w, reduce256, Regs, Wire, Wires};

#[derive(Clone)]
pub struct CpuMemInput {
    pub mem: [Regs<4>; 256],
    // bank
    pub mem_bank: Regs<4>,
    pub mem_bank_write_enable: Wire,
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

        let next_bank = mux2_w(input.mem_bank.out, input.reg0, input.mem_bank_write_enable);
        input.mem_bank.set_in(next_bank);

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
    let mem_bank_write_enable = input();

    let input = CpuMemInput {
        mem,
        mem_bank,
        mem_bank_write_enable,
        reg0,
        mem_write_enable,
        imm,
        reg1,
        mem_addr_select,
    };
    let CpuMemOutput { mem_out } = CpuMem::build(&input);

    let mut bank_sw = 0u8;
    let mut mem_sw = [[0u8; 16]; 16];
    let load_mem_imm = |bank_sw: &mut u8, mem_sw: &mut [[u8; 16]; 16], i: u8| {
        println!("load_mem_imm {i}");
        imm.set_u8(i);
        mem_addr_select.set_u8(1 << MemAddrSelect::Imm as u8);
        mem_write_enable.set(0);
        mem_bank_write_enable.set(0);
        execute_gates();
        Some(mem_sw[*bank_sw as usize][i as usize])
    };
    let load_mem_reg = |bank_sw: &mut u8, mem_sw: &mut [[u8; 16]; 16], a: u8| {
        println!("load_mem_reg {a}");
        reg1.set_u8(a);
        mem_addr_select.set_u8(1 << MemAddrSelect::Reg1 as u8);
        mem_write_enable.set(0);
        mem_bank_write_enable.set(0);
        execute_gates();
        Some(mem_sw[*bank_sw as usize][a as usize])
    };
    let store_mem_imm = |bank_sw: &mut u8, mem_sw: &mut [[u8; 16]; 16], i: u8, d: u8| {
        println!("store_mem_imm {i} {d}");
        reg0.set_u8(d);
        imm.set_u8(i);
        mem_addr_select.set_u8(1 << MemAddrSelect::Imm as u8);
        mem_write_enable.set(1);
        mem_bank_write_enable.set(0);
        execute_gates();
        mem_sw[*bank_sw as usize][i as usize] = d;
        None
    };
    let store_mem_reg = |bank_sw: &mut u8, mem_sw: &mut [[u8; 16]; 16], a: u8, d: u8| {
        println!("store_mem_reg {a} {d}");
        reg0.set_u8(d);
        reg1.set_u8(a);
        mem_addr_select.set_u8(1 << MemAddrSelect::Reg1 as u8);
        mem_write_enable.set(1);
        mem_bank_write_enable.set(0);
        execute_gates();
        mem_sw[*bank_sw as usize][a as usize] = d;
        None
    };
    let set_bank = |bank_sw: &mut u8, d: u8| {
        println!("set_bank {d}");
        reg0.set_u8(d);
        reg0.set_u8(d);
        mem_addr_select.set_u8(1 << MemAddrSelect::Reg1 as u8);
        mem_write_enable.set(0);
        mem_bank_write_enable.set(1);
        execute_gates();
        *bank_sw = d;
        None
    };

    for i in 0..16 {
        set_bank(&mut bank_sw, i);
        clock_tick();
        for j in 0..16 {
            store_mem_imm(&mut bank_sw, &mut mem_sw, j, (i + j) % 16);
            clock_tick();
        }
    }

    let testcases = shuffled_list(1 << 9, 1.1);
    for t in testcases {
        let v0 = (t % 16) as u8;
        let v1 = ((t >> 4) % 16) as u8;
        let v2 = ((t >> 3) % 7) as u8;

        let mem_out_sw = match v2 {
            0 => load_mem_imm(&mut bank_sw, &mut mem_sw, v0),
            1 => load_mem_reg(&mut bank_sw, &mut mem_sw, v0),
            2 => store_mem_imm(&mut bank_sw, &mut mem_sw, v0, v1),
            3 => store_mem_reg(&mut bank_sw, &mut mem_sw, v0, v1),
            4 => load_mem_imm(&mut bank_sw, &mut mem_sw, v0),
            5 => store_mem_imm(&mut bank_sw, &mut mem_sw, v0, v1),
            6 => set_bank(&mut bank_sw, v0),
            _ => unreachable!(),
        };
        if let Some(mem_out_sw) = mem_out_sw {
            println!("ref {}, out {}", mem_out_sw, mem_out.get_u8());
            assert_eq!(mem_out_sw, mem_out.get_u8());
        }
        clock_tick();
    }

    for i in 0..256 {
        let sw = mem_sw[i / 16][i % 16];
        let hw = mem[i].out.get_u8();
        println!("mem {i}: sw {sw}, hw {hw}");
        assert_eq!(sw, hw);
    }
}
