//! Bounded constant evaluation shared by checked folding and storage planning.
//!
//! Values remain syntax until the existing target layout pass serializes them.
//! No user host code is executed. Storage planning enables only its explicit
//! allocation primitive and retains original expressions for rustc checking.
use super::{err, lit_int_value};
use proc_macro2::Span;
use quote::quote_spanned;
use std::collections::{HashMap, HashSet};
use syn::{parse_quote, spanned::Spanned, visit_mut::VisitMut, Expr, Item, Pat, Stmt, Type};

#[path = "constant_fpu.rs"]
mod fpu;

const STEPS: usize = 100_000;
const DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Integer {
    Inferred,
    U8,
    U16,
    I16,
    U32,
    Usize,
}
impl Integer {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "u8" => Self::U8,
            "u16" => Self::U16,
            "i16" => Self::I16,
            "u32" => Self::U32,
            "usize" => Self::Usize,
            _ => return None,
        })
    }
    fn of(ty: &Type) -> Option<Self> {
        match ty {
            Type::Path(p) => Self::named(&p.path.get_ident()?.to_string()),
            Type::Array(a) => Self::of(&a.elem),
            Type::Slice(s) => Self::of(&s.elem),
            Type::Reference(r) => Self::of(&r.elem),
            _ => None,
        }
    }
    fn bounds(self) -> (i128, i128) {
        match self {
            Self::U8 => (0, 255),
            Self::U16 => (0, 65535),
            Self::I16 => (-32768, 32767),
            Self::U32 => (0, u32::MAX.into()),
            // Sizes are host-independent compile-time quantities, never CPU pointers.
            Self::Usize => (0, u64::MAX.into()),
            Self::Inferred => (i64::MIN.into(), u64::MAX.into()),
        }
    }
    fn checked(self, n: i128, at: &Expr) -> Result<Value, syn::Error> {
        let (lo, hi) = self.bounds();
        if !(lo..=hi).contains(&n) {
            return Err(err(at, "constant integer overflow"));
        }
        Ok(Value::Int(n, self))
    }
}

#[derive(Clone)]
enum Value {
    Int(i128, Integer),
    Bool(bool),
    Struct(syn::ExprStruct),
    Tuple(Vec<Value>),
    Array(Vec<Value>),
    Bytes(Vec<u8>),
    // Existing builtin constructors and fieldless enum variants keep their ABI.
    Native(Expr),
    Unit,
}
impl Value {
    fn expression(&self, span: Span) -> Expr {
        match self {
            Self::Int(n, kind) => {
                let suffix = match kind {
                    Integer::U8 => "u8",
                    Integer::I16 => "i16",
                    Integer::U16 => "u16",
                    _ => "",
                };
                let lit = syn::LitInt::new(&format!("{}{suffix}", n.abs()), span);
                if *n < 0 {
                    syn::parse2(quote_spanned!(span=> -#lit)).unwrap()
                } else {
                    syn::parse2(quote_spanned!(span=> #lit)).unwrap()
                }
            }
            Self::Bool(b) => syn::parse2(quote_spanned!(span=> #b)).unwrap(),
            Self::Struct(s) => Expr::Struct(s.clone()),
            Self::Tuple(values) => {
                let values = values.iter().map(|v| v.expression(span));
                syn::parse2(quote_spanned!(span=> (#(#values,)*))).unwrap()
            }
            Self::Array(values) => {
                let values = values.iter().map(|v| v.expression(span));
                syn::parse2(quote_spanned!(span=> [#(#values),*])).unwrap()
            }
            Self::Bytes(bytes) => {
                let literal = syn::LitByteStr::new(bytes, span);
                syn::parse2(quote_spanned!(span=> #literal)).unwrap()
            }
            Self::Native(e) => e.clone(),
            Self::Unit => parse_quote!(()),
        }
    }
    fn number(&self, at: &Expr) -> Result<(i128, Integer), syn::Error> {
        match self {
            Self::Int(n, kind) => Ok((*n, *kind)),
            _ => Err(err(at, "expected a constant integer")),
        }
    }
    fn boolean(&self, at: &Expr) -> Result<bool, syn::Error> {
        match self {
            Self::Bool(b) => Ok(*b),
            _ => Err(err(at, "expected a constant bool")),
        }
    }
}

struct Evaluator {
    methods: super::value_methods::Methods,
    enum_values: HashMap<(String, String), u16>,
    constants: HashMap<String, syn::ItemConst>,
    functions: HashMap<String, syn::ItemFn>,
    fields: HashMap<String, HashMap<String, Type>>,
    values: HashMap<String, Value>,
    active: HashSet<String>,
    steps: usize,
    depth: usize,
    storage: Option<super::static_buffers::Planner>,
}
impl Evaluator {
    fn tick(&mut self, e: &Expr) -> Result<(), syn::Error> {
        if self.steps == 0 {
            return Err(err(e, "constant evaluation step budget exceeded"));
        }
        self.steps -= 1;
        Ok(())
    }
    fn constant(&mut self, name: &str, at: &Expr) -> Result<Value, syn::Error> {
        if let Some(value) = self.values.get(name) {
            return Ok(value.clone());
        }
        let item = self
            .constants
            .get(name)
            .cloned()
            .ok_or_else(|| err(at, format!("constant evaluation cannot read `{name}`")))?;
        if !self.active.insert(name.to_owned()) {
            return Err(err(at, "cyclic constant dependency"));
        }
        if self.depth == DEPTH {
            return Err(err(at, "constant dependency depth budget exceeded"));
        }
        self.depth += 1;
        let previous = self.storage.as_mut().and_then(|planner| {
            let previous = planner.context.take();
            planner.context = planner
                .declarations
                .get(name)
                .map(|a| (name.to_owned(), *a));
            previous
        });
        let value = self.eval(&item.expr, &HashMap::new(), Integer::of(&item.ty));
        if let Some(planner) = &mut self.storage {
            planner.context = previous;
        }
        self.depth -= 1;
        self.active.remove(name);
        let value = value?;
        self.values.insert(name.into(), value.clone());
        Ok(value)
    }
    fn block(
        &mut self,
        block: &syn::Block,
        outer: &HashMap<String, Value>,
        expected: Option<Integer>,
    ) -> Result<Value, syn::Error> {
        let mut locals = outer.clone();
        for statement in &block.stmts {
            match statement {
                Stmt::Local(local) => {
                    let (pattern, kind) = match &local.pat {
                        Pat::Type(p) => (p.pat.as_ref(), Integer::of(&p.ty)),
                        p => (p, None),
                    };
                    let Pat::Ident(name) = pattern else {
                        return Err(err(pattern, "constant let requires a named binding"));
                    };
                    if name.mutability.is_some() || name.by_ref.is_some() || name.subpat.is_some() {
                        return Err(err(name, "constant let bindings must be immutable values"));
                    }
                    let Some((_, initializer)) = &local.init else {
                        return Err(err(local, "constant let requires an initializer"));
                    };
                    let value = self.eval(initializer, &locals, kind)?;
                    locals.insert(name.ident.to_string(), value);
                }
                Stmt::Expr(e) => return self.eval(e, &locals, expected),
                Stmt::Semi(Expr::Macro(m), _)
                    if m.mac.path.is_ident("assert") || m.mac.path.is_ident("debug_assert") =>
                {
                    let condition: Expr = syn::parse2(m.mac.tokens.clone())?;
                    if !self.eval(&condition, &locals, None)?.boolean(&condition)? {
                        return Err(err(m, "constant assertion failed"));
                    }
                }
                Stmt::Semi(e, _) => {
                    return Err(err(
                        e,
                        "constant functions support let bindings and a tail value",
                    ));
                }
                other => return Err(err(other, "item not supported in a constant block")),
            }
        }
        Ok(Value::Unit)
    }
    fn eval(
        &mut self,
        e: &Expr,
        locals: &HashMap<String, Value>,
        expected: Option<Integer>,
    ) -> Result<Value, syn::Error> {
        self.tick(e)?;
        match e {
            Expr::Lit(l) => match &l.lit {
                syn::Lit::Int(n) => {
                    let kind = Integer::named(n.suffix())
                        .or(expected)
                        .unwrap_or(Integer::Inferred);
                    kind.checked(lit_int_value(n)?.into(), e)
                }
                syn::Lit::Byte(b) => Ok(Value::Int(b.value().into(), Integer::U8)),
                syn::Lit::Bool(b) => Ok(Value::Bool(b.value)),
                syn::Lit::ByteStr(b) => Ok(Value::Bytes(b.value())),
                _ => Err(err(e, "unsupported constant literal")),
            },
            Expr::Path(p) => {
                if let Some(name) = p.path.get_ident() {
                    let name = name.to_string();
                    if let Some(v) = locals.get(&name) {
                        Ok(v.clone())
                    } else {
                        self.constant(&name, e)
                    }
                } else if p.path.segments.len() == 2 {
                    Ok(Value::Native(e.clone()))
                } else {
                    Err(err(e, "unsupported constant path"))
                }
            }
            Expr::Paren(p) => self.eval(&p.expr, locals, expected),
            Expr::Group(g) => self.eval(&g.expr, locals, expected),
            Expr::Reference(r) if r.mutability.is_none() => self.eval(&r.expr, locals, expected),
            Expr::Block(b) => self.block(&b.block, locals, expected),
            Expr::Unsafe(b) => self.block(&b.block, locals, expected),
            Expr::If(i) => {
                if self.eval(&i.cond, locals, None)?.boolean(&i.cond)? {
                    self.block(&i.then_branch, locals, expected)
                } else if let Some((_, branch)) = &i.else_branch {
                    self.eval(branch, locals, expected)
                } else {
                    Ok(Value::Unit)
                }
            }
            Expr::Struct(s) => {
                let owner = s.path.segments.last().unwrap().ident.to_string();
                let fields = self.fields.get(&owner).cloned().unwrap_or_default();
                let mut value = s.clone();
                for field in &mut value.fields {
                    let ty = match &field.member {
                        syn::Member::Named(name) => fields.get(&name.to_string()),
                        _ => None,
                    };
                    field.expr = self
                        .eval(&field.expr, locals, ty.and_then(Integer::of))?
                        .expression(field.expr.span());
                    field.colon_token = Some(Default::default());
                }
                if let Some(rest) = &s.rest {
                    let Value::Struct(base) = self.eval(rest, locals, None)? else {
                        return Err(err(rest, "struct update requires a constant struct"));
                    };
                    for field in base.fields {
                        if !value.fields.iter().any(|f| f.member == field.member) {
                            value.fields.push(field);
                        }
                    }
                    value.rest = None;
                    value.dot2_token = None;
                }
                Ok(Value::Struct(value))
            }
            Expr::Tuple(t) => Ok(Value::Tuple(
                t.elems
                    .iter()
                    .map(|x| self.eval(x, locals, None))
                    .collect::<Result<_, _>>()?,
            )),
            Expr::Array(a) => Ok(Value::Array(
                a.elems
                    .iter()
                    .map(|x| self.eval(x, locals, expected))
                    .collect::<Result<_, _>>()?,
            )),
            Expr::Repeat(r) => {
                let count = self.eval(&r.len, locals, None)?.number(&r.len)?.0;
                if count < 0 || count as u128 > self.steps as u128 {
                    return Err(err(e, "constant array exceeds evaluation budget"));
                }
                self.steps -= count as usize;
                Ok(Value::Array(vec![
                    self.eval(&r.expr, locals, expected)?;
                    count as usize
                ]))
            }
            Expr::Field(f) => match (self.eval(&f.base, locals, None)?, &f.member) {
                (Value::Struct(s), member) => {
                    let field = s
                        .fields
                        .iter()
                        .find(|f| &f.member == member)
                        .ok_or_else(|| err(e, "unknown constant field"))?;
                    self.eval(&field.expr, locals, expected)
                }
                (Value::Tuple(values), syn::Member::Unnamed(index)) => values
                    .get(index.index as usize)
                    .cloned()
                    .ok_or_else(|| err(e, "constant tuple index out of bounds")),
                _ => Err(err(e, "constant field access requires a struct or tuple")),
            },
            Expr::Index(i) => {
                let index = self.eval(&i.index, locals, None)?.number(&i.index)?.0;
                let value = self.eval(&i.expr, locals, None)?;
                if index < 0 {
                    return Err(err(e, "constant index out of bounds"));
                }
                match value {
                    Value::Array(values) => values.get(index as usize).cloned(),
                    Value::Bytes(bytes) => bytes
                        .get(index as usize)
                        .map(|b| Value::Int((*b).into(), Integer::U8)),
                    _ => return Err(err(e, "constant index requires an array or byte slice")),
                }
                .ok_or_else(|| err(e, "constant index out of bounds"))
            }
            Expr::MethodCall(m) if m.method == "len" && m.args.is_empty() => {
                let len = match self.eval(&m.receiver, locals, None)? {
                    Value::Array(v) => v.len(),
                    Value::Bytes(v) => v.len(),
                    _ => return Err(err(e, "constant len requires an array or byte slice")),
                };
                Ok(Value::Int(len as i128, Integer::Usize))
            }
            Expr::MethodCall(m) => {
                let receiver = self.eval(&m.receiver, locals, None)?;
                if let Some(value) = fpu::method(self, &receiver, m, locals)? {
                    return Ok(value);
                }
                let owner = match &receiver {
                    Value::Struct(value) => value.path.segments.last().map(|s| s.ident.to_string()),
                    Value::Native(Expr::Path(path)) if path.path.segments.len() == 2 => {
                        Some(path.path.segments[0].ident.to_string())
                    }
                    _ => None,
                }
                .ok_or_else(|| err(e, "constant value method requires a struct or enum"))?;
                let function = super::value_methods::constant_value_function(
                    &self.methods,
                    &owner,
                    &m.method.to_string(),
                )
                .filter(|name| self.functions.contains_key(&name.to_string()))
                .ok_or_else(|| {
                    err(
                        e,
                        "only const value methods are allowed in constant expressions",
                    )
                })?;
                let receiver = receiver.expression(m.receiver.span());
                let args = &m.args;
                let call: Expr = parse_quote!(#function(#receiver,#args));
                self.eval(&call, locals, expected)
            }
            Expr::Cast(c) => {
                if matches!(c.ty.as_ref(),Type::Path(p) if p.path.is_ident("Ptr")) {
                    // Preserve the existing checked Ptr projection/word ABI.
                    let word = self.eval(&c.expr, locals, None)?.number(&c.expr)?.0 as u16;
                    return Ok(Value::Native(parse_quote!(#word as Ptr)));
                }
                let kind = Integer::of(&c.ty).ok_or_else(|| err(e, "unsupported constant cast"))?;
                let value = self.eval(&c.expr, locals, None)?;
                let n = match value {
                    Value::Bool(b) => i128::from(b),
                    Value::Native(Expr::Path(path)) if path.path.segments.len() == 2 => {
                        let key = (
                            path.path.segments[0].ident.to_string(),
                            path.path.segments[1].ident.to_string(),
                        );
                        i128::from(
                            *self
                                .enum_values
                                .get(&key)
                                .ok_or_else(|| err(e, "constant cast requires a fieldless enum"))?,
                        )
                    }
                    other => other.number(&c.expr)?.0,
                };
                let n = match kind {
                    Integer::U8 => n as u8 as i128,
                    Integer::U16 => n as u16 as i128,
                    Integer::I16 => n as i16 as i128,
                    Integer::U32 => n as u32 as i128,
                    Integer::Usize => n as u64 as i128,
                    Integer::Inferred => unreachable!(),
                };
                Ok(Value::Int(n, kind))
            }
            Expr::Unary(u) => {
                if matches!(u.op, syn::UnOp::Deref(_)) {
                    return self.eval(&u.expr, locals, expected);
                }
                if matches!(u.op, syn::UnOp::Neg(_))
                    && matches!(u.expr.as_ref(),Expr::Lit(l) if matches!(&l.lit,syn::Lit::Int(n) if n.suffix()=="i16" && matches!(lit_int_value(n),Ok(32768))))
                {
                    return Integer::I16.checked(-32768, e);
                }
                // Permit the magnitude of i16::MIN before applying unary minus.
                let value = self.eval(
                    &u.expr,
                    locals,
                    if matches!(u.op, syn::UnOp::Neg(_)) {
                        None
                    } else {
                        expected
                    },
                )?;
                match (&u.op, value) {
                    (syn::UnOp::Not(_), Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (syn::UnOp::Neg(_), Value::Int(n, kind)) => {
                        expected.unwrap_or(kind).checked(-n, e)
                    }
                    (syn::UnOp::Not(_), Value::Int(n, kind)) => {
                        let kind = expected.unwrap_or(kind);
                        let value = match kind {
                            Integer::U8 => (!n as u8) as i128,
                            Integer::U16 => (!n as u16) as i128,
                            Integer::I16 => (!n as i16) as i128,
                            Integer::U32 => (!n as u32) as i128,
                            _ => (!n as u64) as i128,
                        };
                        kind.checked(value, e)
                    }
                    _ => Err(err(e, "unsupported constant unary operation")),
                }
            }
            Expr::Binary(b) => self.binary(b, locals, expected),
            Expr::Call(c) => {
                let Expr::Path(p) = c.func.as_ref() else {
                    return Err(err(e, "constant calls require a named function"));
                };
                let name = p.path.get_ident().map(ToString::to_string);
                let allocation = self
                    .methods
                    .get(&("StaticBuffer".into(), "new".into()))
                    .is_some_and(|m| name.as_deref() == Some(m.function.to_string().as_str()));
                if allocation {
                    if c.args.len() != 2 {
                        return Err(err(e, "StaticBuffer::new requires bytes and alignment"));
                    }
                    let mut size = |argument: &Expr| -> Result<usize, syn::Error> {
                        let n = self
                            .eval(argument, locals, Some(Integer::Usize))?
                            .number(argument)?
                            .0;
                        usize::try_from(n).map_err(|_| {
                            err(argument, "buffer size is outside host allocation range")
                        })
                    };
                    let bytes = size(&c.args[0])?;
                    let alignment = size(&c.args[1])?;
                    let planner = self
                        .storage
                        .as_mut()
                        .ok_or_else(|| err(e, "StaticBuffer::new requires #[static_buffer]"))?;
                    let expression = planner.allocate(bytes, alignment, e)?;
                    let Expr::Struct(value) = expression else {
                        unreachable!()
                    };
                    return Ok(Value::Struct(value));
                }
                if let Some(f) = name.and_then(|n| self.functions.get(&n)).cloned() {
                    if self.depth == DEPTH {
                        return Err(err(e, "constant call depth budget exceeded"));
                    }
                    if f.sig.inputs.len() != c.args.len() {
                        return Err(err(e, "constant function argument count mismatch"));
                    }
                    let mut args = HashMap::new();
                    for (parameter, argument) in f.sig.inputs.iter().zip(&c.args) {
                        let syn::FnArg::Typed(p) = parameter else {
                            return Err(err(parameter, "unsupported constant receiver"));
                        };
                        let Pat::Ident(id) = p.pat.as_ref() else {
                            return Err(err(parameter, "constant parameter must be named"));
                        };
                        args.insert(
                            id.ident.to_string(),
                            self.eval(argument, locals, Integer::of(&p.ty))?,
                        );
                    }
                    let result_kind = match &f.sig.output {
                        syn::ReturnType::Type(_, ty) => Integer::of(ty),
                        _ => None,
                    };
                    self.depth += 1;
                    let result = self.block(&f.block, &args, result_kind);
                    self.depth -= 1;
                    result
                } else if let Some(value) = fpu::call(self, c, locals)? {
                    Ok(value)
                } else if p.path.segments.len() == 2
                    && matches!(
                        p.path.segments[0].ident.to_string().as_str(),
                        "Buf" | "fix32"
                    )
                {
                    let mut result = c.clone();
                    for argument in &mut result.args {
                        *argument = self
                            .eval(argument, locals, None)?
                            .expression(argument.span());
                    }
                    if p.path.segments[0].ident == "fix32" {
                        let method = p.path.segments[1].ident.to_string();
                        let number =
                            |index: usize| super::const_eval(&result.args[index], &HashMap::new());
                        let bits = match (method.as_str(), result.args.len()) {
                            ("one", 0) => Some(65536),
                            ("from_ratio", 2) => Some(
                                crate::fixed_constants::ratio(number(0)? as i16, number(1)?)
                                    .ok_or_else(|| {
                                        err(e, "fix32 ratio denominator must not be zero")
                                    })?,
                            ),
                            ("degrees", 1) => {
                                Some(crate::fixed_constants::degrees(number(0)? as i16))
                            }
                            _ => None,
                        };
                        if let Some(bits) = bits {
                            let lo = bits as u16;
                            let hi = (bits >> 16) as u16;
                            return Ok(Value::Native(parse_quote!(fix32::from_words(#lo,#hi))));
                        }
                    }
                    Ok(Value::Native(Expr::Call(result)))
                } else {
                    Err(err(
                        e,
                        "only const fn calls are allowed in constant expressions",
                    ))
                }
            }
            _ => Err(err(e, "unsupported expression in constant evaluation")),
        }
    }
    fn binary(
        &mut self,
        b: &syn::ExprBinary,
        locals: &HashMap<String, Value>,
        expected: Option<Integer>,
    ) -> Result<Value, syn::Error> {
        use syn::BinOp::*;
        let at = Expr::Binary(b.clone());
        let left = self.eval(&b.left, locals, expected)?;
        if matches!(b.op, And(_) | Or(_)) {
            let l = left.boolean(&b.left)?;
            if matches!(b.op, And(_)) && !l || matches!(b.op, Or(_)) && l {
                return Ok(Value::Bool(l));
            }
            return Ok(Value::Bool(
                self.eval(&b.right, locals, None)?.boolean(&b.right)?,
            ));
        }
        let right = self.eval(&b.right, locals, expected)?;
        if let (Value::Bool(a), Value::Bool(c)) = (&left, &right) {
            return match b.op {
                Eq(_) => Ok(Value::Bool(a == c)),
                Ne(_) => Ok(Value::Bool(a != c)),
                _ => Err(err(&at, "unsupported constant bool operation")),
            };
        }
        let (a, ka) = left.number(&b.left)?;
        let (c, kc) = right.number(&b.right)?;
        let kind = expected.unwrap_or(if ka != Integer::Inferred { ka } else { kc });
        let value = match b.op {
            Eq(_) => return Ok(Value::Bool(a == c)),
            Ne(_) => return Ok(Value::Bool(a != c)),
            Lt(_) => return Ok(Value::Bool(a < c)),
            Le(_) => return Ok(Value::Bool(a <= c)),
            Gt(_) => return Ok(Value::Bool(a > c)),
            Ge(_) => return Ok(Value::Bool(a >= c)),
            Add(_) => a.checked_add(c),
            Sub(_) => a.checked_sub(c),
            Mul(_) => a.checked_mul(c),
            Div(_) => a.checked_div(c),
            Rem(_) => a.checked_rem(c),
            BitAnd(_) => Some(a & c),
            BitOr(_) => Some(a | c),
            BitXor(_) => Some(a ^ c),
            Shl(_) | Shr(_) => {
                let width = match kind {
                    Integer::U8 => 8,
                    Integer::U16 | Integer::I16 => 16,
                    Integer::U32 => 32,
                    _ => 64,
                };
                if c < 0 || c >= width {
                    return Err(err(&at, "constant shift exceeds integer width"));
                }
                if matches!(b.op, Shl(_)) {
                    let shifted = a << c;
                    Some(match kind {
                        Integer::U8 => shifted as u8 as i128,
                        Integer::U16 => shifted as u16 as i128,
                        Integer::I16 => shifted as i16 as i128,
                        Integer::U32 => shifted as u32 as i128,
                        _ => shifted as u64 as i128,
                    })
                } else {
                    Some(a >> c)
                }
            }
            _ => return Err(err(&at, "unsupported constant binary operation")),
        }
        .ok_or_else(|| err(&at, "constant arithmetic overflow or division by zero"))?;
        kind.checked(value, &at)
    }
}

fn evaluator(
    files: &[syn::File],
    methods: &super::value_methods::Methods,
) -> Result<Evaluator, (usize, syn::Error)> {
    let mut evaluator = Evaluator {
        methods: methods.clone(),
        enum_values: HashMap::new(),
        constants: HashMap::new(),
        functions: HashMap::new(),
        fields: HashMap::new(),
        values: HashMap::new(),
        active: HashSet::new(),
        steps: STEPS,
        depth: 0,
        storage: None,
    };
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            match item {
                Item::Enum(e) => {
                    for (name, value) in
                        super::enum_values::discriminants(e).map_err(|error| (fi, error))?
                    {
                        evaluator
                            .enum_values
                            .insert((e.ident.to_string(), name), value);
                    }
                }
                Item::Const(c) => {
                    if evaluator
                        .constants
                        .insert(c.ident.to_string(), c.clone())
                        .is_some()
                    {
                        return Err((fi, err(c, "duplicate constant")));
                    }
                }
                Item::Fn(f) if f.sig.constness.is_some() => {
                    if !f.sig.generics.params.is_empty() {
                        return Err((fi, err(f, "constant functions must be nongeneric")));
                    }
                    evaluator
                        .functions
                        .insert(f.sig.ident.to_string(), f.clone());
                }
                Item::Struct(s) => {
                    evaluator.fields.insert(
                        s.ident.to_string(),
                        s.fields
                            .iter()
                            .filter_map(|f| Some((f.ident.as_ref()?.to_string(), f.ty.clone())))
                            .collect(),
                    );
                }
                Item::Static(s) if matches!(s.ty.as_ref(), Type::Path(p) if p.path.is_ident("AssetBytes")) =>
                {
                    let name = &s.ident;
                    let ty = &s.ty;
                    let expression = &s.expr;
                    evaluator
                        .constants
                        .insert(name.to_string(), parse_quote!(const #name:#ty=#expression;));
                }
                _ => {}
            }
        }
    }
    Ok(evaluator)
}

fn register_storage_statics(
    evaluator: &mut Evaluator,
    files: &[syn::File],
    names: &HashSet<String>,
) {
    for item in files.iter().flat_map(|f| &f.items) {
        if let Item::Static(s) = item {
            if s.mutability.is_none() && names.contains(&s.ident.to_string()) {
                let (name, ty, expr) = (&s.ident, &s.ty, &s.expr);
                evaluator
                    .constants
                    .insert(name.to_string(), parse_quote!(const #name: #ty = #expr;));
            }
        }
    }
}

pub(super) struct Sizes(Evaluator);

/// The same bounded evaluator, with allocation enabled only inside marked roots.
pub(super) struct StorageEvaluation(Evaluator);
impl StorageEvaluation {
    pub(super) fn new(
        files: &[syn::File],
        methods: &super::value_methods::Methods,
        planner: super::static_buffers::Planner,
    ) -> Result<Self, (usize, syn::Error)> {
        let mut evaluator = evaluator(files, methods)?;
        let statics = planner.declarations.keys().cloned().collect();
        register_storage_statics(&mut evaluator, files, &statics);
        evaluator.storage = Some(planner);
        Ok(Self(evaluator))
    }
    pub(super) fn description(&mut self, name: &str, at: &Expr) -> Result<Expr, syn::Error> {
        self.0.steps = STEPS;
        Ok(self.0.constant(name, at)?.expression(at.span()))
    }
    pub(super) fn finish(self) -> Vec<super::static_buffers::StaticBuffer> {
        self.0.storage.unwrap().buffers
    }
}
impl Sizes {
    pub(super) fn new(files: &[syn::File]) -> Result<Self, (usize, syn::Error)> {
        evaluator(files, &Default::default()).map(Self)
    }
    pub(super) fn bytes(&mut self, expression: &Expr) -> Result<usize, syn::Error> {
        self.0.steps = STEPS;
        self.0.depth = 0;
        self.0.active.clear();
        let value = self
            .0
            .eval(expression, &HashMap::new(), Some(Integer::Usize))?
            .number(expression)?
            .0;
        usize::try_from(value)
            .map_err(|_| err(expression, "asset size is outside host allocation range"))
    }
}

/// Expand ordinary assertions after rustc but before resource/value lowering.
/// Const function assertions stay available to CTFE until its work is complete.
pub(super) fn runtime_assertions(files: &mut [syn::File], debug: bool, include_const: bool) {
    struct RuntimeAssertions {
        debug: bool,
        include_const: bool,
        in_const: bool,
    }
    impl VisitMut for RuntimeAssertions {
        fn visit_item_fn_mut(&mut self, function: &mut syn::ItemFn) {
            let previous = self.in_const;
            self.in_const = function.sig.constness.is_some();
            self.visit_block_mut(&mut function.block);
            self.in_const = previous;
        }
        fn visit_impl_item_method_mut(&mut self, method: &mut syn::ImplItemMethod) {
            let previous = self.in_const;
            self.in_const = method.sig.constness.is_some();
            self.visit_block_mut(&mut method.block);
            self.in_const = previous;
        }
        fn visit_expr_mut(&mut self, expression: &mut Expr) {
            if let Expr::Macro(mac) = expression {
                let diagnostic = mac.mac.path.is_ident("debug_assert");
                if (self.include_const || !self.in_const)
                    && (diagnostic || mac.mac.path.is_ident("assert"))
                {
                    if let Ok(condition) = syn::parse2::<Expr>(mac.mac.tokens.clone()) {
                        *expression = if diagnostic && !self.debug {
                            parse_quote!({})
                        } else {
                            syn::parse_quote_spanned!(mac.mac.path.span()=> assert(#condition,65535u16))
                        };
                        return;
                    }
                }
            }
            syn::visit_mut::visit_expr_mut(self, expression);
        }
    }
    for file in files.iter_mut() {
        RuntimeAssertions {
            debug,
            include_const,
            in_const: false,
        }
        .visit_file_mut(file);
    }
}

/// Fold constant declarations/initializers, then substitute aggregate constants
/// at uses. Scalar constants keep the existing debug representation and ABI.
pub(super) fn prepare(
    files: &mut [syn::File],
    methods: &super::value_methods::Methods,
    debug: bool,
    storage_statics: &HashSet<String>,
) -> Result<(), (usize, syn::Error)> {
    let mut evaluator = evaluator(files, methods)?;
    register_storage_statics(&mut evaluator, files, storage_statics);
    struct Lengths<'a> {
        evaluator: &'a mut Evaluator,
        error: Option<syn::Error>,
    }
    impl VisitMut for Lengths<'_> {
        fn visit_type_array_mut(&mut self, array: &mut syn::TypeArray) {
            syn::visit_mut::visit_type_array_mut(self, array);
            if self.error.is_none() {
                match self.evaluator.eval(&array.len, &HashMap::new(), None) {
                    Ok(value) => array.len = value.expression(array.len.span()),
                    Err(error) => self.error = Some(error),
                }
            }
        }
        fn visit_generic_argument_mut(&mut self, argument: &mut syn::GenericArgument) {
            syn::visit_mut::visit_generic_argument_mut(self, argument);
            if let syn::GenericArgument::Const(expression) = argument {
                if self.error.is_none() {
                    match self.evaluator.eval(expression, &HashMap::new(), None) {
                        Ok(value) => *expression = value.expression(expression.span()),
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }
    }
    for (fi, file) in files.iter_mut().enumerate() {
        let mut lengths = Lengths {
            evaluator: &mut evaluator,
            error: None,
        };
        lengths.visit_file_mut(file);
        if let Some(error) = lengths.error {
            return Err((fi, error));
        }
    }
    let mut aggregate = HashMap::new();
    for (fi, file) in files.iter_mut().enumerate() {
        for item in &mut file.items {
            match item {
                Item::Const(c) => {
                    let at = &c.expr;
                    let value = evaluator
                        .constant(&c.ident.to_string(), at)
                        .map_err(|e| (fi, e))?;
                    *c.expr = value.expression(at.span());
                    if !(matches!(c.ty.as_ref(), Type::Path(_))
                        && matches!(
                            Integer::of(&c.ty),
                            Some(Integer::U8 | Integer::U16 | Integer::I16)
                        ))
                        && !super::byte_slices::constant_type(&c.ty)
                    {
                        aggregate.insert(c.ident.to_string(), *c.expr.clone());
                    }
                }
                Item::Static(s) => {
                    *s.expr = evaluator
                        .eval(&s.expr, &HashMap::new(), Integer::of(&s.ty))
                        .map_err(|e| (fi, e))?
                        .expression(s.expr.span());
                }
                _ => {}
            }
        }
    }
    // A local with the same spelling must retain normal Rust shadowing.
    struct Substitute<'a> {
        values: &'a HashMap<String, Expr>,
        bound: HashSet<String>,
    }
    impl VisitMut for Substitute<'_> {
        fn visit_item_fn_mut(&mut self, function: &mut syn::ItemFn) {
            let previous = std::mem::take(&mut self.bound);
            for parameter in &function.sig.inputs {
                if let syn::FnArg::Typed(p) = parameter {
                    if let Pat::Ident(id) = p.pat.as_ref() {
                        self.bound.insert(id.ident.to_string());
                    }
                }
            }
            self.visit_block_mut(&mut function.block);
            self.bound = previous;
        }
        fn visit_block_mut(&mut self, block: &mut syn::Block) {
            let previous = self.bound.clone();
            for statement in &mut block.stmts {
                if let Stmt::Local(local) = statement {
                    if let Some((_, e)) = &mut local.init {
                        self.visit_expr_mut(e);
                    }
                    let pattern = match &local.pat {
                        Pat::Type(p) => p.pat.as_ref(),
                        p => p,
                    };
                    if let Pat::Ident(id) = pattern {
                        self.bound.insert(id.ident.to_string());
                    }
                } else {
                    self.visit_stmt_mut(statement);
                }
            }
            self.bound = previous;
        }
        fn visit_expr_mut(&mut self, expression: &mut Expr) {
            if let Expr::Path(path) = expression {
                if let Some(name) = path.path.get_ident() {
                    let name = name.to_string();
                    if !self.bound.contains(&name) {
                        if let Some(value) = self.values.get(&name) {
                            *expression = value.clone();
                            return;
                        }
                    }
                }
            }
            syn::visit_mut::visit_expr_mut(self, expression);
        }
    }
    for file in files.iter_mut() {
        file.items.retain(
            |item| !matches!(item, Item::Const(c) if aggregate.contains_key(&c.ident.to_string())),
        );
        Substitute {
            values: &aggregate,
            bound: HashSet::new(),
        }
        .visit_file_mut(file);
    }
    // Known pure calls in runtime code use the same constructors and quantization.
    // A failed attempt remains a normal runtime call, preserving its target ABI.
    struct FoldCalls<'a> {
        evaluator: &'a mut Evaluator,
        bound: HashSet<String>,
    }
    impl VisitMut for FoldCalls<'_> {
        fn visit_local_mut(&mut self, local: &mut syn::Local) {
            syn::visit_mut::visit_local_mut(self, local);
            // Folding a constructor must preserve its inferred result type.
            // Do not broaden legacy place/index bindings into value copies.
            if !matches!(local.pat, Pat::Type(_)) {
                if let Some((_, initializer)) = &local.init {
                    let mut value = initializer.as_ref();
                    while let Expr::Paren(paren) = value {
                        value = &paren.expr;
                    }
                    if let Expr::Struct(value) = value {
                        let ty = &value.path;
                        local.pat = super::typed_pattern(local.pat.clone(), parse_quote!(#ty));
                    }
                }
            }
        }
        fn visit_item_fn_mut(&mut self, function: &mut syn::ItemFn) {
            // Conservatively exclude every local spelling in this function.
            // A shadowed global constant must never be folded as the global.
            struct Bindings(HashSet<String>);
            impl<'ast> syn::visit::Visit<'ast> for Bindings {
                fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
                    self.0.insert(pattern.ident.to_string());
                    syn::visit::visit_pat_ident(self, pattern);
                }
            }
            let mut bindings = Bindings(HashSet::new());
            syn::visit::Visit::visit_item_fn(&mut bindings, function);
            let previous = std::mem::replace(&mut self.bound, bindings.0);
            self.visit_block_mut(&mut function.block);
            self.bound = previous;
        }
        fn visit_expr_mut(&mut self, expression: &mut Expr) {
            syn::visit_mut::visit_expr_mut(self, expression);
            let pure = match expression {
                Expr::Call(call) => {
                    matches!(call.func.as_ref(),Expr::Path(p) if p.path.get_ident().is_some_and(|name|self.evaluator.functions.contains_key(&name.to_string())))
                }
                Expr::MethodCall(_) | Expr::Field(_) | Expr::Index(_) | Expr::Unsafe(_) => true,
                _ => false,
            };
            if pure {
                struct Shadowed<'a> {
                    bound: &'a HashSet<String>,
                    found: bool,
                }
                impl<'ast> syn::visit::Visit<'ast> for Shadowed<'_> {
                    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
                        self.found |= path
                            .path
                            .get_ident()
                            .is_some_and(|name| self.bound.contains(&name.to_string()));
                    }
                }
                let mut shadowed = Shadowed {
                    bound: &self.bound,
                    found: false,
                };
                syn::visit::Visit::visit_expr(&mut shadowed, expression);
                if shadowed.found {
                    return;
                }
                self.evaluator.steps = STEPS;
                self.evaluator.depth = 0;
                self.evaluator.active.clear();
                if let Ok(value) = self.evaluator.eval(expression, &HashMap::new(), None) {
                    // Keep forms whose inferred runtime ABI depends on the call
                    // itself. In particular, an empty block is a statement, not
                    // a tuple-valued expression in the target language.
                    let keep = matches!(value, Value::Unit | Value::Tuple(_) | Value::Array(_))
                        || matches!(&value,Value::Native(Expr::Call(call)) if matches!(call.func.as_ref(),Expr::Path(p) if p.path.segments.first().is_some_and(|s|s.ident=="Buf")));
                    if !keep {
                        *expression = value.expression(expression.span());
                    }
                }
            }
        }
    }
    for file in files.iter_mut() {
        FoldCalls {
            evaluator: &mut evaluator,
            bound: HashSet::new(),
        }
        .visit_file_mut(file);
    }
    runtime_assertions(files, debug, true);
    // Compile-time-only helpers need no target ABI; retain runtime dependencies
    // after diagnostic removal, so release does not keep debug-only helpers.
    struct Calls<'a> {
        names: HashSet<String>,
        methods: &'a super::value_methods::Methods,
    }
    impl<'ast> syn::visit::Visit<'ast> for Calls<'_> {
        fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
            if let Some(name) = p.path.get_ident() {
                self.names.insert(name.to_string());
            }
            syn::visit::visit_expr_path(self, p);
        }
        fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
            for ((_, name), method) in self.methods {
                if name == &m.method.to_string() {
                    self.names.insert(method.function.to_string());
                }
            }
            syn::visit::visit_expr_method_call(self, m);
        }
    }
    use syn::visit::Visit;
    let mut calls = Calls {
        names: HashSet::new(),
        methods,
    };
    for item in files.iter().flat_map(|f| &f.items) {
        if !matches!(item, Item::Fn(f) if f.sig.constness.is_some()) {
            calls.visit_item(item);
        }
    }
    loop {
        let count = calls.names.len();
        for function in files
            .iter()
            .flat_map(|file| &file.items)
            .filter_map(|item| match item {
                Item::Fn(function) if function.sig.constness.is_some() => Some(function),
                _ => None,
            })
        {
            if calls.names.contains(&function.sig.ident.to_string()) {
                calls.visit_block(&function.block);
            }
        }
        if count == calls.names.len() {
            break;
        }
    }
    if methods
        .get(&("StaticBuffer".into(), "new".into()))
        .is_some_and(|m| calls.names.contains(&m.function.to_string()))
    {
        return Err((
            0,
            syn::Error::new(
                Span::call_site(),
                "StaticBuffer::new requires #[static_buffer]",
            ),
        ));
    }
    for file in files {
        file.items.retain(|item| !matches!(item, Item::Fn(f) if f.sig.constness.is_some() && !calls.names.contains(&f.sig.ident.to_string())));
        for item in &mut file.items {
            if let Item::Fn(f) = item {
                f.sig.constness = None;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn folded_constructor_keeps_its_type_without_copying_index_bindings() {
        let mut files=[syn::parse_file("struct Point{x:u16}const fn make()->Point{Point{x:1}}static TABLE:Buf<Point,1>=Buf::new([Point{x:1}]);fn main(){let made=make();let row=TABLE[0u16];}").unwrap()];
        super::prepare(&mut files, &Default::default(), false, &Default::default()).unwrap();
        let main = files[0]
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Fn(f) if f.sig.ident == "main" => Some(f),
                _ => None,
            })
            .unwrap();
        let syn::Stmt::Local(made) = &main.block.stmts[0] else {
            panic!()
        };
        assert!(matches!(made.pat, syn::Pat::Type(_)));
        let syn::Stmt::Local(row) = &main.block.stmts[1] else {
            panic!()
        };
        assert!(matches!(row.pat, syn::Pat::Ident(_)));
        assert!(matches!(
            row.init.as_ref().unwrap().1.as_ref(),
            syn::Expr::Index(_)
        ));
    }

    #[test]
    fn runtime_call_folding_preserves_local_shadowing() {
        use quote::ToTokens;
        let mut files=[syn::parse_file("const N:u16=2;const fn twice(x:u16)->u16{x*2}fn main(){let N:u16=6;let dynamic=twice(N);let folded=twice(4);}").unwrap()];
        super::prepare(&mut files, &Default::default(), false, &Default::default()).unwrap();
        let source = files[0].to_token_stream().to_string();
        assert!(source.contains("twice (N)"), "{source}");
        assert!(!source.contains("twice (4)"), "{source}");
        assert!(source.contains("folded = 8u16"), "{source}");
    }

    #[test]
    fn cyclic_and_unbounded_constant_evaluation_fails_with_a_budget() {
        for (source, expected) in [
            ("const A:u16=B;const B:u16=A;", "cyclic constant"),
            (
                "const fn f(n:u16)->u16{f(n)}const A:u16=f(1);",
                "depth budget",
            ),
            ("const A:[u16;100001]=[1;100001];", "evaluation budget"),
        ] {
            let mut files = [syn::parse_file(source).unwrap()];
            let error = super::prepare(&mut files, &Default::default(), false, &Default::default())
                .unwrap_err()
                .1;
            assert!(error.to_string().contains(expected), "{error}");
        }
    }
}
