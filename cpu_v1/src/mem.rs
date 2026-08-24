use crate::decoder::MemAddrSelect;
use crate::{CpuComponent, CpuComponentEmu};
use digital_design_code::*;

#[derive(Clone)]
pub struct CpuMemInput {
    pub mem: [Wires<4>; 256],
    // mem_page
    pub mem_page: Wires<4>,
    pub mem_page_write_enable: Wire,
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
    // read mem
    pub mem_out: Wires<4>,
    // write regs
    pub mem_next: [Wires<4>; 256],
    pub mem_page_next: Wires<4>,
}

pub struct CpuMem;
impl CpuComponent for CpuMem {
    type Input = CpuMemInput;
    type Output = CpuMemOutput;

    fn build(input: &CpuMemInput) -> CpuMemOutput {
        let mem_array = input.mem;
        let mem_data = mem_array.as_slice();

        let use_imm = input.mem_addr_select.wires[MemAddrSelect::Imm as usize];
        let use_reg1 = input.mem_addr_select.wires[MemAddrSelect::Reg1 as usize];
        let addr_low = (use_imm.expand() & input.imm) | (use_reg1.expand() & input.reg1);
        let addr_high = input.mem_page;
        let addr = flatten2(addr_low, addr_high);

        let enable_line = decode8(addr);
        let mut lines: [Wires<4>; 256] = [Wires::uninitialized(); 256];
        let mut mem_next: [Wires<4>; 256] = [Wires::uninitialized(); 256];
        for i in 0..256 {
            lines[i] = enable_line[i].expand() & mem_data[i];

            let write_enable = enable_line[i] & input.mem_write_enable;
            let write_data = mux2_w(mem_data[i], input.reg0, write_enable);
            mem_next[i] = write_data;
        }
        let mem_out = reduce256(lines.as_slice(), &|a, b| a | b);

        let mem_page_next = mux2_w(input.mem_page, input.reg0, input.mem_page_write_enable);

        CpuMemOutput {
            mem_out,
            mem_next,
            mem_page_next,
        }
    }
}

pub struct CpuMemEmu;
impl CpuComponentEmu<CpuMem> for CpuMemEmu {
    fn create(i: &CpuMemInput) -> (Self, CpuMemOutput) {
        let output = CpuMemOutput {
            mem_out: input_w(),
            mem_next: [0; 256].map(|_| input_w()),
            mem_page_next: input_w(),
        };
        //TODO accurate latency
        output
            .mem_out
            .set_latency_external(i.imm.get_max_latency_external() + 10);
        output
            .mem_next
            .into_iter()
            .for_each(|mem| mem.set_latency_external(i.reg0.get_max_latency_external() + 1));
        output
            .mem_page_next
            .set_latency_external(i.reg0.get_max_latency_external() + 1);
        (Self, output)
    }
    fn execute(&mut self, c: &mut CircuitWires, input: &CpuMemInput, output: &CpuMemOutput) {
        let mem_page = input.mem_page.get_u8(c);
        let imm = input.imm.get_u8(c);
        let reg0 = input.reg0.get_u8(c);
        let reg1 = input.reg1.get_u8(c);

        let addr_imm = input.mem_addr_select.wires[MemAddrSelect::Imm as usize].get(c);
        let addr_reg1 = input.mem_addr_select.wires[MemAddrSelect::Reg1 as usize].get(c);

        if input.mem_page_write_enable.get(c) > 0 {
            output.mem_page_next.set_u8(c, reg0);
        } else {
            output.mem_page_next.set_u8(c, mem_page);
        }

        let addr_low = imm * addr_imm + reg1 * addr_reg1;
        let addr = addr_low + (mem_page << 4);

        output.mem_out.set_u8(c, input.mem[addr as usize].get_u8(c));
        (0..256).for_each(|i| {
            output.mem_next[i].set_u8(c, input.mem[i].get_u8(c));
        });
        if input.mem_write_enable.get(c) > 0 {
            output.mem_next[addr as usize].set_u8(c, reg0);
        }
    }
}

#[test]
fn test_mem() {
    let (
        mut c,
        (
            mem,
            _mem_page,
            imm,
            reg0,
            reg1,
            mem_addr_select,
            mem_write_enable,
            mem_page_write_enable,
            mem_out,
        ),
    ) = build_circuit(|| {
        let mem = [0u8; 256].map(|_| reg_w());
        let mem_page = reg_w();

        let imm = input_w();
        let reg0 = input_w();
        let reg1 = input_w();
        let mem_addr_select = input_w();
        let mem_write_enable = input();
        let mem_page_write_enable = input();

        let input = CpuMemInput {
            mem: mem.map(|v| v.out),
            mem_page: mem_page.out,
            mem_page_write_enable,
            reg0,
            mem_write_enable,
            imm,
            reg1,
            mem_addr_select,
        };
        let CpuMemOutput {
            mem_out,
            mem_next,
            mem_page_next,
        } = CpuMem::build(&input);
        for i in 0..256 {
            mem[i].set_in(mem_next[i]);
        }
        mem_page.set_in(mem_page_next);

        (
            mem,
            mem_page,
            imm,
            reg0,
            reg1,
            mem_addr_select,
            mem_write_enable,
            mem_page_write_enable,
            mem_out,
        )
    });
    let c = &mut c;

    let mut page_sw = 0u8;
    let mut mem_sw = [[0u8; 16]; 16];
    let load_mem_imm =
        |c: &mut CircuitWires, page_sw: &mut u8, mem_sw: &mut [[u8; 16]; 16], i: u8| {
            println!("load_mem_imm {i}");
            imm.set_u8(c, i);
            mem_addr_select.set_u8(c, 1 << MemAddrSelect::Imm as u8);
            mem_write_enable.set(c, 0);
            mem_page_write_enable.set(c, 0);
            Some(mem_sw[*page_sw as usize][i as usize])
        };
    let load_mem_reg =
        |c: &mut CircuitWires, page_sw: &mut u8, mem_sw: &mut [[u8; 16]; 16], a: u8| {
            println!("load_mem_reg {a}");
            reg1.set_u8(c, a);
            mem_addr_select.set_u8(c, 1 << MemAddrSelect::Reg1 as u8);
            mem_write_enable.set(c, 0);
            mem_page_write_enable.set(c, 0);
            Some(mem_sw[*page_sw as usize][a as usize])
        };
    let store_mem_imm =
        |c: &mut CircuitWires, page_sw: &mut u8, mem_sw: &mut [[u8; 16]; 16], i: u8, d: u8| {
            println!("store_mem_imm {i} {d}");
            reg0.set_u8(c, d);
            imm.set_u8(c, i);
            mem_addr_select.set_u8(c, 1 << MemAddrSelect::Imm as u8);
            mem_write_enable.set(c, 1);
            mem_page_write_enable.set(c, 0);
            mem_sw[*page_sw as usize][i as usize] = d;
            None
        };
    let store_mem_reg =
        |c: &mut CircuitWires, page_sw: &mut u8, mem_sw: &mut [[u8; 16]; 16], a: u8, d: u8| {
            println!("store_mem_reg {a} {d}");
            reg0.set_u8(c, d);
            reg1.set_u8(c, a);
            mem_addr_select.set_u8(c, 1 << MemAddrSelect::Reg1 as u8);
            mem_write_enable.set(c, 1);
            mem_page_write_enable.set(c, 0);
            mem_sw[*page_sw as usize][a as usize] = d;
            None
        };
    let set_page = |c: &mut CircuitWires, page_sw: &mut u8, d: u8| {
        println!("set_page {d}");
        reg0.set_u8(c, d);
        reg0.set_u8(c, d);
        mem_addr_select.set_u8(c, 1 << MemAddrSelect::Reg1 as u8);
        mem_write_enable.set(c, 0);
        mem_page_write_enable.set(c, 1);
        *page_sw = d;
        None
    };

    for i in 0..16 {
        set_page(c, &mut page_sw, i);
        c.simulate();
        for j in 0..16 {
            store_mem_imm(c, &mut page_sw, &mut mem_sw, j, (i + j) % 16);
            c.simulate();
        }
    }

    let testcases = shuffled_list(1 << 9, 1.1);
    for t in testcases {
        let v0 = (t % 16) as u8;
        let v1 = ((t >> 4) % 16) as u8;
        let v2 = ((t >> 3) % 7) as u8;

        let mem_out_sw = match v2 {
            0 => load_mem_imm(c, &mut page_sw, &mut mem_sw, v0),
            1 => load_mem_reg(c, &mut page_sw, &mut mem_sw, v0),
            2 => store_mem_imm(c, &mut page_sw, &mut mem_sw, v0, v1),
            3 => store_mem_reg(c, &mut page_sw, &mut mem_sw, v0, v1),
            4 => load_mem_imm(c, &mut page_sw, &mut mem_sw, v0),
            5 => store_mem_imm(c, &mut page_sw, &mut mem_sw, v0, v1),
            6 => set_page(c, &mut page_sw, v0),
            _ => unreachable!(),
        };
        c.simulate();
        if let Some(mem_out_sw) = mem_out_sw {
            println!("ref {}, out {}", mem_out_sw, mem_out.get_u8(c));
            assert_eq!(mem_out_sw, mem_out.get_u8(c));
        }
    }

    for i in 0..256 {
        let sw = mem_sw[i / 16][i % 16];
        let hw = mem[i].out.get_u8(c);
        println!("mem {i}: sw {sw}, hw {hw}");
        assert_eq!(sw, hw);
    }
}
