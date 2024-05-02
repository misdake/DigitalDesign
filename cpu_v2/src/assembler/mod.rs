use crate::isa::{Instruction, Reg};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

#[derive(Copy, Clone)]
pub struct InstructionSlot {
    data: Instruction,
    addr: usize,
}

pub struct Assembler {
    instructions: [Option<Instruction>; 65536],
    function_names: HashMap<usize, &'static str>,
    function_addrs: HashMap<&'static str, Range<usize>>,
    comments: HashMap<usize, String>,

    cursor: usize,
}

pub struct SharedAsm {
    assembler: Rc<RefCell<Assembler>>,
    //TODO .pc
    //TODO .r0~r15 -> AsmRegister
    //TODO .reg(usize) -> AsmRegister
    //TODO .mem(usize) -> AsmMemoryAddr
}
impl SharedAsm {}

pub struct AsmRegister {
    assembler: SharedAsm,
    reg_addr: Reg,
}
pub struct AsmMemoryAddr {
    assembler: Rc<RefCell<Assembler>>,
    addr: usize,
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            instructions: [None; 65536],
            function_names: HashMap::new(),
            function_addrs: HashMap::new(),
            comments: HashMap::new(),
            cursor: 0,
        }
    }
}
