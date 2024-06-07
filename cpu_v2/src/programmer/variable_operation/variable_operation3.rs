use crate::programmer::*;
use std::collections::HashSet;
use std::fmt::Debug;
use std::ops::Sub;

#[derive(Clone, Debug)]
pub enum VariableOperation3 {
    /// alloc new variable
    Alloc(Variable),
    /// create new variable with operation inputs
    Result(ResultOp<Variable>),
    /// update value of variable
    Update(UpdateOp<Variable>),
    /// write last result op result to this variable
    Write(Variable),
    /// after last result/update usage
    Free(Variable),

    // recursive structures
    /// list of operations
    List(Vec<VariableOperation3>),
    /// condition, after condition (free), then, else
    /// then and else will free the same variables
    If(
        CondOp<Variable>,
        Option<Box<VariableOperation3>>,
        Box<VariableOperation3>,
        Option<Box<VariableOperation3>>,
    ),
    /// condition, loop body
    Loop(CondOp<Variable>, Box<VariableOperation3>),

    // external flow control
    /// function name, return addr(output no alloc), params(output no alloc)
    Func(FuncName, Variable, FuncParams),
    /// function name, params, return values(output no alloc)
    Call(FuncName, FuncParams, ReturnValues), //TODO freed params at call (before return)
    /// return addr, return values
    Return(Variable, ReturnValues),
}

struct Context {
    living: HashSet<Variable>,
}
impl Context {
    fn input_no_alloc(&mut self, variable: Variable) {
        self.living.insert(variable);
    }
    fn alloc_write(&mut self, variable: Variable, output: &mut Vec<VariableOperation3>) {
        if self.living.insert(variable) {
            output.push(VariableOperation3::Alloc(variable));
            output.push(VariableOperation3::Write(variable));
        } else {
            unreachable!()
        }
    }
    fn check_alloc(&mut self, variable: Variable, output: &mut Vec<VariableOperation3>) {
        if self.living.insert(variable) {
            output.push(VariableOperation3::Alloc(variable));
        }
    }
    fn check_free(&mut self, scope_info: &ScopeInfo, output: &mut Vec<VariableOperation3>) {
        scope_info.inputs_drop_after.iter().for_each(|v| {
            if self.living.remove(v) {
                output.push(VariableOperation3::Free(*v));
            }
        })
    }
}

impl VariableOperation3 {
    pub fn from(vo2s: VariableOperation2Scope) -> Self {
        let mut ctx = Context {
            living: HashSet::new(),
        };
        let mut output = vec![];

        Self::from_inner(vo2s, &mut ctx, &mut output);

        let len = output.len();
        let r = match len {
            0 => panic!("no operation?"),
            1 => output.remove(0),
            _ => VariableOperation3::List(output),
        };

        r
    }

    fn from_inner(
        vo2s: VariableOperation2Scope,
        ctx: &mut Context,
        output: &mut Vec<VariableOperation3>,
    ) {
        let info = vo2s.info;
        if !matches!(&vo2s.op, VariableOperation2::List(_)) {
            info.inputs.iter().for_each(|v| {
                ctx.check_alloc(*v, output);
            });
        }

        match vo2s.op {
            VariableOperation2::Alloc(_) => {}
            VariableOperation2::Result(op, v) => {
                output.push(VariableOperation3::Result(op));
                ctx.check_free(&info, output);
                ctx.alloc_write(v, output);
            }
            VariableOperation2::Update(op) => {
                output.push(VariableOperation3::Update(op));
            }

            VariableOperation2::List(list) => {
                for scope in list {
                    Self::from_inner(scope, ctx, output);
                }
            }
            VariableOperation2::If(cond, then_block, else_block) => {
                // cond inputs already covered in outer scope
                let mut cond_free = vec![];
                ctx.check_free(&cond.info, &mut cond_free);
                let cond_op = cond.op;
                let cond_free = vec_to_vo3(cond_free);

                let then_else_inputs_drop_after =
                    info.inputs_drop_after.sub(&cond.info.inputs_drop_after);

                let mut then_output = vec![];
                let then_drop_first = then_else_inputs_drop_after.sub(&then_block.info.inputs);
                then_output.extend(then_drop_first.into_iter().map(VariableOperation3::Free)); // drop early
                Self::from_inner(*then_block, ctx, &mut then_output);
                let then_block = vec_to_vo3(then_output).expect("then_block should not be empty");

                let else_block = else_block.map(|else_block| {
                    let mut else_output = vec![];
                    let else_drop_first = then_else_inputs_drop_after.sub(&else_block.info.inputs);
                    else_output.extend(else_drop_first.into_iter().map(VariableOperation3::Free)); // drop early
                    Self::from_inner(*else_block, ctx, &mut else_output);
                    vec_to_vo3(else_output).expect("else_block should not be empty")
                });

                output.push(VariableOperation3::If(
                    cond_op, cond_free, then_block, else_block,
                ))
            }
            VariableOperation2::Loop(cond, loop_block) => {
                let cond_op = cond.op;

                let mut loop_output = vec![];
                Self::from_inner(*loop_block, ctx, &mut loop_output);
                let loop_block = vec_to_vo3(loop_output).expect("loop_block should not be empty");

                output.push(VariableOperation3::Loop(cond_op, loop_block))
            }

            VariableOperation2::Func(name, return_addr, params) => {
                ctx.input_no_alloc(return_addr);
                for param in &params {
                    ctx.input_no_alloc(*param);
                }
                output.push(VariableOperation3::Func(name, return_addr, params));
            }
            VariableOperation2::Call(name, params, return_values) => {
                for value in &return_values {
                    ctx.input_no_alloc(*value);
                }
                output.push(VariableOperation3::Call(name, params, return_values));
            }
            VariableOperation2::Return(return_addr, return_values) => {
                output.push(VariableOperation3::Return(return_addr, return_values));
                //TODO drop all living variables?
            }
        }

        ctx.check_free(&info, output);
    }
}

fn vec_to_vo3(mut list: Vec<VariableOperation3>) -> Option<Box<VariableOperation3>> {
    match list.len() {
        0 => None,
        1 => Some(Box::new(list.remove(0))),
        _ => Some(Box::new(VariableOperation3::List(list))),
    }
}

#[cfg(test)]
fn test_print(vo1: VariableOperation1) {
    let vo2s = VariableOperation2Scope::from(vo1);
    let vo3 = VariableOperation3::from(vo2s);
    println!("vo3: {:#?}", vo3);
}
#[test]
fn test_vo3s_basic() {
    test_print(vo1_basic_program().0);
}
#[test]
fn test_vo3s_if() {
    test_print(vo1_if_program().0);
}
#[test]
fn test_vo3s_loop() {
    test_print(vo1_loop_program().0);
}
