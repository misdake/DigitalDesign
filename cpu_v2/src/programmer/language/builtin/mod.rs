use crate::dsl::v;
use crate::{ProgramFunction, VariableOperation1};
use std::ops::Neg;

pub fn mul_16x4() -> (ProgramFunction<2, 1>, VariableOperation1) {
    const BIT: usize = 4;
    let mul_16x4 = ProgramFunction::new("mul_16x4", ["b16", "b4"], ["r"]);

    let ops = mul_16x4.define(|[b, a], ret| {
        let one_bit = v(1);
        let mut sum = v(0);

        let bit0 = a & one_bit;
        sum += bit0.neg() & b;

        // unrolled
        for _ in 1..BIT {
            a.lsr_assign(1);
            b.lsl_assign(1);
            let bit = a & one_bit;
            sum += bit.neg() & b;
        }

        ret([sum]);
    });

    (mul_16x4, ops)
}

pub fn mul_16x8() -> (ProgramFunction<2, 1>, VariableOperation1) {
    const BIT: usize = 8;
    let mul_16x8 = ProgramFunction::new("mul_16x8", ["b16", "b8"], ["r"]);

    let ops = mul_16x8.define(|[b, a], ret| {
        let one_bit = v(1);
        let mut sum = v(0);

        let bit0 = a & one_bit;
        sum += bit0.neg() & b;

        // unrolled
        for _ in 1..BIT {
            a.lsr_assign(1);
            b.lsl_assign(1);
            let bit = a & one_bit;
            sum += bit.neg() & b;
        }

        ret([sum]);
    });

    (mul_16x8, ops)
}

pub fn mul_16x16() -> (ProgramFunction<2, 1>, VariableOperation1) {
    const BIT: usize = 16;
    let mul_16x16 = ProgramFunction::new("mul_16x16", ["b16", "b16"], ["r"]);

    let ops = mul_16x16.define(|[b, a], ret| {
        let one_bit = v(1);
        let mut sum = v(0);

        let bit0 = a & one_bit;
        sum += bit0.neg() & b;

        // unrolled
        for _ in 1..BIT {
            a.lsr_assign(1);
            b.lsl_assign(1);
            let bit = a & one_bit;
            sum += bit.neg() & b;
        }

        ret([sum]);
    });

    (mul_16x16, ops)
}

#[test]
fn test_mul() {
    use crate::programmer::language::dsl::*;
    use crate::test;

    let x = 37;
    let y = 1111;

    let (mul, mul_vo1) = mul_16x16();

    let test_mul = ProgramFunction::new("test_mul", [], []);

    let call_vo1 = test_mul.define(|[], _ret| {
        let a = v(x);
        let b = v(y);
        let [r] = mul.call([a, b]);
        halt_with_signal(r);
    });
    let (_state, signal) = test(vec![
        (call_vo1, test_mul.func_decl),
        (mul_vo1, mul.func_decl),
    ]);
    assert_eq!(signal, Some(x * y));
}
