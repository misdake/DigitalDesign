use crate::*;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub type LibFunc = dyn FnOnce() -> (VariableOperation1, FuncDecl);

pub struct Compiler {
    reg_usages: Rc<RegisterUsages>,
    asm: Assembler,
    linker: Linker,
    cursor: usize,

    functions: HashMap<&'static str, Box<LibFunc>>,
}
impl Default for Compiler {
    fn default() -> Self {
        Self {
            reg_usages: Rc::new(default_reg_usages()),
            asm: Default::default(),
            linker: Default::default(),
            cursor: 0,
            functions: Default::default(),
        }
    }
}

impl Compiler {
    pub fn function(&mut self, name: &'static str, creator: Box<LibFunc>) {
        self.functions.insert(name, creator);
    }

    pub fn finish(mut self, entry: &'static str) -> Vec<Instruction> {
        let mut called = HashSet::new();
        let mut called_vec = vec![];
        let mut next = vec![entry];

        while !next.is_empty() {
            let curr = std::mem::take(&mut next);
            for name in curr {
                if !called.contains(name) {
                    called.insert(name);
                    called_vec.push(name);
                    next.push(name);
                }
            }
        }
        println!("functions: {:?}", called);

        for name in called_vec {
            let creator = self
                .functions
                .remove(name)
                .expect(format!("unknown function `{name}`").as_str());
            let (op, decl) = creator();
            self.new_function((op, decl));
        }

        self.linker.relocate_all(&mut self.asm);
        let end = self.asm.get_cursor();
        let instructions = self.asm.slice_ref();

        for (addr, inst) in instructions.iter().enumerate() {
            println!("inst {addr:04x}: {inst}");
        }

        instructions[0..end].to_vec()
    }

    fn new_function(&mut self, (vo1, decl): (VariableOperation1, FuncDecl)) {
        let vo2s = VariableOperation2Scope::from(vo1);
        let vo3 = VariableOperation3::from(vo2s);

        let ops = RegisterAllocator::execute(self.reg_usages.clone(), decl.clone(), &vo3);

        let relocations =
            RegisterOperation::write_function_assembly(&ops, &mut self.asm, self.cursor);

        let end = self.asm.get_cursor();

        self.linker
            .register_function((self.cursor, end), decl, relocations);

        self.cursor = end + 4;
    }
}

pub fn compile_program(functions: Vec<(VariableOperation1, FuncDecl)>) -> Vec<Instruction> {
    let mut compiler = Compiler::default();
    for (vo1, decl) in functions {
        compiler.new_function((vo1.clone(), decl.clone()));
    }
    compiler.finish()
}

pub fn simulate(instructions: &[Instruction], max_cycles: usize) -> (SimState, Option<u16>) {
    let mut sim = SimEnv::new(instructions);
    let halt_signal = sim.run_to_halt(max_cycles, |pc, inst, change| {
        let inst = format!("pc {pc:04x}: {inst}");
        let change = change.desc(pc);
        println!("{inst:40}{change}");
    });
    (sim.state, halt_signal)
}

#[test]
fn test_basic() {
    use crate::*;
    let x = 12;
    let y = 43;
    let mut compiler = Compiler::default();
    compiler.function("func", box vo1_func_program);
    compiler.function("call", box || vo1_call_program(x, y));
    let instructions = compiler.finish("call");
    let (_state, halt_signal) = simulate(&instructions, 100);
    println!("halt_signal = {:?}", halt_signal);
    assert_eq!(halt_signal, Some((x + y) as u16));
}
