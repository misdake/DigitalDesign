use crate::dsl::v;
use crate::{Compiler, DslFunction, Variable, VariableOperation1};
use once_cell::sync::Lazy;
use std::ops::Neg;

pub fn define_mul(compiler: &mut Compiler) {
    compiler.func_gen(&MUL_16X4_FUNC, box || mul_define(&MUL_16X4_FUNC, 4));
    compiler.func_gen(&MUL_16X8_FUNC, box || mul_define(&MUL_16X8_FUNC, 8));
    compiler.func_gen(&MUL_16X16_FUNC, box || mul_define(&MUL_16X16_FUNC, 16));
}

static MUL_16X4_FUNC: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("mul_16x4", ["a", "b4"], ["r"]));
static MUL_16X8_FUNC: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("mul_16x8", ["a", "b8"], ["r"]));
static MUL_16X16_FUNC: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("mul_16x16", ["a", "b16"], ["r"]));

pub fn mul_16x4(a: Variable, b4: Variable) -> Variable {
    MUL_16X4_FUNC.call([a, b4])[0]
}
pub fn mul_16x8(a: Variable, b8: Variable) -> Variable {
    MUL_16X8_FUNC.call([a, b8])[0]
}
pub fn mul_16x16(a: Variable, b16: Variable) -> Variable {
    MUL_16X16_FUNC.call([a, b16])[0]
}

fn mul_define(func: &DslFunction<2, 1>, bit: usize) -> VariableOperation1 {
    func.define(|[a, b], ret| {
        let one_bit = v(1);
        let mut sum = v(0);

        let bit0 = b & one_bit;
        sum += bit0.neg() & a;

        // unrolled
        for _ in 1..bit {
            b.lsr_assign(1);
            a.lsl_assign(1);
            let bit = b & one_bit;
            sum += bit.neg() & a;
        }

        ret([sum]);
    })
}

#[test]
fn test_mul() {
    use crate::programmer::language::dsl::*;

    let x = 37;
    let y = 1111;

    let mut compiler = Compiler::default();
    define_mul(&mut compiler);

    let test_mul = DslFunction::new("test_mul", [], []);
    test_mul.compile(&mut compiler, |[], _ret| {
        let a = v(x);
        let b = v(y);
        let r = mul_16x16(a, b);
        halt_with_signal(r);
    });

    let instructions = compiler.finish("test_mul");
    let (_state, halt_signal) = crate::simulate(&instructions, 1000);
    assert_eq!(halt_signal, Some(x * y));
}
