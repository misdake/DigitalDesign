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
            "helper" => Ok("fn helper() {\n    let x = 1u16 * 2u16;\n    halt(x);\n}\n".to_string()),
            _ => Err(format!("unknown module `{name}`")),
        },
    ) {
        Ok(_) => panic!("unsupported module expression unexpectedly compiled"),
        Err(error) => error,
    };

    let (file, line, column) = error.location().expect("missing diagnostic location");
    assert_eq!((file, line), ("helper.rs", 2));
    assert!(column > 1);
    let rendered = error.to_string();
    assert!(rendered.contains(" --> helper.rs:2:"), "{rendered}");
    assert!(rendered.contains("not supported yet"), "{rendered}");
}

#[test]
fn test_value_conversion_error_uses_the_expression_location() {
    let source = concat!(
        "fn main() {\n",
        "    let x = 1u16;\n",
        "    let invalid = x < 3u16;\n",
        "}\n",
    );
    let error = match cpu_v2::frontend::compile_program_named(
        "value.rs",
        source,
        &cpu_v2::CompilerOptions::default(),
        &mut |name| Err(format!("unknown module `{name}`")),
    ) {
        Ok(_) => panic!("stored boolean unexpectedly compiled"),
        Err(error) => error,
    };

    assert_eq!(error.location().map(|(_, line, _)| line), Some(3));
    let rendered = error.to_string();
    assert!(rendered.contains("3 |     let invalid = x < 3u16;"), "{rendered}");
}

#[test]
fn test_unsupported_constructs() {
    expect_error("fn f(x: u16) -> u16 { x / 2 }", "not supported");
    expect_error("fn f(x: u16) -> u16 { x * 2 }", "not supported");
    expect_error("fn f(x: u16) -> u16 { x % 2 }", "not supported");
    expect_error("fn f(x: u16) -> u16 { match x { _ => 0 } }", "not supported");
    expect_error("fn f(x: u16) -> u16 { let g = |y| y; x }", "not supported");
    expect_error("fn f<T>(x: T) -> T { x }", "not supported");
    expect_error("fn f(x: u16) -> u16 { x as u32 }", "not supported");
    expect_error("struct S { x: u16 }", "not supported");
    expect_error("fn f(x: u16) -> u32 { x }", "not supported");
    expect_error("fn f(x: u16) { let b = x < 3u16; }", "bool");
    expect_error("fn f(x: u16) { if x { halt(0); } }", "boolean");
    expect_error("fn f(x: u16) { x = 1; }", "not mutable");
    expect_error("fn f(x: u16) -> u16 { return; }", "return");
    expect_error("fn f() { let a: [u16; 3]; }", "initializer");
    expect_error("fn f() { let mut x: u16 = 1; let p = &x; }", "not supported");
    expect_error("static mut X: u16 = 0; fn f() {}", "static mut");
    expect_error("fn f(a: [u16; 2]) {}", "array");
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
    // destructuring patterns: only plain identifiers may be bound
    expect_error("fn f() { let (a, b) = (1u16, 2u16); }", "pattern");
    expect_error("fn f() { let s = \"hi\"; }", "string");
    expect_error("fn f() { let x = 1.5; }", "float");
}

#[test]
fn test_unsupported_types() {
    // only u16/i16/Ptr/fn pointer exist (spec §1)
    expect_error("fn f(x: u8) {}", "type not supported");
    expect_error("fn f(x: u32) {}", "type not supported");
    expect_error("fn f(x: usize) {}", "type not supported");
    expect_error("fn f() -> u16 { 1u8 as u16 }", "suffix");
    // no fat slices, no references, no arrays in parameter position
    expect_error("fn f(s: [u16]) {}", "slice");
    expect_error("fn f(r: &u16) {}", "reference");
    expect_error("fn f(a: [u16; 2]) {}", "arrays are not allowed");
}

#[test]
fn test_operator_restrictions() {
    // no hardware mul/div (spec §1.1)
    expect_error("fn f(x: u16) -> u16 { x * 2 }", "not supported yet");
    expect_error("fn f(x: u16) -> u16 { x / 2 }", "not supported yet");
    expect_error("fn f(x: u16) -> u16 { x % 2 }", "not supported yet");
    // unary minus is i16-only, same as Rust
    expect_error("fn f(x: u16) -> u16 { -x }", "only allowed on i16");
    // the ISA has no register-shift: the amount must be a literal in 0..=15
    expect_error("fn f(x: u16, n: u16) -> u16 { x << n }", "shift amount");
    expect_error("fn f(x: u16) -> u16 { x << 16 }", "0..=15");
}

#[test]
fn test_bool_restrictions() {
    // bool only lives in conditions; it cannot be stored (spec §6)
    expect_error("fn f(x: u16) { let b = x < 3u16; }", "bool");
    expect_error("fn f(x: u16) { let b: bool = true; }", "bool");
    // a bare u16 is not a condition
    expect_error("fn f(x: u16) { if x { halt(0); } }", "boolean expression");
    expect_error("fn f(x: u16) { while x { halt(0); } }", "boolean expression");
    // a bool cannot be compared with an integer
    expect_error("fn f(x: u16) { if (x < 3u16) == 1 { halt(0); } }", "bool");
}

#[test]
fn test_mixed_and_ptr_comparisons() {
    // u16/i16 never mix without an explicit `as` cast
    expect_error("fn f(x: u16) { if x < 3i16 { halt(0); } }", "cannot compare");
    expect_error("fn f(x: i16) { if 1u16 == x { halt(0); } }", "cannot compare");
    // Ptr only compares with Ptr, never with integers
    expect_error("fn f(p: Ptr) { if p == 0u16 { halt(0); } }", "cannot compare");
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
    expect_error("fn g(a: u16) {} fn f(p: Ptr) { g(p); }", "expected u16, got Ptr");
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
    expect_error("fn f() { let a: [u16; 3]; }", "initializer");
    // list length must match the declared length
    expect_error("fn f() { let mut a: [u16; 3] = [1, 2]; }", "expected 3");
    expect_error("static A: [u16; 3] = [1, 2];", "expected 3");
    // the initializer must be [v; N] or a full list
    expect_error("fn f() { let mut a: [u16; 2] = 0; }", "array initializer");
    // static array initializers are compile-time constants only
    expect_error(
        "static A: [u16; 2] = [f(), 0]; fn f() -> u16 { 1 }",
        "constant expression",
    );
    expect_error("static A: [u16; 2] = [X, 0];", "unknown const");
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
fn test_bool_param_rejected() {
    expect_error("fn f(b: bool) {}", "bool");
}

#[test]
fn test_return_type_mismatch() {
    expect_error("fn f(x: i16) -> u16 { x }", "type mismatch");
    expect_error("fn f(x: i16) -> u16 { return x; }", "type mismatch");
}

#[test]
fn test_missing_return_at_end_of_body() {
    expect_error("fn f() -> u16 { let x = 1; }", "without returning");
}
