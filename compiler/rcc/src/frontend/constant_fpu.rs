//! Const scalar arithmetic and special functions through the real host shim.
use super::{err, Evaluator, Integer, Value};
use crate::dsl_rt::{self, fix32};
use crate::FpuMulWindow;
use proc_macro2::Span;
use std::collections::HashMap;
use syn::{parse_quote, spanned::Spanned, Expr};

fn fixed(raw: i32) -> Value {
    let lo = raw as u16;
    let hi = (raw >> 16) as u16;
    Value::Native(parse_quote!(fix32::from_words(#lo, #hi)))
}
fn bits(value: &Value) -> Option<i32> {
    let Value::Native(Expr::Call(call)) = value else {
        return None;
    };
    let Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    if path.path.segments.len() != 2 || path.path.segments[0].ident != "fix32" {
        return None;
    }
    let word = |i: usize| super::super::const_eval(&call.args[i], &HashMap::new()).ok();
    match (
        path.path.segments[1].ident.to_string().as_str(),
        call.args.len(),
    ) {
        ("zero", 0) => Some(0),
        ("one", 0) => Some(65536),
        ("from_int", 1) => Some((word(0)? as i16 as i32) << 16),
        ("from_words", 2) => Some(((u32::from(word(1)?) << 16) | u32::from(word(0)?)) as i32),
        _ => None,
    }
}
fn require(value: Value, at: &Expr) -> Result<fix32, syn::Error> {
    bits(&value)
        .map(fix32)
        .ok_or_else(|| err(at, "constant arithmetic requires fix32 operands"))
}

pub(super) fn method(
    evaluator: &mut Evaluator,
    receiver: &Value,
    method: &syn::ExprMethodCall,
    locals: &HashMap<String, Value>,
) -> Result<Option<Value>, syn::Error> {
    if let Value::Native(Expr::Call(call)) = receiver {
        if let Expr::Path(path) = call.func.as_ref() {
            if path.path.segments.len() == 2
                && path.path.segments[1].ident == "new"
                && matches!(
                    path.path.segments[0].ident.to_string().as_str(),
                    "vec2" | "vec3" | "vec4"
                )
            {
                let lane = match method.method.to_string().as_str() {
                    "x" => 0,
                    "y" => 1,
                    "z" => 2,
                    "w" => 3,
                    _ => return Ok(None),
                };
                if !method.args.is_empty() || lane >= call.args.len() {
                    return Err(err(method, "invalid constant vector lane access"));
                }
                return Ok(Some(evaluator.eval(&call.args[lane], locals, None)?));
            }
        }
    }
    let Some(raw) = bits(receiver) else {
        return Ok(None);
    };
    let value = fix32(raw);
    let name = method.method.to_string();
    let binary = matches!(
        name.as_str(),
        "wrapping_add"
            | "wrapping_sub"
            | "wrapping_mul"
            | "less_than"
            | "less_or_equal"
            | "greater_than"
            | "greater_or_equal"
            | "equals"
    );
    if method.args.len() != usize::from(binary) {
        return Err(err(method, "invalid constant fix32 method arity"));
    }
    if binary {
        let rhs = require(
            evaluator.eval(&method.args[0], locals, None)?,
            &method.args[0],
        )?;
        return Ok(Some(match name.as_str() {
            "wrapping_add" => fixed(value.wrapping_add(rhs).0),
            "wrapping_sub" => fixed(value.wrapping_sub(rhs).0),
            "wrapping_mul" => fixed(value.wrapping_mul(rhs).0),
            "less_than" => Value::Bool(value.less_than(rhs)),
            "less_or_equal" => Value::Bool(value.less_or_equal(rhs)),
            "greater_than" => Value::Bool(value.greater_than(rhs)),
            "greater_or_equal" => Value::Bool(value.greater_or_equal(rhs)),
            _ => Value::Bool(value.equals(rhs)),
        }));
    }
    Ok(Some(match name.as_str() {
        "x" => fixed(raw),
        "abs" => fixed(value.abs().0),
        "floor" => fixed(value.floor().0),
        "ceil" => fixed(value.ceil().0),
        "round" => fixed(value.round().0),
        "trunc" => fixed(value.trunc().0),
        "wrapping_neg" => fixed(value.wrapping_neg().0),
        "to_radians" => fixed(value.to_radians().0),
        "lo_bits" => Value::Int(value.lo_bits().into(), Integer::U16),
        "hi_bits" => Value::Int(value.hi_bits().into(), Integer::U16),
        "to_int" => Value::Int(value.to_int().into(), Integer::I16),
        _ => return Ok(None),
    }))
}

pub(super) fn call(
    evaluator: &mut Evaluator,
    call: &syn::ExprCall,
    locals: &HashMap<String, Value>,
) -> Result<Option<Value>, syn::Error> {
    let Expr::Path(path) = call.func.as_ref() else {
        return Ok(None);
    };
    if let Some(name) = path.path.get_ident() {
        let name = name.to_string();
        if let Some(window) = FpuMulWindow::from_intrinsic(&name) {
            if call.args.len() != 2 {
                return Err(err(call, format!("{name}(a, b) takes 2 arguments")));
            }
            let a = require(evaluator.eval(&call.args[0], locals, None)?, &call.args[0])?;
            let b = require(evaluator.eval(&call.args[1], locals, None)?, &call.args[1])?;
            return Ok(Some(fixed(dsl_rt::fix32_mul_shift(
                a.0,
                b.0,
                window.shift(),
            ))));
        }
        if !matches!(
            name.as_str(),
            "frcp" | "frsqrt" | "fsin" | "fcos" | "fsincos"
        ) {
            return Ok(None);
        }
        if call.args.len() != 1 {
            return Err(err(call, "constant special function takes one fix32"));
        }
        let value = require(evaluator.eval(&call.args[0], locals, None)?, &call.args[0])?;
        return Ok(Some(match name.as_str() {
            "frcp" => fixed(dsl_rt::frcp(value).0),
            "frsqrt" => fixed(dsl_rt::frsqrt(value).0),
            "fsin" => fixed(dsl_rt::fsin(value).0),
            "fcos" => fixed(dsl_rt::fcos(value).0),
            _ => {
                let pair = dsl_rt::fsincos(value);
                let sine = fixed(pair.x().0).expression(Span::call_site());
                let cosine = fixed(pair.y().0).expression(Span::call_site());
                Value::Native(parse_quote!(vec2::new(#sine,#cosine)))
            }
        }));
    }
    if path.path.segments.len() == 2 && path.path.segments[1].ident == "new" {
        let count = match path.path.segments[0].ident.to_string().as_str() {
            "vec2" => 2,
            "vec3" => 3,
            "vec4" => 4,
            _ => return Ok(None),
        };
        if call.args.len() != count {
            return Err(err(call, "invalid constant FPU vector arity"));
        }
        let mut result = call.clone();
        for argument in &mut result.args {
            *argument = fixed(require(evaluator.eval(argument, locals, None)?, argument)?.0)
                .expression(argument.span());
        }
        return Ok(Some(Value::Native(Expr::Call(result))));
    }
    Ok(None)
}
