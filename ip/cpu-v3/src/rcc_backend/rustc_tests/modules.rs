//! Ordinary namespaces resolve before target lowering and retain source locations.
use super::{options, CompilerOptions};
use crate::{CpuV3Sim, RunOutcome};

fn compile(
    label: &str,
    source: &str,
    modules: &[(&str, &str)],
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    rcc::frontend::compile_program_named_checked(
        "module-main.rs",
        source,
        &CompilerOptions::default(),
        &options(label),
        &mut |name| {
            modules
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, body)| body.to_string())
                .ok_or_else(|| format!("missing module {name}"))
        },
    )
}

#[test]
fn rustc_modules_isolate_duplicate_names_types_and_nested_paths() {
    let source = r#"
mod left;mod right;mod scene;
use left::Point as Left;use right::{Point as Right,value as right_value};
fn base()->u16{5}
fn main(){
 let a=Left{x:left::value()};let b=Right{x:right_value()};
 if a.x!=3||b.x!=9||scene::camera::value()!=16||scene::reexport()!=16{halt(0xbad);}
 halt(0x600d);
}
"#;
    let modules = [
        ("left", "pub struct Point{pub x:u16} pub fn value()->u16{3}"),
        (
            "right",
            "pub struct Point{pub x:u16} pub fn value()->u16{9}",
        ),
        (
            "scene",
            "pub mod camera;pub mod light;pub use camera::value as reexport;",
        ),
        (
            "scene/camera",
            "pub fn value()->u16{super::light::value()+crate::base()}",
        ),
        ("scene/light", "pub(crate) fn value()->u16{11}"),
    ];
    let code = super::super::try_compile(
        compile("module-namespaces", source, &modules).unwrap(),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}

#[test]
fn rustc_modules_reject_private_and_ambiguous_imports() {
    let modules = [
        ("left", "fn secret()->u16{3}pub fn value()->u16{3}"),
        ("right", "pub fn value()->u16{9}"),
    ];
    for (label, source, expected) in [
        (
            "module-private",
            "mod left;fn main(){let x=left::secret();}",
            "private module item",
        ),
        (
            "module-ambiguous",
            "mod left;mod right;use left::*;use right::*;fn main(){let x=value();}",
            "ambiguous module import",
        ),
    ] {
        let error = compile(label, source, &modules)
            .err()
            .expect("invalid namespace use must fail")
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn rustc_modules_preserve_field_and_method_privacy_in_the_original_module_tree() {
    let modules = [
        ("owner", "pub struct Value{word:u16} impl Value{pub fn new()->Self{Self{word:7}}fn hidden()->u16{9}}pub mod child;"),
        ("owner/child", "pub fn read(value:&super::Value)->u16{value.word}"),
    ];
    for (label, source, expected) in [
        (
            "module-private-field",
            "mod owner;fn main(){let value=owner::Value::new();let word=value.word;}",
            "rustc E0616",
        ),
        (
            "module-private-method",
            "mod owner;fn main(){let value=owner::Value::hidden();}",
            "rustc E0624",
        ),
    ] {
        let error = compile(label, source, &modules)
            .err()
            .expect("private member must remain private")
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
    let source="mod owner;fn main(){if owner::child::read(&owner::Value::new())!=7{halt(1);}halt(0x600d);}";
    let code = super::super::try_compile(
        compile("module-child-fields", source, &modules).unwrap(),
        &CompilerOptions::default(),
        "main",
    )
    .unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    assert!(matches!(
        cpu.run(100_000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
}
