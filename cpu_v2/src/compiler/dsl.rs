//! Rust-embedded DSL frontend (M3): ergonomic layer over FuncBuilder.
//!
//! Unlike the legacy DSL there is no global state: every Variable is bound to
//! an explicit builder context (via Rc<RefCell<FuncBuilder>>), so compilation
//! is re-entrant and testable in parallel. Variables are *handles* to
//! frontend variables; their current SSA value is resolved at use time, so
//! mutation and control-flow joins (phis) behave as one expects.

use crate::isa::Cond;
use crate::compiler::builder::{BoolExpr, FuncBuilder, VarId};
use crate::compiler::driver::Compiler;
use crate::compiler::ir::*;
use std::cell::RefCell;
use std::ops::*;
use std::rc::Rc;

/// builder handle passed to function body closures
#[derive(Clone)]
pub struct B {
    inner: Rc<RefCell<FuncBuilder>>,
}

/// a DSL value (handle to a frontend variable; cheap to clone)
#[derive(Clone)]
pub struct Variable {
    b: Rc<RefCell<FuncBuilder>>,
    var: VarId,
}

impl B {
    fn wrap(&self, var: VarId) -> Variable {
        Variable {
            b: self.inner.clone(),
            var,
        }
    }

    /// a fresh variable holding an immediate constant
    pub fn v(&self, value: u16) -> Variable {
        let mut b = self.inner.borrow_mut();
        let v = b.load_imm(value);
        let var = b.new_var();
        b.set(var, v);
        self.wrap(var)
    }

    // ----- control flow -----

    pub fn if_then(&self, cond: Bool, f: impl FnOnce(&B)) {
        let join = {
            let mut b = self.inner.borrow_mut();
            let cond = cond.lower(&mut b);
            b.begin_if(cond)
        };
        f(self);
        self.inner.borrow_mut().end_if(join);
    }

    pub fn if_else(&self, cond: Bool, then_f: impl FnOnce(&B), else_f: impl FnOnce(&B)) {
        let (else_b, join) = {
            let mut b = self.inner.borrow_mut();
            let cond = cond.lower(&mut b);
            b.begin_if_else(cond)
        };
        then_f(self);
        self.inner.borrow_mut().mid_if_else(else_b, join);
        else_f(self);
        self.inner.borrow_mut().end_if_else(join);
    }

    /// while loop; the condition closure runs (once) at the loop header
    pub fn while_loop(&self, cond: impl FnOnce(&B) -> Bool, f: impl FnOnce(&B)) {
        let (header, body_b, exit) = self.inner.borrow_mut().begin_while();
        {
            let mut b = self.inner.borrow_mut();
            let cond = cond(self).lower(&mut b);
            b.while_cond(cond, header, body_b, exit);
        }
        f(self);
        self.inner.borrow_mut().end_while(header, exit);
    }

    pub fn break_(&self) {
        self.inner.borrow_mut().break_();
    }
    pub fn continue_(&self) {
        self.inner.borrow_mut().continue_();
    }

    /// for i in start..end step `stride` (register-trip-count loop)
    pub fn for_loop(&self, start: &Variable, end: &Variable, stride: u16, f: impl FnOnce(&B, Variable)) {
        let i = start.clone_value();
        self.while_loop(
            |_| i.lt(end),
            |b| {
                f(b, i.clone());
                let next = &i + stride;
                i.assign_from(&next);
            },
        );
    }

    /// for i in range (u4 immediates)
    pub fn for_loop_u4(&self, range: Range<u8>, f: impl FnOnce(&B, Variable)) {
        assert!(range.start < 16 && range.end <= 16);
        let start = self.v(range.start as u16);
        let end = self.v(range.end as u16);
        self.for_loop(&start, &end, 1, f);
    }

    /// for i in (start..end).rev() step `stride`
    pub fn for_loop_rev(&self, start: &Variable, end: &Variable, stride: u16, f: impl FnOnce(&B, Variable)) {
        let one = self.v(1);
        let i = (end - &one).clone_value();
        self.while_loop(
            |_| i.ge(start),
            |b| {
                f(b, i.clone());
                let next = &i - stride;
                i.assign_from(&next);
            },
        );
    }

    // ----- exits -----

    pub fn halt(&self, signal: &Variable) {
        let mut b = self.inner.borrow_mut();
        let s = b.get(signal.var);
        b.halt(s);
    }

    pub fn assert(&self, cond: Bool, signal: u16) {
        self.if_then(cond.not(), |b| {
            let s = b.v(signal);
            b.halt(&s);
        });
    }

    // ----- device io -----

    pub fn dev_recv(&self, device: u8, channel: u8) -> Variable {
        let mut b = self.inner.borrow_mut();
        let v = b.dev_recv(device, channel);
        let var = b.new_var();
        b.set(var, v);
        self.wrap(var)
    }
    pub fn dev_send(&self, device: u8, channel: u8, src: &Variable) {
        let mut b = self.inner.borrow_mut();
        let s = b.get(src.var);
        b.dev_send(device, channel, s);
    }
}

impl Variable {
    fn builder(&self) -> std::cell::RefMut<FuncBuilder> {
        self.b.borrow_mut()
    }
    fn same_builder(&self, other: &Variable) {
        debug_assert!(
            Rc::ptr_eq(&self.b, &other.b),
            "variables from different function builders mixed"
        );
    }
    /// current SSA value of this variable in the current block
    fn vreg(&self) -> VReg {
        self.b.borrow_mut().get(self.var)
    }
    /// bind a fresh variable to `vreg`
    fn mk(&self, vreg: VReg) -> Variable {
        let mut b = self.b.borrow_mut();
        let var = b.new_var();
        b.set(var, vreg);
        Variable {
            b: self.b.clone(),
            var,
        }
    }

    /// a copy of this variable's current value (fresh variable)
    pub fn clone_value(&self) -> Variable {
        let v = self.vreg();
        self.mk(v)
    }
    /// overwrite this variable with `src`'s current value
    pub fn assign_from(&self, src: &Variable) {
        self.same_builder(src);
        let mut b = self.builder();
        let v = b.get(src.var);
        b.set(self.var, v);
    }

    pub fn not0(&self) -> Variable {
        let v = self.vreg();
        let r = self.builder().un(UnOp::Not0, v);
        self.mk(r)
    }
    pub fn cnt1(&self) -> Variable {
        let v = self.vreg();
        let r = self.builder().un(UnOp::Cnt1, v);
        self.mk(r)
    }
    pub fn log2(&self) -> Variable {
        let v = self.vreg();
        let r = self.builder().un(UnOp::Log2, v);
        self.mk(r)
    }

    pub fn lsl(&self, u4: u8) -> Variable {
        let v = self.vreg();
        let r = self.builder().shift(ShiftOp::Lsl, v, u4);
        self.mk(r)
    }
    pub fn lsr(&self, u4: u8) -> Variable {
        let v = self.vreg();
        let r = self.builder().shift(ShiftOp::Lsr, v, u4);
        self.mk(r)
    }
    pub fn asr(&self, u4: u8) -> Variable {
        let v = self.vreg();
        let r = self.builder().shift(ShiftOp::Asr, v, u4);
        self.mk(r)
    }
    pub fn lsl_assign(&self, u4: u8) {
        let v = self.lsl(u4);
        self.assign_from(&v);
    }
    pub fn lsr_assign(&self, u4: u8) {
        let v = self.lsr(u4);
        self.assign_from(&v);
    }
    pub fn asr_assign(&self, u4: u8) {
        let v = self.asr(u4);
        self.assign_from(&v);
    }

    pub fn mul_imm_simple(&self, imm: u8) -> Variable {
        match imm {
            1 => self.clone_value(),
            2 => self.lsl(1),
            3 => &self.lsl(1) + self,
            4 => self.lsl(2),
            6 => (&self.lsl(1) + self).lsl(1),
            8 => self.lsl(3),
            _ => unimplemented!("mul_imm_simple {imm}"),
        }
    }

    // ----- comparisons (produce Bool conditions) -----

    fn cmp(&self, rhs: &Variable, cond: Cond) -> Bool {
        self.same_builder(rhs);
        Bool::Cmp {
            lhs: self.clone(),
            rhs: CmpRhsD::Reg(rhs.clone()),
            cond,
            signed: false,
        }
    }
    fn cmp_imm(&self, imm: u16, cond: Cond) -> Bool {
        Bool::Cmp {
            lhs: self.clone(),
            rhs: CmpRhsD::Imm(imm),
            cond,
            signed: false,
        }
    }
    pub fn lt(&self, rhs: &Variable) -> Bool {
        self.cmp(rhs, Cond::Less)
    }
    pub fn le(&self, rhs: &Variable) -> Bool {
        self.cmp(rhs, Cond::LessEqual)
    }
    pub fn gt(&self, rhs: &Variable) -> Bool {
        self.cmp(rhs, Cond::Greater)
    }
    pub fn ge(&self, rhs: &Variable) -> Bool {
        self.cmp(rhs, Cond::GreaterEqual)
    }
    pub fn eq(&self, rhs: &Variable) -> Bool {
        self.cmp(rhs, Cond::Equal)
    }
    pub fn ne(&self, rhs: &Variable) -> Bool {
        self.cmp(rhs, Cond::NotEqual)
    }
    pub fn lt_imm(&self, imm: u16) -> Bool {
        self.cmp_imm(imm, Cond::Less)
    }
    pub fn le_imm(&self, imm: u16) -> Bool {
        self.cmp_imm(imm, Cond::LessEqual)
    }
    pub fn gt_imm(&self, imm: u16) -> Bool {
        self.cmp_imm(imm, Cond::Greater)
    }
    pub fn ge_imm(&self, imm: u16) -> Bool {
        self.cmp_imm(imm, Cond::GreaterEqual)
    }
    pub fn eq_imm(&self, imm: u16) -> Bool {
        self.cmp_imm(imm, Cond::Equal)
    }
    pub fn ne_imm(&self, imm: u16) -> Bool {
        self.cmp_imm(imm, Cond::NotEqual)
    }

    // ----- memory -----

    pub fn ptr(&self) -> DslPtr {
        DslPtr::new(self.clone())
    }
}

// ---------------------------------------------------------------------------
// operators
// ---------------------------------------------------------------------------

macro_rules! impl_binop {
    ($trait:ident, $method:ident, $assign_trait:ident, $assign_method:ident, $ir_op:expr) => {
        impl $trait<&Variable> for &Variable {
            type Output = Variable;
            fn $method(self, rhs: &Variable) -> Variable {
                self.same_builder(rhs);
                let mut b = self.builder();
                let l = b.get(self.var);
                let r = b.get(rhs.var);
                let v = b.bin($ir_op, l, r);
                drop(b);
                self.mk(v)
            }
        }
        impl $trait<&Variable> for Variable {
            type Output = Variable;
            fn $method(self, rhs: &Variable) -> Variable {
                (&self).$method(rhs)
            }
        }
        impl $trait<Variable> for Variable {
            type Output = Variable;
            fn $method(self, rhs: Variable) -> Variable {
                (&self).$method(&rhs)
            }
        }
        impl $assign_trait<&Variable> for Variable {
            fn $assign_method(&mut self, rhs: &Variable) {
                self.same_builder(rhs);
                let mut b = self.builder();
                let l = b.get(self.var);
                let r = b.get(rhs.var);
                let v = b.bin($ir_op, l, r);
                b.set(self.var, v);
            }
        }
        impl $assign_trait<Variable> for Variable {
            fn $assign_method(&mut self, rhs: Variable) {
                self.$assign_method(&rhs);
            }
        }
        impl $trait<u16> for &Variable {
            type Output = Variable;
            fn $method(self, rhs: u16) -> Variable {
                let c = {
                    let mut b = self.builder();
                    let v = b.load_imm(rhs);
                    let var = b.new_var();
                    b.set(var, v);
                    Variable {
                        b: self.b.clone(),
                        var,
                    }
                };
                self.$method(&c)
            }
        }
        impl $trait<u16> for Variable {
            type Output = Variable;
            fn $method(self, rhs: u16) -> Variable {
                (&self).$method(rhs)
            }
        }
        impl $assign_trait<u16> for Variable {
            fn $assign_method(&mut self, rhs: u16) {
                let c = {
                    let mut b = self.builder();
                    let v = b.load_imm(rhs);
                    let var = b.new_var();
                    b.set(var, v);
                    Variable {
                        b: self.b.clone(),
                        var,
                    }
                };
                self.$assign_method(&c);
            }
        }
    };
}

impl_binop!(Add, add, AddAssign, add_assign, BinOp::Add);
impl_binop!(Sub, sub, SubAssign, sub_assign, BinOp::Sub);
impl_binop!(BitAnd, bitand, BitAndAssign, bitand_assign, BinOp::And);
impl_binop!(BitOr, bitor, BitOrAssign, bitor_assign, BinOp::Or);
impl_binop!(BitXor, bitxor, BitXorAssign, bitxor_assign, BinOp::Xor);

impl Not for &Variable {
    type Output = Variable;
    fn not(self) -> Variable {
        let v = self.vreg();
        let r = self.builder().un(UnOp::Inv, v);
        self.mk(r)
    }
}
impl Not for Variable {
    type Output = Variable;
    fn not(self) -> Variable {
        (&self).not()
    }
}
impl Neg for &Variable {
    type Output = Variable;
    fn neg(self) -> Variable {
        let v = self.vreg();
        let r = self.builder().un(UnOp::Neg, v);
        self.mk(r)
    }
}
impl Neg for Variable {
    type Output = Variable;
    fn neg(self) -> Variable {
        (&self).neg()
    }
}

// ---------------------------------------------------------------------------
// Bool conditions
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum CmpRhsD {
    Reg(Variable),
    Imm(u16),
}

/// a boolean condition, combinable with `&`, `|`, `!` (short-circuit)
#[derive(Clone)]
pub enum Bool {
    Cmp {
        lhs: Variable,
        rhs: CmpRhsD,
        cond: Cond,
        signed: bool,
    },
    And(Box<Bool>, Box<Bool>),
    Or(Box<Bool>, Box<Bool>),
    Not(Box<Bool>),
}

impl Bool {
    fn lower(&self, b: &mut FuncBuilder) -> BoolExpr {
        match self {
            Bool::Cmp {
                lhs,
                rhs,
                cond,
                signed,
            } => {
                let l = b.get(lhs.var);
                let r = match rhs {
                    CmpRhsD::Reg(v) => CmpRhs::Reg(b.get(v.var)),
                    CmpRhsD::Imm(x) => CmpRhs::Imm(*x),
                };
                BoolExpr::Cmp(Cmp {
                    lhs: l,
                    rhs: r,
                    cond: *cond,
                    signed: *signed,
                })
            }
            Bool::And(a, x) => BoolExpr::And(Box::new(a.lower(b)), Box::new(x.lower(b))),
            Bool::Or(a, x) => BoolExpr::Or(Box::new(a.lower(b)), Box::new(x.lower(b))),
            Bool::Not(a) => BoolExpr::Not(Box::new(a.lower(b))),
        }
    }
}
impl BitAnd for Bool {
    type Output = Bool;
    fn bitand(self, rhs: Bool) -> Bool {
        Bool::And(Box::new(self), Box::new(rhs))
    }
}
impl BitOr for Bool {
    type Output = Bool;
    fn bitor(self, rhs: Bool) -> Bool {
        Bool::Or(Box::new(self), Box::new(rhs))
    }
}
impl Not for Bool {
    type Output = Bool;
    fn not(self) -> Bool {
        Bool::Not(Box::new(self))
    }
}

// ---------------------------------------------------------------------------
// pointers, arrays, structs
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct DslPtr {
    pub ptr: Variable,
    pub offset: i16,
}
impl DslPtr {
    pub fn new(ptr: Variable) -> Self {
        Self { ptr, offset: 0 }
    }

    pub fn add_u4(&self, u4: u8) {
        let next = &self.ptr + u4 as u16;
        self.ptr.assign_from(&next);
    }
    pub fn add_var(&self, v: &Variable) {
        let next = &self.ptr + v;
        self.ptr.assign_from(&next);
    }

    pub fn read(&self) -> Variable {
        let base = self.ptr.vreg();
        let r = self.ptr.builder().load_mem(base, self.offset);
        self.ptr.mk(r)
    }
    pub fn read_to(&self, v: &Variable) {
        let r = self.read();
        v.assign_from(&r);
    }
    pub fn write(&self, v: &Variable) {
        self.ptr.same_builder(v);
        let mut b = self.ptr.builder();
        let base = b.get(self.ptr.var);
        let src = b.get(v.var);
        b.store_mem(base, self.offset, src);
    }
}
impl Add<u16> for DslPtr {
    type Output = DslPtr;
    fn add(self, rhs: u16) -> DslPtr {
        DslPtr {
            ptr: self.ptr,
            offset: self.offset + rhs as i16,
        }
    }
}
impl Add<u16> for &DslPtr {
    type Output = DslPtr;
    fn add(self, rhs: u16) -> DslPtr {
        DslPtr {
            ptr: self.ptr.clone(),
            offset: self.offset + rhs as i16,
        }
    }
}
impl AddAssign<u16> for DslPtr {
    fn add_assign(&mut self, rhs: u16) {
        self.offset += rhs as i16;
    }
}
impl Add<&Variable> for DslPtr {
    type Output = DslPtr;
    fn add(self, rhs: &Variable) -> DslPtr {
        DslPtr {
            ptr: &self.ptr + rhs,
            offset: self.offset,
        }
    }
}
impl Add<&Variable> for &DslPtr {
    type Output = DslPtr;
    fn add(self, rhs: &Variable) -> DslPtr {
        DslPtr {
            ptr: &self.ptr + rhs,
            offset: self.offset,
        }
    }
}

#[derive(Clone)]
pub struct DslArray<const STRIDE: usize> {
    pub base: DslPtr,
}
impl<const STRIDE: usize> DslArray<STRIDE> {
    pub fn new(base: DslPtr) -> Self {
        Self { base }
    }
    pub fn index_imm(&self, index: usize) -> DslPtr {
        self.base.clone() + (STRIDE * index) as u16
    }
    pub fn index_reg(&self, index: &Variable) -> DslPtr {
        DslPtr {
            ptr: &self.base.ptr + &index.mul_imm_simple(STRIDE as u8),
            offset: 0,
        }
    }
}

pub trait DslStruct {
    const SIZE: usize;
    type ValueType;
    fn new(ptr: DslPtr) -> Self;
    fn base(&self) -> DslPtr;
    fn read(&self) -> Self::ValueType;
    fn read_to(&self, value: &Self::ValueType);
    fn write(&self, value: Self::ValueType);
}

#[macro_export]
macro_rules! define_struct {
    ($struct_name:ident { $($field_name:ident),+ }) => { paste::paste! {
        #[allow(unused)]
        #[derive(Clone)]
        pub struct $struct_name {
            base: $crate::dsl::DslPtr,
            $($field_name: $crate::dsl::DslPtr,)+
        }
        #[allow(unused)]
        #[derive(Clone)]
        pub struct [< $struct_name Value >] {
            $($field_name: $crate::dsl::Variable,)+
        }
        impl $crate::dsl::DslStruct for $struct_name {
            const SIZE: usize = [$(stringify!($field_name)),+].len();
            type ValueType = [< $struct_name Value >];
            fn new(mut _ptr: $crate::dsl::DslPtr) -> Self {
                let base = _ptr.clone();
                $( let $field_name = _ptr.clone(); _ptr += 1; )+
                Self {
                    base,
                    $($field_name,)+
                }
            }
            fn base(&self) -> $crate::dsl::DslPtr {
                self.base.clone()
            }
            fn read(&self) -> Self::ValueType {
                Self::ValueType {
                    $($field_name: self.$field_name.read(),)+
                }
            }
            fn read_to(&self, value: &Self::ValueType) {
                $(self.$field_name.read_to(&value.$field_name);)+
            }
            fn write(&self, value: Self::ValueType) {
                $(self.$field_name.write(&value.$field_name);)+
            }
        }
    }}
}

// ---------------------------------------------------------------------------
// functions
// ---------------------------------------------------------------------------

pub struct DslFunction<const PARAM: usize, const RETURN: usize> {
    pub name: &'static str,
    pub param_names: [&'static str; PARAM],
    pub return_names: [&'static str; RETURN],
}

impl<const PARAM: usize, const RETURN: usize> DslFunction<PARAM, RETURN> {
    pub fn new(
        name: &'static str,
        param_names: [&'static str; PARAM],
        return_names: [&'static str; RETURN],
    ) -> Self {
        Self {
            name,
            param_names,
            return_names,
        }
    }

    /// build the function body and register it with the compiler.
    /// `f` receives the builder handle, the parameter variables, and a `ret`
    /// callback. every execution path must end with ret/halt (checked by the
    /// builder at finish).
    pub fn compile(
        &self,
        compiler: &mut Compiler,
        f: impl FnOnce(&B, [Variable; PARAM], &dyn Fn(&B, [Variable; RETURN])),
    ) {
        let (builder, params) = FuncBuilder::new(self.name, PARAM, RETURN);
        let inner = Rc::new(RefCell::new(builder));
        inner
            .borrow_mut()
            .set_names(&self.param_names, &self.return_names);
        let b = B { inner: inner.clone() };
        let param_vars: [Variable; PARAM] = params
            .into_iter()
            .map(|var| b.wrap(var))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap_or_else(|_| panic!("param count mismatch"));

        f(&b, param_vars, &|b: &B, values: [Variable; RETURN]| {
            let mut bb = b.inner.borrow_mut();
            let vregs: Vec<VReg> = values.iter().map(|v| bb.get(v.var)).collect();
            bb.ret(&vregs);
        });

        drop(b);
        let builder = match Rc::try_unwrap(inner) {
            Ok(b) => b.into_inner(),
            Err(_) => panic!("a Variable escaped the function body closure"),
        };
        compiler.add_func(builder.finish());
    }

    /// call this function from another function's body
    pub fn call(&self, b: &B, args: [&Variable; PARAM]) -> [Variable; RETURN] {
        let ret_vregs = {
            let mut bb = b.inner.borrow_mut();
            let arg_vregs: Vec<VReg> = args.iter().map(|a| bb.get(a.var)).collect();
            bb.call(self.name, &arg_vregs, RETURN)
        };
        let vars: Vec<Variable> = ret_vregs
            .into_iter()
            .map(|v| {
                let mut bb = b.inner.borrow_mut();
                let var = bb.new_var();
                bb.set(var, v);
                Variable {
                    b: b.inner.clone(),
                    var,
                }
            })
            .collect();
        vars.try_into()
            .unwrap_or_else(|_| panic!("return value count mismatch"))
    }
}
