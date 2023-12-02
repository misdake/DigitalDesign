#![allow(unused)]

use crate::devices::Devices;
use crate::isa::{Instruction, RegisterIndex};
use crate::CpuV1State;
use std::cell::RefCell;
use std::rc::Rc;

pub struct EmuEnv {
    inst: [Instruction; 256],
    state: EmuState,
    device: Rc<RefCell<Devices>>,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct EmuState {
    pc: u8,
    reg: [u8; 4],
    mem: [u8; 256],
    mem_page: u8,
    flag_p: u8,
    flag_nz: u8,
    flag_n: u8,
    bus_addr0: u8,
    bus_addr1: u8,
}
impl Default for EmuState {
    fn default() -> Self {
        Self {
            pc: 0,
            reg: [0; 4],
            mem: [0; 256],
            mem_page: 0,
            flag_p: 0,
            flag_nz: 0,
            flag_n: 0,
            bus_addr0: 0,
            bus_addr1: 0,
        }
    }
}
impl EmuState {
    pub fn diff(a: &EmuState, b: &EmuState) -> String {
        let mut r = vec![];
        if a.pc != b.pc {
            r.push(format!("pc {:?} {:?}", a.pc, b.pc));
        }
        if a.reg != b.reg {
            r.push(format!("reg {:?} {:?}", a.reg, b.reg));
        }
        if a.mem != b.mem {
            r.push(format!("mem {:?} {:?}", a.mem, b.mem));
        }
        if a.mem_page != b.mem_page {
            r.push(format!("mem_page {:?} {:?}", a.mem_page, b.mem_page));
        }
        if a.flag_p != b.flag_p {
            r.push(format!("flag_p {:?} {:?}", a.flag_p, b.flag_p));
        }
        if a.flag_nz != b.flag_nz {
            r.push(format!("flag_nz {:?} {:?}", a.flag_nz, b.flag_nz));
        }
        if a.flag_n != b.flag_n {
            r.push(format!("flag_n {:?} {:?}", a.flag_n, b.flag_n));
        }
        if a.bus_addr0 != b.bus_addr0 {
            r.push(format!("bus_addr0 {:?} {:?}", a.bus_addr0, b.bus_addr0));
        }
        if a.bus_addr1 != b.bus_addr1 {
            r.push(format!("bus_addr1 {:?} {:?}", a.bus_addr1, b.bus_addr1));
        }
        r.join(", ")
    }
}

impl CpuV1State {
    pub fn export_emu_state(&self) -> EmuState {
        EmuState {
            pc: self.pc.out.get_u8(),
            reg: self.reg.map(|i| i.out.get_u8()),
            mem: self.mem.map(|i| i.out.get_u8()),
            mem_page: self.mem_page.out.get_u8(),
            flag_p: self.flag_p.out().get(),
            flag_nz: self.flag_nz.out().get(),
            flag_n: self.flag_n.out().get(),
            bus_addr0: self.bus_addr0.out.get_u8(),
            bus_addr1: self.bus_addr1.out.get_u8(),
        }
    }
}

fn update_flags(state: &mut EmuState, value: u8) {
    state.flag_n = (8u8..16u8).contains(&value) as u8;
    state.flag_nz = (value != 0) as u8;
    state.flag_p = (1u8..8u8).contains(&value) as u8;
}

impl EmuEnv {
    pub fn new(inst: [Instruction; 256]) -> EmuEnv {
        Self {
            inst,
            state: EmuState::default(),
            device: Rc::new(RefCell::new(Devices::new())),
        }
    }

    pub fn get_state(&self) -> &EmuState {
        &self.state
    }

    pub fn clock(&mut self) {
        use crate::isa::Instruction::*;
        use crate::isa::RegisterIndex::*;

        fn pc_offset_from_u8(pc: u8, v: u8) -> u8 {
            let mut offset = v as i32;
            if offset >= 8 {
                offset -= 16;
            }
            ((pc as i32 + offset) % 256) as u8
        }

        fn op2(
            (reg1, reg0): (RegisterIndex, RegisterIndex),
            state: &mut EmuState,
            f: impl FnOnce(u8, u8) -> u8,
        ) {
            let reg0_curr = state.reg[reg0 as usize];
            let reg1_curr = state.reg[reg1 as usize];
            let reg0_next = f(reg1_curr, reg0_curr);
            state.reg[reg0 as usize] = reg0_next;
            update_flags(state, reg0_next);
        }
        fn op1(reg0: RegisterIndex, state: &mut EmuState, f: impl FnOnce(u8) -> u8) {
            let reg0_curr = state.reg[reg0 as usize];
            let reg0_next = f(reg0_curr);
            state.reg[reg0 as usize] = reg0_next;
            update_flags(state, reg0_next);
        }

        let pc = self.state.pc;
        let inst = self.inst[pc as usize];
        let mut pc_next = pc + 1;
        let reg = &mut self.state.reg;

        match inst {
            mov(param) => {
                op2(param, &mut self.state, |reg1, _reg0| reg1);
            }
            and(param) => {
                op2(param, &mut self.state, |reg1, reg0| reg0 & reg1);
            }
            or(param) => {
                op2(param, &mut self.state, |reg1, reg0| reg0 | reg1);
            }
            xor(param) => {
                op2(param, &mut self.state, |reg1, reg0| reg0 ^ reg1);
            }
            add(param) => {
                op2(param, &mut self.state, |reg1, reg0| (reg0 + reg1) % 16);
            }
            inv(reg0) => op1(reg0, &mut self.state, |reg0| (!reg0) % 16),
            neg(reg0) => op1(reg0, &mut self.state, |reg0| 16 - reg0),
            dec(reg0) => op1(reg0, &mut self.state, |reg0| (reg0 + 15) % 16),
            inc(reg0) => op1(reg0, &mut self.state, |reg0| (reg0 + 1) % 16),
            load_imm(imm4) => op1(Reg0, &mut self.state, |_| imm4),
            load_mem(imm4) => {
                let mem = if imm4 == 0 {
                    self.state.mem[self.state.mem_page as usize * 16 + self.state.reg[1] as usize]
                } else {
                    self.state.mem[self.state.mem_page as usize * 16 + imm4 as usize]
                };
                op1(Reg0, &mut self.state, |_| mem)
            }
            store_mem(imm4) => {
                let reg0 = self.state.reg[0];
                if imm4 == 0 {
                    self.state.mem
                        [self.state.mem_page as usize * 16 + self.state.reg[1] as usize] = reg0;
                } else {
                    self.state.mem[self.state.mem_page as usize * 16 + imm4 as usize] = reg0;
                };
            }
            jmp_long(imm4) => {
                if imm4 == 0 {
                    pc_next = self.state.reg[0] * 16;
                } else {
                    pc_next = imm4 * 16;
                }
            }
            jmp_offset(imm4) => {
                let offset = if imm4 == 0 { self.state.reg[0] } else { imm4 };
                pc_next = pc_offset_from_u8(pc, offset);
            }
            jne_offset(imm4) => {
                if self.state.flag_nz > 0 {
                    let offset = if imm4 == 0 { self.state.reg[0] } else { imm4 };
                    pc_next = pc_offset_from_u8(pc, offset);
                }
            }
            jl_offset(imm4) => {
                if self.state.flag_n > 0 {
                    let offset = if imm4 == 0 { self.state.reg[0] } else { imm4 };
                    pc_next = pc_offset_from_u8(pc, offset);
                }
            }
            jg_offset(imm4) => {
                if self.state.flag_p > 0 {
                    let offset = if imm4 == 0 { self.state.reg[0] } else { imm4 };
                    pc_next = pc_offset_from_u8(pc, offset);
                }
            }
            reset(_) => {}
            halt(_) => {}
            sleep(_) => {}
            set_mem_page(_) => {
                self.state.mem_page = self.state.reg[0];
            }
            set_bus_addr0(_) => {
                self.state.bus_addr0 = self.state.reg[0];
            }
            set_bus_addr1(_) => {
                self.state.bus_addr1 = self.state.reg[0];
            }
            bus0(imm3) => {
                let result =
                    self.device
                        .borrow_mut()
                        .execute(self.state.bus_addr0, imm3, reg[0], reg[1]);
                op1(Reg0, &mut self.state, |_| result.reg0_write_data);
            }
            bus1(imm3) => {
                let result =
                    self.device
                        .borrow_mut()
                        .execute(self.state.bus_addr1, imm3, reg[0], reg[1]);
                op1(Reg0, &mut self.state, |_| result.reg0_write_data);
            }
        }

        self.state.pc = pc_next;
    }
}
