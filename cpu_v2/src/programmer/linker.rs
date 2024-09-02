use crate::Instruction::{load_hi, load_lo};
use crate::{u8_to_hi_lo, Assembler, FuncDecl, FuncName, Relocation, TMP_REG};
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
            far_jmp(asm, rel.slots[0].addr, rel.slots[1].addr, start);
        }
    }

    /// returns HashMap<addr, func_name>
    pub fn get_all_calls(&self) -> HashMap<usize, &'static str> {
        let mut map = HashMap::new();
        for func in self.functions.values() {
            for rel in &func.relocations {
                let addr = rel.slots[1].addr + 1;
                map.insert(addr, rel.func_name);
            }
        }
        map
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
