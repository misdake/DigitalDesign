use crate::{
    default_reg_usages, Assembler, FuncDecl, Linker, RegisterAllocator, RegisterOperation,
    VariableOperation1, VariableOperation2Scope, VariableOperation3,
};

#[derive(Default)]
pub struct Compiler {
    asm: Assembler,
    linker: Linker,
    cursor: usize,
}

impl Compiler {
    pub fn new_function(&mut self, (vo1, decl): (VariableOperation1, FuncDecl)) {
        let vo2s = VariableOperation2Scope::from(vo1);
        let vo3 = VariableOperation3::from(vo2s);

        use std::rc::Rc;
        let mut allocator = RegisterAllocator::new(Rc::new(default_reg_usages()), decl.clone());
        let ops = allocator.run(&vo3);

        let relocations =
            RegisterOperation::write_function_assembly(&ops, &mut self.asm, self.cursor);
        let end = self.asm.get_cursor();

        self.linker
            .register_function((self.cursor, end), decl, relocations);

        self.cursor = end + 4;
    }

    pub fn finish(mut self) -> Assembler {
        self.linker.relocate_all(&mut self.asm);
        self.asm
    }
}

fn test_program(functions: Vec<(VariableOperation1, FuncDecl)>) {
    let mut compiler = Compiler::default();
    for (vo1, decl) in functions {
        compiler.new_function((vo1.clone(), decl.clone()));
    }
    let asm = compiler.finish();

    let end = asm.get_cursor();
    let instructions = asm.finish();
    let instructions = &instructions[0..end];

    for (addr, inst) in instructions.iter().enumerate() {
        println!("inst {addr:04x}: {inst}");
    }
}

#[cfg(test)]
use crate::programmer::*;
#[test]
fn test_basic() {
    test_program(vec![vo1_call_program(), vo1_func_program()])
}
