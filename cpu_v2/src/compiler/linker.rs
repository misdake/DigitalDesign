use crate::Instruction::{call_abs, call_reg, call_rel, load_hi, load_lo, mov};
use crate::{
    u8_to_hi_lo, Assembler, FuncDecl, FuncName, InstructionSlot, RelocKind, Relocation, TMP_REG,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct Linker {
    pub(crate) functions: HashMap<FuncName, FunctionObj>,
}

#[allow(unused)] //TODO use decl
pub(crate) struct FunctionObj {
    pub inst_range: (usize, usize),
    pub func_decl: FuncDecl,
    pub relocations: Vec<Relocation>,
}

impl Linker {
    pub fn register_function(
        &mut self,
        inst_range: (usize, usize),
        func_decl: FuncDecl,
        relocations: Vec<Relocation>,
    ) {
        self.functions.insert(
            func_decl.func_name,
            FunctionObj {
                inst_range,
                func_decl,
                relocations,
            },
        );
    }

    pub fn relocate_all(&self, asm: &mut Assembler) {
        for func in self.functions.values() {
            self.relocate_func(func, asm);
        }
    }
    fn relocate_func(&self, func: &FunctionObj, asm: &mut Assembler) {
        for rel in &func.relocations {
            let target = self
                .functions
                .get(&rel.func_name)
                .unwrap_or_else(|| panic!("function not found: {}", rel.func_name));
            let start = target.inst_range.0;
            match rel.kind {
                RelocKind::Call3 => relocate_call(asm, &rel.slots, start),
                RelocKind::CallAbs { index } => {
                    let (hi, lo) = u8_to_hi_lo(index);
                    asm.inst_at(call_abs(hi, lo), rel.slots[0].addr);
                }
                RelocKind::LoadAddr { reg } => {
                    let (hi, lo) = u8_to_hi_lo((start & 0xff) as u8);
                    asm.inst_at(load_lo(hi, lo, reg), rel.slots[0].addr);
                    let (hi, lo) = u8_to_hi_lo((start >> 8) as u8);
                    asm.inst_at(load_hi(hi, lo, reg), rel.slots[1].addr);
                }
            }
        }
    }

    /// returns HashMap<addr of the actual call instruction, func_name>
    pub fn get_all_calls(&self) -> HashMap<usize, &'static str> {
        let mut map = HashMap::new();
        for func in self.functions.values() {
            for rel in &func.relocations {
                let addr = match rel.kind {
                    RelocKind::CallAbs { .. } => rel.slots[0].addr,
                    RelocKind::Call3 => match self.functions.get(&rel.func_name) {
                        Some(target)
                            if call_rel_offset(rel.slots[0].addr, target.inst_range.0)
                                .is_some() =>
                        {
                            rel.slots[0].addr
                        }
                        _ => rel.slots[2].addr,
                    },
                    RelocKind::LoadAddr { .. } => continue,
                };
                map.insert(addr, rel.func_name);
            }
        }
        map
    }
}

/// nop = mov rX, rX (tmp self-mov, no side effect)
fn nop() -> crate::Instruction {
    mov(TMP_REG, TMP_REG)
}

/// i8 offset for call_rel at `from` jumping to `to`, None if out of range or zero
fn call_rel_offset(from: usize, to: usize) -> Option<i8> {
    let offset = to as i64 - from as i64;
    if (-128..=127).contains(&offset) && offset != 0 {
        Some(offset as i8)
    } else {
        None
    }
}

/// fill the 3 reserved call slots: near -> call_rel + 2 nop, far -> load_lo(tmp) + load_hi(tmp) + call_reg(tmp)
fn relocate_call(asm: &mut Assembler, slots: &[InstructionSlot], target: usize) {
    if let Some(offset) = call_rel_offset(slots[0].addr, target) {
        let v = offset as u8;
        asm.inst_at(call_rel(v >> 4, v & 0xf), slots[0].addr);
        asm.inst_at(nop(), slots[1].addr);
        asm.inst_at(nop(), slots[2].addr);
    } else {
        let lo = target & 0b1111_1111;
        let (hi, lo) = u8_to_hi_lo(lo as u8);
        asm.inst_at(load_lo(hi, lo, TMP_REG), slots[0].addr);
        let hi = target >> 8;
        let (hi, lo) = u8_to_hi_lo(hi as u8);
        asm.inst_at(load_hi(hi, lo, TMP_REG), slots[1].addr);
        asm.inst_at(call_reg(TMP_REG), slots[2].addr);
    }
}
