//! Nominal enum methods use the existing checked value/view ABI.
use super::{try_compile, CompilerOptions};
use crate::{CpuV3Sim, RunOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::PathBuf;

fn parse(label: &str, source: &str) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    let root = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/rcc-enum-methods")
        });
    compile_program_named_checked(
        label,
        source,
        &CompilerOptions::default(),
        &RustcCheck::new(root.join(label)),
        &mut |name| {
            match name {
            "choices" => {
                Ok("#[repr(u16)] #[derive(Copy,Clone)] pub enum Choice{First,Last}".to_owned())
            }
            "methods" => Ok(
                "use crate::choices::Choice;impl Choice{pub fn last()->Self{Self::Last}pub fn code(&self)->u16{*self as u16}}"
                    .to_owned(),
            ),
            _ => Err(format!("unexpected enum test module {name}")),
        }
        },
    )
}
fn execute(label: &str, source: &str) {
    let ir = parse(label, source).unwrap_or_else(|error| panic!("{label}: {error}"));
    let program = try_compile(ir, &CompilerOptions::default(), "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    assert!(
        matches!(
            cpu.run(100_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ),
        "{label}: target failed"
    );
}

#[test]
fn rustc_enum_associated_and_value_shared_mutable_receivers_execute() {
    execute(
        "enum-receivers",
        r#"
#[repr(u16)]#[derive(Copy,Clone)]enum Mode{Off,On}
impl Mode{
 fn choose(value:u16)->Self{if value==0{Self::Off}else{Self::On}}
 fn code(&self)->u16{*self as u16}
 fn address(&self)->u16{addr_of(&*self).addr()}
 fn owned(self)->u16{self as u16}
 fn set(&mut self,other:Self){*self=other;}
 fn changed(mut self)->Self{self.set(Self::On);self}
 unsafe fn unsafe_on()->Self{Self::On}
 fn six(self,a:Self,b:Self,c:Self,d:Self,e:Self)->Self{e}
}
struct Holder{mode:Mode,tail:u16}
fn parameter(mut mode:Mode)->u16{mode.set(Mode::On);mode.code()}
fn conditional(flag:u16)->u16{let mut mode=Mode::Off;if flag!=0{mode.set(Mode::On);}mode.code()}
fn main(){let mut mode=Mode::choose(0);
 let address=addr_of(&mode).addr();if mode.address()!=address{halt(5);}
 if mode.code()!=0||mode.owned()!=0{halt(1);}mode.set(Mode::On);if mode.code()!=1{halt(2);}
 let changed=Mode::Off.changed();if changed.code()!=1||parameter(Mode::Off)!=1{halt(3);}
 let mut holder:Holder=Holder{mode:Mode::Off,tail:123};holder.mode.set(Mode::On);
 if holder.mode.code()!=1||holder.tail!=123{halt(4);}
 if conditional(0)!=0||conditional(1)!=1{halt(6);}
 let from_unsafe=unsafe{Mode::unsafe_on()};if from_unsafe.code()!=1{halt(7);}
 if Mode::Off.six(Mode::Off,Mode::On,Mode::Off,Mode::On,Mode::On).code()!=1{halt(8);}
 halt(0x600d);}
"#,
    );
}

#[test]
fn rustc_enum_cross_file_methods_keep_single_word_discriminants() {
    let source = r#"mod choices;mod methods;use choices::*;
struct Pair{first:Choice,last:u16}
fn main(){let selected=Choice::last();let pair:Pair=Pair{first:selected,last:55};
 if pair.first.code()!=1||pair.last!=55{halt(1);}halt(0x600d);}"#;
    let ir = parse("enum-cross-file", source).unwrap();
    assert_eq!(
        ir.debug
            .types
            .iter()
            .find(|t| t.name == "Pair")
            .unwrap()
            .size,
        2
    );
    execute("enum-cross-file-execute", source);
}

#[test]
fn rustc_enum_temporary_receivers_execute() {
    execute(
        "enum-temporaries",
        r#"
#[repr(u16)]#[derive(Copy,Clone)]enum Mode{A,B}
static CALLS:u16=0;
impl Mode{
 fn choose()->Self{unsafe{addr_of(&CALLS).write(0,CALLS+1);}Self::B}
 fn code(&self)->u16{*self as u16}
 fn replace(&mut self,other:Self)->Self{let old=*self;*self=other;old}
 fn with(&self,extra:u16)->u16{*self as u16+extra}
}
fn later()->u16{if CALLS==1{7}else{99}}
fn main(){
 if Mode::choose().code()!=1||Mode::A.code()!=0||CALLS!=1{halt(1);}
 if Mode::A.replace(Mode::B).code()!=0{halt(2);}
 unsafe{addr_of(&CALLS).write(0,0);}
 if Mode::choose().with(later())!=8||CALLS!=1{halt(3);}
 halt(0x600d);}
"#,
    );
}

#[test]
fn rustc_enum_methods_preserve_checked_rejections() {
    for (label,source,expected) in [
        ("immutable","#[repr(u16)] enum E{A,B}impl E{fn set(&mut self){*self=Self::B;}}fn main(){let e=E::A;e.set();}","rustc E0596"),
        ("alias","#[repr(u16)] enum E{A,B}impl E{fn set(&mut self){*self=Self::B;}}fn main(){let mut e=E::A;let r=&e;e.set();let n=*r as u16;}","rustc E0502"),
        ("generic","#[repr(u16)] enum E{A}impl E{fn id<T>(&self,value:T)->T{value}}fn main(){}","generic/default value methods"),
        ("wrong-return","#[repr(u16)] enum E{A}impl E{fn wrong(&self)->Self{3}}fn main(){}","rustc E0308"),
        ("trait","#[repr(u16)] enum E{A}impl core::convert::From<u16> for E{fn from(value:u16)->Self{Self::A}}fn main(){}","nongeneric inherent"),
        ("tuple","#[repr(u16)] enum E{A}impl E{fn code(&self)->u16{0}}fn main(){let (a,b)=(E::A,E::A);}","tuple elements"),
        ("payload","#[repr(u16)] enum E{A(u16)}impl E{fn make()->Self{Self::A(1)}}fn main(){}","fieldless variants"),
        ("repr","#[repr(u8)] enum E{A}impl E{fn make()->Self{Self::A}}fn main(){}","repr(u16)"),
        ("associated-const","#[repr(u16)] enum E{A}impl E{const V:Self=Self::A;}fn main(){}","functions only"),
        ("unsafe","#[repr(u16)] enum E{A}impl E{unsafe fn make()->Self{Self::A}}fn main(){let value=E::make();}","rustc E0133"),
        ("entry","#[repr(u16)] enum E{A}impl E{fn __rcc_value_hidden(&self)->u16{0}}fn main(){}","reserved value method name"),
    ] {
        let error=parse(&format!("enum-reject-{label}"),source).err().expect("invalid enum method passed").to_string();
        assert!(error.contains(expected),"{label}: {error}");
    }
}

#[test]
fn unchecked_enum_methods_remain_outside_the_legacy_entry() {
    let error = rcc::frontend::compile_program_named(
        "unchecked-enum",
        "#[repr(u16)] enum E{A}impl E{fn make()->Self{Self::A}}fn main(){}",
        &CompilerOptions::default(),
        &mut |name| Err(format!("unexpected {name}")),
    )
    .err()
    .expect("legacy enum impl must remain rejected")
    .to_string();
    assert!(error.contains("impl blocks"), "{error}");
}

#[test]
#[ignore = "executes the system SDK source fixture"]
fn rustc_enum_real_shininess_production_sources_execute() {
    let source_root =
        PathBuf::from(std::env::var_os("RCC_GRAPHICS_API_RCC").unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../systems/cpu-v3-tang-nano-20k/rcc")
                .into_os_string()
        }));
    let schema_root = PathBuf::from(
        std::env::var_os("RCC_GRAPHICS_SCHEMA_RCC")
            .unwrap_or_else(|| source_root.clone().into_os_string()),
    );
    let out = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/rcc-enum-methods")
        })
        .join("actual-shininess");
    std::fs::create_dir_all(&out).unwrap();
    let source = r#"mod gpu_scene_value_methods;
static EXPONENTS:Buf<u16,17>=Buf::new([4,5,6,7,8,10,12,14,16,20,24,28,32,40,48,56,64]);
fn main(){let mut index:u16=0;while index<17{
 let selected=Shininess::nearest(EXPONENTS[index]);
 if selected as u16!=index||selected.exponent()!=EXPONENTS[index]{halt(1);}index+=1;}
 if Shininess::nearest(13).exponent()!=12||Shininess::nearest(36).exponent()!=32{halt(2);}
 if Shininess::nearest(0) as u16!=0||Shininess::nearest(65535) as u16!=16{halt(3);}
 halt(0x600d);}"#;
    std::fs::write(out.join("caller.rs"), source).unwrap();
    for gpu_debug in [false, true] {
        let options = CompilerOptions {
            gpu_debug,
            ..CompilerOptions::default()
        };
        let profile = if gpu_debug { "debug" } else { "release" };
        let ir = rcc::frontend::compile_program_named_with_libraries_checked(
            "actual-shininess",
            source,
            &options,
            &[],
            &RustcCheck::new(out.join(profile)),
            &mut |name| {
                let b_file = source_root.join(format!("{name}.rs"));
                let file = if name == "gpu_schema" || !b_file.exists() {
                    schema_root.join(format!("{name}.rs"))
                } else {
                    b_file
                };
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("{}: {e}", file.display()))?;
                std::fs::write(out.join(format!("input-{name}.rs")), &text)
                    .map_err(|e| e.to_string())?;
                Ok(rcc::frontend::ModuleSource {
                    source: text,
                    origin: Some(file.canonicalize().map_err(|e| e.to_string())?),
                    namespace: rcc::frontend::ModuleNamespace::Flat,
                })
            },
        )
        .unwrap_or_else(|error| panic!("{profile}: {error}"));
        let program = try_compile(ir, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        let result = cpu.run(250_000).unwrap();
        assert!(
            matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
            "{profile}: {result:?}"
        );
        std::fs::write(
            out.join(format!("result-{profile}.txt")),
            format!("{result:?}\ncode_words={}\n", program.words.len()),
        )
        .unwrap();
    }
}

#[test]
fn explicit_enum_values_and_both_conversions_keep_the_one_word_abi() {
    execute(
        "enum-values",
        r#"
#[repr(u16)]#[derive(Copy,Clone,PartialEq)]enum Mode{First=1,Second,High=8,Last,Max=65535,Zero=0}
const HIGH:u16=Mode::High as u16;
fn choose(value:u16)->Mode{enum_from_u16::<Mode>(value)}
fn raw(value:u16)->Mode{unsafe{enum_from_u16_unchecked::<Mode>(value)}}
struct Holder{mode:Mode,tail:u16}
fn main(){
 let value=Holder{mode:choose(2),tail:99};
 if value.mode!=Mode::Second||value.tail!=99||HIGH!=8{halt(1);}
 let mut i=8u16;while i<10{if choose(i) as u16!=i{halt(2);}i+=1;}
 if choose(65535)!=Mode::Max||choose(0)!=Mode::Zero||raw(1)!=Mode::First{halt(3);}
 halt(0x600d);
}"#,
    );
    let ir=parse("enum-raw-cost", "#[repr(u16)]enum E{A=1,B}fn raw(v:u16)->E{unsafe{enum_from_u16_unchecked::<E>(v)}}fn main(){let v=raw(1);halt(v as u16);}").unwrap();
    let raw = ir.funcs.iter().find(|f| f.name == "raw").unwrap();
    assert_eq!(
        raw.blocks.len(),
        1,
        "unchecked conversion must not emit a test"
    );
    assert!(
        raw.blocks[0].insts.is_empty(),
        "unchecked conversion reuses its input vreg"
    );
}

#[test]
fn checked_enum_conversion_rejects_gaps_and_outside_values_in_both_profiles() {
    for optimized in [true, false] {
        for value in [0, 3, 10, 65535] {
            let source=format!("#[repr(u16)]enum E{{A=1,B,C=8,D}}fn main(){{let value=enum_from_u16::<E>({value});halt(value as u16);}}");
            let options = CompilerOptions {
                opt: if optimized {
                    rcc::Opts::default()
                } else {
                    rcc::Opts::disabled()
                },
                ..CompilerOptions::default()
            };
            let program = try_compile(
                parse(&format!("enum-invalid-{value}-{optimized}"), &source).unwrap(),
                &options,
                "main",
            )
            .unwrap();
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&program.words);
            assert!(matches!(
                cpu.run(10000).unwrap(),
                RunOutcome::Halted { signal: 0xffe5, .. }
            ));
        }
    }
}

#[test]
fn enum_conversion_keeps_nominal_and_unsafe_constraints() {
    for (label, source) in [
        (
            "unsafe",
            "#[repr(u16)]enum E{A}fn main(){let v=enum_from_u16_unchecked::<E>(0);}",
        ),
        ("not-enum", "fn main(){let v=enum_from_u16::<u16>(0);}"),
        ("duplicate", "#[repr(u16)]enum E{A=1,B=1}fn main(){}"),
        ("overflow", "#[repr(u16)]enum E{A=65535,B}fn main(){}"),
        ("negative", "#[repr(u16)]enum E{A=-1}fn main(){}"),
    ] {
        assert!(
            parse(&format!("enum-invalid-{label}"), source).is_err(),
            "{label}"
        );
    }
}
