//! debugger core tests: stepping, breakpoints, variable resolution.

use cpu_v2::{CompilerOptions, FunctionTableConfig};
use cpu_v2_sim::debugger::{DebugSession, VarValue};

fn make_session(src: &str) -> DebugSession {
    make_session_with_options(src, CompilerOptions::default())
}

fn make_session_with_options(src: &str, opts: CompilerOptions) -> DebugSession {
    let program = cpu_v2::frontend::compile_program(src, &opts, &mut |name| {
        Err(format!("unknown module `{name}`"))
    })
    .expect("parse failed");
    let mut c = cpu_v2::Compiler::new();
    c.opts = opts;
    c.set_debug(program.debug);
    for f in program.funcs {
        c.add_func(f);
    }
    let (instructions, listing, debug) = c.finish_with_debug("main");

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
    std::fs::write(bin.with_extension("lst"), listing).unwrap();
    DebugSession::load(&bin).unwrap()
}

const SIGNED_OPS: &str = r#"fn clamp(x: i16) -> i16 {
    if x >= 2 && x <= 10 {
        x
    } else {
        0
    }
}
fn main() {
    let a = clamp(7);
    let b = clamp(-3);
    let d = (-8i16) >> 1;
    let e = d + a + b;
    halt(e as u16);
}
"#;

#[test]
fn test_call_disassembly_is_marked_with_a_navigation_target() {
    let s = make_session(SIGNED_OPS);
    let clamp_calls: Vec<_> = s
        .disasm
        .iter()
        .filter(|line| line.call && line.target_name.as_deref() == Some("clamp"))
        .collect();
    assert_eq!(clamp_calls.len(), 2);
    assert!(clamp_calls.iter().all(|line| line.target.is_some()));
    assert!(s.state_json().contains("\"call\":true"));
}

#[test]
fn test_call_abs_debug_info_restores_navigation_target() {
    let source = r#"fn inc(x: u16) -> u16 { x + 1 }
fn main() {
    let a = inc(0);
    let b = inc(a);
    let c = inc(b);
    let d = inc(c);
    halt(d);
}
"#;
    let opts = CompilerOptions {
        function_table: FunctionTableConfig::All,
        ..CompilerOptions::default()
    };
    let s = make_session_with_options(source, opts);
    assert_eq!(s.debug.function_table, vec![(0, "inc".to_string())]);
    assert!(s
        .debug
        .init_sections
        .iter()
        .any(|section| section.name == "function-table"));
    assert!(
        s.disasm
            .iter()
            .any(|line| line.init_start
                && line.init.as_deref().is_some_and(|s| s.contains("entries")))
    );
    assert!(s.state_json().contains("\"initStart\":true"));
    let calls: Vec<_> = s
        .disasm
        .iter()
        .filter(|line| line.call && line.target_name.as_deref() == Some("inc"))
        .collect();
    assert_eq!(calls.len(), 4);
    assert!(calls
        .iter()
        .all(|line| line.text.starts_with("call_abs") && line.target.is_some()));
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

    // The first source parameter lives in ABI argument register r2 (not r0).
    let set_f = s.debug.functions.iter().find(|f| f.name == "set").unwrap();
    let p = set_f.locals.iter().find(|v| v.name == "g").unwrap();
    assert_eq!(p.loc, cpu_v2::VarLoc::Param(2));
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

#[test]
fn test_call_stack_and_step_over_out() {
    let src = r#"
fn inc(x: u16) -> u16 {
    x + 1
}
fn main() {
    let a = inc(1);
    let b = inc(a);
    halt(b);
}
"#;
    let mut s = make_session(src);

    // step over the first inc() call: no descent into inc
    s.step_over(1000);
    assert_eq!(s.depth(), 0, "step_over must not descend into the call");

    // next_line actually enters inc
    let mut s2 = make_session(src);
    s2.next_line(1000);
    assert!(
        s2.depth() >= 1 || s2.last_halt.is_some(),
        "next_line should enter the call (or have run past)"
    );

    // step into inc manually, then step out
    let mut s3 = make_session(src);
    for _ in 0..20 {
        if s3.depth() > 0 {
            break;
        }
        s3.step();
    }
    assert_eq!(s3.depth(), 1, "expected to be inside inc");
    let f = s3.current_func().unwrap();
    assert_eq!(f.name, "inc");
    s3.step_out(1000);
    assert_eq!(s3.depth(), 0, "step_out must return to main");

    // call stack is empty after the program halts
    let _ = s3.continue_run(1000);
    assert_eq!(s3.last_halt, Some(3));
}

#[test]
fn test_call_site_in_stack_frame() {
    let src = r#"
fn inc(x: u16) -> u16 {
    x + 1
}
fn main() {
    let a = inc(1);
    halt(a);
}
"#;
    let mut s = make_session(src);
    for _ in 0..30 {
        if s.depth() > 0 {
            break;
        }
        s.step();
    }
    assert_eq!(s.depth(), 1);
    // the frame knows where inc was called from (the `let a = inc(1);` line)
    let frame = &s.call_stack[0];
    assert_eq!(frame.func_name, "inc");
    let site = s
        .debug
        .lines
        .iter()
        .filter(|(a, _, _)| *a <= frame.return_addr.saturating_sub(1))
        .max_by_key(|(a, _, _)| *a)
        .map(|&(_, f, l)| (f, l));
    assert_eq!(site, Some((0, 6)), "call site should be line 6");
}

#[test]
fn test_signed_ops_line_mapping_covers_conditions_and_call_slots() {
    let s = make_session(SIGNED_OPS);
    let clamp = s
        .debug
        .functions
        .iter()
        .find(|f| f.name == "clamp")
        .unwrap();
    let main = s.debug.functions.iter().find(|f| f.name == "main").unwrap();

    let condition: Vec<_> = s
        .debug
        .lines
        .iter()
        .filter(|(addr, file, line)| {
            *file == clamp.file && *line == 2 && clamp.addr.0 <= *addr && *addr < clamp.addr.1
        })
        .map(|(addr, _, _)| (*addr, &s.disasm[*addr].text))
        .collect();
    assert!(
        condition.iter().any(|(_, text)| text.contains("cmp(")),
        "condition={condition:?}\nlines={:?}\ndisasm={:?}",
        s.debug.lines,
        s.disasm
            .iter()
            .map(|d| (d.addr, &d.text))
            .collect::<Vec<_>>()
    );
    assert!(
        condition
            .iter()
            .filter(|(_, text)| text.starts_with('j'))
            .count()
            >= 3,
        "short-circuit branches and their trampoline must all be mapped: {condition:?}"
    );

    let call: Vec<_> = s
        .debug
        .lines
        .iter()
        .filter(|(addr, file, line)| {
            *file == main.file && *line == 10 && main.addr.0 <= *addr && *addr < main.addr.1
        })
        .map(|(addr, _, _)| (*addr, &s.disasm[*addr].text))
        .collect();
    assert!(
        call.iter().any(|(_, text)| text.starts_with("call_rel")),
        "{call:?}"
    );
    assert_eq!(
        call.iter()
            .filter(|(_, text)| text.starts_with("call_rel"))
            .count(),
        1,
        "{call:?}"
    );
    assert!(
        call.iter().all(|(_, text)| text.as_str() != "r15 = r15"),
        "a relaxed near call must not retain padding: {call:?}"
    );
}

#[test]
fn test_entering_function_uses_that_function_source_line() {
    let mut s = make_session(SIGNED_OPS);
    for _ in 0..100 {
        if s.current_func().is_some_and(|f| f.name == "clamp") {
            break;
        }
        s.step();
    }
    assert_eq!(s.current_func().map(|f| f.name.as_str()), Some("clamp"));
    assert_eq!(s.current_line(), Some((0, 2)));
}

#[test]
fn test_parameter_value_survives_nested_call_register_clobber() {
    let src = r#"fn clobber(x: u16) -> u16 {
    x + 1
}
fn keep(p: u16) -> u16 {
    let q = clobber(99);
    p + q
}
fn main() {
    halt(keep(42));
}
"#;
    let mut s = make_session(src);
    for _ in 0..100 {
        if s.current_func().is_some_and(|f| f.name == "clobber") {
            break;
        }
        s.step();
    }
    assert_eq!(s.current_func().map(|f| f.name.as_str()), Some("clobber"));
    s.step_out(100);
    assert_eq!(s.current_func().map(|f| f.name.as_str()), Some("keep"));
    assert_ne!(
        s.sim.state.reg[2], 42,
        "nested call should have clobbered live r2"
    );

    let keep = s.current_func().unwrap();
    let p = keep.locals.iter().find(|v| v.name == "p").unwrap();
    assert_eq!(s.var_value(p), VarValue::Reg(2, 42));
}

#[test]
fn test_locals_follow_lexical_scope_and_frame_values() {
    let src = r#"fn main() {
    let outer: u16 = 1;
    if outer == 1 {
        let mut inner: u16 = 7;
        addr_of(&inner).write(0, 8);
    }
    halt(outer);
}
"#;
    let mut s = make_session(src);
    assert!(!s.state_json().contains("\"name\":\"inner\""));

    let inner_pc = s.toggle_breakpoint_line(0, 5, true).unwrap();
    assert_eq!(s.continue_run(100).0, Some(inner_pc));
    let json = s.state_json();
    assert!(json.contains("\"name\":\"inner\""), "{json}");
    assert!(json.contains("\"words\":[7]"), "{json}");

    let halt_pc = s.toggle_breakpoint_line(0, 7, true).unwrap();
    assert_eq!(s.continue_run(100).0, Some(halt_pc));
    assert!(!s.state_json().contains("\"name\":\"inner\""));
}
