//! Fixed graphics ownership carriers, after the original Rust ownership gate.
use super::err;
use quote::ToTokens;
use std::collections::{HashMap, HashSet};
use syn::{parse_quote, visit_mut::VisitMut, Expr, FnArg, Item, Pat, ReturnType, Stmt, Type};

const PREFIX: &str = "__rcc_container_";
/// Generated destruction bypasses ordinary source-level `drop` name lookup.
pub(super) const DROP: &str = "__rcc_container_drop";
pub(super) struct Carrier {
    pub(super) name: String,
    pub(super) file: usize,
    pub(super) cleanup: String,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Owner,
    Value,
    Pair,
}
impl Kind {
    fn ident(self, suffix: &str) -> syn::Ident {
        syn::Ident::new(
            &format!(
                "{PREFIX}{}{suffix}",
                match self {
                    Self::Owner => "option_cb",
                    Self::Value => "option_memory",
                    Self::Pair => "pair_cb",
                }
            ),
            proc_macro2::Span::call_site(),
        )
    }
    fn ty(self) -> Type {
        let name = self.ident("");
        parse_quote!(#name)
    }
    fn from_type(ty: &Type) -> Option<Self> {
        [Self::Owner, Self::Value, Self::Pair]
            .into_iter()
            .find(|k| matches!(ty,Type::Path(p) if p.path.is_ident(&k.ident(""))))
    }
    fn payload(self) -> Type {
        match self {
            Self::Owner | Self::Pair => parse_quote!(CommandBuffer),
            Self::Value => parse_quote!(CommandMemory),
        }
    }
}
fn plain(ty: &Type) -> Option<String> {
    let Type::Path(p) = ty else { return None };
    p.path.get_ident().map(ToString::to_string)
}
fn output(ret: &ReturnType) -> Type {
    match ret {
        ReturnType::Type(_, ty) => *ty.clone(),
        ReturnType::Default => parse_quote!(()),
    }
}
struct Normalize {
    required: HashSet<Kind>,
    error: Option<syn::Error>,
}
impl VisitMut for Normalize {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Path(p) = ty {
            if p.path.segments.len() == 1 && p.path.segments[0].ident == "Option" {
                let kind = (|| {
                    let syn::PathArguments::AngleBracketed(a) = &p.path.segments[0].arguments
                    else {
                        return None;
                    };
                    if a.args.len() != 1 {
                        return None;
                    }
                    let syn::GenericArgument::Type(payload) = a.args.first()? else {
                        return None;
                    };
                    match plain(payload)?.as_str() {
                        "CommandBuffer" => Some(Kind::Owner),
                        "CommandMemory" => Some(Kind::Value),
                        _ => None,
                    }
                })();
                if let Some(k) = kind {
                    self.required.insert(k);
                    *ty = k.ty();
                } else {
                    self.error = Some(err(
                        p,
                        "only Option<CommandMemory> and Option<CommandBuffer> are supported",
                    ));
                }
                return;
            }
        }
        if let Type::Tuple(tuple) = ty {
            if tuple.elems.len() == 2
                && tuple
                    .elems
                    .iter()
                    .all(|t| plain(t).as_deref() == Some("CommandBuffer"))
            {
                self.required.insert(Kind::Pair);
                *ty = Kind::Pair.ty();
                return;
            }
        }
        syn::visit_mut::visit_type_mut(self, ty);
    }
}
#[derive(Clone)]
struct Signature {
    params: Vec<Type>,
    ret: Type,
}
type Environment = HashMap<String, Type>;
struct Rewrite<'a> {
    signatures: HashMap<String, Signature>,
    methods: HashMap<(String, String), Signature>,
    value_names: HashSet<String>,
    required: &'a mut HashSet<Kind>,
    counter: usize,
    ret: Type,
}
impl Rewrite<'_> {
    fn option_constructor(&self, path: &syn::Path, variant: &str, env: &Environment) -> bool {
        let qualified = path.segments.len() == 2 && path.segments[0].ident == "Option";
        let unqualified = path.is_ident(variant)
            && !env.contains_key(variant)
            && !self.value_names.contains(variant);
        path.leading_colon.is_none()
            && (qualified || unqualified)
            && path.segments.last().is_some_and(|p| {
                p.ident == variant && matches!(p.arguments, syn::PathArguments::None)
            })
    }
    fn temporary(&mut self) -> syn::Ident {
        let name = syn::Ident::new(
            &format!("{PREFIX}temp_{}", self.counter),
            proc_macro2::Span::call_site(),
        );
        self.counter += 1;
        name
    }
    fn infer(&self, e: &Expr, env: &Environment) -> Option<Type> {
        match e {
            Expr::Tuple(tuple)
                if tuple.elems.len() == 2
                    && tuple.elems.iter().all(|e| {
                        self.infer(e, env).as_ref().and_then(plain).as_deref()
                            == Some("CommandBuffer")
                    }) =>
            {
                Some(Kind::Pair.ty())
            }
            Expr::Paren(p) => self.infer(&p.expr, env),
            Expr::Path(p) => env.get(&p.path.get_ident()?.to_string()).cloned(),
            Expr::MethodCall(method) => {
                let owner = plain(&self.infer(&method.receiver, env)?)?;
                self.methods
                    .get(&(owner, method.method.to_string()))
                    .map(|signature| signature.ret.clone())
            }
            Expr::Struct(s) => Some(Type::Path(syn::TypePath {
                qself: None,
                path: s.path.clone(),
            })),
            Expr::Call(c) => {
                let Expr::Path(p) = c.func.as_ref() else {
                    return None;
                };
                if self.option_constructor(&p.path, "Some", env) {
                    return match plain(&self.infer(c.args.first()?, env)?)?.as_str() {
                        "CommandBuffer" => Some(Kind::Owner.ty()),
                        "CommandMemory" => Some(Kind::Value.ty()),
                        _ => None,
                    };
                }
                self.signatures
                    .get(&p.path.get_ident()?.to_string())
                    .map(|s| s.ret.clone())
            }
            _ => None,
        }
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        env: &mut Environment,
        expected: Option<&Type>,
    ) -> Result<(), syn::Error> {
        match e {
            Expr::Path(path) if self.option_constructor(&path.path, "None", env) => {
                let k = expected
                    .and_then(Kind::from_type)
                    .ok_or_else(|| err(path, "None needs a known supported Option type"))?;
                self.required.insert(k);
                let f = k.ident("_none");
                *e = parse_quote!(#f());
            }
            Expr::Call(c) => {
                let name = match c.func.as_ref() {
                    Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
                    _ => None,
                };
                if matches!(c.func.as_ref(),Expr::Path(p) if self.option_constructor(&p.path,"Some",env))
                {
                    if c.args.len() != 1 {
                        return Err(err(c, "Some needs one payload"));
                    }
                    let k = expected
                        .and_then(Kind::from_type)
                        .or_else(|| {
                            self.infer(&Expr::Call(c.clone()), env)
                                .as_ref()
                                .and_then(Kind::from_type)
                        })
                        .ok_or_else(|| err(c, "Some needs CommandMemory or CommandBuffer"))?;
                    self.required.insert(k);
                    self.expression(&mut c.args[0], env, Some(&k.payload()))?;
                    let f = k.ident("_some");
                    *c.func = parse_quote!(#f);
                } else {
                    let sig = name.and_then(|n| self.signatures.get(&n).cloned());
                    for (i, arg) in c.args.iter_mut().enumerate() {
                        self.expression(arg, env, sig.as_ref().and_then(|s| s.params.get(i)))?;
                    }
                }
            }
            Expr::Tuple(tuple)
                if expected.and_then(Kind::from_type) == Some(Kind::Pair)
                    || self
                        .infer(&Expr::Tuple(tuple.clone()), env)
                        .as_ref()
                        .and_then(Kind::from_type)
                        == Some(Kind::Pair) =>
            {
                self.required.insert(Kind::Pair);
                for value in &mut tuple.elems {
                    self.expression(value, env, Some(&Kind::Pair.payload()))?;
                }
                let a = &tuple.elems[0];
                let b = &tuple.elems[1];
                let function = Kind::Pair.ident("_some");
                *e = parse_quote!(#function(#a,#b));
            }
            Expr::Return(r) => {
                if let Some(value) = &mut r.expr {
                    let ty = self.ret.clone();
                    self.expression(value, env, Some(&ty))?;
                }
            }
            Expr::Assign(a)
                if matches!(a.left.as_ref(), Expr::Tuple(_))
                    && self.infer(&a.right, env).as_ref().and_then(Kind::from_type)
                        == Some(Kind::Pair) =>
            {
                let Expr::Tuple(left) = a.left.as_ref() else {
                    unreachable!()
                };
                if left.elems.len() != 2 {
                    return Err(err(
                        left,
                        "an affine pair assignment needs two destinations",
                    ));
                }
                let mut value = *a.right.clone();
                self.expression(&mut value, env, Some(&Kind::Pair.ty()))?;
                let temp = self.temporary();
                let first = self.temporary();
                let second = self.temporary();
                let raw_first = self.temporary();
                let raw_second = self.temporary();
                let ty = Kind::Pair.ty();
                let unpack = Kind::Pair.ident("_unpack");
                let make = Kind::Pair.ident("_from_word");
                let a = &left.elems[0];
                let b = &left.elems[1];
                *e = parse_quote!({let #temp:#ty=#value;let(#raw_first,#raw_second)=#unpack(#temp);let #first=#make(#raw_first);let #second=#make(#raw_second);#a=#first;#b=#second;});
            }
            Expr::Field(field)
                if self
                    .infer(&field.base, env)
                    .as_ref()
                    .and_then(Kind::from_type)
                    == Some(Kind::Pair) =>
            {
                return Err(err(field, "affine pairs must be destructured as a whole"));
            }
            Expr::Assign(a) => {
                let ty = self.infer(&a.left, env);
                self.expression(&mut a.right, env, ty.as_ref())?;
            }
            Expr::If(i) => {
                if let Expr::Let(pattern) = i.cond.as_ref() {
                    let Pat::TupleStruct(some) = &pattern.pat else {
                        return Err(err(pattern, "only Some payload if-let is supported"));
                    };
                    if some.path.segments.last().is_none_or(|p| p.ident != "Some")
                        || some.pat.elems.len() != 1
                    {
                        return Err(err(some, "only Some payload if-let is supported"));
                    }
                    let k = self
                        .infer(&pattern.expr, env)
                        .as_ref()
                        .and_then(Kind::from_type)
                        .ok_or_else(|| {
                            err(pattern, "if-let needs a known supported Option value")
                        })?;
                    self.required.insert(k);
                    let temp = self.temporary();
                    let ty = k.ty();
                    let inspect = k.ident("_is_some");
                    let take = k.ident("_take");
                    let mut value = *pattern.expr.clone();
                    self.expression(&mut value, env, Some(&ty))?;
                    let bind = some.pat.elems[0].clone();
                    let mut yes = i.then_branch.clone();
                    let mut yes_env = env.clone();
                    let Pat::Ident(name) = &bind else {
                        return Err(err(&bind, "Some payloads need a simple binding"));
                    };
                    if name.by_ref.is_some() || name.subpat.is_some() {
                        return Err(err(name, "Some payloads must move into a simple binding"));
                    }
                    yes_env.insert(name.ident.to_string(), k.payload());
                    self.block(&mut yes, &mut yes_env, expected)?;
                    yes.stmts.insert(0, parse_quote!(let #bind=#take(#temp);));
                    let mut no = i
                        .else_branch
                        .as_ref()
                        .map(|(_, e)| *e.clone())
                        .unwrap_or_else(|| parse_quote!({}));
                    self.expression(&mut no, &mut env.clone(), expected)?;
                    let guard: Expr = if k == Kind::Owner {
                        parse_quote!(#inspect(&#temp))
                    } else {
                        parse_quote!(#inspect(#temp))
                    };
                    let discard: Option<Stmt> = if k == Kind::Owner {
                        Some(parse_quote!(__rcc_container_drop(#temp);))
                    } else {
                        None
                    };
                    *e = parse_quote!({let #temp:#ty=#value;if #guard #yes else{#discard #no}});
                } else {
                    self.expression(&mut i.cond, env, None)?;
                    self.block(&mut i.then_branch, &mut env.clone(), expected)?;
                    if let Some((_, other)) = &mut i.else_branch {
                        self.expression(other, &mut env.clone(), expected)?;
                    }
                }
            }
            Expr::Match(m) => {
                if m.arms.len() != 2 || m.arms.iter().any(|a| a.guard.is_some()) {
                    return Ok(());
                }
                let some=m.arms.iter().find(|a|matches!(&a.pat,Pat::TupleStruct(p) if p.path.segments.last().is_some_and(|s|s.ident=="Some")));
                if let Some(some) = some {
                    let other = m.arms.iter().find(|a| !std::ptr::eq(*a, some)).unwrap();
                    if !(matches!(&other.pat, Pat::Wild(_))
                        || matches!(&other.pat,Pat::Ident(p) if p.ident=="None" && p.by_ref.is_none() && p.subpat.is_none())
                        || matches!(&other.pat,Pat::Path(p) if p.path.segments.last().is_some_and(|s|s.ident=="None")))
                    {
                        return Err(err(&other.pat, "Option match needs Some and None arms"));
                    }
                    let pat = &some.pat;
                    let value = &m.expr;
                    let yes = &some.body;
                    let no = &other.body;
                    *e = parse_quote!(if let #pat=#value{#yes}else{#no});
                    self.expression(e, env, expected)?;
                }
            }
            Expr::Block(b) => self.block(&mut b.block, &mut env.clone(), expected)?,
            Expr::Unsafe(b) => self.block(&mut b.block, &mut env.clone(), expected)?,
            Expr::While(w) => {
                self.expression(&mut w.cond, env, None)?;
                self.block(&mut w.body, &mut env.clone(), None)?;
            }
            Expr::ForLoop(loop_) => {
                let mut scope = env.clone();
                if let Pat::Ident(index) = &loop_.pat {
                    scope.insert(index.ident.to_string(), parse_quote!(u16));
                }
                self.block(&mut loop_.body, &mut scope, None)?;
            }
            Expr::Loop(l) => self.block(&mut l.body, &mut env.clone(), None)?,
            Expr::MethodCall(m) => {
                let signature = self
                    .infer(&m.receiver, env)
                    .as_ref()
                    .and_then(plain)
                    .and_then(|owner| self.methods.get(&(owner, m.method.to_string())))
                    .cloned();
                for (index, arg) in m.args.iter_mut().enumerate() {
                    self.expression(
                        arg,
                        env,
                        signature.as_ref().and_then(|s| s.params.get(index)),
                    )?;
                }
            }
            Expr::Binary(b) => {
                self.expression(&mut b.left, env, None)?;
                self.expression(&mut b.right, env, None)?;
            }
            Expr::Unary(u) => self.expression(&mut u.expr, env, None)?,
            Expr::Paren(p) => self.expression(&mut p.expr, env, expected)?,
            Expr::Reference(r) => self.expression(&mut r.expr, env, None)?,
            Expr::Cast(c) => self.expression(&mut c.expr, env, None)?,
            Expr::Struct(s) => {
                for field in &mut s.fields {
                    self.expression(&mut field.expr, env, None)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn block(
        &mut self,
        b: &mut syn::Block,
        env: &mut Environment,
        expected: Option<&Type>,
    ) -> Result<(), syn::Error> {
        let original = std::mem::take(&mut b.stmts);
        let n = original.len();
        for (i, mut statement) in original.into_iter().enumerate() {
            match &mut statement {
                Stmt::Local(local) => {
                    let ann = match &local.pat {
                        Pat::Type(p) => Some(*p.ty.clone()),
                        _ => None,
                    };
                    if let Some((_, init)) = &mut local.init {
                        let ty = ann.or_else(|| self.infer(init, env));
                        self.expression(init, env, ty.as_ref())?;
                        let pattern = match &local.pat {
                            Pat::Type(p) => p.pat.as_ref(),
                            p => p,
                        };
                        if let Pat::Tuple(pair) = pattern {
                            if ty.as_ref().and_then(Kind::from_type) == Some(Kind::Pair) {
                                if pair.elems.len() != 2 {
                                    return Err(err(pair, "an affine pair needs two bindings"));
                                }
                                let temp = self.temporary();
                                let raw0 = self.temporary();
                                let raw1 = self.temporary();
                                let pair_ty = Kind::Pair.ty();
                                let unpack = Kind::Pair.ident("_unpack");
                                let make = Kind::Pair.ident("_from_word");
                                b.stmts.push(parse_quote!(let #temp:#pair_ty=#init;));
                                b.stmts.push(parse_quote!(let(#raw0,#raw1)=#unpack(#temp);));
                                for (pattern, raw) in pair.elems.iter().zip([raw0, raw1]) {
                                    if let Pat::Ident(name) = pattern {
                                        if name.by_ref.is_some() || name.subpat.is_some() {
                                            return Err(err(
                                                name,
                                                "affine pair bindings must move their owners",
                                            ));
                                        }
                                        env.insert(name.ident.to_string(), Kind::Pair.payload());
                                        b.stmts.push(parse_quote!(let #pattern=#make(#raw);));
                                    } else if matches!(pattern, Pat::Wild(_)) {
                                        let discarded = self.temporary();
                                        b.stmts.push(parse_quote!(let #discarded=#make(#raw);));
                                        b.stmts
                                            .push(parse_quote!(__rcc_container_drop(#discarded);));
                                    } else {
                                        return Err(err(
                                            pattern,
                                            "affine pair bindings need simple names",
                                        ));
                                    }
                                }
                                continue;
                            }
                        }
                        if ty.as_ref().and_then(Kind::from_type) == Some(Kind::Value)
                            && !matches!(local.pat, Pat::Type(_))
                        {
                            let p = local.pat.clone();
                            local.pat = super::typed_pattern(p, ty.as_ref().unwrap().clone());
                        }
                        let pattern = match &local.pat {
                            Pat::Type(p) => p.pat.as_ref(),
                            p => p,
                        };
                        if let (Pat::Ident(name), Some(ty)) = (pattern, ty) {
                            env.insert(name.ident.to_string(), ty);
                        }
                    }
                }
                Stmt::Expr(e) => {
                    self.expression(e, env, if i + 1 == n { expected } else { None })?
                }
                Stmt::Semi(e, _) => self.expression(e, env, None)?,
                _ => {}
            }
            b.stmts.push(statement);
        }
        Ok(())
    }
}
fn zero(
    ty: &Type,
    structs: &HashMap<String, (usize, syn::ItemStruct)>,
) -> Result<Expr, syn::Error> {
    match plain(ty).as_deref() {
        Some("u16") => Ok(parse_quote!(0u16)),
        Some("u8") => Ok(parse_quote!(0u8)),
        Some("i16") => Ok(parse_quote!(0i16)),
        Some("bool") => Ok(parse_quote!(false)),
        Some("Ptr") => Ok(parse_quote!(unsafe { 0u16 as Ptr })),
        Some("fix32") => Ok(parse_quote!(fix32::from_int(0i16))),
        Some(name) => {
            let (_, s) = structs
                .get(name)
                .ok_or_else(|| err(ty, "unsupported CommandMemory field"))?;
            if s.attrs.iter().any(|a| a.path.is_ident("rcc_resource")) {
                return Err(err(ty, "CommandMemory must be an ordinary Copy value"));
            }
            let name = &s.ident;
            let fields = s
                .fields
                .iter()
                .map(|f| {
                    let id = f
                        .ident
                        .as_ref()
                        .ok_or_else(|| err(f, "CommandMemory needs named fields"))?;
                    let z = zero(&f.ty, structs)?;
                    Ok(quote::quote!(#id:#z))
                })
                .collect::<Result<Vec<_>, syn::Error>>()?;
            syn::parse2(quote::quote!(#name{#(#fields),*}))
        }
        None => Err(err(ty, "unsupported CommandMemory field")),
    }
}
fn generate(
    k: Kind,
    structs: &HashMap<String, (usize, syn::ItemStruct)>,
) -> Result<(usize, Vec<Item>, Option<Carrier>), syn::Error> {
    let payload = k.payload();
    let (file, definition) = structs
        .get(&plain(&payload).unwrap())
        .ok_or_else(|| err(&payload, "missing Option payload definition"))?;
    let ty = k.ident("");
    let some = k.ident("_some");
    let none = k.ident("_none");
    let inspect = k.ident("_is_some");
    let take = k.ident("_take");
    let cleanup = k.ident("_drop");
    if k == Kind::Value {
        let empty = zero(&payload, structs)?;
        return Ok((
            *file,
            vec![
                parse_quote!(struct #ty{present:u16,payload:CommandMemory}),
                parse_quote!(pub fn #some(payload:CommandMemory)->#ty{#ty{present:1,payload:payload}}),
                parse_quote!(pub fn #none()->#ty{#ty{present:0,payload:#empty}}),
                parse_quote!(pub fn #inspect(value:#ty)->bool{value.present!=0}),
                parse_quote!(pub fn #take(value:#ty)->CommandMemory{value.payload}),
            ],
            None,
        ));
    }
    if !definition
        .attrs
        .iter()
        .any(|a| a.path.is_ident("rcc_resource"))
    {
        return Err(err(definition, "CommandBuffer must be an owning resource"));
    }
    let field = definition
        .fields
        .iter()
        .next()
        .and_then(|f| f.ident.as_ref())
        .ok_or_else(|| err(definition, "CommandBuffer needs a word identity"))?;
    if k == Kind::Pair {
        let unpack = k.ident("_unpack");
        let make = k.ident("_from_word");
        return Ok((
            *file,
            vec![
                parse_quote!(struct #ty{first:u16,second:u16}),
                parse_quote!(pub fn #some(a:CommandBuffer,b:CommandBuffer)->#ty{let first=a.#field;let second=b.#field;resource_disarm(a);resource_disarm(b);#ty{first:first,second:second}}),
                parse_quote!(pub fn #unpack(value:#ty)->(u16,u16){let first=value.first;let second=value.second;resource_disarm(value);(first,second)}),
                parse_quote!(pub fn #make(word:u16)->CommandBuffer{CommandBuffer{#field:word}}),
                parse_quote!(fn #cleanup(value:#ty){let first=value.first;let second=value.second;resource_disarm(value);let a=CommandBuffer{#field:first};let b=CommandBuffer{#field:second};__rcc_container_drop(a);__rcc_container_drop(b);}),
            ],
            Some(Carrier {
                name: ty.to_string(),
                file: *file,
                cleanup: cleanup.to_string(),
            }),
        ));
    }
    Ok((
        *file,
        vec![
            parse_quote!(struct #ty{present:u16,payload:u16}),
            parse_quote!(pub fn #some(value:CommandBuffer)->#ty{let payload=value.#field;resource_disarm(value);#ty{present:1,payload:payload}}),
            parse_quote!(pub fn #none()->#ty{#ty{present:0,payload:0}}),
            parse_quote!(pub fn #inspect(value:&#ty)->bool{value.present!=0}),
            parse_quote!(pub fn #take(value:#ty)->CommandBuffer{let payload=value.payload;resource_disarm(value);CommandBuffer{#field:payload}}),
            parse_quote!(fn #cleanup(value:#ty){if value.present!=0{let payload=value.payload;resource_disarm(value);let owner=CommandBuffer{#field:payload};__rcc_container_drop(owner);}else{resource_disarm(value);}}),
        ],
        Some(Carrier {
            name: ty.to_string(),
            file: *file,
            cleanup: cleanup.to_string(),
        }),
    ))
}
pub(super) fn prepare(
    files: &mut [syn::File],
    checked: bool,
) -> Result<Vec<Carrier>, (usize, syn::Error)> {
    for (fi, file) in files.iter().enumerate() {
        if let Some(name) = super::value_methods::reserved(file.to_token_stream(), PREFIX) {
            return Err((fi, err(&name, "reserved ownership container name")));
        }
    }
    if !checked {
        return Ok(Vec::new());
    }
    let structs: HashMap<_, _> = files
        .iter()
        .enumerate()
        .flat_map(|(fi, file)| {
            file.items.iter().filter_map(move |item| match item {
                Item::Struct(s) => Some((s.ident.to_string(), (fi, s.clone()))),
                _ => None,
            })
        })
        .collect();
    let mut normalize = Normalize {
        required: HashSet::new(),
        error: None,
    };
    for (fi, file) in files.iter_mut().enumerate() {
        normalize.visit_file_mut(file);
        if let Some(e) = normalize.error.take() {
            return Err((fi, e));
        }
    }
    let signatures = files
        .iter()
        .flat_map(|file| &file.items)
        .filter_map(|item| {
            let Item::Fn(f) = item else { return None };
            Some((
                f.sig.ident.to_string(),
                Signature {
                    params: f
                        .sig
                        .inputs
                        .iter()
                        .filter_map(|a| match a {
                            FnArg::Typed(p) => Some(*p.ty.clone()),
                            _ => None,
                        })
                        .collect(),
                    ret: output(&f.sig.output),
                },
            ))
        })
        .collect();
    let mut methods = HashMap::new();
    for file in files.iter() {
        for item in &file.items {
            if let Item::Impl(block) = item {
                if let Some(owner) = plain(&block.self_ty) {
                    for member in &block.items {
                        if let syn::ImplItem::Method(m) = member {
                            methods.insert(
                                (owner.clone(), m.sig.ident.to_string()),
                                Signature {
                                    params: m
                                        .sig
                                        .inputs
                                        .iter()
                                        .filter_map(|a| match a {
                                            FnArg::Typed(p) => Some(*p.ty.clone()),
                                            _ => None,
                                        })
                                        .collect(),
                                    ret: output(&m.sig.output),
                                },
                            );
                        }
                    }
                }
            }
        }
    }
    let value_names = files
        .iter()
        .flat_map(|file| &file.items)
        .filter_map(|item| match item {
            Item::Fn(f) => Some(f.sig.ident.to_string()),
            Item::Const(c) => Some(c.ident.to_string()),
            Item::Static(s) => Some(s.ident.to_string()),
            _ => None,
        })
        .collect();
    let mut rewrite = Rewrite {
        signatures,
        methods,
        value_names,
        required: &mut normalize.required,
        counter: 0,
        ret: parse_quote!(()),
    };
    for (fi, file) in files.iter_mut().enumerate() {
        for item in &mut file.items {
            let mut run = |sig: &syn::Signature,
                           body: &mut syn::Block,
                           owner: Option<&Type>|
             -> Result<(), syn::Error> {
                rewrite.ret = output(&sig.output);
                let mut env = Environment::new();
                if let Some(owner) = owner {
                    env.insert("self".into(), owner.clone());
                }
                for a in &sig.inputs {
                    if let FnArg::Typed(p) = a {
                        if let Pat::Ident(name) = p.pat.as_ref() {
                            let ty = match p.ty.as_ref() {
                                Type::Reference(r) => *r.elem.clone(),
                                ty => ty.clone(),
                            };
                            env.insert(name.ident.to_string(), ty);
                        }
                    }
                }
                let ret = rewrite.ret.clone();
                rewrite.block(body, &mut env, Some(&ret))
            };
            match item {
                Item::Fn(f) => run(&f.sig, &mut f.block, None),
                Item::Impl(block) => {
                    for m in &mut block.items {
                        if let syn::ImplItem::Method(m) = m {
                            run(&m.sig, &mut m.block, Some(&block.self_ty)).map_err(|e| (fi, e))?;
                        }
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
            .map_err(|e| (fi, e))?;
        }
    }
    let mut carriers = Vec::new();
    for k in [Kind::Owner, Kind::Value, Kind::Pair] {
        if normalize.required.contains(&k) {
            let (fi, items, carrier) = generate(k, &structs).map_err(|e| (0, e))?;
            files[fi].items.extend(items);
            if let Some(carrier) = carrier {
                carriers.push(carrier);
            }
        }
    }
    Ok(carriers)
}
