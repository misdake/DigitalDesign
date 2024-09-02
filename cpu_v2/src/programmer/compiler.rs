use crate::*;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

type FuncGen = Box<dyn FnOnce() -> VariableOperation1>;

pub struct Compiler {
    reg_usages: Rc<RegisterUsages>,
    asm: Assembler,
    linker: Linker,
    cursor: usize,

    functions: HashMap<FuncName, (FuncGen, FuncDecl)>,
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
    /// used by user defined functions, it's prefered to just call DslFunc::compile
    pub fn func_op(&mut self, func_decl: &FuncDecl, op: VariableOperation1) {
        self.functions
            .insert(func_decl.func_name, (box move || op, func_decl.clone()));
    }

    //TODO compiler flags? for example heap config
    //TODO a hashset of markers to skip redundant, for example compiler.define_func(Marker, impl FnOnce(&mut Compiler))
    /// used by builtin functions for on-demand code generation
    pub fn func_gen<const PARAM: usize, const RETURN: usize>(
        &mut self,
        func: &DslFunction<PARAM, RETURN>,
        generator: Box<dyn FnOnce() -> VariableOperation1>,
    ) {
        self.functions.insert(
            func.func_decl.func_name,
            (generator, func.func_decl.clone()),
        );
    }

    pub fn finish(mut self, main: &'static str) -> Vec<Instruction> {
        let mut called = HashSet::new();
        let mut called_vec = vec![];
        let mut next = vec![main];
        called.insert(main);

        while !next.is_empty() {
            let curr = std::mem::take(&mut next);
            for name in curr {
                let (generator, decl) = self
                    .functions
                    .remove(name)
                    .unwrap_or_else(|| panic!("unknown function `{name}`"));
                println!("generating {}", decl.func_name);
                let vo1 = generator();
                let vo2s = VariableOperation2Scope::from(vo1);
                let vo3 = VariableOperation3::from(vo2s);
                let ro = RegisterAllocator::execute(self.reg_usages.clone(), decl.clone(), &vo3);

                let relocations =
                    RegisterOperation::write_function_assembly(&ro, &mut self.asm, self.cursor);

                let end = self.asm.get_cursor();

                // enqueue called functions
                for rel in &relocations {
                    if !called.contains(rel.func_name) {
                        called.insert(rel.func_name);
                        called_vec.push(rel.func_name);
                        next.push(rel.func_name);
                    }
                }

                self.linker
                    .register_function((self.cursor, end), decl, relocations);

                self.cursor = end + 4;
            }
        }
        println!("main: {main:?}, functions: {called:?}");

        self.linker.relocate_all(&mut self.asm);
        let end = self.asm.get_cursor();
        let instructions = self.asm.slice_ref()[0..end].to_vec();

        for (addr, inst) in instructions.iter().enumerate() {
            println!("inst {addr:04x}: {inst}");
        }

        instructions
    }
}

pub fn simulate(instructions: &[Instruction], max_cycles: usize) -> (SimState, Option<u16>) {
    let mut sim = SimEnv::new(instructions);
    let halt_signal = sim.run_to_halt(max_cycles, |pc, inst, change| {
        let inst = format!("pc {pc:04x}: {inst}");
        let change = change.desc(pc);
        println!("{inst:40}{change}");
    });
    if let Some(halt_signal) = halt_signal {
        println!(
            "halt with signal = {halt_signal} after {} cycles",
            sim.state.cycles
        )
    }
    (sim.state, halt_signal)
}

#[test]
fn test_basic() {
    use crate::*;
    let x = 12;
    let y = 43;
    let mut compiler = Compiler::default();
    let (func_vo1, func_decl) = vo1_func_program();
    let (call_vo1, call_decl) = vo1_call_program(x, y);
    compiler.func_op(&func_decl, func_vo1);
    compiler.func_op(&call_decl, call_vo1);
    let instructions = compiler.finish("call");
    let (_state, halt_signal) = simulate(&instructions, 100);
    assert_eq!(halt_signal, Some((x + y) as u16));
}
