//! Checked field privacy follows the same host-authorized library domains.
use super::{try_compile, CompilerOptions};
use crate::{CpuV3Sim, RunOutcome};
use rcc::frontend::{
    compile_program_named_with_libraries_checked, rustc_check::RustcCheck, ModuleSource,
};
use std::path::PathBuf;

const OWNER: &str = r#"#![rcc_resource_library(schema,helper)]
mod schema;mod helper;
#[rcc_resource]pub struct Lease{word:u16}
impl Drop for Lease{fn drop(&mut self){}}
impl GpuMemory{pub unsafe fn from_sectors(first:u16,count:u16)->Self{construct(first,count)}}
pub fn default_memory()->GpuMemory{construct(7,9)}
"#;
const SCHEMA: &str = r#"#[derive(Copy,Clone)]pub struct GpuMemory{first_sector:u16,sector_count:u16}
impl GpuMemory{pub fn first_sector(&self)->u16{self.first_sector}pub fn sector_count(&self)->u16{self.sector_count}}
pub struct OpenMemory{pub first_sector:u16}
"#;
const HELPER: &str = r#"fn construct(first:u16,count:u16)->GpuMemory{
 let mut value:GpuMemory=GpuMemory{first_sector:first,sector_count:count};
 value.first_sector=first;value}
"#;

fn compile(label: &str, source: &str, gpu_debug: bool) -> Result<rcc::frontend::Program, String> {
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/rcc-checked")
        })
        .join("field-privacy")
        .join(format!("{label}-{gpu_debug}"));
    let library = output.join("library");
    std::fs::create_dir_all(&library).unwrap();
    for (name, text) in [("owner", OWNER), ("schema", SCHEMA), ("helper", HELPER)] {
        std::fs::write(library.join(format!("{name}.rs")), text).unwrap();
    }
    std::fs::write(output.join("application.rs"), source).unwrap();
    let root = std::fs::canonicalize(library.join("owner.rs")).unwrap();
    let options = CompilerOptions {
        gpu_debug,
        ..CompilerOptions::default()
    };
    compile_program_named_with_libraries_checked(
        label,
        source,
        &options,
        &[root],
        &RustcCheck::new(output.join("checked")),
        &mut |name| {
            if name == "outsider" {
                return Ok(ModuleSource {
                    source: "pub fn forge()->GpuMemory{GpuMemory{first_sector:1,sector_count:2}}"
                        .into(),
                    origin: None,
                });
            }
            let path = std::fs::canonicalize(library.join(format!("{name}.rs")))
                .map_err(|e| e.to_string())?;
            Ok(ModuleSource {
                source: std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
                origin: Some(path),
            })
        },
    )
    .map_err(|e| e.to_string())
}

#[test]
fn checked_library_internal_factories_and_public_fields_execute() {
    let source = r#"mod owner;
fn main(){let memory:GpuMemory=unsafe{GpuMemory::from_sectors(12,34)};
 let default:GpuMemory=default_memory();
 let mut open:OpenMemory=OpenMemory{first_sector:5};open.first_sector=6;
 if memory.first_sector()!=12||memory.sector_count()!=34||default.first_sector()!=7||open.first_sector!=6{halt(1);}
 halt(0x600d);}"#;
    for gpu_debug in [false, true] {
        let ir = compile("legal", source, gpu_debug).unwrap();
        let options = CompilerOptions {
            gpu_debug,
            ..CompilerOptions::default()
        };
        let program = try_compile(ir, &options, "main").unwrap();
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&program.words);
        assert!(matches!(
            cpu.run(100_000).unwrap(),
            RunOutcome::Halted { signal: 0x600d, .. }
        ));
    }
}

#[test]
fn checked_library_private_value_fields_reject_external_use() {
    for gpu_debug in [false, true] {
        for (label,source) in [
            ("construct","mod owner;fn main(){let memory:GpuMemory=GpuMemory{first_sector:1,sector_count:2};}"),
            ("static","mod owner;static MEMORY:GpuMemory=GpuMemory{first_sector:1,sector_count:2};fn main(){}"),
            ("read","mod owner;fn main(){let memory:GpuMemory=default_memory();halt(memory.first_sector);}"),
            ("write","mod owner;fn main(){let mut memory:GpuMemory=default_memory();memory.first_sector=3;}"),
            ("borrow","mod owner;fn main(){let memory:GpuMemory=default_memory();let pointer=addr_of(&memory.first_sector);halt(pointer.read(0));}"),
            ("update","mod owner;fn main(){let memory:GpuMemory=default_memory();let other:GpuMemory=GpuMemory{first_sector:3,..memory};}"),
            ("pattern","mod owner;fn main(){let memory:GpuMemory=default_memory();let GpuMemory{first_sector,..}=memory;halt(first_sector);}"),
            ("external-impl","mod owner;impl GpuMemory{fn forged()->Self{Self{first_sector:1,sector_count:2}}}fn main(){}"),
            ("forwarding-module","mod owner;mod outsider;fn main(){let memory:GpuMemory=forge();}"),
        ] {
            let error=compile(label,source,gpu_debug).err().unwrap_or_else(||panic!("private fields accepted: {label}/{gpu_debug}"));
            assert!(error.contains("private"),"{label}/{gpu_debug}: {error}");
            assert!(error.contains("rustc E0"),"privacy must use normal rustc: {error}");
        }
    }
}
