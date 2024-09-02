use crate::Instruction::{load_hi, load_lo};
use crate::{u8_to_hi_lo, Assembler, FuncDecl, FuncName, Relocation, TMP_REG};
use std::collections::HashMap;

#[derive(Default)]
pub struct Linker {
    functions: HashMap<FuncName, FunctionObj>,
}

#[allow(unused)] //TODO use decl
struct FunctionObj {
    inst_range: (usize, usize),
    func_decl: FuncDecl,
    relocations: Vec<Relocation>,
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

    pub fn functions(&self) -> impl Iterator<Item = (FuncDecl, (usize, usize))> + '_ {
        self.functions
            .values()
            .map(|func| (func.func_decl.clone(), func.inst_range))
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
            far_jmp(asm, rel.slots[0].addr, rel.slots[1].addr, start);
        }
    }
}

fn far_jmp(asm: &mut Assembler, slot0: usize, slot1: usize, addr: usize) {
    let lo = addr & 0b1111_1111;
    let (hi, lo) = u8_to_hi_lo(lo as u8);
    asm.inst_at(load_lo(hi, lo, TMP_REG), slot0);
    let hi = addr >> 8;
    let (hi, lo) = u8_to_hi_lo(hi as u8);
    asm.inst_at(load_hi(hi, lo, TMP_REG), slot1);
}
