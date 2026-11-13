use super::*;

#[test]
fn debug_info_covers_modules_lines_and_locals() {
    let source = r#"
            mod helper;
            fn main() {
                let x: u16 = double(21);
                halt(x);
            }
        "#;
    let module = "fn double(v: u16) -> u16 { v + v }";
    let program = rcc::frontend::compile_program_named(
        "games/My  frame Demo.rs",
        source,
        &CompilerOptions::default(),
        &mut |name| {
            assert_eq!(name, "helper");
            Ok(module.to_string())
        },
    )
    .unwrap();
    let program = super::super::compile(program, &CompilerOptions::default(), "main");
    let debug = &program.debug;
    // both files recorded
    assert!(debug.files.len() >= 2, "files: {:?}", debug.files);
    // the line map is non-empty, sorted, and in range
    assert!(!debug.lines.is_empty());
    assert!(debug.lines.windows(2).all(|w| w[0].0 <= w[1].0));
    let main_fn = debug
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("main in debug info");
    assert!(main_fn.addr.1 > main_fn.addr.0);
    // `x` is a local in main with a concrete lowered location
    let x = main_fn
        .locals
        .iter()
        .find(|v| v.name == "x")
        .expect("local x in debug info");
    match x.loc {
        rcc::VarLoc::Frame(_) | rcc::VarLoc::Param(_) | rcc::VarLoc::Ssa => {}
        ref other => panic!("unexpected location for x: {other:?}"),
    }
    // render/parse round-trip
    let parsed = rcc::parse_debug(&debug.render()).expect("debug info round-trips");
    assert_eq!(parsed.files.len(), debug.files.len());
    assert_eq!(parsed.files, debug.files);
    assert_eq!(parsed.functions.len(), debug.functions.len());
    for (actual, expected) in parsed.functions.iter().zip(&debug.functions) {
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.file, expected.file);
        assert_eq!(actual.frame_size, expected.frame_size);
    }
    assert_eq!(parsed.lines.len(), debug.lines.len());
}
