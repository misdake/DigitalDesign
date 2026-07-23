//! debug info tests: variables/files/locations and the pc->line table.

use cpu_v2::CompilerOptions;

#[test]
fn test_debug_info_contents() {
    let src = r#"
static TILE: [u16; 2] = [5, 6];
fn main() {
    let mut buf: [u16; 4] = [0; 4];
    buf.write(0, TILE.read(1));
    halt(buf.read(0));
}
"#;
    let opts = CompilerOptions::default();
    let program = cpu_v2::frontend::compile_program_named("test.rs", src, &opts, &mut |name| {
        Err(format!("unknown module `{name}`"))
    })
    .expect("parse failed");
    let mut c = cpu_v2::Compiler::new();
    c.set_debug(program.debug);
    for f in program.funcs {
        c.add_func(f);
    }
    let (_instructions, _listing, debug) = c.finish_with_debug("main");
    let text = debug.render();
    println!("{text}");

    // files: main file + std files
    assert!(text.contains("file 0 test.rs"));
    assert!(text.contains("rcc_std/heap.rs"));
    // globals with addresses
    assert!(text.contains("global TILE [u16; 2] 0x0000"));
    // the main function with its frame and the frame-local array
    let main = debug.functions.iter().find(|f| f.name == "main").unwrap();
    assert_eq!(main.file, 0);
    assert!(main.frame_size > 0);
    let buf = main.locals.iter().find(|v| v.name == "buf").unwrap();
    assert!(matches!(buf.loc, cpu_v2::VarLoc::Frame(_)), "{buf:?}");
    assert_eq!(buf.scope, Some((4, 7)));
    // pc->line table covers some of main's instructions
    let main_lines: Vec<_> = debug
        .lines
        .iter()
        .filter(|(addr, _, _)| (main.addr.0..main.addr.1).contains(addr))
        .collect();
    assert!(!main_lines.is_empty());
    assert!(main_lines.iter().all(|(_, file, _)| *file == 0));
}
