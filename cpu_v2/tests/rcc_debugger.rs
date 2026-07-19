//! debugger core tests: stepping, breakpoints, variable resolution.

mod common;

use cpu_v2::CompilerOptions;
use cpu_v2::debugger::{DebugSession, VarValue};

fn make_session(src: &str) -> DebugSession {
    let opts = CompilerOptions::default();
    let program = cpu_v2::frontend::compile_program(src, &opts, &mut |name| {
        Err(format!("unknown module `{name}`"))
    })
    .expect("parse failed");
    let mut c = cpu_v2::Compiler::new();
    c.set_debug(program.debug);
    for f in program.funcs {
        c.add_func(f);
    }
    let (instructions, _lst, debug) = c.finish_with_debug("main");

    // write bin/dbg to a unique temp path (tests run in parallel)
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rcc_dbg_test_{}_{}", std::process::id(), id));
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("t.bin");
    let mut bytes = b"RCC1".to_vec();
    bytes.extend_from_slice(&(instructions.len() as u32).to_le_bytes());
    for w in &instructions {
        bytes.extend_from_slice(&w.encode().to_le_bytes());
    }
    std::fs::write(&bin, &bytes).unwrap();
    std::fs::write(bin.with_extension("dbg"), debug.render()).unwrap();
    DebugSession::load(&bin).unwrap()
}

#[test]
fn test_step_and_breakpoint() {
    let src = r#"
fn main() {
    let mut x: u16 = 0;
    for i in 0..10u16 {
        x += i;
    }
    halt(x);
}
"#;
    let mut s = make_session(src);
    // single step advances pc by one
    let pc0 = s.sim.state.pc;
    s.step();
    assert_eq!(s.sim.state.pc, pc0 + 1);

    // breakpoint on the halt instruction of main
    let main = s.debug.functions.iter().find(|f| f.name == "main").unwrap();
    let halt_addr = s
        .disasm
        .iter()
        .find(|d| d.addr >= main.addr.0 && d.addr < main.addr.1 && d.text.starts_with("halt"))
        .map(|d| d.addr)
        .expect("no halt instruction in main");
    s.toggle_breakpoint(halt_addr, true);
    let (hit, halted) = s.continue_run(10_000);
    assert_eq!(hit, Some(halt_addr));
    assert_eq!(halted, None);
    // continuing from the breakpoint does not re-trigger and halts with 45
    let (_, halted) = s.continue_run(10_000);
    assert_eq!(halted, Some(45));
}

#[test]
fn test_variables() {
    let src = r#"
static G: u16 = 0;
fn set(g: u16) {
    addr_of(&G).write(0, g);
}
fn main() {
    set(1234);
    halt(G);
}
"#;
    let mut s = make_session(src);
    let _ = s.continue_run(10_000);
    assert_eq!(s.halted(), Some(1234));

    // the global reads through with its value
    let g = s.debug.globals.iter().find(|v| v.name == "G").unwrap();
    match s.var_value(g) {
        VarValue::Mem(_addr, words) => assert_eq!(words, vec![1234]),
        _ => panic!("global G should be memory-resident"),
    }

    // params of set() resolve to their ABI register value
    let set_f = s.debug.functions.iter().find(|f| f.name == "set").unwrap();
    let p = set_f.locals.iter().find(|v| v.name == "g").unwrap();
    // after the program ended, r2 keeps the last passed value
    match s.var_value(p) {
        VarValue::Reg(x) => assert_eq!(x, 1234),
        _ => panic!("param g should be a register"),
    }
}

#[test]
fn test_source_line_debugging() {
    let src = r#"
fn main() {
    let mut x: u16 = 0;
    for i in 0..10u16 {
        x += i;
    }
    halt(x);
}
"#;
    let mut s = make_session(src);

    // the first mapped line: `let mut x = 0` (line 2) folds into the loop phi
    // as an immediate, so the first surviving instruction is the loop cmp (3)
    let (file, line) = s.current_line().unwrap();
    assert_eq!((file, line), (0, 3));

    // breakpoint on the halt line (7) maps to the halt instruction
    let addr = s.toggle_breakpoint_line(0, 7, true).unwrap();
    let (hit, halted) = s.continue_run(10_000);
    assert_eq!(hit, Some(addr));
    assert_eq!(halted, None);

    // next_line runs to the halt and reports the signal
    let halted = s.next_line(10_000);
    assert_eq!(halted, Some(45));

    // no instruction for a nonexistent line
    assert_eq!(s.toggle_breakpoint_line(0, 999, true), None);
}
