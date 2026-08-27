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

fn write_artifact(output: &Path, name: &str, bytes: &[u8]) {
    std::fs::write(output.join(name), bytes)
        .unwrap_or_else(|error| panic!("write generated boot artifact {name}: {error}"));
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let sources = root.join("rcc");
    for name in [
        "stage0.rs",
        "stage1.rs",
        "boot-demo.rs",
        "boot-alt.rs",
        "cpu-self-test.rs",
        "display-demo.rs",
        "mmio-diagnostic.rs",
        "sdram-self-test.rs",
    ] {
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
    let alternate_application = compile(
        &sources.join("boot-alt.rs"),
        &CompilerOptions {
            code_base: 0x0200,
            stack_init: 0xe000,
            ..CompilerOptions::default()
        },
    );
    let display_demo = compile(
        &sources.join("display-demo.rs"),
        &CompilerOptions {
            code_base: 0,
            stack_init: 0xf000,
            ..CompilerOptions::default()
        },
    );
    let cpu_self_test = compile(
        &sources.join("cpu-self-test.rs"),
        &CompilerOptions {
            code_base: 0,
            stack_init: 0xf000,
            ..CompilerOptions::default()
        },
    );
    let mmio_diagnostic = compile(
        &sources.join("mmio-diagnostic.rs"),
        &CompilerOptions::default(),
    );
    let sdram_self_test = compile(
        &sources.join("sdram-self-test.rs"),
        &CompilerOptions::default(),
    );
    let stage0_bytes = word_bytes(&stage0);
    let stage1_bytes = word_bytes(&stage1);
    let application_bytes = word_bytes(&application);
    let alternate_application_bytes = word_bytes(&alternate_application);
    let data = [0xef, 0xbe, 0x55];
    let image = build_boot_image(BootImageSpec {
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
                memory_size_bytes: stage1_bytes.len() as u32,
                data: stage1_bytes.clone(),
                alignment_bytes: 32,
            },
            InputSection {
                name: "application".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_EXECUTE,
                destination: PhysicalWordAddress::new(0x0003_0200),
                memory_size_bytes: application_bytes.len() as u32,
                data: application_bytes.clone(),
                alignment_bytes: 32,
            },
            InputSection {
                name: "application-alt".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_EXECUTE,
                destination: PhysicalWordAddress::new(0x0005_0200),
                memory_size_bytes: alternate_application_bytes.len() as u32,
                data: alternate_application_bytes.clone(),
                alignment_bytes: 32,
            },
            InputSection {
                name: "data".into(),
                kind: SectionKind::Load,
                flags: SECTION_READ | SECTION_WRITE,
                destination: PhysicalWordAddress::new(0x0004_0000),
                data: data.to_vec(),
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
    .expect("build boot image");
    let package = &image.bytes;

    assert_eq!(
        fnv1a64(&stage0_bytes),
        12_700_990_772_538_292_489,
        "Stage0 bytes changed from the CPU V3 boot-format baseline"
    );
    assert_eq!(
        fnv1a64(package),
        13_875_194_008_803_525_579,
        "Flash package bytes changed from the CPU V3 boot-format baseline"
    );

    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    write_artifact(&output, "stage0.v3bin", &stage0_bytes);
    write_artifact(&output, "stage1.v3bin", &stage1_bytes);
    write_artifact(&output, "boot-demo.v3bin", &application_bytes);
    write_artifact(&output, "boot-alt.v3bin", &alternate_application_bytes);
    write_artifact(&output, "cpu-self-test.v3bin", &word_bytes(&cpu_self_test));
    write_artifact(&output, "display-demo.v3bin", &word_bytes(&display_demo));
    write_artifact(
        &output,
        "mmio-diagnostic.v3bin",
        &word_bytes(&mmio_diagnostic),
    );
    write_artifact(
        &output,
        "sdram-self-test.v3bin",
        &word_bytes(&sdram_self_test),
    );
    write_artifact(&output, "data.bin", &data);
    write_artifact(&output, "cpu-v3-boot.bin", package);
    write_artifact(&output, "cpu-v3-boot.map", image.map().as_bytes());

    let mut generated = String::new();
    writeln!(generated, "const STAGE0_PROGRAM: &[u16] = &{:?};", stage0).unwrap();
    writeln!(
        generated,
        "const FLASH_PACKAGE: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/cpu-v3-boot.bin\"));"
    )
    .unwrap();
    std::fs::write(output.join("boot_images.rs"), generated)
        .expect("write generated boot image bindings");
    std::fs::write(
        output.join("cpu_self_test_image.rs"),
        format!(
            "const CPU_SELF_TEST_PROGRAM: &[u16] = &{:?};\n",
            cpu_self_test
        ),
    )
    .expect("write generated CPU self-test binding");
    std::fs::write(
        output.join("mmio_diagnostic_image.rs"),
        format!(
            "const MMIO_DIAGNOSTIC_PROGRAM: &[u16] = &{:?};\n",
            mmio_diagnostic
        ),
    )
    .expect("write generated MMIO diagnostic binding");
    std::fs::write(
        output.join("sdram_self_test_image.rs"),
        format!(
            "const SDRAM_SELF_TEST_PROGRAM: &[u16] = &{:?};\n",
            sdram_self_test
        ),
    )
    .expect("write generated SDRAM self-test binding");
    std::fs::write(
        output.join("display_image.rs"),
        format!(
            "const DISPLAY_DEMO_PROGRAM: &[u16] = &{:?};\n",
            display_demo
        ),
    )
    .expect("write generated display demo binding");
}
