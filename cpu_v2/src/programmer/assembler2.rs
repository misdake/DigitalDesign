use crate::isa::*;
use crate::programmer::Assembler1;
use std::cell::RefCell;
use std::rc::Rc;

// Register usages:
//   registers: r0 - r15
//     r0-r3 are function args and return values
//     r4-r13 are temps
//     r14 is sp, r15 is return pc

pub struct Assembler2 {
    assembler: Assembler1,
    //TODO register pool
}
impl Assembler2 {
    pub fn new() -> Self {
        Self {
            assembler: Assembler1::new(),
        }
    }
}

pub struct Assembler2Shared {
    inner: Rc<RefCell<Assembler2>>,
}

pub struct AsmReg {
    assembler: Assembler2Shared,
    reg: Reg,
}
//TODO AsmReg operators? use raii for register lifetime tracking
