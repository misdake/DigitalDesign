//! Execute lexical-resource cleanup through the production CPU V3 scalar ABI.
use super::{try_compile, CompilerOptions};
use crate::{CpuV3Sim, RunOutcome};

const LIBRARY: &str = r#"
#[rcc_resource(cleanup = renderer_cleanup)]
pub struct Renderer { word: u16 }
#[rcc_resource(cleanup = frame_cleanup, parent = Renderer)]
pub struct Frame { word: u16 }
#[rcc_resource(cleanup = pass_cleanup, parent = Frame)]
pub struct Pass { word: u16 }
static EVENTS: Buf<u16,32> = Buf::new([0;32]);
static COUNT:u16=0;
const SECRET:u16=123;
fn event(code:u16){EVENTS.as_array()[COUNT]=code;unsafe{addr_of(&COUNT).write(0,COUNT+1);}}
fn renderer_cleanup(r:Renderer){event(10+r.word);}
fn frame_cleanup(f:Frame){event(20+f.word);}
fn pass_cleanup(p:Pass){event(30+p.word);}
pub fn renderer_open(word:u16)->Renderer{Renderer{word:word}}
pub fn frame_open(r:&mut Renderer)->Frame{Frame{word:r.word}}
pub fn pass_open(f:&mut Frame)->Pass{Pass{word:f.word}}
pub fn renderer_query(r:&mut Renderer)->u16{r.word}
pub fn inspect_two(a:&mut Renderer,b:&mut Renderer)->u16{a.word+b.word}
pub fn inspect_code(a:&mut Renderer,code:u16)->u16{a.word+code}
pub fn consume_return(a:Renderer)->u16{resource_disarm(a);return 0;}
pub fn consume_true(a:Renderer)->bool{resource_disarm(a);return true;}
pub fn with_code(f:Frame,code:u16)->u16{code}
pub fn close_and_zero(r:Renderer)->u16{0}
pub fn count_then_close(r:Renderer)->u16{event_count()}
pub fn fpu_then_close(r:Renderer)->fix32{fix32::from_int(7)}
pub fn identity(code:u16)->u16{code}
pub fn pass_end(p:Pass){event(40+p.word);resource_disarm(p);}
pub fn end(p:Pass){resource_disarm(p);}
pub fn both_frames(a:Frame,b:Frame){resource_disarm(a);resource_disarm(b);}
pub fn frame_complete(f:Frame,fault:bool){if fault{event(50+f.word);}else{event(60+f.word);}resource_disarm(f);}
pub fn renderer_close(r:Renderer){event(70+r.word);resource_disarm(r);}
pub fn branch_close(r:Renderer,choice:bool){if choice{renderer_close(r);}else{renderer_close(r);}}
pub fn event_count()->u16{COUNT}
pub fn event_at(i:u16)->u16{EVENTS.as_array()[i]}
fn raw_word()->u16{7}
"#;

fn parse(main: &str) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    let options = CompilerOptions::default();
    rcc::frontend::compile_program_named("resource application", main, &options, &mut |n| {
        if n == "resource_library" {
            Ok(LIBRARY.to_owned())
        } else {
            Err(format!("unknown module {n}"))
        }
    })
}
fn execute(main: &str) {
    let parsed = parse(main).unwrap_or_else(|e| panic!("{e}"));
    let options = CompilerOptions::default();
    let code = try_compile(parsed, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

pub(crate) fn protocol_program() -> Vec<u16> {
    let source = r#"mod resource_library;
fn work()->u16{let mut r=renderer_open(1);let mut f=frame_open(&mut r);
 {let p=pass_open(&mut f);}frame_complete(f,false);return 9;}
fn main(){if work()!=9||event_count()!=3||event_at(0)!=31||event_at(1)!=61||event_at(2)!=11{halt(0xbad);}halt(0x600d);}"#;
    let options = CompilerOptions::default();
    try_compile(parse(source).unwrap(), &options, "main")
        .unwrap()
        .words
}

#[test]
fn lexical_resources_transfer_cancel_and_parent_identity_execute() {
    execute(
        r#"mod resource_library;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let mut r=renderer_open(1);let mut other=renderer_open(2);
 {let mut f=frame_open(&mut r);
  check(renderer_query(&mut other)==2);
  {let p=pass_open(&mut f);}
  frame_complete(f,false);
 }
 {let mut f=frame_open(&mut r);let p=pass_open(&mut f);let moved=p;pass_end(moved);frame_complete(f,true);}
 branch_close(other,true);renderer_close(r);
 check(event_count()==6);
 check(event_at(0)==31&&event_at(1)==61&&event_at(2)==41&&event_at(3)==51);
 check(event_at(4)==72&&event_at(5)==71);halt(0x600d);
}"#,
    );
}

#[test]
fn lexical_resources_early_return_partial_construction_and_loop_exits_execute() {
    execute(
        r#"mod resource_library;
fn check(ok:bool){if !ok{halt(0xbad);}}
fn early(flag:bool)->u16{
 let mut r=renderer_open(1);
 if flag{return event_count();}
 let mut f=frame_open(&mut r);let p=pass_open(&mut f);
 return event_count();
}
fn main(){
 check(early(true)==0);check(event_count()==1&&event_at(0)==11);
 check(early(false)==1);check(event_count()==4);
 check(event_at(1)==31&&event_at(2)==21&&event_at(3)==11);
 let mut r=renderer_open(2);let mut i:u16=0;
 while i<3{let mut f=frame_open(&mut r);let p=pass_open(&mut f);i+=1;
  if i==1{continue;}if i==2{break;}
 }
 renderer_close(r);check(event_count()==9);
 check(event_at(4)==32&&event_at(5)==22&&event_at(6)==32&&event_at(7)==22&&event_at(8)==72);
 halt(0x600d);
}"#,
    );
}

#[test]
fn lexical_resources_reject_lifetime_and_escape_errors() {
    for (body, expected) in [
        (
            "let r=renderer_open(1);let copy=r;renderer_close(r);",
            "moved or consumed",
        ),
        (
            "let mut r=renderer_open(1);renderer_close(r);renderer_query(&mut r);",
            "moved or consumed",
        ),
        (
            "let mut r=renderer_open(1);let f=frame_open(&mut r);renderer_close(r);",
            "live child",
        ),
        (
            "let mut r=renderer_open(1);inspect_two(&mut r,&mut r);",
            "aliased call arguments",
        ),
        (
            "let mut r=renderer_open(1);inspect_code(&mut r,consume_return(r));",
            "pending call",
        ),
        (
            "let r=renderer_open(1);let flag=false&&consume_true(r);",
            "short-circuit",
        ),
        ("let r=renderer_open(1);{let r:u16=0;}", "shadowing"),
        (
            "let r=renderer_open(1);renderer_query(&mut r);",
            "mutable owner",
        ),
        ("let r:Renderer=1;", "forge"),
        ("let r=Renderer{word:1};", "construction is private"),
        ("let r=renderer_open(1);let n=r.word;", "fields are private"),
        ("let r=renderer_open(1);let n=r as u16;", "ordinary value"),
        (
            "let mut r=renderer_open(1);let p=view_of(&mut r);",
            "direct &mut call",
        ),
        ("let r:u16=renderer_open(1);", "ordinary type"),
        ("let ptr=renderer_open;", "function pointers"),
        ("let ptr=raw_word;", "private resource-module"),
        (
            "let ptr=EVENTS.as_array();",
            "private resource-module storage",
        ),
        ("let n=COUNT;", "private resource-module storage"),
        ("let renamed=raw_word;renamed();", "private resource-module"),
        ("let n=1 as Renderer;", "cast to a resource"),
        (
            "let r=renderer_open(1);let p=addr_of(&r);",
            "direct &mut call",
        ),
        (
            "let r=renderer_open(1);resource_disarm(r);",
            "private to the resource",
        ),
        (
            "use resource_library::raw_word;raw_word();",
            "private resource-module",
        ),
        (
            "let r=renderer_open(1);if true{renderer_close(r);}",
            "branch merge",
        ),
        (
            "let r=renderer_open(1);while true{renderer_close(r);}",
            "loop backedge",
        ),
        (
            "let r=renderer_open(1);while true{renderer_close(r);break;}",
            "loop exit/backedge",
        ),
    ] {
        let source = format!("mod resource_library;fn main(){{{body}halt(0x600d);}}");
        let e = match parse(&source) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("unexpectedly accepted {body}"),
        };
        assert!(e.contains(expected), "{body}: expected {expected}, got {e}");
    }
    for source in [
        "mod resource_library;use resource_library::raw_word as renamed;fn main(){renamed();}",
        "mod resource_library;use resource_library::raw_word;fn main(){raw_word();}",
        "mod resource_library;fn main(){resource_library::raw_word();}",
        "mod resource_library;fn escape()->Frame{let mut r=renderer_open(1);frame_open(&mut r)}fn main(){}",
        "mod resource_library;static X:Buf<Renderer,1>=Buf::new([0;1]);fn main(){}",
        "mod resource_library;struct Boxed{r:Renderer}fn main(){}",
    ] { assert!(parse(source).is_err(), "unexpectedly accepted {source}"); }
}

#[test]
fn lexical_resources_reject_internal_field_addresses_and_identity_mutation() {
    for extra in [
        "fn illegal(r:&mut Renderer)->Ptr{addr_of(&r.word)}",
        "fn illegal(r:&mut Renderer){r.word=7;}",
        "fn illegal(r:&mut Renderer){r.word+=1;}",
        "fn illegal(){let mut r=renderer_open(1);let f=Frame{word:close_and_zero(r)};}",
    ] {
        let options = CompilerOptions::default();
        let result = rcc::frontend::compile_program_named(
            "resource application",
            "mod resource_library;fn main(){}",
            &options,
            &mut |_| Ok(format!("{LIBRARY}\n{extra}")),
        );
        assert!(result.is_err(), "unexpectedly accepted {extra}");
    }
}

#[test]
fn lexical_resources_fail_stop_does_not_unwind_retained_owners() {
    execute(
        r#"mod resource_library;
fn freeze()->!{halt(0x600d);}
fn main(){let mut r=renderer_open(1);let mut f=frame_open(&mut r);let p=pass_open(&mut f);freeze();}"#,
    );
    execute(r#"mod resource_library;fn main(){let r=renderer_open(1);halt(0x600d);}"#);
}

#[test]
fn lexical_resources_reject_reparenting_and_unbound_sibling_arguments() {
    for (source, expected) in [
        ("mod resource_library;fn reparent(f:Frame,other:&mut Renderer)->Frame{f}fn main(){let mut a=renderer_open(1);let mut b=renderer_open(2);let f=frame_open(&mut a);let g=reparent(f,&mut b);let n=renderer_query(&mut a);frame_complete(g,false);}", "no provable borrowed parent"),
        ("mod resource_library;fn main(){let mut a=renderer_open(1);both_frames(frame_open(&mut a),frame_open(&mut a));}", "bind factory results first"),
    ] {
        let failure = match parse(source) { Ok(_) => panic!("accepted {source}"), Err(e) => e.to_string() };
        assert!(failure.contains(expected), "{failure}");
    }
}

#[test]
fn lexical_resources_private_initializers_and_type_lengths_ignore_module_order() {
    let options = CompilerOptions::default();
    for module_order in [
        "mod resource_library;mod thief;",
        "mod thief;mod resource_library;",
    ] {
        for thief in [
            "const LEAK:u16=SECRET;pub fn reveal()->u16{LEAK}",
            "static LEAK:u16=SECRET;pub fn reveal()->u16{LEAK}",
            "struct Payload{data:Buf<u16,SECRET>}pub fn reveal()->u16{0}",
            "pub fn reveal()->u16{let b:Buf<u16,SECRET>=Buf::new([0;1]);0}",
            "pub fn reveal(arg:Array<Buf<u16,SECRET>>)->u16{0}",
        ] {
            let source = format!("{module_order}fn main(){{halt(0x600d);}}");
            let result = rcc::frontend::compile_program_named(
                "privacy review",
                &source,
                &options,
                &mut |n| {
                    Ok(if n == "thief" {
                        thief.to_owned()
                    } else {
                        LIBRARY.to_owned()
                    })
                },
            );
            let e = match result {
                Ok(_) => panic!("accepted {thief}"),
                Err(e) => e.to_string(),
            };
            assert!(e.contains("private resource-module"), "{e}");
        }
    }
}

#[test]
fn lexical_resources_preserve_ordinary_lexical_name_shadowing() {
    execute(
        r#"mod resource_library;
fn keep(end:u16)->u16{end}
fn main(){let end:u16=7;let COUNT:u16=9;if end!=7||keep(8)!=8||COUNT!=9{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn lexical_resources_owned_arguments_keep_their_parent_locked_until_the_call() {
    for (value, message) in [
        ("renderer_query(&mut a)", "live child"),
        ("identity(renderer_query(&mut a))", "live child"),
        ("close_and_zero(a)", "live child"),
        ("if true{return;}else{42}", "arguments are pending"),
    ] {
        let source = format!("mod resource_library;fn main(){{let mut a=renderer_open(1);let f=frame_open(&mut a);with_code(f,{value});}}");
        let e = match parse(&source) {
            Ok(_) => panic!("accepted {source}"),
            Err(e) => e.to_string(),
        };
        assert!(e.contains(message), "{e}");
    }
    execute(
        r#"mod resource_library;fn main(){let mut a=renderer_open(1);let f=frame_open(&mut a);
if with_code(f,42)!=42||event_count()!=1||event_at(0)!=21{halt(0xbad);}renderer_close(a);halt(0x600d);}"#,
    );
}

#[test]
fn lexical_resources_scalar_tail_values_precede_exactly_once_cleanup() {
    execute(
        r#"mod resource_library;fn main(){
let a=renderer_open(1);if close_and_zero(a)!=0||event_count()!=1||event_at(0)!=11{halt(0xbad);}
let b=renderer_open(2);if count_then_close(b)!=1||event_count()!=2||event_at(1)!=12{halt(0xbad);}
let c=renderer_open(3);if fpu_then_close(c).to_int()!=7||event_count()!=3||event_at(2)!=13{halt(0xbad);}halt(0x600d);}"#,
    );
}
