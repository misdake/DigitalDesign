//! shift-add multiplication

use crate::Compiler;
use crate::compiler::dsl::*;
use once_cell::sync::Lazy;

pub static MUL_16X4: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("mul_16x4", ["a", "b4"], ["r"]));
pub static MUL_16X8: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("mul_16x8", ["a", "b8"], ["r"]));
pub static MUL_16X16: Lazy<DslFunction<2, 1>> =
    Lazy::new(|| DslFunction::new("mul_16x16", ["a", "b16"], ["r"]));

pub fn define_mul(compiler: &mut Compiler) {
    if compiler.has_func("mul_16x4") {
        return;
    }
    MUL_16X4.compile(compiler, |b, [a, b4], ret| mul_body(b, a, b4, ret, 4));
    MUL_16X8.compile(compiler, |b, [a, b8], ret| mul_body(b, a, b8, ret, 8));
    MUL_16X16.compile(compiler, |b, [a, b16], ret| mul_body(b, a, b16, ret, 16));
}

/// r = a * b, b fits in `bit` bits (b is destroyed)
fn mul_body(b: &B, a: Variable, bx: Variable, ret: &dyn Fn(&B, [Variable; 1]), bit: usize) {
    let one = b.v(1);
    let mut sum = b.v(0);

    let bit0 = &bx & &one;
    sum += &(-&bit0 & &a);
    for _ in 1..bit {
        bx.lsr_assign(1);
        a.lsl_assign(1);
        let bit = &bx & &one;
        sum += &(-&bit & &a);
    }

    ret(b, [sum]);
}

pub fn mul_16x4(b: &B, a: &Variable, b4: &Variable) -> Variable {
    let [r] = MUL_16X4.call(b, [a, b4]);
    r
}
pub fn mul_16x8(b: &B, a: &Variable, b8: &Variable) -> Variable {
    let [r] = MUL_16X8.call(b, [a, b8]);
    r
}
pub fn mul_16x16(b: &B, a: &Variable, b16: &Variable) -> Variable {
    let [r] = MUL_16X16.call(b, [a, b16]);
    r
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulate;

    #[test]
    fn test_mul() {
        let x = 37u16;
        let y = 1111u16;

        let mut compiler = Compiler::new();
        define_mul(&mut compiler);

        let test_mul = DslFunction::new("test_mul", [], []);
        test_mul.compile(&mut compiler, |b, [], _ret| {
            let a = b.v(x);
            let c = b.v(y);
            let r = mul_16x16(b, &a, &c);
            b.halt(&r);
        });

        let (instructions, _) = compiler.finish("test_mul");
        let (_state, signal) = simulate(&instructions, 1000);
        assert_eq!(signal, Some(x.wrapping_mul(y)));
    }
}
