pub mod builtin;
mod func;
mod helper;
mod operators;
mod r#struct;

pub use func::*;
pub use operators::*;
pub mod dsl {
    pub use super::builtin::*;
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

pub(crate) fn compose_variable_operations_lock(f: impl FnOnce()) -> VariableOperation1 {
    unsafe {
        let _lock = LOCK.lock().unwrap();
        compose_variable_operations(f)
    }
}
pub fn compose_variable_operations(f: impl FnOnce()) -> VariableOperation1 {
    unsafe {
        assert!(
            LOCK.try_lock().is_err(),
            "must be locked to compose variable operations"
        );

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

#[cfg(test)]
pub fn test(functions: Vec<(VariableOperation1, FuncDecl)>) -> (SimState, Option<u16>) {
    let instructions = compile_program(functions);
    simulate(&instructions, 1000)
}

#[test]
fn test_call_add() {
    use crate::programmer::language::dsl::*;
    let x = 12;
    let y = 43;
    let call = ProgramFunction::new("call", [], []);
    let add = ProgramFunction::new("add", ["a", "b"], ["r"]);

    let call_vo1 = call.define(|[], _ret| {
        let a = v(x);
        let b = v(y);
        let [r] = add.call([a, b]);
        halt_with_signal(r);
    });
    let add_vo1 = add.define(|[a, b], ret| {
        let r = a + b;
        ret([r]);
    });
    let (_state, signal) = test(vec![(call_vo1, call.func_decl), (add_vo1, add.func_decl)]);
    assert_eq!(signal, Some(x + y));
}
#[test]
fn test_for_loop() {
    use crate::programmer::language::dsl::*;
    let n = 10;
    let call = ProgramFunction::new("loop", [], []);

    let call_vo1 = call.define(|[], _ret| {
        let mut sum = v(0);
        for_loop_u4(1..(n + 1), |i| {
            sum += i;
        });
        halt_with_signal(sum);
    });
    let (_state, signal) = test(vec![(call_vo1, call.func_decl)]);
    let n = n as u16;
    assert_eq!(signal, Some(n * (n + 1) / 2));
}
