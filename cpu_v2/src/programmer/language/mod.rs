mod func;
mod helper;
mod operators;

pub use func::*;
pub use operators::*;
pub mod dsl {
    pub use super::helper::*;
}

use crate::*;

use std::sync::Mutex;

static mut LOCK: Mutex<()> = Mutex::new(());
static mut OPS: Option<Vec<VariableOperation1>> = None;

fn push_op(op: VariableOperation1) {
    unsafe {
        OPS.as_mut().unwrap().push(op);
    }
}

pub fn compose_variable_operations(f: impl FnOnce()) -> VariableOperation1 {
    unsafe {
        let _lock = LOCK.lock().unwrap();
        let prev = OPS.take();
        OPS = Some(vec![]);

        f();

        let mut list = OPS.take().unwrap();
        OPS = prev;

        match list.len() {
            0 => panic!("empty list"),
            1 => list.remove(0),
            _ => VariableOperation1::List(list),
        }
    }
}

#[test]
fn test_call() {
    use crate::programmer::language::dsl::*;

    let x = 12;
    let y = 43;

    let call = ProgramFunction::new("call", [], []);
    let add = ProgramFunction::new("add", ["a", "b"], ["r"]);

    let call_vo1 = call.define(|[], _ret| {
        let a = v(x);
        let b = v(y);
        let [_r] = add.call([a, b]);
        h();
    });
    let add_vo1 = add.define(|[a, b], ret| {
        let r = a + b;
        ret([r]);
    });

    let instructions = compile_program(vec![(call_vo1, call.func_decl), (add_vo1, add.func_decl)]);
    let (sim, cycles) = simulate(&instructions, 100);
    println!("r0 = {}", sim.state.reg[0]);
    println!("cycles = {}", cycles);
    assert_eq!(sim.state.reg[0], x + y);
}
