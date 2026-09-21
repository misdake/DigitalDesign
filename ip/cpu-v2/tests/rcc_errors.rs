//! rcc subset violations: everything outside the spec is a hard error.

mod common;

use common::*;

#[test]
fn test_compile_diagnostics_include_file_line_column_and_source() {
    let source = "fn main() {\n    let x = ;\n}\n";
    let error = match cpu_v2::frontend::compile_program_named(
        "broken.rs",
        source,
        &cpu_v2::CompilerOptions::default(),
        &mut |name| Err(format!("unknown module `{name}`")),
    ) {
        Ok(_) => panic!("invalid syntax unexpectedly compiled"),
        Err(error) => error,
    };

    let (file, line, column) = error.location().expect("missing diagnostic location");
    assert_eq!(file, "broken.rs");
    assert_eq!(line, 2);
    assert!(column > 1);
    let rendered = error.to_string();
    assert!(rendered.contains(" --> broken.rs:2:"), "{rendered}");
    assert!(rendered.contains("2 |     let x = ;"), "{rendered}");
    assert!(rendered.contains('^'), "{rendered}");
}

#[test]
fn test_module_semantic_error_keeps_its_source_file() {
    let source = "mod helper;\nfn main() { helper(); }\n";
    let error = match cpu_v2::frontend::compile_program_named(
        "main.rs",
        source,
        &cpu_v2::CompilerOptions::default(),
        &mut |name| match name {
            "helper" => Ok("fn helper() {\n    let x = missing;\n    halt(x);\n}\n".to_string()),
            _ => Err(format!("unknown module `{name}`")),
        },
    ) {
        Ok(_) => panic!("module with a semantic error unexpectedly compiled"),
        Err(error) => error,
    };

    let (file, line, column) = error.location().expect("missing diagnostic location");
    assert_eq!((file, line), ("helper.rs", 2));
    assert!(column > 1);
    let rendered = error.to_string();
    assert!(rendered.contains(" --> helper.rs:2:"), "{rendered}");
    assert!(rendered.contains("undefined name"), "{rendered}");
}

#[test]
fn test_value_conversion_error_uses_the_expression_location() {
    let source = concat!(
        "fn main() {\n",
        "    let x = 1u16;\n",
        "    let invalid: i16 = x;\n",
        "}\n",
    );
    let error = match cpu_v2::frontend::compile_program_named(
        "value.rs",
        source,
        &cpu_v2::CompilerOptions::default(),
        &mut |name| Err(format!("unknown module `{name}`")),
    ) {
        Ok(_) => panic!("mismatched value unexpectedly compiled"),
        Err(error) => error,
    };

    assert_eq!(error.location().map(|(_, line, _)| line), Some(3));
    let rendered = error.to_string();
    assert!(
        rendered.contains("3 |     let invalid: i16 = x;"),
        "{rendered}"
    );
}

#[test]
fn test_static_data_cannot_overlap_the_function_table() {
    let source = "static X: u16 = 1;\nfn main() { halt(X); }\n";
    let options = cpu_v2::CompilerOptions {
        data_base: cpu_v2::FUNCTION_TABLE_BASE,
        ..Default::default()
    };
    let error = match cpu_v2::frontend::compile_program_named(
        "table_overlap.rs",
        source,
        &options,
        &mut |name| Err(format!("unknown module `{name}`")),
    ) {
        Ok(_) => panic!("static data in function-table memory unexpectedly compiled"),
        Err(error) => error,
    };
    assert_eq!(error.location().map(|(_, line, _)| line), Some(1));
    assert!(error.to_string().contains("reserved function-table memory"));
}

#[test]
fn test_unsupported_constructs() {
    expect_error(
        "fn f(x: u16) -> u16 { match x { _ => 0 } }",
        "match is a statement",
    );
    expect_error("fn f(x: u16) -> u16 { let g = |y| y; x }", "not supported");
    expect_error("fn f<T>(x: T) -> T { x }", "not supported");
    expect_error("fn f(x: u16) -> u16 { x as u32 }", "not supported");
    expect_error("fn f(x: u16) -> u32 { x }", "not supported");
    expect_error("fn f(x: u16) { if x { halt(0); } }", "boolean");
    expect_error("fn f(x: u16) { x = 1; }", "not mutable");
    expect_error("fn f(x: u16) -> u16 { return; }", "return");
    expect_error("fn f() { let a: Buf<u16, 3>; }", "initializer");
    expect_error(
        "fn f() { let mut x: u16 = 1; let p = &x; }",
        "not supported",
    );
    expect_error("static mut X: u16 = 0; fn f() {}", "static mut");
    expect_error("fn f(a: Buf<u16, 2>) {}", "cannot be a parameter");
}

// ---------------------------------------------------------------------------
// spec §6: every unsupported Rust feature is a hard error naming the feature
// ---------------------------------------------------------------------------

#[test]
fn test_unsupported_items() {
    // generics / trait / impl / macro definitions at file scope
    expect_error("fn id<T>(x: T) -> T { x }", "generics");
    expect_error("trait Show { fn show(&self) -> u16; }", "traits");
    expect_error("impl Ptr { fn g(x: u16) -> u16 { x } }", "impl");
    expect_error("macro_rules! m { () => {} }", "macros");
    // function flavors outside the subset
    expect_error("unsafe fn f() {}", "unsafe");
    expect_error("extern \"C\" fn f(x: u16) -> u16 { x }", "extern");
    // globals: only plain `static` is allowed (spec §9)
    expect_error("static mut X: u16 = 0;", "static mut");
    // items do not exist inside function bodies
    expect_error("fn f() { const X: u16 = 1; }", "items inside functions");
}

#[test]
fn test_unsupported_expressions() {
    expect_error("fn f(x: u16) -> u16 { match x { _ => 0 } }", "match");
    expect_error("fn f(x: u16) -> u16 { let g = |y: u16| y; x }", "closures");
    expect_error("fn f() { println!(\"x\"); }", "macros");
    expect_error("fn f() { let mut x: u16 = 1; let p = &x; }", "references");
    // destructuring patterns: only identifiers, tuples and `_` may be bound
    expect_error(
        "fn f() { let (a, (b, c)) = (1u16, 2u16); }",
        "unsupported tuple pattern",
    );
    expect_error("fn f() { let s = \"hi\"; }", "string");
    expect_error("fn f() { let x = 1.5; }", "float");
}

#[test]
fn test_unsupported_types() {
    // only u16/i16/Ptr/Array<T>/fn pointer types exist (spec §1)
    expect_error("fn f(x: u8) {}", "type not supported");
    expect_error("fn f(x: u32) {}", "type not supported");
    expect_error("fn f(x: usize) {}", "rcc has no `usize`");
    expect_error("fn f() -> u16 { 1u8 as u16 }", "suffix");
    // no fat slices, references, or owned arrays in parameter position
    expect_error("fn f(s: [u16]) {}", "slice");
    expect_error("fn f(r: &u16) {}", "reference");
    expect_error("fn f(a: Buf<u16, 2>) {}", "cannot be a parameter");
}

#[test]
fn test_operator_restrictions() {
    // integer `*` works (hardware MUL on CpuV3, the mul_16x16 library on CpuV2)
    // and so do `/` and `%` (the rcc_std div module, spec §1.2)
    // unary minus is i16-only, same as Rust
    expect_error("fn f(x: u16) -> u16 { -x }", "only allowed on i16");
    // dynamic (register-count) shifts parse now, but the v2.6 ISA cannot
    // encode them, so the v2 backend rejects them explicitly
    let program = cpu_v2::frontend::parse_source(
        "fn f(x: u16, n: u16) -> u16 { x << n } fn main() { halt(f(1, 2)); }",
    )
    .expect("dynamic shifts parse");
    let result = std::panic::catch_unwind(|| {
        let mut c = cpu_v2::Compiler::new();
        for f in program.funcs {
            c.add_func(f);
        }
        c.finish("main");
    });
    let error = result.expect_err("the v2 backend must reject register-count shifts");
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(message.contains("register-count shifts"), "{message}");
    // immediate shifts still reject out-of-range literal amounts
    expect_error("fn f(x: u16) -> u16 { x << 16 }", "0..=15");
}

#[test]
fn test_bit_intrinsic_errors() {
    expect_error("fn f() -> u16 { cnt1() }", "takes 1 argument");
    expect_error("fn f(x: i16) -> u16 { log2(x) }", "expected u16, got i16");
}

#[test]
fn test_bool_restrictions() {
    // a stored bool is legal now (spec §1.1), but a bare integer is not a
    // condition and a bool still does not mix with integers
    expect_error("fn f(x: u16) { if x { halt(0); } }", "boolean expression");
    expect_error(
        "fn f(x: u16) { while x { halt(0); } }",
        "boolean expression",
    );
    expect_error("fn f(x: u16) { if (x < 3u16) == 1 { halt(0); } }", "bool");
}

#[test]
fn test_mixed_and_ptr_comparisons() {
    // u16/i16 never mix without an explicit `as` cast
    expect_error(
        "fn f(x: u16) { if x < 3i16 { halt(0); } }",
        "cannot compare",
    );
    expect_error(
        "fn f(x: i16) { if 1u16 == x { halt(0); } }",
        "cannot compare",
    );
    // Ptr only compares with Ptr, never with integers
    expect_error(
        "fn f(p: Ptr) { if p == 0u16 { halt(0); } }",
        "cannot compare",
    );
    expect_error("fn f(p: Ptr) { if p == 0 { halt(0); } }", "cannot compare");
}

#[test]
fn test_name_errors() {
    expect_error("fn f() { g(); }", "undefined function `g`");
    expect_error("fn f() -> u16 { y }", "undefined name `y`");
    expect_error("fn f() { y = 1; }", "undefined variable `y`");
    expect_error("fn f() {} fn f() {}", "defined twice");
}

#[test]
fn test_call_signature_errors() {
    // wrong argument count, both directions
    expect_error("fn g(a: u16) {} fn f() { g(1, 2); }", "takes");
    expect_error("fn g(a: u16, b: u16) {} fn f(x: u16) { g(x); }", "takes");
    // wrong argument type
    expect_error("fn g(a: u16) {} fn f(x: i16) { g(x); }", "argument 1");
    expect_error(
        "fn g(a: u16) {} fn f(p: Ptr) { g(p); }",
        "expected u16, got Ptr",
    );
}

#[test]
fn test_assignment_and_return_errors() {
    // assignment targets must be declared `mut`
    expect_error("fn f(x: u16) { x = 1; }", "not mutable");
    expect_error("fn f() { let x: u16 = 1; x = 2; }", "not mutable");
    // `return;` in a value-returning function lacks the value
    expect_error("fn f() -> u16 { return; }", "missing return value");
    // returning a value from a procedure
    expect_error("fn f() { return 1; }", "without return type");
    // nothing may follow return/halt
    expect_error("fn f() -> u16 { return 1; let x = 2; x }", "unreachable");
    expect_error("fn f() { halt(0); let x = 1; }", "unreachable");
}

#[test]
fn test_array_errors() {
    // local arrays always need an initializer
    expect_error("fn f() { let a: Buf<u16, 3>; }", "initializer");
    // list length must match the declared length
    expect_error(
        "fn f() { let mut a: Buf<u16, 3> = Buf::new([1, 2]); }",
        "expected 3",
    );
    expect_error("static A: Buf<u16, 3> = Buf::new([1, 2]);", "expected 3");
    // the initializer must be `Buf::new([v; N])` or `Buf::new([e0, e1, ...])`
    expect_error("fn f() { let mut a: Buf<u16, 2> = 0; }", "Buf::new");
    // native arrays are not part of the subset (spec §10): they index by usize
    expect_error(
        "fn f() { let a: [u16; 2] = [0; 2]; }",
        "native arrays are not part of the subset",
    );
    expect_error(
        "fn f(a: [u16; 2]) {}",
        "native arrays are not part of the subset",
    );
    // static array initializers are compile-time constants only
    expect_error(
        "static A: Buf<u16, 2> = Buf::new([f(), 0]); fn f() -> u16 { 1 }",
        "constant expression",
    );
    expect_error("static A: Buf<u16, 2> = Buf::new([X, 0]);", "unknown const");
}

#[test]
fn test_allow_attribute_is_ignored() {
    // spec §6: #[allow(...)] is the one attribute that is parsed and ignored
    assert_eq!(run("#[allow(dead_code)] fn main() { halt(7); }"), Some(7));
}

// ---------------------------------------------------------------------------
// known compiler gaps (reported): per spec these are hard errors, but the
// frontend currently accepts them (or panics) — ignored until fixed
// ---------------------------------------------------------------------------

#[test]
fn test_attribute_macro_rejected() {
    expect_error("#[inline] fn f() {}", "attribute");
}

#[test]
fn test_return_type_mismatch() {
    expect_error("fn f(x: i16) -> u16 { x }", "type mismatch");
    expect_error("fn f(x: i16) -> u16 { return x; }", "type mismatch");
}

#[test]
fn test_loop_label_restrictions() {
    // a labeled jump must name an enclosing loop
    expect_error("fn f() { break; }", "outside of a loop");
    expect_error("fn f() { continue; }", "outside of a loop");
    expect_error(
        "fn f() { 'outer: loop { break 'nope; } }",
        "no enclosing loop labeled 'nope",
    );
    expect_error(
        "fn f() { 'outer: loop { continue 'nope; } }",
        "no enclosing loop labeled 'nope",
    );
    expect_error("fn f() { break 5; }", "with a value is not supported");
    // every compound assignment operator is supported
    for op in ["*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="] {
        let src = format!("fn f(x: u16) {{ let mut y = x; y {op} 2u16; }}");
        assert!(
            cpu_v2::frontend::parse_source(&src).is_ok(),
            "y {op} 2u16 should compile"
        );
    }
}

#[test]
fn test_missing_return_at_end_of_body() {
    expect_error("fn f() -> u16 { let x = 1; }", "without returning");
}

#[test]
fn test_struct_restrictions() {
    // a bare struct is not a parameter; a view is, and returning one is fine
    expect_error(
        "struct P { x: u16 }\nfn f(p: P) {}",
        "cannot be a parameter",
    );
    assert!(
        cpu_v2::frontend::parse_source("struct P { x: u16 }\nfn f() -> P { P { x: 1 } }").is_ok()
    );
    // ... but a view of one is fine
    assert!(cpu_v2::frontend::parse_source("struct P { x: u16 }\nfn f(p: Array<P>) { }").is_ok());
    // a bare struct name is not a value
    expect_error(
        "struct P { x: u16 }\nfn f() { let mut p: P = P { x: 1 }; let q = p; }",
        "used as a value",
    );
    // a struct literal needs a type annotation
    expect_error(
        "struct P { x: u16 }\nfn f() { let p = P { x: 1 }; }",
        "needs a type annotation",
    );
    // unknown and missing fields
    expect_error(
        "struct P { x: u16 }\nfn f() { let mut p: P = P { y: 1 }; }",
        "no field `y`",
    );
    expect_error(
        "struct P { x: u16, y: u16 }\nfn f() { let mut p: P = P { x: 1 }; }",
        "missing field",
    );
    // immutability is enforced through the struct binding
    expect_error(
        "struct P { x: u16 }\nfn f() { let p: P = P { x: 1 }; p.x = 2; }",
        "not mutable",
    );
    // recursive layouts stay out of scope; a struct static is fine (spec §9.4)
    assert!(cpu_v2::frontend::parse_source(
        "struct P { x: u16 }\nstatic S: P = P { x: 1 };\nfn main() { halt(S.x); }"
    )
    .is_ok());
    expect_error("struct P { p: P }", "contains itself");
    // an aggregate static needs a literal, a matching length and known fields
    expect_error(
        "struct P { x: u16 }\nstatic A: P = make();\nfn make() -> P { P { x: 1 } }",
        "needs a P { .. } literal",
    );
    expect_error(
        "struct P { x: u16 }\nstatic A: Buf<P, 2> = Buf::new([P { x: 1 }]);",
        "expected 2",
    );
    expect_error(
        "struct P { x: u16 }\nstatic A: P = P { y: 1 };",
        "no field `y`",
    );
    expect_error("static A: (u16, u16) = (1, 2, 3);", "expected 2");
    // attributes other than repr/allow are still rejected
    expect_error("struct P { x: u16 }\n#[inline] fn f() {}", "attribute");
}

#[test]
fn test_tuple_and_sret_restrictions() {
    // a tuple is a value type: it can be returned, bound and destructured
    assert!(cpu_v2::frontend::parse_source("fn f() -> (u16, u16) { (1, 2) }").is_ok());
    assert!(cpu_v2::frontend::parse_source(
        "fn f() { let (a, b) = f2(); }\nfn f2() -> (u16, u16) { (1, 2) }"
    )
    .is_ok());
    // ... but it is not a parameter (spec §9c)
    expect_error("fn f(t: (u16, u16)) {}", "cannot be a parameter");
    // an aggregate return uses a hidden destination pointer: at most 5 parameters
    expect_error(
        "fn f(a: u16, b: u16, c: u16, d: u16, e: u16, g: u16) -> (u16, u16) { (a, g) }",
        "at most 5 parameters",
    );
    // the hidden destination means the call must be bound or returned directly
    expect_error(
        "fn g() -> (u16, u16) { (1, 2) }\nfn f(x: u16) {}\nfn main() { f(g().0); }",
        "hidden destination pointer",
    );
    // tuple elements are scalars, at most four
    expect_error(
        "fn f() -> (u16, u16, u16, u16, u16) { (1, 2, 3, 4, 5) }",
        "at most 4",
    );
    expect_error(
        "fn f(x: fix32) -> (fix32, u16) { (x, 0) }",
        "tuple elements must be",
    );
    // a tuple pattern must match the value's arity
    expect_error(
        "fn g() -> (u16, u16) { (1, 2) }\nfn main() { let (a, b, c) = g(); }",
        "the value has 2 elements",
    );
    // a tuple index must exist
    expect_error(
        "fn g() -> (u16, u16) { (1, 2) }\nfn main() { let t = g(); halt(t.5); }",
        "out of range",
    );
    // fn pointers cannot carry an aggregate return (no indirect sret)
    expect_error(
        "struct P { x: u16 }\nfn f(g: fn() -> P) {}",
        "fn pointer cannot return an aggregate",
    );
    // a tuple name is not a value on its own
    expect_error("fn main() { let t = (1u16, 2u16); }", "memory-resident");
}

#[test]
fn test_enum_restrictions() {
    // a C-style enum: fieldless variants, no explicit discriminants, no duplicates
    expect_error("enum E { A(u16) }", "fieldless");
    expect_error("enum E { A = 5 }", "explicit discriminants");
    expect_error("enum E { A, A }", "defined twice");
    expect_error("enum E { }", "at least one variant");
    assert!(
        cpu_v2::frontend::parse_source("enum E { A, B }\nfn main() { halt(E::A as u16); }").is_ok()
    );
    // an unknown variant lists the known ones
    expect_error(
        "enum E { A, B }\nfn main() { halt(E::C as u16); }",
        "has no variant `C`",
    );
    // comparing needs the derive, ordering is undefined, and types must match
    expect_error(
        "enum E { A, B }\nfn main() { if E::A == E::B { halt(1); } }",
        "needs #[derive(PartialEq)]",
    );
    expect_error(
        "#[derive(PartialEq)] enum E { A, B }\nfn main() { if E::A < E::B { halt(1); } }",
        "compare with `==`/`!=` only",
    );
    expect_error(
        "#[derive(PartialEq)] enum A { X }\n#[derive(PartialEq)] enum B { X }\nfn main() { if A::X == B::X { halt(1); } }",
        "cannot compare",
    );
    // an enum is a word, not an integer
    expect_error(
        "enum E { A }\nfn main() { halt((E::A + 1u16) as u16); }",
        "type mismatch",
    );
    // ... and it is a word, so it fits a struct field and a Buf element
    assert!(
        cpu_v2::frontend::parse_source(
            "enum E { A }\nstruct S { e: E }\nfn main() { let b: Buf<E, 2> = Buf::new([E::A, E::A]); halt(b.len()); }"
        )
        .is_ok()
    );
}

#[test]
fn test_match_restrictions() {
    // an enum match must be exhaustive; an integer match needs `_`
    expect_error(
        "#[derive(PartialEq)] enum E { A, B }\nfn main() { let e = E::A; match e { E::A => { halt(1); } } }",
        "not exhaustive",
    );
    expect_error(
        "fn main() { let x = 1u16; match x { 1u16 => { halt(1); } } }",
        "needs a `_` arm",
    );
    // bindings, guards, ranges and alternations are out of scope
    expect_error(
        "fn main() { let x = 1u16; match x { n => { halt(1); } _ => {} } }",
        "bindings are not supported",
    );
    expect_error(
        "fn main() { let x = 1u16; match x { 1u16 if x > 0u16 => { halt(1); } _ => {} } }",
        "guards are not supported",
    );
    expect_error(
        "fn main() { let x = 1u16; match x { 1u16..=3u16 => { halt(1); } _ => {} } }",
        "unsupported pattern",
    );
    expect_error(
        "fn main() { let x = 1u16; match x { 1u16 | 2u16 => { halt(1); } _ => {} } }",
        "unsupported pattern",
    );
    // duplicate arms, and a pattern from another enum
    expect_error(
        "fn main() { let x = 1u16; match x { 1u16 => {} 1u16 => {} _ => {} } }",
        "duplicate match arm",
    );
    expect_error(
        "#[derive(PartialEq)] enum A { X }\n#[derive(PartialEq)] enum B { X }\nfn main() { let a = A::X; match a { B::X => {} } }",
        "does not match",
    );
    // all variants, no `_`: fine
    assert!(
        cpu_v2::frontend::parse_source(
            "#[derive(PartialEq)] enum E { A, B }\nfn main() { let e = E::A; match e { E::A => { halt(1); } E::B => { halt(2); } } }"
        )
        .is_ok()
    );
    // match is a statement, so it cannot be a function''s tail value
    expect_error("fn f(x: u16) -> u16 { match x { _ => 0 } }", "match");
}
