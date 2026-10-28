//! The checked source is then lowered and executed on CPU V3, never on a host mock.
use super::{try_compile, CompilerOptions};
use crate::{CpuV3Sim, RunOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::PathBuf;
const LIBRARY: &str = r#"
#[rcc_resource]
pub struct Renderer{word:u16}
#[rcc_resource(parent=Renderer)]
pub struct Frame{word:u16}
#[rcc_resource(parent=Frame)]
pub struct Pass{word:u16}
static EVENTS:Buf<u16,32>=Buf::new([0;32]);
static COUNT:u16=0;
fn event(code:u16){EVENTS.as_array()[COUNT]=code;unsafe{addr_of(&COUNT).write(0,COUNT+1);}}
impl Drop for Renderer{fn drop(&mut self){event(10+self.word);}}
impl Drop for Frame{fn drop(&mut self){event(20+self.word);}}
impl Drop for Pass{fn drop(&mut self){event(30+self.word);}}
pub fn open(word:u16)->Renderer{Renderer{word:word}}
pub fn pair(a:&mut Renderer,b:&mut Renderer)->u16{a.word+b.word}
pub fn event_count()->u16{COUNT}
pub fn event_at(i:u16)->u16{EVENTS.as_array()[i]}
impl Renderer{
 pub fn frame(&mut self)->Frame{Frame{word:self.word}}
 pub fn finish(self){event(70+self.word);resource_disarm(self);}
}
impl Frame{
 pub fn pass(&mut self)->Pass{Pass{word:self.word}}
 pub fn finish(self){event(60+self.word);resource_disarm(self);}
}
impl Pass{
 pub fn draw(&mut self,code:u16){event(100+self.word+code);}
 pub fn finish(self){event(40+self.word);resource_disarm(self);}
}
"#;
fn options(label: &str) -> RustcCheck {
    let root = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("rcc-lifetimes-{}", std::process::id()))
        });
    RustcCheck::new(root.join(label))
}
fn parse(label: &str, source: &str) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    compile_program_named_checked(
        "checked-main.rs",
        source,
        &CompilerOptions::default(),
        &options(label),
        &mut |n| {
            if n == "owner" {
                Ok(LIBRARY.into())
            } else {
                Err(format!("unknown module {n}"))
            }
        },
    )
}
fn execute(label: &str, source: &str) {
    let code = try_compile(
        parse(label, source).unwrap_or_else(|e| panic!("{label}: {e}")),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(
        matches!(
            cpu.run(100_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ),
        "{label}"
    );
}
#[test]
fn rustc_drop_executes_real_method_chain_and_reverse_cleanup() {
    execute(
        "reverse",
        r#"mod owner;
fn main(){let mut r=open(1);{let mut f=r.frame();{let mut p=f.pass();p.draw(4);}}
 if event_count()!=3||event_at(0)!=105||event_at(1)!=31||event_at(2)!=21{halt(0xbad);}
 drop(r);if event_count()!=4||event_at(3)!=11{halt(0xbad);}halt(0x600d);}"#,
    );
}
#[test]
fn rustc_drop_return_exit_evaluates_value_before_cleanup() {
    execute(
        "return",
        r#"mod owner;
fn early(mut r:Renderer)->u16{let mut f=r.frame();let p=f.pass();return event_count()+9;}
fn main(){let r=open(2);if early(r)!=9{halt(0xbad);}
 if event_count()!=3||event_at(0)!=32||event_at(1)!=22||event_at(2)!=12{halt(0xbad);}halt(0x600d);}"#,
    );
}
#[test]
fn rustc_drop_loop_exits_and_consumption_are_exactly_once() {
    execute(
        "loop",
        r#"mod owner;
fn forward(r:Renderer)->Renderer{r}
fn main(){let mut i:u16=0;while i<3{let mut r=open(3);let f=r.frame();i+=1;if i==1{continue;}break;}
 if event_count()!=4||event_at(0)!=23||event_at(1)!=13||event_at(2)!=23||event_at(3)!=13{halt(0xbad);}
 let r=open(4);let mut a=forward(r);{let mut f=a.frame();{let p=f.pass();p.finish();}f.finish();}a.finish();
 if event_count()!=7||event_at(4)!=44||event_at(5)!=64||event_at(6)!=74{halt(0xbad);}halt(0x600d);}"#,
    );
}
#[test]
fn rustc_rejects_alias_moves_and_local_parent_escape_before_erasure() {
    for (label, body, code) in [
        (
            "alias",
            "fn main(){let mut r=open(1);pair(&mut r,&mut r);}",
            "E0499",
        ),
        (
            "parent-live",
            "fn main(){let mut r=open(1);let f=r.frame();r.finish();drop(f);}",
            "E0505",
        ),
        (
            "moved",
            "fn main(){let r=open(1);r.finish();drop(r);}",
            "E0382",
        ),
        (
            "escape",
            "fn bad(r:&mut Renderer)->Frame{let mut local=open(1);local.frame()}fn main(){}",
            "E0515",
        ),
    ] {
        let error = match parse(label, &format!("mod owner;\n{body}")) {
            Ok(_) => panic!("{label} accepted"),
            Err(e) => e,
        };
        assert!(
            error.to_string().contains(&format!("rustc {code}")),
            "{label}: {error}"
        );
        assert_eq!(
            (error.location().unwrap().0, error.location().unwrap().1),
            ("checked-main.rs", 2)
        );
    }
}
#[test]
fn drop_requires_checked_entry_and_never_silently_skips_rustc() {
    let opts = CompilerOptions::default();
    let source = "mod owner;fn main(){let r=open(1);}";
    let error = match rcc::frontend::compile_program_named("unchecked", source, &opts, &mut |_| {
        Ok(LIBRARY.into())
    }) {
        Ok(_) => panic!("unchecked Drop"),
        Err(e) => e,
    };
    assert!(
        error
            .to_string()
            .contains("Drop requires the rustc-checked frontend"),
        "{error}"
    );
    let mut check = options("missing-tool");
    check.executable = check.directory.join("absent-rustc-executable");
    let error =
        match compile_program_named_checked("missing tool", source, &opts, &check, &mut |_| {
            Ok(LIBRARY.into())
        }) {
            Ok(_) => panic!("skipped rustc"),
            Err(e) => e,
        };
    assert!(
        error
            .to_string()
            .contains("rustc ownership check unavailable"),
        "{error}"
    );
    assert!(check.directory.join("ownership.rs").is_file());
}

#[test]
fn checked_legacy_cleanup_keeps_the_same_target_instructions() {
    let library = r#"#[rcc_resource(cleanup=release)]pub struct R{word:u16}
static COUNT:u16=0;
fn release(r:R){unsafe{addr_of(&COUNT).write(0,COUNT+r.word);}}
pub fn make()->R{R{word:3}}
pub fn total()->u16{COUNT}
impl R{pub fn read(&mut self)->u16{self.word}}"#;
    let source="mod owner;fn main(){{let mut r=make();if r.read()!=3{halt(0xbad);}}if total()!=3{halt(0xbad);}halt(0x600d);}";
    let opts = CompilerOptions::default();
    let a =
        rcc::frontend::compile_program_named("legacy", source, &opts, &mut |_| Ok(library.into()))
            .unwrap();
    let b = compile_program_named_checked("legacy", source, &opts, &options("legacy"), &mut |_| {
        Ok(library.into())
    })
    .unwrap();
    let a = try_compile(a, &opts, "main").unwrap();
    let b = try_compile(b, &opts, "main").unwrap();
    assert_eq!(a.words, b.words);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&b.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn rustc_drop_fail_stop_does_not_unwind() {
    let code = try_compile(
        parse("halt", "mod owner;fn main(){let r=open(9);halt(0x600d);}").unwrap(),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    let count = code
        .debug
        .globals
        .iter()
        .find_map(|g| match g.loc {
            rcc::VarLoc::Global(a) if g.name == "COUNT" => Some(a),
            _ => None,
        })
        .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    assert_eq!(cpu.memory(count), 0);
}

#[test]
fn checked_drop_keeps_real_library_origins_and_private_domain() {
    let check = options("library");
    let dir = check.directory.join("sources");
    std::fs::create_dir_all(&dir).unwrap();
    let owner = r#"#![rcc_resource_library(helper)]
mod helper;
#[rcc_resource]pub struct Owner{word:u16}
impl Drop for Owner{fn drop(&mut self){helper_event(self.word);}}
pub fn make()->Owner{Owner{word:7}}
pub fn total()->u16{helper_total()}"#;
    let helper = r#"static COUNT:u16=0;
pub fn helper_event(v:u16){unsafe{addr_of(&COUNT).write(0,COUNT+v);}}
pub fn helper_total()->u16{COUNT}"#;
    std::fs::write(dir.join("owner.rs"), owner).unwrap();
    std::fs::write(dir.join("helper.rs"), helper).unwrap();
    let root = std::fs::canonicalize(dir.join("owner.rs")).unwrap();
    let opts = CompilerOptions::default();
    let compile = |source: &str, check: &RustcCheck| {
        rcc::frontend::compile_program_named_with_libraries_checked(
            "library-main.rs",
            source,
            &opts,
            std::slice::from_ref(&root),
            check,
            &mut |name| {
                let path = std::fs::canonicalize(dir.join(format!("{name}.rs")))
                    .map_err(|e| e.to_string())?;
                Ok(rcc::frontend::ModuleSource {
                    source: std::fs::read_to_string(&path).unwrap(),
                    origin: Some(path),
                })
            },
        )
    };
    let ir = compile(
        "mod owner;fn main(){{let r=make();}if total()!=7{halt(0xbad);}halt(0x600d);}",
        &check,
    )
    .unwrap();
    let program = try_compile(ir, &opts, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    let error = match compile(
        "mod owner;fn main(){helper_event(9);}",
        &options("library-private"),
    ) {
        Ok(_) => panic!("private helper accepted"),
        Err(e) => e,
    };
    assert!(
        error.to_string().contains("resource-library function"),
        "{error}"
    );
}

#[test]
fn checked_drop_rejects_unsupported_or_ambiguous_destructors() {
    for (label,library,expected) in [
        ("double-cleanup","#[rcc_resource(cleanup=cleanup)]struct R{word:u16}fn cleanup(r:R){}impl Drop for R{fn drop(&mut self){}}","cannot coexist"),
        ("ordinary-drop","struct R{word:u16}impl Drop for R{fn drop(&mut self){}}","limited to one-word"),
        ("reserved-drop","#[rcc_resource]struct R{word:u16}impl Drop for R{fn drop(&mut self){}}fn __rcc_drop_fake(){}","reserved Drop lowering name"),
    ] {
        let error=match compile_program_named_checked("drop-contract.rs",library,&CompilerOptions::default(),&options(label),&mut |_|Err("no modules".into())){Ok(_)=>panic!("{label} accepted"),Err(e)=>e};
        assert!(error.to_string().contains(expected),"{label}: {error}");
    }
}

#[test]
fn checked_drop_rejects_conditional_attributes_before_rustc() {
    for (index, attribute) in [
        "#[cfg(any())]",
        "#[cfg(all())]",
        "#[cfg_attr(all(), cfg(any()))]",
        "#[cfg_attr(all(), cfg(all()))]",
        "#[cfg_attr(any(), cfg(any()))]",
    ]
    .iter()
    .enumerate()
    {
        for scope in ["impl", "method"] {
            let (impl_attr, method_attr) = if scope == "impl" {
                (*attribute, "")
            } else {
                ("", *attribute)
            };
            let source = format!(
                "#[rcc_resource]struct R{{word:u16}}\n\
                 {impl_attr}impl Drop for R{{{method_attr}fn drop(&mut self){{halt(0xbad);}}}}\n\
                 fn main(){{let r=R{{word:1}};drop(r);halt(0x600d);}}"
            );
            let label = format!("drop-attribute-{scope}-{index}");
            let mut check = options(&label);
            // This syntax boundary must reject even a cfg-disabled method
            // before rustc's own missing-Drop-method diagnostic can take over.
            check.executable = check.directory.join("must-not-run-rustc.exe");
            let error = match compile_program_named_checked(
                "drop-attribute.rs",
                &source,
                &CompilerOptions::default(),
                &check,
                &mut |_| Err("no modules".into()),
            ) {
                Ok(_) => panic!("{label} accepted {attribute}"),
                Err(error) => error.to_string(),
            };
            assert!(
                error.contains("unsupported resource Drop attribute")
                    && error.contains("drop-attribute.rs"),
                "{label}: {error}"
            );
        }
    }
}

#[test]
fn checked_drop_accepts_documentation_and_lint_allowances() {
    execute(
        "drop-metadata",
        r#"#[rcc_resource]struct R{word:u16}
static COUNT:u16=0;
/// Owner cleanup.
#[allow(dead_code)]
impl Drop for R{
    /// Release one identity.
    #[allow(unused_variables)]
    fn drop(&mut self){unsafe{addr_of(&COUNT).write(0,COUNT+self.word);}}
}
fn main(){{let r=R{word:7};}if COUNT!=7{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn checked_drop_accepts_raw_resource_identifier_without_panicking() {
    execute(
        "raw-identifier",
        r#"#[rcc_resource]struct r#type{word:u16}
static COUNT:u16=0;
impl Drop for r#type{fn drop(&mut self){unsafe{addr_of(&COUNT).write(0,COUNT+self.word);}}}
fn main(){{let r=r#type{word:4};}if COUNT!=4{halt(0xbad);}halt(0x600d);}"#,
    );
    execute(
        "raw-cleanup",
        r#"#[rcc_resource(cleanup=r#match)]struct R{word:u16}
static COUNT:u16=0;
fn r#match(r:R){unsafe{addr_of(&COUNT).write(0,COUNT+r.word);}}
fn main(){{let a=R{word:4};}let b=R{word:5};drop(b);if COUNT!=9{halt(0xbad);}halt(0x600d);}"#,
    );
}
