pub mod builtin;
mod func;
mod helper;
mod operators;
mod ptr;
mod structure;

pub use func::*;
pub use operators::*;
pub mod dsl {
    pub use super::builtin::*;
    pub use super::helper::*;
    pub use super::ptr::*;
    pub use super::structure::*;
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
    call.compile(&mut compiler, |[], _ret| {
        let mut sum = v(0);
        for_loop_u4(1..(n + 1), |i| {
            sum += i;
        });
        halt_with_signal(sum);
    });

    let instructions = compiler.finish("loop");
    let (_state, signal) = simulate(&instructions, 1000);
    let n = n as u16;
    assert_eq!(signal, Some(n * (n + 1) / 2));
}

#[test]
fn test_for_loop2() {
    use crate::programmer::language::dsl::*;
    let call = DslFunction::new("loop2", [], []);

    let mut compiler = Compiler::default();
    compiler.func_op(
        &call.func_decl,
        call.define(|[], _ret| {
            // 1..=5
            let start = v(1);
            let end = v(6);

            let mut sum = v(0);
            let r1 = v(0);
            for_loop_reg_up(start, end, 1, |i| {
                sum += i;
                if_then(CondOp::CmpI(sum, 6, Cond::LessEqual), || {
                    r1.assign_from(i);
                })
            });

            let mut sum = v(0);
            let r2 = v(0);
            for_loop_reg_up_rev(start, end, 1, |i| {
                sum += i;
                if_then(CondOp::CmpI(sum, 6, Cond::Less), || {
                    r2.assign_from(i);
                })
            });

            halt_with_signal(r1.lsl(4) + r2);
        }),
    );

    let instructions = compiler.finish("loop2");
    let (_state, signal) = simulate(&instructions, 1000);
    // up: 1 + 2 + 3, r1 = 3
    // up_rev: 5, r2 = 5
    assert_eq!(signal, Some((3 << 4) + 5));
}
