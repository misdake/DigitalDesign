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
pub fn shared_pair(a:&Renderer,b:&Renderer)->u16{a.word+b.word}
pub fn shared_forward(a:&mut Renderer)->u16{shared_pair(a,a)}
pub fn event_count()->u16{COUNT}
pub fn event_at(i:u16)->u16{EVENTS.as_array()[i]}
impl Renderer{
 pub fn new(word:u16)->Self{Self{word:word}}
 pub fn relay(word:u16)->Self{Self::new(word)}
 pub fn transfer(owner:Self)->Self{owner}
 fn hidden()->Self{Self{word:9}}
 pub fn identity(&self)->u16{self.word}
 pub fn frame(&mut self)->Frame{Frame{word:self.word}}
 pub fn finish(self){event(70+self.word);resource_disarm(self);}
}
impl Frame{
 pub fn from_parent(r:&mut Renderer)->Self{Self{word:r.word}}
 pub fn relay(r:&mut Renderer)->Self{Self::from_parent(r)}
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
    parse_with_options(label, source, &CompilerOptions::default())
}
fn parse_with_options(
    label: &str,
    source: &str,
    opts: &CompilerOptions,
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    compile_program_named_checked("checked-main.rs", source, opts, &options(label), &mut |n| {
        if n == "owner" {
            Ok(LIBRARY.into())
        } else if n == "graphics" {
            Ok(GRAPHICS_LIBRARY.into())
        } else {
            Err(format!("unknown module {n}"))
        }
    })
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
    let outcome = cpu.run(100_000).unwrap();
    assert!(
        matches!(outcome, RunOutcome::Halted { signal: 0x600d, .. }),
        "{label}: {outcome:?}"
    );
}

#[test]
fn rustc_one_word_values_use_registers_and_preserve_argument_snapshots() {
    let calls = "if read(identity(value))!=21{halt(0xbad);}".repeat(260);
    execute(
        "word-value-abi",
        &format!(
            r#"
#[derive(Copy,Clone)]struct Word{{value:u16}}
#[derive(Copy,Clone)]struct Nested{{word:Word}}
fn identity(value:Word)->Word{{value}}
fn read(value:Word)->u16{{value.value}}
impl Word{{fn mutate(&mut self)->u16{{self.value=9;0}}}}
fn snapshot(mut value:Word,ignored:u16)->u16{{value.value+=1;value.value}}
fn six(a:u16,b:u16,c:u16,d:u16,e:u16,f:u16)->Word{{return Word{{value:a+b+c+d+e+f}};}}
fn nested(value:Nested)->Nested{{Nested{{word:identity(value.word)}}}}
fn tuple(value:(u16,))->(u16,){{value}}
fn array()->Buf<u16,1>{{Buf::new([8u16;1])}}
fn main(){{
 let mut original=Word{{value:3}};
 if snapshot(original,original.mutate())!=4||original.value!=9{{halt(0xbad);}}
 let source=six(1,2,3,4,5,6);let value=source;
 {calls}
 let call:fn(Word)->u16=read;
 if call(value)!=21{{halt(0xbad);}}
 let result=nested(Nested{{word:value}});
 let tuple_value=tuple((7u16,));
 let array_value=array();
 if result.word.value!=21||tuple_value.0!=7||array_value[0u16]!=8{{halt(0xbad);}}
 halt(0x600d);
}}
"#
        ),
    );
}

#[test]
fn rustc_temporary_storage_tracks_live_values_instead_of_call_count() {
    for optimized in [true, false] {
        let options = CompilerOptions {
            opt: if optimized {
                rcc::Opts::default()
            } else {
                rcc::Opts::disabled()
            },
            ..CompilerOptions::default()
        };
        let mut frames = Vec::new();
        for count in [8u16, 160] {
            let assignments = "value=bump(value,1);".repeat(usize::from(count / 2));
            let calls =
                format!("if read(bump(value,1),0)!={}{{halt(0xbad);}}", count + 4).repeat(32);
            let source = format!(
                r#"
#[derive(Copy,Clone)]struct Pair{{left:u16,right:u16}}
impl Pair{{fn mutate(&mut self)->u16{{self.left=9;0}}}}
fn read(value:Pair,ignored:u16)->u16{{value.left+value.right}}
fn bump(value:Pair,amount:u16)->Pair{{Pair{{left:value.left+amount,right:value.right}}}}
fn combine(a:Pair,b:Pair)->Pair{{Pair{{left:a.left+b.left,right:a.right+b.right}}}}
fn main(){{
 let mut value=Pair{{left:1,right:2}};
 if read(value,value.mutate())!=3||value.left!=9{{halt(0xbad);}}
 value.left=1;
 {assignments}
 let mut marker=Pair{{left:100,right:200}};
 let held=view_of(&mut marker);
 {assignments}
 let joined=combine(value,bump(value,3));
 if joined.left!={}||joined.right!=4{{halt(0xbad);}}
 {calls}
 if value.left!={}||held[0u16].left!=100||held[0u16].right!=200{{halt(0xbad);}}
 halt(0x600d);
}}
"#,
                count * 2 + 5,
                count + 1
            );
            let parsed = parse_with_options(
                &format!("temporary-live-{count}-{optimized}"),
                &source,
                &options,
            )
            .unwrap_or_else(|e| panic!("{e}"));
            frames.push(
                parsed
                    .funcs
                    .iter()
                    .find(|f| f.name == "main")
                    .unwrap()
                    .local_slots,
            );
            let code = try_compile(parsed, &options, "main").unwrap();
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&code.words);
            assert!(matches!(
                cpu.run(100_000).unwrap(),
                RunOutcome::Halted { signal: 0x600d, .. }
            ));
        }
        assert_eq!(
            frames[0], frames[1],
            "scratch must depend on simultaneous live values"
        );
        println!(
        "temporary storage (optimized={optimized}): 8 and 160 assignment sites both use {} local words",
        frames[0]
    );
    }
}

#[test]
fn rustc_temporary_receivers_and_address_results_keep_their_storage() {
    let calls = "if Pair::new(1).total(bump(Pair::new(2),3))!=11{halt(0xbad);}".repeat(80);
    execute(
        "temporary-receiver-escape",
        &format!(
            r#"
#[derive(Copy,Clone)]struct Pair{{left:u16,right:u16}}
#[derive(Copy,Clone)]struct Reference{{view:Array<Pair>,tag:u16}}
#[derive(Copy,Clone)]struct ReferenceWord{{view:Array<Pair>}}
impl Pair{{
 fn new(left:u16)->Self{{Self{{left:left,right:left+1}}}}
 fn total(&self,other:Pair)->u16{{self.left+self.right+other.left+other.right}}
 fn pointer(&self)->Ptr{{addr_of(&self.left)}}
}}
fn bump(value:Pair,amount:u16)->Pair{{Pair{{left:value.left+amount,right:value.right}}}}
fn alias(mut value:Pair)->Array<Pair>{{view_of(&mut value)}}
fn wrap(mut value:Pair)->Reference{{Reference{{view:view_of(&mut value),tag:3}}}}
fn word_alias(mut value:Pair)->ReferenceWord{{ReferenceWord{{view:view_of(&mut value)}}}}
fn choose(flag:bool,value:Pair)->Pair{{if flag{{return bump(value,7);}}bump(value,1)}}
fn main(){{
 let escaped=alias(Pair::new(10));let wrapped=wrap(Pair::new(20));
 let pointer=Pair::new(30).pointer();let word=word_alias(Pair::new(40));
 {calls}
 let mut value=Pair::new(1);let mut index=0u16;
 while index<3{{
  if index==1{{value=bump(value,2);}}else{{value=bump(value,1);}}
  index+=1;
 }}
 let yes=choose(true,value);let no=choose(false,value);
 if yes.left!=12||no.left!=6||yes.right!=2{{halt(0xbad);}}
 if escaped[0u16].left!=10||escaped[0u16].right!=11{{halt(0xbad);}}
 if wrapped.view[0u16].left!=20||wrapped.view[0u16].right!=21||wrapped.tag!=3{{halt(0xbad);}}
 if unsafe{{pointer.read(0)}}!=30||word.view[0u16].left!=40{{halt(0xbad);}}
 halt(0x600d);
}}
"#
        ),
    );
    let overflow = r#"
struct Wide{values:Buf<u16,100>}
fn wide()->Wide{Wide{values:Buf::new([0u16;100])}}
fn read(a:Wide,b:Wide,c:Wide)->u16{a.values[0u16]+b.values[0u16]+c.values[0u16]}
fn main(){halt(read(wide(),wide(),wide()));}
"#;
    let error = parse("temporary-real-overflow", overflow)
        .err()
        .expect("simultaneously live storage cannot exceed 255 words")
        .to_string();
    assert!(
        error.contains("aggregate argument storage exceeds the 255-word"),
        "{error}"
    );
}

#[test]
fn rustc_owned_struct_resources_borrow_fields_and_drop_in_declaration_order() {
    execute(
        "owned-struct-basic",
        r#"mod owner;
pub struct Scene{first:Renderer,second:Renderer,count:u16}
impl Scene{
 pub fn new()->Self{Self{first:open(1),second:open(2),count:0}}
 pub fn update(&mut self){self.count=self.count+1;let a=self.first.identity();assert!(a==1);}
 pub fn sum(&self)->u16{self.first.identity()+self.second.identity()+self.count}
}
fn main(){
 {let mut scene=Scene::new();scene.update();assert!(scene.sum()==4);}
 assert!(event_count()==2);assert!(event_at(0)==11);assert!(event_at(1)==12);halt(0x600d);
}"#,
    );
}

#[test]
fn rustc_owned_struct_rejects_moves_and_aliased_field_borrows() {
    for (label, body, expected) in [
        (
            "owned-struct-alias",
            "let mut s=Scene{first:open(1),second:open(2)};pair(&mut s.first,&mut s.first);",
            "E0499",
        ),
        (
            "owned-struct-move",
            "let s=Scene{first:open(1),second:open(2)};drop(s.first);drop(s);",
            "E0382",
        ),
    ] {
        let source = format!(
            "mod owner;pub struct Scene{{first:Renderer,second:Renderer}}fn main(){{{body}}}"
        );
        let error = parse(label, &source).err().unwrap().to_string();
        assert!(error.contains(expected), "{label}: {error}");
    }
}

#[test]
fn rustc_owned_struct_nested_moves_replacement_and_early_return_cleanup() {
    execute(
        "owned-struct-nested",
        r#"mod owner;
pub struct Scene{first:Renderer,second:Renderer,count:u16}
pub struct Page{scene:Scene,last:Renderer}
impl Scene{
 pub fn new(a:u16,b:u16)->Self{Self{first:open(a),second:open(b),count:0}}
 pub fn replace(&mut self){self.first=open(4);self.count+=1;}
 pub fn identity(&self)->u16{self.first.identity()+self.count}
}
fn transfer(page:Page)->Page{page}
fn work(){
 let mut page=Page{scene:Scene::new(1,2),last:open(3)};
 page.scene.replace();assert!(page.scene.identity()==5);
 let moved=transfer(page);drop(moved.scene.first);return;
}
fn main(){work();assert!(event_count()==4);assert!(event_at(0)==11);
 assert!(event_at(1)==14);assert!(event_at(2)==12);assert!(event_at(3)==13);halt(0x600d);}
"#,
    );
}

#[test]
fn rustc_owned_struct_rejects_private_payload_access_and_child_storage() {
    for (label, source, expected) in [
        ("owned-struct-private", "mod owner;pub struct Scene{r:Renderer}fn main(){let s=Scene{r:open(1)};halt(s.r.word);}", "private"),
        ("owned-struct-child", "mod owner;pub struct Scene{f:Frame}fn main(){halt(0x600d);}", "lifetime"),
    ] {
        let error=parse(label,source).err().unwrap().to_string();
        assert!(error.contains(expected),"{label}: {error}");
    }
}

#[test]
fn rustc_owned_struct_reborrows_and_loop_replacements_keep_storage_and_cleanup() {
    execute(
        "owned-struct-loop-replace",
        r#"mod owner;
pub struct Scene{first:Renderer,second:Renderer}
impl Scene{pub fn new(a:u16,b:u16)->Self{Self{first:open(a),second:open(b)}}}
fn main(){
 {let mut scene=Scene::new(1,2);scene=Scene::new(3,4);
  {let view=&mut scene;view.first=open(5);assert!(view.first.identity()==5);}
  let mut i:u16=0;while i<2{scene.first=open(6+i);i+=1;}
 }
 assert!(event_count()==7);assert!(event_at(0)==11);assert!(event_at(1)==12);
 assert!(event_at(2)==13);assert!(event_at(3)==15);assert!(event_at(4)==16);
 assert!(event_at(5)==17);assert!(event_at(6)==14);halt(0x600d);
}"#,
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

#[test]
fn rustc_graphics_shared_resource_calls_reborrow_without_cleanup_or_copy() {
    execute(
        "graphics-shared",
        r#"mod owner;
fn main(){let mut r=open(3);
 if shared_pair(&r,&r)!=6||shared_forward(&mut r)!=6||r.identity()!=3{halt(0xbad);}
 if event_count()!=0{halt(0xbad);}drop(r);
 if event_count()!=1||event_at(0)!=13{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_enum_repr_and_copy_markers_execute_on_cpu_v3() {
    execute(
        "graphics-enum",
        r#"mod owner;
#[repr(u16)] #[derive(Copy,Clone,PartialEq,Eq)]
enum Shininess{S0,S1,S2,S3,S4,S5,S6,S7,S8,S9,S10,S11,S12,S13,S14,S15,S16}
fn main(){let a=Shininess::S16;let b=a;
 if a!=b||b as u16!=16||Shininess::S0 as u16!=0{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_checked_enum_requires_u16_repr() {
    for (label, attribute) in [("missing", ""), ("wrong", "#[repr(u8)]")] {
        let source=format!("{attribute} #[derive(Copy,Clone,PartialEq,Eq)] enum E{{A,B}} fn main(){{halt(E::B as u16);}}");
        let error = parse(&format!("graphics-repr-{label}"), &source)
            .err()
            .expect("invalid enum representation must be rejected")
            .to_string();
        assert!(error.contains("repr(u16)"), "{error}");
    }
}

#[test]
fn rustc_graphics_local_shared_aliases_and_selection_execute() {
    execute(
        "graphics-local-shared",
        r#"mod owner;
fn selected(flag:bool,a:&Renderer,b:&Renderer)->u16{
 let value=if flag{a}else{b};value.identity()
}
fn main(){let a=open(3);let b=open(5);let first:&Renderer=&a;let forwarded=first;
 if shared_pair(first,forwarded)!=6||selected(true,&a,&b)!=3||selected(false,&a,&b)!=5{halt(0xbad);}
 drop(a);drop(b);if event_count()!=2||event_at(0)!=13||event_at(1)!=15{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_shared_borrows_reject_conflicts_and_owner_extraction() {
    for (label, body, code) in [
        ("shared-mut", "fn conflict(a:&Renderer,b:&mut Renderer){} fn main(){let mut r=open(1);conflict(&r,&mut r);}", "E0502"),
        ("shared-move", "fn main(){let r=open(1);let shared=&r;r.finish();let value=shared.identity();}", "E0505"),
        ("shared-extract", "fn bad(r:&Renderer){r.finish();} fn main(){}", "E0507"),
        ("shared-write", "fn bad(r:&Renderer){let f=r.frame();} fn main(){}", "E0596"),
    ] {
        let error=parse(&format!("graphics-{label}"), &format!("mod owner;{body}"))
            .err().expect("invalid shared resource borrowing must be rejected").to_string();
        assert!(error.contains(&format!("rustc {code}")), "{label}: {error}");
    }
}

#[test]
fn rustc_graphics_aggregate_parameters_snapshot_before_later_argument_effects() {
    execute(
        "graphics-value-parameters",
        r#"mod owner;
struct Pair{x:u16,y:u16}
static DATA:Pair=Pair{x:7,y:11};
fn overwrite()->u16{unsafe{addr_of(&DATA).write(0,77);}0}
fn observed(value:Pair,ignored:u16)->u16{value.x+value.y}
fn changed(mut value:Pair)->u16{value.x=99;value.x+value.y}
fn main(){let original:Pair=Pair{x:3,y:5};
 if changed(original)!=104||original.x!=3||original.y!=5{halt(0xbad);}
 if observed(DATA,overwrite())!=18||DATA.x!=77{halt(0xbad);}
 if changed(Pair{x:1,y:2})!=101{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_value_constructors_methods_and_mutable_receivers_execute() {
    execute(
        "graphics-value-methods",
        r#"mod owner;
#[derive(Copy,Clone)]
struct Pair{x:u16,y:u16}
impl Pair{
 fn new(x:u16,y:u16)->Self{Self{x:x,y:y}}
 fn sum(&self)->u16{self.x+self.y}
 fn update(&mut self,x:u16){self.x=x;}
 fn changed(mut self,x:u16)->Self{self.x=x;self}
}


fn main(){let mut original=Pair::new(3,5);
 if original.sum()!=8{halt(0xbad);}original.update(7);
 let changed=original.changed(11);
 if original.sum()!=12||changed.sum()!=16{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_value_methods_preserve_mutability_and_private_entries() {
    for (label, body, expected) in [
        ("immutable", "struct P{x:u16} impl P{fn update(&mut self){self.x=1;}} fn main(){let p=P{x:0};p.update();}", "rustc E0596"),
        ("borrowed", "struct P{x:u16} impl P{fn update(&mut self){self.x=1;}} fn main(){let mut p=P{x:0};let r=&p;p.update();let x=r.x;}", "rustc E0502"),
        ("generic", "struct P{x:u16} impl P{fn id<T>(&self,v:T)->T{v}} fn main(){}", "generic/default value methods"),
        ("entry", "fn __rcc_value_view()->u16{0} fn main(){halt(__rcc_value_view());}", "reserved value method name"),
    ] {
        let error=parse(&format!("graphics-method-{label}"), body).err()
            .expect("invalid value method source must be rejected").to_string();
        assert!(error.contains(expected), "{label}: {error}");
    }
}

#[test]
fn rustc_graphics_u8_arithmetic_casts_fields_and_byte_constants_execute() {
    execute(
        "graphics-u8",
        r#"mod owner;
const GLYPH:u8=b'A';
#[derive(Copy,Clone)] struct Color{r:u8,g:u8}
fn add(a:u8,b:u8)->u8{let mut result=a;result+=b;result}
fn multiply(a:u8,b:u8)->u8{a*b}
fn narrow(a:u16)->u8{a as u8}
fn shift(a:u8)->u8{a<<1u16}
fn main(){let mut color:Color=Color{r:GLYPH,g:255u8};color.g=add(color.g,3u8);
 if color.r as u16!=65||color.g as u16!=2||add(250u8,10u8)!=4u8{halt(0xbad);}
 if multiply(20u8,20u8)!=144u8||narrow(511u16)!=255u8||shift(128u8)!=0u8{halt(0xbad);}
 if !narrow(511u16)!=0u8||(128u8>>7u16)!=1u8{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_packed_byte_slices_preserve_byte_lengths_and_odd_tail() {
    execute(
        "graphics-byte-slices",
        r#"mod owner;
const ODD:&[u8]=b"ABCDE";
const EVEN:[u8;4]=[1u8,128u8,3u8,255u8];
const REPEAT:[u8;3]=[9u8;3];
fn sum(data:&[u8])->u16{let mut total:u16=0;let mut i:u16=0;
 while i<data.len() as u16{total+=data[i as usize] as u16;i+=1;}total}
fn forward(data:&[u8])->u16{sum(data)}
fn main(){let empty=b"";let local=b"\x01\x80\xff";
 if sum(ODD)!=335{halt(0xba01);}if sum(&EVEN)!=387{halt(0xba02);}if forward(&REPEAT)!=27{halt(0xba03);}
 if sum(local)!=384{halt(0xba04);}if sum(b"ab")!=195{halt(0xba05);}if empty.len() as u16!=0{halt(0xba06);}
 if ODD[4] as u16!=69{halt(0xba07);}if EVEN[3] as u16!=255{halt(0xba08);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_byte_constant_evaluation_preserves_narrow_intermediates() {
    execute(
        "graphics-byte-constant-width",
        r#"mod owner;
const SHIFT:u8=128<<1;
const CAST:u16=(!128u8) as u16;
const WIDE:u16=0x1234u16>>1u8;
const DATA:[u8;3]=[(128<<1)/2,(!128u8),255u16 as u8];
fn main(){if SHIFT as u16!=0||CAST!=127||WIDE!=0x091a{halt(0xbad);}
 if DATA[0] as u16!=0||DATA[1] as u16!=127||DATA[2] as u16!=255{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_byte_slices_reject_mutable_storage_and_general_references() {
    for (label, source) in [
        ("mutable", "fn bad(data:&mut [u8]){data[0]=1;} fn main(){}"),
        (
            "subslice",
            "fn bad(data:&[u8])->u16{let tail=&data[1..];tail.len() as u16} fn main(){}",
        ),
        (
            "element-ref",
            "fn bad(data:&[u8])->u8{let reference=&data[0];*reference} fn main(){}",
        ),
        ("mixed", "fn bad(value:u8)->u16{value} fn main(){}"),
    ] {
        assert!(
            parse(&format!("graphics-bytes-{label}"), source).is_err(),
            "{label} accepted"
        );
    }
}

#[test]
fn rustc_graphics_u8_ui_updates_use_word_read_modify_write() {
    execute(
        "graphics-byte-word-rmw",
        r#"mod owner;
static WORDS:Buf<u16,2>=Buf::new([0x1234,0x5678]);
fn write_byte(destination:Ptr,index:u16,glyph:u8){let word_index=index>>1;
 let shift=(index&1)<<3;let old=unsafe{destination.read(word_index as i16)};
 let mask=0xffu16<<shift;let next=(old&!mask)|((glyph as u16)<<shift);
 unsafe{destination.write(word_index as i16,next);}}
fn main(){write_byte(WORDS.as_ptr(),1,0xccu8);write_byte(WORDS.as_ptr(),2,0xaau8);
 if WORDS.as_array()[0u16]!=0xcc34||WORDS.as_array()[1u16]!=0x56aa{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_owner_reassignment_and_loop_joins_drop_exactly_once() {
    execute(
        "graphics-owner-reinitialization",
        r#"mod owner;
fn identity(value:Renderer)->Renderer{value}
fn repeat(mut value:Renderer)->Renderer{let mut i:u16=0;
 while i<3{if (i&1)==0{value=identity(value);}else{value=identity(value);}i+=1;}
 value}
fn main(){let mut value=open(1);value=repeat(value);
 if event_count()!=0{halt(0xbad);}value=open(2);
 if event_count()!=1||event_at(0)!=11{halt(0xbad);}
 let mut i:u16=0;loop{value=identity(value);i+=1;if i<2{continue;}break;}
 value.finish();value=open(3);drop(value);
 if event_count()!=3||event_at(1)!=72||event_at(2)!=13{halt(0xbad);}halt(0x600d);}"#,
    );
}

const GRAPHICS_LIBRARY: &str = r#"
#[derive(Copy,Clone)] pub struct CommandMemory{pub base:u16,pub words:u16}
#[rcc_resource] pub struct CommandBuffer{word:u16}
#[rcc_resource(parent=CommandBuffer)] pub struct RecordingPass{word:u16}
#[rcc_resource] pub struct Gpu{word:u16}
static ACTIVE_WORD:u16=0;static ACTIVE_VALID:u16=0;
static FIRST_FAULT:u16=0;static FAULT_WORD:u16=0;static FAULT_VALID:u16=0;
static EVENTS:Buf<u16,64>=Buf::new([0;64]);static COUNT:u16=0;
fn event(code:u16){EVENTS.as_array()[COUNT]=code;unsafe{addr_of(&COUNT).write(0,COUNT+1);}}
impl Drop for CommandBuffer{fn drop(&mut self){event(100+self.word);}}
impl Drop for RecordingPass{fn drop(&mut self){event(200+self.word);}}
impl Drop for Gpu{fn drop(&mut self){if ACTIVE_VALID!=0{gpu_panic(0x1002);}event(300+self.word);}}
pub fn gpu_open()->Gpu{Gpu{word:0}}
fn uart_emit(byte:u8){while (dev_recv(0,3)&1)!=0{}dev_send(0,3,byte as u16);}
fn uart_hex4(word:u16){let mut count:u16=4;while count!=0{count-=1;let nibble=(word>>(count<<2))&15;
 if nibble<10{uart_emit((48+nibble) as u8);}else{uart_emit((55+nibble) as u8);}}}
fn remember_fault(code:u16){if FIRST_FAULT==0{unsafe{addr_of(&FIRST_FAULT).write(0,code);
 addr_of(&FAULT_WORD).write(0,ACTIVE_WORD);addr_of(&FAULT_VALID).write(0,ACTIVE_VALID);}}}
fn gpu_panic(code:u16)->!{remember_fault(code);uart_emit(b'F');uart_hex4(FIRST_FAULT);
 uart_emit(b'@');uart_hex4(FAULT_WORD);uart_emit(b'/');uart_emit((48+FAULT_VALID) as u8);
 uart_emit(b'#');uart_hex4(COUNT);uart_emit(10u8);
 while (dev_recv(0,3)&1)!=0{}halt(FIRST_FAULT);}
pub fn fixture_remember_first_fault(code:u16){remember_fault(code);}
impl CommandBuffer{
 pub fn new(word:u16)->Self{Self{word:word}}
 pub fn explicit(word:u16)->CommandBuffer{CommandBuffer{word:word}}
 fn secret_new(word:u16)->Self{Self{word:word}}
 pub fn from_held(word:u16)->Self{let cb=Self::new(word);let held=Some(cb);
  if let Some(value)=held{return value;}halt(0xbad);}
 pub fn keep(owner:Self)->Self{let held=Some(owner);
  if let Some(value)=held{return value;}halt(0xbad);}
 pub fn identity(&self)->u16{self.word}
 pub fn pass(&mut self)->RecordingPass{RecordingPass{word:self.word}}
 pub fn reset(&mut self){}
}
impl Gpu{
 pub fn submit(&mut self,cb:CommandBuffer){if ACTIVE_VALID!=0{gpu_panic(0x1001);}
  unsafe{addr_of(&ACTIVE_WORD).write(0,cb.word);addr_of(&ACTIVE_VALID).write(0,1);}
  dev_send(4,0,cb.word);resource_disarm(cb);
 }
 pub fn try_wait(&mut self)->Option<CommandBuffer>{let status=dev_recv(4,0);
  if (status&0x8000)!=0{gpu_panic(0x2000|(status&255));}
  if ACTIVE_VALID==0||(status&1)!=0{return None;}
  let word=ACTIVE_WORD;unsafe{addr_of(&ACTIVE_VALID).write(0,0);}
  let cb=CommandBuffer{word:word};Some(cb)
 }
 pub fn wait(&mut self)->CommandBuffer{loop{let done=self.try_wait();if let Some(cb)=done{return cb;}}}
}

pub fn cb_open(word:u16)->CommandBuffer{CommandBuffer{word:word}}
pub fn command_memory(base:u16,words:u16)->CommandMemory{CommandMemory{base:base,words:words}}
pub fn graphics_count()->u16{COUNT}
pub fn graphics_event(index:u16)->u16{EVENTS.as_array()[index]}
"#;

#[test]
fn rustc_graphics_tagged_options_preserve_zero_owner_and_conditional_drop() {
    execute(
        "graphics-tagged-options",
        r#"mod graphics;
fn optional(flag:bool)->Option<CommandBuffer>{if !flag{return None;}let cb=cb_open(0);Some(cb)}
fn main(){ {let held=optional(true);} {let empty=optional(false);}
 if graphics_count()!=1||graphics_event(0)!=100{halt(0xbad);}
 let done=optional(true);if let Some(cb)=done{drop(cb);}else{halt(0xbad);}
 let empty=optional(false);match empty{Some(cb)=>{drop(cb);halt(0xbad);},None=>{}}
 if graphics_count()!=2||graphics_event(1)!=100{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_copy_options_keep_plain_memory_values_isolated() {
    execute(
        "graphics-copy-options",
        r#"mod graphics;
fn missing()->Option<CommandMemory>{None}
fn main(){let memory=command_memory(7,9);let optional:Option<CommandMemory>=Some(memory);let copied=optional;
 if let Some(value)=copied{if value.base!=7||value.words!=9{halt(0xbad);}}else{halt(0xbad);}
 if let Some(value)=optional{if value.base!=7{halt(0xbad);}}else{halt(0xbad);}
 let empty=missing();if let Some(value)=empty{halt(0xbad);}
 if memory.words!=9||graphics_count()!=0{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_affine_pairs_transfer_swap_and_drop_in_order() {
    execute(
        "graphics-affine-pair",
        r#"mod graphics;
fn create()->(CommandBuffer,CommandBuffer){let a=cb_open(1);let b=cb_open(2);(a,b)}
fn swapped(a:CommandBuffer,b:CommandBuffer)->(CommandBuffer,CommandBuffer){(b,a)}
fn main(){let(mut a,mut b)=create();let mut i:u16=0;
 while i<3{(a,b)=swapped(a,b);i+=1;}
 if graphics_count()!=0{halt(0xbad);}drop(a);drop(b);
 if graphics_count()!=2||graphics_event(0)!=102||graphics_event(1)!=101{halt(0xbad);}
 {let pair=create();}
 if graphics_count()!=4||graphics_event(2)!=101||graphics_event(3)!=102{halt(0xbad);}halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_option_owner_reinitialization_preserves_each_lifetime() {
    execute(
        "graphics-option-reinitialization",
        r#"mod graphics;
fn same(value:CommandBuffer)->CommandBuffer{value}
fn main(){let first=cb_open(0);let mut pending:Option<CommandBuffer>=Some(first);
 let mut i:u16=0;while i<3{if let Some(cb)=pending{let cb2=same(cb);pending=Some(cb2);}else{halt(0xbad);}i+=1;}
 if graphics_count()!=0{halt(0xbad);}pending=None;
 if graphics_count()!=1||graphics_event(0)!=100{halt(0xbad);}drop(pending);halt(0x600d);}"#,
    );
}

#[test]
fn rustc_graphics_optional_and_pair_owners_reject_copies_and_private_bridges() {
    for(label,source,expected)in [
        ("option-copy","mod graphics;fn main(){let cb=cb_open(0);let value=Some(cb);let copied=value;drop(value);}","rustc E0382"),
        ("pair-copy","mod graphics;fn pair()->(CommandBuffer,CommandBuffer){let a=cb_open(1);let b=cb_open(2);(a,b)}fn main(){let value=pair();let copied=value;drop(value);}","rustc E0382"),
        ("private","fn __rcc_container_option_cb_take()->u16{0}fn main(){halt(0);}","reserved ownership container name"),
        ("other-option","fn value()->Option<u16>{Some(1)}fn main(){halt(0);}","only Option<CommandMemory> and Option<CommandBuffer>"),
    ]{
        let error=parse(&format!("graphics-negative-{label}"),source).err().expect("invalid ownership source must be rejected").to_string();
        assert!(error.contains(expected),"{label}: {error}");
    }
}

#[derive(Default)]
struct HandoffGpu {
    submitted: Vec<u16>,
    busy_reads: usize,
    fault: bool,
}
impl crate::Device for HandoffGpu {
    fn read(&mut self, _memory: &mut [u16], channel: u8) -> u16 {
        assert_eq!(channel, 0);
        let busy = self.busy_reads != 0;
        if self.fault {
            return 0x8000 | u16::from(busy);
        }
        if busy {
            self.busy_reads -= 1;
        }
        u16::from(busy)
    }
    fn write(&mut self, _memory: &mut [u16], channel: u8, value: u16) {
        assert_eq!(channel, 0);
        assert_eq!(self.busy_reads, 0);
        self.submitted.push(value);
        self.busy_reads = 3;
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
#[derive(Default)]
struct HandoffUart {
    bytes: Vec<u8>,
    bits_left: usize,
    completed: usize,
    polls: usize,
}
impl crate::Device for HandoffUart {
    fn read(&mut self, _memory: &mut [u16], channel: u8) -> u16 {
        assert_eq!(channel, 3);
        self.polls += 1;
        let busy = self.bits_left != 0;
        if busy {
            self.bits_left -= 1;
            if self.bits_left == 0 {
                self.completed += 1;
            }
        }
        u16::from(busy)
    }
    fn write(&mut self, _memory: &mut [u16], channel: u8, value: u16) {
        assert_eq!(channel, 3);
        assert_eq!(self.bits_left, 0, "UART overwritten before stop bit");
        self.bytes.push(value as u8);
        self.bits_left = 10;
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
fn handoff_execution(label: &str, source: &str, fault: bool) -> (CpuV3Sim, u16) {
    let code = try_compile(
        parse(label, source).unwrap_or_else(|e| panic!("{label}: {e}")),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    cpu.attach_device(
        4,
        Box::new(HandoffGpu {
            fault,
            ..Default::default()
        }),
    );
    cpu.attach_device(0, Box::<HandoffUart>::default());
    let outcome = cpu.run(100_000).unwrap();
    let RunOutcome::Halted { signal, .. } = outcome else {
        panic!("{label}: {outcome:?}")
    };
    (cpu, signal)
}
#[test]
fn rustc_graphics_single_active_driver_returns_the_same_cb_and_overlaps_recording() {
    let (cpu, signal) = handoff_execution(
        "graphics-handoff",
        r#"mod graphics;
fn main(){let mut gpu=gpu_open();let mut a=cb_open(0);let mut b=cb_open(1);
 gpu.submit(a);{let pass=b.pass();}
 let pending=gpu.try_wait();if let Some(cb)=pending{drop(cb);halt(0xbad);}
 a=gpu.wait();if a.identity()!=0{halt(0xbad);}(a,b)=(b,a);
 gpu.submit(a);{let pass=b.pass();}a=gpu.wait();if a.identity()!=1{halt(0xbad);}
 drop(b);drop(a);drop(gpu);
 if graphics_count()!=5||graphics_event(0)!=201||graphics_event(1)!=200||graphics_event(2)!=100||graphics_event(3)!=101||graphics_event(4)!=300{halt(0xbad);}halt(0x600d);}"#,
        false,
    );
    assert_eq!(signal, 0x600d);
    assert_eq!(cpu.device::<HandoffGpu>(4).unwrap().submitted, [0, 1]);
    assert!(cpu.device::<HandoffUart>(0).unwrap().bytes.is_empty());
}
#[test]
fn rustc_graphics_busy_submit_and_device_fault_stop_without_reclaiming_active_data() {
    for (label, source, fault, signal, bytes) in [
        (
            "busy-submit",
            r#"mod graphics;fn main(){let mut gpu=gpu_open();let a=cb_open(0);let b=cb_open(1);gpu.submit(a);gpu.submit(b);halt(0xbad);}"#,
            false,
            0x1001,
            b"F1001@0000/1#0000\n".as_slice(),
        ),
        (
            "device-fault",
            r#"mod graphics;fn main(){let mut gpu=gpu_open();let a=cb_open(0);gpu.submit(a);let done=gpu.try_wait();halt(0xbad);}"#,
            true,
            0x2001,
            b"F2001@0000/1#0000\n".as_slice(),
        ),
        (
            "first-fault",
            r#"mod graphics;fn main(){let mut gpu=gpu_open();let a=cb_open(0);gpu.submit(a);fixture_remember_first_fault(0x3210);let done=gpu.wait();halt(0xbad);}"#,
            true,
            0x3210,
            b"F3210@0000/1#0000\n".as_slice(),
        ),
    ] {
        let (cpu, actual) = handoff_execution(&format!("graphics-fault-{label}"), source, fault);
        assert_eq!(actual, signal, "{label}");
        let uart = cpu.device::<HandoffUart>(0).unwrap();
        assert_eq!(uart.bytes, bytes, "{label}");
        assert_eq!(uart.bits_left, 0);
        assert_eq!(uart.completed, bytes.len());
        assert!(uart.polls >= bytes.len() * 10);
        let gpu = cpu.device::<HandoffGpu>(4).unwrap();
        assert_eq!(gpu.submitted, [0]);
        assert_ne!(
            gpu.busy_reads, 0,
            "must stop without waiting for GPU completion"
        );
    }
}
#[test]
fn rustc_graphics_active_pass_blocks_submit_and_reset() {
    for (label, source) in [
        (
            "submit",
            r#"mod graphics;fn main(){let mut gpu=gpu_open();let mut cb=cb_open(0);let pass=cb.pass();gpu.submit(cb);drop(pass);}"#,
        ),
        (
            "reset",
            r#"mod graphics;fn main(){let mut cb=cb_open(0);let pass=cb.pass();cb.reset();drop(pass);}"#,
        ),
    ] {
        let error = parse(&format!("graphics-pass-{label}"), source)
            .err()
            .expect("active Pass must lock CB")
            .to_string();
        assert!(
            error.contains("rustc E0505") || error.contains("rustc E0499"),
            "{label}: {error}"
        );
    }
}

#[test]
fn rustc_graphics_bounded_for_ranges_preserve_reinitialized_owners() {
    execute(
        "graphics-owner-for",
        r#"mod graphics;
fn same(cb:CommandBuffer)->CommandBuffer{cb}
fn main(){let mut cb=cb_open(7);for index in 0u16..3u16{cb=same(cb);}
 for index in 1u16..=2u16{cb=same(cb);}if graphics_count()!=0{halt(0xbad);}
 drop(cb);if graphics_count()!=1||graphics_event(0)!=107{halt(0xbad);}halt(0x600d);}"#,
    );
}

pub(crate) fn graphics_prerequisites_program(gpu_debug: bool) -> Vec<u16> {
    let source = r#"mod graphics;
#[repr(u16)]#[derive(Copy,Clone,PartialEq,Eq)]enum Mode{A,B}
const BYTES:&[u8]=b"\x01\x80\xffxZ";
fn changed(mut value:CommandMemory)->u16{value.base=99;value.base+value.words}
fn shifted(value:u8,count:u16)->u8{value<<count}
fn sum(data:&[u8])->u16{let mut i:u16=0;let mut total:u16=0;while i<data.len() as u16{total+=data[i as usize] as u16;i+=1;}total}
fn main(){let memory=command_memory(7,9);if changed(memory)!=108||memory.base!=7||sum(BYTES)!=594||shifted(3u8,8u16)!=3u8{halt(0xbad);}
 #[cfg(gpu_debug)]{if BYTES[1u16 as usize]!=128u8{halt(0xbad);}}
 let mode=Mode::B;let copied=mode;if copied!=mode||copied as u16!=1{halt(0xbad);}
 let mut a=cb_open(0);let mut b=cb_open(1);for index in 0u16..2u16{(a,b)=(b,a);}
 let pending:Option<CommandBuffer>=Some(a);if let Some(done)=pending{a=done;}else{halt(0xbad);}
 if a.identity()!=0||b.identity()!=1{halt(0xbad);}drop(a);drop(b);
 if graphics_count()!=2||graphics_event(0)!=100||graphics_event(1)!=101{halt(0xbad);}halt(0x600d);}"#;
    let options = CompilerOptions {
        gpu_debug,
        ..CompilerOptions::default()
    };
    try_compile(
        parse_with_options(
            &format!("graphics-core-cosim-{gpu_debug}"),
            source,
            &options,
        )
        .unwrap_or_else(|e| panic!("{e}")),
        &options,
        "main",
    )
    .unwrap()
    .words
}

#[test]
fn rustc_graphics_u8_shifts_wrap_at_eight_bits_and_evaluate_rhs_once() {
    execute(
        "graphics-byte-shifts",
        r#"mod owner;
static CALLS:u16=0;
struct C{a:u8,b:u8}
fn amount()->u16{unsafe{addr_of(&CALLS).write(0,CALLS+1);}8}
fn shifted(value:u8,count:u16)->u8{value<<count}
fn main(){let mut value:u8=3;value<<=amount();
 let mut color:C=C{a:7,b:2};color.b+=255u8;color.b<<=amount();
 if value!=3u8||CALLS!=2||shifted(5u8,8u16)!=5u8||color.b!=1u8{halt(0xbad);}
 if unsafe{addr_of(&color).read(1)}!=1u16{halt(0xbad);}halt(0x600d);}"#,
    );
}

fn cfg_program(
    label: &str,
    source: &str,
    options: &CompilerOptions,
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    parse_with_options(label, source, options)
}

#[test]
fn rustc_gpu_debug_prunes_calls_arguments_helpers_and_static_storage() {
    let source = r#"mod owner;
static EFFECT:u16=0;
#[cfg(gpu_debug)]static DEBUG_TABLE:Buf<u16,9>=Buf::new([17;9]);
fn argument()->u16{unsafe{addr_of(&EFFECT).write(0,EFFECT+1);}7}
#[cfg(gpu_debug)]fn probe(value:u16){DEBUG_TABLE.as_array()[0u16]=value;}
#[cfg(gpu_debug)]fn active()->u16{1}
#[cfg(not(gpu_debug))]fn active()->u16{0}
fn main(){
 #[cfg(gpu_debug)]{probe(argument());}
 if EFFECT!=active(){halt(0xbad);}
 #[cfg(gpu_debug)]{if DEBUG_TABLE.as_array()[0u16]!=7{halt(0xbad);}}
 halt(0x600d);}"#;
    let mut sizes = Vec::new();
    for disabled in [false, true] {
        for enabled in [false, true] {
            let mut options = CompilerOptions {
                gpu_debug: enabled,
                ..CompilerOptions::default()
            };
            if disabled {
                options.opt = rcc::Opts::disabled();
            }
            let program = cfg_program(
                &format!("cfg-layout-{disabled}-{enabled}"),
                source,
                &options,
            )
            .unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(program.debug.gpu_debug, enabled);
            assert_eq!(program.funcs.iter().any(|f| f.name == "probe"), enabled);
            sizes.push(program.debug.static_data_end);
            let code = try_compile(program, &options, "main").unwrap();
            assert_eq!(code.gpu_debug, enabled);
            assert_eq!(
                code.debug.functions.iter().any(|f| f.name == "argument"),
                enabled
            );
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&code.words);
            assert!(matches!(
                cpu.run(100_000).unwrap(),
                RunOutcome::Halted { signal: 0x600d, .. }
            ));
        }
    }
    assert_eq!(sizes[1] - sizes[0], 9);
    assert_eq!(sizes[3] - sizes[2], 9);
}
#[test]
fn rustc_gpu_debug_selected_source_preserves_owner_cleanup() {
    let source = r#"mod owner;fn main(){let owner=open(0);
 #[cfg(gpu_debug)]{let moved=owner;drop(moved);}
 #[cfg(not(gpu_debug))]{drop(owner);}
 if event_count()!=1||event_at(0)!=10{halt(0xbad);}halt(0x600d);}"#;
    for enabled in [false, true] {
        let options = CompilerOptions {
            gpu_debug: enabled,
            ..CompilerOptions::default()
        };
        let program = cfg_program(&format!("cfg-owners-{enabled}"), source, &options)
            .unwrap_or_else(|e| panic!("{e}"));
        let code = try_compile(program, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        assert!(matches!(
            cpu.run(100_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
    }
    let bad = r#"mod owner;fn main(){let owner=open(0);#[cfg(gpu_debug)]{let moved=owner;drop(moved);}drop(owner);}"#;
    let enabled = CompilerOptions {
        gpu_debug: true,
        ..CompilerOptions::default()
    };
    assert!(cfg_program("cfg-double-move", bad, &enabled)
        .err()
        .unwrap()
        .to_string()
        .contains("E0382"));
    assert!(cfg_program("cfg-no-double-move", bad, &CompilerOptions::default()).is_ok());
}
#[test]
fn rustc_gpu_debug_rejects_inactive_syntax_and_contract_bypasses() {
    let cases=[
  ("#[cfg(gpu_debug)]fn hidden(){#[cfg(unknown)]{}}fn main(){}","only gpu_debug"),
  ("#[cfg(gpu_debug)]fn hidden(){#[cfg_attr(gpu_debug,allow(dead_code))]{}}fn main(){}","cfg_attr"),
  ("#[cfg(gpu_debug)]#[rcc_resource]struct R{word:u16}fn main(){}","cfg is not allowed"),
  ("#[rcc_resource]struct R{word:u16}#[cfg(gpu_debug)]impl Drop for R{fn drop(&mut self){}}fn main(){}","unsupported resource Drop attribute"),
  ("#[cfg(gpu_debug)]fn hidden(){__rcc_container_forge();}fn main(){}","reserved"),
  ("#[cfg(gpu_debug)]fn hidden(){resource_disarm(0);}fn main(){}","resource_disarm"),
  ("#[cfg(gpu_debug)]mod missing;fn main(){}","cfg is not allowed"),
  ("fn main(){#[cfg(all(gpu_debug))]{} }","only gpu_debug"),
 ("#![cfg(gpu_debug)] fn main(){}","cfg is not allowed"),
 ("#[rcc_resource]struct R{word:u16}impl Drop for R{fn drop(&mut self){#[cfg(gpu_debug)]{}}}fn main(){}","cfg is not allowed"),
 ("#[rcc_resource(cleanup=clean)]struct R{word:u16}#[cfg(gpu_debug)]fn clean(r:R){}fn main(){}","cfg is not allowed"),
 ("#[cfg(gpu_debug)]#[rcc_resource_library(helper)]fn hidden(){}fn main(){}","contracts cannot be conditional"),

 ("#[cfg(test)]fn hidden(){}fn main(){}","only gpu_debug"),
 ("#[cfg(gpu_debug)]fn r#__rcc_value_fake(){}fn main(){}","reserved"),
 ("#[cfg(gpu_debug)]#[allow(__rcc_method_fake)]fn hidden(){}fn main(){}","reserved"),
 ];
    for (i, (source, expected)) in cases.iter().enumerate() {
        for enabled in [false, true] {
            let options = CompilerOptions {
                gpu_debug: enabled,
                ..CompilerOptions::default()
            };
            let error = cfg_program(&format!("cfg-invalid-{i}-{enabled}"), source, &options)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains(expected), "{i}/{enabled}: {error}");
        }
    }
}

#[test]
fn rustc_gpu_debug_loader_and_private_domains_keep_the_same_profile() {
    for enabled in [false, true] {
        let options = CompilerOptions {
            gpu_debug: enabled,
            ..CompilerOptions::default()
        };
        let source = format!(
            "mod piece;fn main(){{if piece_value()!={}{{halt(0xbad);}}halt(0x600d);}}",
            u16::from(enabled)
        );
        let program=compile_program_named_checked("cfg-loader.rs",&source,&options,&self::options(&format!("cfg-loader-{enabled}")),&mut |name|{
            assert_eq!(name,"piece");Ok("#[cfg(gpu_debug)]pub fn piece_value()->u16{1} #[cfg(not(gpu_debug))]pub fn piece_value()->u16{0}".into())
        }).unwrap_or_else(|e|panic!("{e}"));
        let code = try_compile(program, &options, "main").unwrap();
        assert_eq!(code.gpu_debug, enabled);
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        assert!(matches!(
            cpu.run(100_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
        for (index, bad) in [
            "mod owner;fn main(){let forged=Renderer{word:0};drop(forged);}",
            "mod owner;fn main(){event(7);}",
        ]
        .iter()
        .enumerate()
        {
            let error = cfg_program(&format!("cfg-private-{enabled}-{index}"), bad, &options)
                .err()
                .unwrap()
                .to_string();
            assert!(
                error.contains("private") || error.contains("E0425"),
                "{error}"
            );
        }
    }
}

const REVIEW_OWNER: &str = r#"
#[rcc_resource]struct CommandBuffer{word:u16}
static EVENTS:Buf<u16,16>=Buf::new([0;16]);static COUNT:u16=0;
fn event(word:u16){EVENTS.as_array()[COUNT]=word;unsafe{addr_of(&COUNT).write(0,COUNT+1);}}
fn open_cb(word:u16)->CommandBuffer{CommandBuffer{word:word}}
fn count()->u16{COUNT}fn at(index:u16)->u16{EVENTS.as_array()[index]}
impl Drop for CommandBuffer{fn drop(&mut self){event(self.word);}}
impl CommandBuffer{fn finish(self){event(256+self.word);resource_disarm(self);}}
"#;
#[test]
fn rustc_review_implicit_container_cleanup_ignores_shadowed_drop() {
    let body = r#"
fn main(){
 {let direct=open_cb(9);}
 {let cb=open_cb(7);let held:Option<CommandBuffer>=Some(cb);}
 {let a=open_cb(3);let b=open_cb(4);let held:(CommandBuffer,CommandBuffer)=(a,b);}
 {let a=open_cb(5);let b=open_cb(6);let (_,kept)=(a,b);}
 let empty:Option<CommandBuffer>=None;
 if let Some(unexpected)=empty{let saved=unexpected;}
 let empty_again:Option<CommandBuffer>=None;
 match empty_again{Some(unexpected)=>{let saved=unexpected;},_=>{}}
 let cb=open_cb(8);let full:Option<CommandBuffer>=Some(cb);
 if let Some(done)=full{let transferred=done;}else{halt(0xbad);}
 if count()!=7||at(0)!=9||at(1)!=7||at(2)!=3||at(3)!=4||at(4)!=5||at(5)!=6||at(6)!=8{halt(0xbad);}
 EXPLICIT
 halt(0x600d);
}"#;
    for (label,definition,explicit) in [
        ("ordinary","","let explicit_cb=open_cb(10);drop(explicit_cb);if count()!=8||at(7)!=10{halt(0xbad);}"),
        ("word","static EXPLICIT_WORD:u16=0;fn drop(word:u16){unsafe{addr_of(&EXPLICIT_WORD).write(0,word);}}","drop(11u16);if EXPLICIT_WORD!=11||count()!=7{halt(0xbad);}"),
        ("owner","fn drop(cb:CommandBuffer){cb.finish();}","let explicit_cb=open_cb(10);drop(explicit_cb);if count()!=8||at(7)!=266{halt(0xbad);}"),
    ]{
        let source=format!("{REVIEW_OWNER}\n{definition}\n{}",body.replace("EXPLICIT",explicit));
        execute(&format!("review-drop-{label}"),&source);
    }
}
#[test]
fn rustc_review_option_constructors_do_not_capture_ordinary_names() {
    let source = format!(
        r#"{REVIEW_OWNER}
#[repr(u16)]#[derive(Copy,Clone,PartialEq,Eq)]enum Choice{{None,Some,Other}}
const None:u16=11;
fn Some(word:u16)->u16{{word+2}}
fn empty()->Option<CommandBuffer>{{Option::None}}
fn main(){{
 let value:Choice=Choice::None;if value==Choice::Other||value as u16!=0{{halt(0xbad);}}
 if Choice::Some as u16!=1||Some(4u16)!=6||None!=11{{halt(0xbad);}}
 let pending=empty();if let Option::Some(unexpected)=pending{{let saved=unexpected;}}
 let cb=open_cb(0);let ready:Option<CommandBuffer>=Option::Some(cb);
 if let Option::Some(done)=ready{{let saved=done;}}else{{halt(0xbad);}}
 if count()!=1||at(0)!=0{{halt(0xbad);}}halt(0x600d);
}}"#
    );
    execute("review-option-names", &source);
    execute(
        "review-qualified-enum-none",
        r#"#[repr(u16)]#[derive(Copy,Clone,PartialEq,Eq)]enum Choice{None,Other}
fn main(){let value:Choice=Choice::None;if value==Choice::Other{halt(0xbad);}halt(0x600d);}"#,
    );
}
#[test]
fn rustc_review_raw_cfg_is_rejected_before_host_selection() {
    let cases=[
        "static COUNT:u16=0;fn check(){unsafe{addr_of(&COUNT).write(0,1);}}fn main(){#[r#cfg(gpu_debug)]{check();}halt(COUNT);}",
        "#[r#cfg(gpu_debug)]fn check(){}fn main(){}",
        "#[r#cfg(gpu_debug)]static TABLE:u16=1;fn main(){}",
        "fn main(){#[r#cfg(gpu_debug)]let value:u16=1;}",
        "fn main(){#[r#cfg_attr(gpu_debug,allow(dead_code))]{}}",
        "#[r#cfg_attr(gpu_debug,allow(dead_code))]fn check(){}fn main(){}",
        "#[cfg(gpu_debug)]fn hidden(){#[r#cfg(gpu_debug)]{}}fn main(){}",
        "#![r#cfg(gpu_debug)]fn main(){}",
        "#[r#cfg(gpu_debug)]mod missing;fn main(){}",
    ];
    for (index, source) in cases.iter().enumerate() {
        for enabled in [false, true] {
            let options = CompilerOptions {
                gpu_debug: enabled,
                ..CompilerOptions::default()
            };
            let error = cfg_program(
                &format!("review-raw-cfg-{index}-{enabled}"),
                source,
                &options,
            )
            .err()
            .expect("raw cfg must reject");
            assert!(error.to_string().contains("raw cfg/cfg_attr"), "{error}");
        }
    }
}

#[test]
fn rustc_resource_associated_factory_executes() {
    execute(
        "associated-factory",
        r#"#[rcc_resource]struct Gpu{word:u16}
impl Drop for Gpu{fn drop(&mut self){}}
impl Gpu{fn init()->Gpu{Gpu{word:0}}}
fn main(){let gpu:Gpu=Gpu::init();halt(0x600d);}"#,
    );
}

#[test]
fn rustc_resource_associated_self_factories_preserve_parent_and_drop_order() {
    execute(
        "associated-self-factories",
        r#"mod owner;
fn forward()->Renderer{return Renderer::relay(4);}
fn work(){let mut r:Renderer=Renderer::new(2);
 {let f:Frame=Frame::relay(&mut r);}
 let moved:Renderer=Renderer::transfer(r);drop(moved);}
fn main(){work();{let r=Renderer::relay(3);}{let r=forward();}
 if event_count()!=4||event_at(0)!=22||event_at(1)!=12||event_at(2)!=13||event_at(3)!=14{halt(0xbad);}
 halt(0x600d);}"#,
    );
}

#[test]
fn rustc_resource_associated_factories_retain_affine_and_parent_checks() {
    for (label, source, expected) in [
        ("associated-double-move", "mod owner;fn main(){let r=Renderer::new(1);drop(r);drop(r);}", "E0382"),
        ("associated-live-child", "mod owner;fn main(){let mut r=Renderer::new(1);let f=Frame::from_parent(&mut r);r.identity();drop(f);}", "E0502"),
        ("associated-escaping-child", "mod owner;fn escape()->Frame{let mut r=Renderer::new(1);Frame::from_parent(&mut r)}fn main(){}", "E0515"),
        ("associated-function-pointer", "mod owner;fn main(){let make=Renderer::new;let r=make(1);}", "direct calls"),
        ("associated-private", "mod owner;fn main(){let r=Renderer::hidden();}", "not callable"),
    ] {
        let error = match parse(label, source) {
            Ok(_) => panic!("accepted {label}"),
            Err(e) => e.to_string(),
        };
        assert!(error.contains(expected), "{label}: expected {expected}, got {error}");
    }
}

#[test]
fn rustc_resource_associated_factories_compose_with_inferred_ownership_containers() {
    for (label, body, events) in [
        ("free-some", "let cb=cb_open(7);let held=Some(cb);", "graphics_count()==1&&graphics_event(0)==107"),
        ("associated-some", "let cb=CommandBuffer::new(7);let held=Some(cb);", "graphics_count()==1&&graphics_event(0)==107"),
        ("associated-some-explicit-return", "let cb=CommandBuffer::explicit(7);let held=Some(cb);", "graphics_count()==1&&graphics_event(0)==107"),
        ("associated-some-carrier-annotated", "let cb=CommandBuffer::new(7);let held:Option<CommandBuffer>=Some(cb);", "graphics_count()==1&&graphics_event(0)==107"),
        ("associated-some-owner-annotated", "let cb:CommandBuffer=CommandBuffer::new(7);let held=Some(cb);", "graphics_count()==1&&graphics_event(0)==107"),
        ("free-pair", "let a=cb_open(3);let b=cb_open(4);let held=(a,b);", "graphics_count()==2&&graphics_event(0)==103&&graphics_event(1)==104"),
        ("associated-pair", "let a=CommandBuffer::new(3);let b=CommandBuffer::new(4);let held=(a,b);", "graphics_count()==2&&graphics_event(0)==103&&graphics_event(1)==104"),
        ("associated-pair-annotated", "let a=CommandBuffer::new(3);let b=CommandBuffer::new(4);let held:(CommandBuffer,CommandBuffer)=(a,b);", "graphics_count()==2&&graphics_event(0)==103&&graphics_event(1)==104"),
        ("associated-self-body", "let cb=CommandBuffer::from_held(8);let held=Some(cb);", "graphics_count()==1&&graphics_event(0)==108"),
        ("associated-self-parameter", "let cb=CommandBuffer::new(9);let moved=CommandBuffer::keep(cb);let held=Some(moved);", "graphics_count()==1&&graphics_event(0)==109"),
    ] {
        execute(&format!("factory-container-{label}"), &format!("mod graphics;fn work(){{{body}}}fn main(){{work();if !({events}){{halt(0xbad);}}halt(0x600d);}}"));
    }
}

#[test]
fn rustc_resource_associated_factory_containers_keep_move_and_parent_gates() {
    for (label, body, expected) in [
        (
            "some-double-move",
            "let cb=CommandBuffer::new(7);let held=Some(cb);drop(cb);",
            "E0382",
        ),
        (
            "pair-double-move",
            "let a=CommandBuffer::new(3);let b=CommandBuffer::new(4);let held=(a,b);drop(a);",
            "E0382",
        ),
        (
            "some-live-child",
            "let mut cb=CommandBuffer::new(7);let p=cb.pass();let held=Some(cb);drop(p);",
            "E0505",
        ),
        (
            "some-private-factory",
            "let cb=CommandBuffer::secret_new(7);let held=Some(cb);",
            "not callable",
        ),
    ] {
        let source = format!("mod graphics;fn main(){{{body}}}");
        let error = parse(&format!("factory-container-{label}"), &source)
            .err()
            .expect("ownership violation accepted")
            .to_string();
        assert!(error.contains(expected), "{label}: {error}");
    }
}

#[test]
fn rustc_unsafe_gpu_memory_constructor_executes_on_target() {
    execute(
        "unsafe-gpu-memory",
        r#"struct GpuMemory{first_sector:u16,sector_count:u16}
impl GpuMemory{pub unsafe fn from_sectors(first_sector:u16,sector_count:u16)->Self{
GpuMemory{first_sector:first_sector,sector_count:sector_count}}}
fn main(){let memory:GpuMemory=unsafe{GpuMemory::from_sectors(0x9800,512)};
if memory.first_sector!=0x9800||memory.sector_count!=512{halt(0xbad);}halt(0x600d);}"#,
    );
}

const UNSAFE_MEMORY: &str = r#"struct GpuMemory{first_sector:u16,sector_count:u16}
impl GpuMemory{pub unsafe fn from_sectors(first_sector:u16,sector_count:u16)->Self{
Self{first_sector:first_sector,sector_count:sector_count}}}
unsafe fn unsafe_value(value:u16)->u16{value+1}
"#;

#[test]
fn rustc_unsafe_direct_calls_preserve_existing_destinations_and_ownership() {
    for (label, source) in [
        (
            "aggregate-handoff",
            r#"fn forward()->GpuMemory{return unsafe{GpuMemory::from_sectors(9,10)};}
fn main(){let mut memory=unsafe{GpuMemory::from_sectors(1,2)};
if memory.first_sector!=1||memory.sector_count!=2{halt(0xbad);}
memory=unsafe{GpuMemory::from_sectors(3,4)};
if memory.first_sector!=3||memory.sector_count!=4{halt(0xbad);}
let returned=forward();if returned.first_sector!=9||returned.sector_count!=10{halt(0xbad);}halt(0x600d);}"#,
        ),
        (
            "scalar-unit-raw",
            r#"static TOTAL:u16=0;
unsafe fn set(value:u16){unsafe{addr_of(&TOTAL).write(0,value);}}
fn main(){unsafe{set(3);}let value=unsafe{unsafe_value(TOTAL)};
if value!=4||TOTAL!=3{halt(0xbad);}halt(0x600d);}"#,
        ),
        (
            "once-only-arguments",
            r#"static COUNT:u16=0;
fn next()->u16{unsafe{addr_of(&COUNT).write(0,COUNT+1);}COUNT}
fn main(){let memory=unsafe{GpuMemory::from_sectors(next(),next())};
if memory.first_sector!=1||memory.sector_count!=2||COUNT!=2{halt(0xbad);}halt(0x600d);}"#,
        ),
        (
            "resource-composition",
            r#"mod graphics;
fn work(){let cb=CommandBuffer::new(7);let held=Some(cb);
let memory:GpuMemory=unsafe{GpuMemory::from_sectors(0x9800,512)};
if memory.first_sector!=0x9800||memory.sector_count!=512{halt(0xbad);}}
fn main(){work();if graphics_count()!=1||graphics_event(0)!=107{halt(0xbad);}halt(0x600d);}"#,
        ),
    ] {
        execute(
            &format!("unsafe-call-{label}"),
            &format!("{UNSAFE_MEMORY}{source}"),
        );
    }
}

#[test]
fn rustc_unsafe_calls_retain_original_rustc_gate_and_narrow_target_scope() {
    for (label, source, expected) in [
        ("aggregate-outside", "fn main(){let memory:GpuMemory=GpuMemory::from_sectors(1,2);}", "E0133"),
        ("scalar-outside", "fn main(){let value=unsafe_value(1);}", "E0133"),
        ("safe-wrapper", "fn wrap()->u16{unsafe_value(1)}fn main(){halt(wrap());}", "E0133"),
        ("function-item", "fn main(){let call=unsafe_value;let value=unsafe{call(1)};}", "require direct calls"),
        ("unsafe-fn-pointer", "fn apply(call:unsafe fn(u16)->u16)->u16{unsafe{call(1)}}fn main(){}", "unsafe function pointers"),
        ("unneeded-scalar", "fn safe_value()->u16{7}fn main(){let value=unsafe{safe_value()};}", "unsafe block contains no raw Ptr"),
        ("unneeded-aggregate", "fn safe_memory()->GpuMemory{GpuMemory{first_sector:1,sector_count:2}}fn main(){let memory:GpuMemory=unsafe{safe_memory()};}", "unsafe block contains no raw Ptr"),
        ("aggregate-block", "fn main(){let memory:GpuMemory=unsafe{let value=GpuMemory::from_sectors(1,2);value};}", "single tail call"),
        ("nested", "fn main(){let value=unsafe{unsafe{unsafe_value(1)}};}", "nested unsafe"),
        ("implicit-unsafe-body", "unsafe fn wrap()->u16{unsafe_value(1)}fn main(){let value=unsafe{wrap()};}", "requires an unsafe block"),
    ] {
        let error=parse(&format!("unsafe-reject-{label}"),&format!("{UNSAFE_MEMORY}{source}"))
            .err().expect("unsupported unsafe source accepted").to_string();
        assert!(error.contains(expected), "{label}: expected {expected}, got {error}");
    }
    let source = "unsafe fn value()->u16{7}fn main(){halt(unsafe{value()});}";
    let error = rcc::frontend::compile_program_named(
        "unsafe-unchecked",
        source,
        &CompilerOptions::default(),
        &mut |_| Err("no modules".into()),
    )
    .err()
    .expect("unchecked unsafe declaration accepted")
    .to_string();
    assert!(error.contains("require checked compilation"), "{error}");
}
