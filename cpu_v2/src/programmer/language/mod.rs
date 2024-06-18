pub mod builtin;
mod func;
mod helper;
mod operators;
mod ptr;

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

#[test]
fn test_call_add() {
    use crate::programmer::language::dsl::*;
    let x = 12;
    let y = 43;
    let call = DslFunction::new("call", [], []);
    let add = DslFunction::new("add", ["a", "b"], ["r"]);

    let mut compiler = Compiler::default();
    call.compile(&mut compiler, |[], _ret| {
        let a = v(x);
        let b = v(y);
        let [r] = add.call([a, b]);
        halt_with_signal(r)
    });
    add.compile(&mut compiler, |[a, b], ret| {
        let r = a + b;
        ret([r]);
    });

    let instructions = compiler.finish("call");
    let (_state, signal) = simulate(&instructions, 1000);
    assert_eq!(signal, Some(x + y));
}
#[test]
fn test_for_loop() {
    use crate::programmer::language::dsl::*;
    let n = 10;
    let call = DslFunction::new("loop", [], []);

    let mut compiler = Compiler::default();
    compiler.func_op(
        &call.func_decl,
        call.define(|[], _ret| {
            let mut sum = v(0);
            for_loop_u4(1..(n + 1), |i| {
                sum += i;
            });
            halt_with_signal(sum);
        }),
    );

    let instructions = compiler.finish("call");
    let (_state, signal) = simulate(&instructions, 1000);
    let n = n as u16;
    assert_eq!(signal, Some(n * (n + 1) / 2));
}
