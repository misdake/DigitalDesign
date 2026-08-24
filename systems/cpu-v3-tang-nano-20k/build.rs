#![allow(dead_code, hidden_glob_reexports, unused_imports)]

pub use cpu_v3::*;

#[path = "src/boot/mod.rs"]
mod boot;

use boot::{
    build_boot_image, BootEntry, BootImageSpec, BootTarget, InputSection, SectionKind,
    SECTION_EXECUTE, SECTION_READ, SECTION_WRITE,
};
use cpu_v3::rcc_backend::{self, CompilerOptions};
use rcc::frontend::compile_program_named;
use std::fmt::Write;
use std::path::{Path, PathBuf};

fn compile(path: &Path, options: &CompilerOptions) -> Vec<u16> {
    let source = std::fs::read_to_string(path).expect("read RCC boot source");
    let program =
        compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
            Err(format!("unknown module `{name}`"))
        })
        .expect("compile RCC boot source");
    rcc_backend::compile(program, options, "main").words
}

fn word_bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let sources = root.join("rcc");
    for name in ["stage0.rs", "stage1.rs", "boot-demo.rs"] {
        println!("cargo:rerun-if-changed={}", sources.join(name).display());
    }

    let stage0 = compile(&sources.join("stage0.rs"), &CompilerOptions::default());
    let stage1 = compile(
        &sources.join("stage1.rs"),
        &CompilerOptions {
            code_base: 0x0100,
            stack_init: 0xf000,
            ..CompilerOptions::default()
        },
    );
    let application = compile(
        &sources.join("boot-demo.rs"),
        &CompilerOptions {
            code_base: 0x0200,
            stack_init: 0xe000,
            ..CompilerOptions::default()
        },
    );
    let stage1 = word_bytes(&stage1);
    let application = word_bytes(&application);
    let package = build_boot_image(BootImageSpec {
        target: BootTarget::TangNano20K,
        stage1_section: "stage1".into(),
        stage1_entry: BootEntry {
            code_segment: 1,
            offset: 0x0100,
            data_segment: 2,
            stack_offset: 0xf000,
        },
        application_entry: BootEntry {
            code_segment: 3,
            offset: 0x0200,
            data_segment: 4,
            stack_offset: 0xe000,
        },
        sections: vec![
            InputSection {
                name: "stage1".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_EXECUTE,
                destination: PhysicalWordAddress::new(0x0001_0100),
                memory_size_bytes: stage1.len() as u32,
                data: stage1,
                alignment_bytes: 32,
            },
            InputSection {
                name: "application".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_EXECUTE,
                destination: PhysicalWordAddress::new(0x0003_0200),
                memory_size_bytes: application.len() as u32,
                data: application,
                alignment_bytes: 32,
            },
            InputSection {
                name: "data".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_WRITE,
                destination: PhysicalWordAddress::new(0x0004_0000),
                data: vec![0xef, 0xbe, 0x55],
                memory_size_bytes: 8,
                alignment_bytes: 32,
            },
            InputSection {
                name: "bss".into(),
                kind: SectionKind::Zero,
                flags: SECTION_READ | SECTION_WRITE,
                destination: PhysicalWordAddress::new(0x0004_0100),
                data: vec![],
                memory_size_bytes: 64,
                alignment_bytes: 32,
            },
        ],
    })
    .expect("build boot image")
    .bytes;

    assert_eq!(
        fnv1a64(&word_bytes(&stage0)),
        6_245_176_589_688_159_720,
        "Stage0 bytes changed from the CPU V3 boot-format baseline"
    );
    assert_eq!(
        fnv1a64(&package),
        17_919_558_294_178_096_904,
        "Flash package bytes changed from the CPU V3 boot-format baseline"
    );

    let mut generated = String::new();
    writeln!(generated, "const STAGE0_PROGRAM: &[u16] = &{:?};", stage0).unwrap();
    writeln!(generated, "const FLASH_PACKAGE: &[u8] = &{:?};", package).unwrap();
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("boot_images.rs");
    std::fs::write(output, generated).expect("write generated boot images");
}
