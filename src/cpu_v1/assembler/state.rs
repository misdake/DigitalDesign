#[derive(Clone)]
struct CpuState {
    inst: Box<[Value8; 256]>,
    pc: Value8,
    reg: [Value4; 4],
    mem: Box<[Value4; 256]>,
    mem_page: Value4,
    bus_addr0: Value4,
    bus_addr1: Value4,
}
impl Default for CpuState {
    fn default() -> Self {
        Self {
            inst: Box::new([Value8::Invalid; 256]),
            pc: Value8::Known(0),
            reg: [Value4::Invalid; 4],
            mem: Box::new([Value4::Invalid; 256]),
            mem_page: Value4::Invalid,
            bus_addr0: Value4::Invalid,
            bus_addr1: Value4::Invalid,
        }
    }
}

#[derive(Copy, Clone)]
enum Value1 {
    Known(u8),
    Valid,
    Invalid,
}
#[derive(Copy, Clone)]
enum Value4 {
    Known(u8),
    Valid,
    Invalid,
}

#[derive(Copy, Clone)]
enum Value8 {
    Known(u8),
    Valid,
    Invalid,
}

struct Variable {
    name: &'static str,
    // slot: VariableSlot,
}
// enum VariableSlot {
//     Reg(u8), // u2
//     Mem(u8), // u8
// }

struct Scope {
    // parent Scope ref
    variables: Vec<Variable>,
    commands: Vec<Command>,
}
enum Command {
    AllocVariable(Variable),

    Assign(Variable, Variable), // left = reg0 (write), right = reg1 (read)

    AndAssign(Variable, Variable),
    OrAssign(Variable, Variable),
    XorAssign(Variable, Variable),
    AddAssign(Variable, Variable),
    Inv(Variable),
    Neg(Variable),
    Dec(Variable),
    Inc(Variable),

    LoadImm(Variable, u8),
    LoadMem(u8, Variable, u8),  // mem_page, variable, addr(!=0)
    StoreMem(u8, Variable, u8), // mem_page, variable, addr(!=0)
    // TODO LoadMemReg / StoreMemReg, custom page
    Call(u8),         // function ptr?
    Return(Variable), // return value
}

struct Compiler {
    functions: [(); 16], // has a default main function at 0
    state: CpuState,
}

// fn run() {
//     state.assume_reg();
//     state.assume_mem();
//
//     // return value in reg0
//     let function = FunctionReturn("name", addr, state, |scope| {});
//     // no return
//     let function2 = Function("name", addr, state, |scope| {});
//
//     Scope(state, |scope| {
//         let r = scope.variable("r");
//         let a = scope.variable("a");
//         let v = function(); // copy reg0 to variable
//     });
// }
