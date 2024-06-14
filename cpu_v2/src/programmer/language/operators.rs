use crate::dsl::v;
use crate::programmer::language::push_op;
use crate::{u16_to_hi_lo, ResultOp, UpdateOp, Variable, VariableOperation1};
use std::ops::*;

macro_rules! define_op_with_assign {
    ($op_ty: ident, $op_name: ident, $vo_name: ident) => {
        paste::paste! {
            impl $op_ty<Variable> for Variable {
                type Output = Variable;
                fn $op_name(self, rhs: Variable) -> Self::Output {
                    let r = Variable::new();
                    push_op(VariableOperation1::Result(ResultOp:: $vo_name (self, rhs), r));
                    r
                }
            }
            impl [<$op_ty Assign>]<Variable> for Variable {
                fn [<$op_name _assign>](&mut self, rhs: Variable) {
                    push_op(VariableOperation1::Update(UpdateOp:: [<$vo_name Assign>] (*self, rhs)));
                }
            }
        }
    };
}
define_op_with_assign!(BitAnd, bitand, And);
define_op_with_assign!(BitOr, bitor, Or);
define_op_with_assign!(BitXor, bitxor, Xor);
define_op_with_assign!(Add, add, Add);
define_op_with_assign!(Sub, sub, Sub);

impl Not for Variable {
    type Output = Variable;
    fn not(self) -> Self::Output {
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Inv(self), r));
        r
    }
}
impl Neg for Variable {
    type Output = Variable;
    fn neg(self) -> Self::Output {
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Neg(self), r));
        r
    }
}

impl Variable {
    pub fn set_imm(self, v: u16) {
        let (hi, lo) = u16_to_hi_lo(v);
        push_op(VariableOperation1::Update(UpdateOp::LoadImmLo(self, lo)));
        if hi > 0 {
            push_op(VariableOperation1::Update(UpdateOp::LoadImmHi(self, hi)));
        }
    }
    pub fn assign_from(self, v: Variable) {
        push_op(VariableOperation1::Update(UpdateOp::Mov(self, v)));
    }
    pub fn not0(self) -> Variable {
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Not0(self), r));
        r
    }
    pub fn cnt1(self) -> Variable {
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Cnt1(self), r));
        r
    }
    pub fn log2(self) -> Variable {
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Log2(self), r));
        r
    }
    pub fn lsl_assign(self, u4: u8) {
        assert!(u4 < 16);
        push_op(VariableOperation1::Update(UpdateOp::Lsl(self, u4)));
    }
    pub fn lsr_assign(self, u4: u8) {
        assert!(u4 < 16);
        push_op(VariableOperation1::Update(UpdateOp::Lsr(self, u4)));
    }
    pub fn asr_assign(self, u4: u8) {
        assert!(u4 < 16);
        push_op(VariableOperation1::Update(UpdateOp::Asr(self, u4)));
    }
    pub fn lsl(self, u4: u8) -> Variable {
        assert!(u4 < 16);
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Mov(self), r));
        push_op(VariableOperation1::Update(UpdateOp::Lsl(r, u4)));
        r
    }
    pub fn lsr(self, u4: u8) -> Variable {
        assert!(u4 < 16);
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Mov(self), r));
        push_op(VariableOperation1::Update(UpdateOp::Lsr(r, u4)));
        r
    }
    pub fn asr(self, u4: u8) -> Variable {
        assert!(u4 < 16);
        let r = Variable::new();
        push_op(VariableOperation1::Result(ResultOp::Mov(self), r));
        push_op(VariableOperation1::Update(UpdateOp::Asr(r, u4)));
        r
    }
}

impl Add<u16> for Variable {
    type Output = Variable;
    fn add(self, rhs: u16) -> Variable {
        let r = Variable::new();
        if rhs < 16 {
            push_op(VariableOperation1::Result(
                ResultOp::Addi(self, (rhs & 0b1111) as u8),
                r,
            ));
            r
        } else {
            self + v(rhs)
        }
    }
}
impl Sub<u16> for Variable {
    type Output = Variable;
    fn sub(self, rhs: u16) -> Variable {
        let r = Variable::new();
        if rhs < 16 {
            push_op(VariableOperation1::Result(
                ResultOp::Subi(self, (rhs & 0b1111) as u8),
                r,
            ));
            r
        } else {
            self - v(rhs)
        }
    }
}
impl AddAssign<u16> for Variable {
    fn add_assign(&mut self, rhs: u16) {
        if rhs < 16 {
            push_op(VariableOperation1::Update(UpdateOp::AddiAssign(
                *self,
                (rhs & 0b1111) as u8,
            )));
        } else {
            *self += v(rhs);
        }
    }
}
impl SubAssign<u16> for Variable {
    fn sub_assign(&mut self, rhs: u16) {
        if rhs < 16 {
            push_op(VariableOperation1::Update(UpdateOp::SubiAssign(
                *self,
                (rhs & 0b1111) as u8,
            )));
        } else {
            *self -= v(rhs);
        }
    }
}
