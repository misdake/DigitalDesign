#![allow(dead_code, hidden_glob_reexports, unused_imports)]

pub use cpu_v3::*;

#[path = "src/boot/mod.rs"]
mod boot;

use boot::{
    build_boot_image, ApplicationLayout, BootApplicationProject, BootImageSpec, BootTarget,
    InputSection, SectionKind, S1_APPLICATION_LAYOUT, S2_APPLICATION_LAYOUT, SECTION_EXECUTE,
    SECTION_READ, STAGE1_LAYOUT,
};
use cpu_v3::rcc_backend::{self, CompilerOptions};
use rcc::frontend::compile_program_named;
use std::fmt::Write;
use std::path::{Path, PathBuf};

const PROJECT_CONFIG: &str = "boot-applications.conf";
const SELECTION_MODULE: &str = "boot_selection";

fn compile(
    path: &Path,
    options: &CompilerOptions,
    generated_modules: &[(&str, &Path)],
) -> Vec<u16> {
    println!("cargo:rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read RCC source {}: {error}", path.display()));
    let source_dir = path.parent().expect("RCC source directory");
    let program =
        compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
            let module_path = generated_modules
                .iter()
                .find_map(|(module, path)| (*module == name).then_some(*path))
                .map(Path::to_owned)
                .unwrap_or_else(|| source_dir.join(format!("{name}.rs")));
            if !generated_modules
                .iter()
                .any(|(_, generated)| *generated == module_path)
            {
                println!("cargo:rerun-if-changed={}", module_path.display());
            }
            std::fs::read_to_string(&module_path).map_err(|error| {
                format!(
                    "read module `{name}` from {}: {error}",
                    module_path.display()
                )
            })
        })
        .unwrap_or_else(|error| panic!("compile RCC source {}: {error}", path.display()));
    rcc_backend::compile(program, options, "main").words
}

fn options(layout: ApplicationLayout) -> CompilerOptions {
    CompilerOptions {
        code_base: layout.entry.offset,
        stack_init: layout.entry.stack_offset,
        ..CompilerOptions::default()
    }
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

fn application_section(layout: ApplicationLayout, data: Vec<u8>) -> InputSection {
    InputSection {
        name: layout.section_name.into(),
        kind: SectionKind::Load,
        flags: SECTION_READ | SECTION_EXECUTE,
        destination: layout.destination(),
        memory_size_bytes: data.len() as u32,
        data,
        alignment_bytes: 32,
    }
}

fn selection_source() -> String {
    let s1 = S1_APPLICATION_LAYOUT.entry;
    let s2 = S2_APPLICATION_LAYOUT.entry;
    format!(
        "// Generated from {PROJECT_CONFIG}; do not edit.\n\
         pub const S1_CODE_SEGMENT: u16 = {s1_cseg};\n\
         pub const S1_ENTRY_OFFSET: u16 = {s1_offset};\n\
         pub const S1_DATA_SEGMENT: u16 = {s1_dseg};\n\
         pub const S2_CODE_SEGMENT: u16 = {s2_cseg};\n\
         pub const S2_ENTRY_OFFSET: u16 = {s2_offset};\n\
         pub const S2_DATA_SEGMENT: u16 = {s2_dseg};\n",
        s1_cseg = s1.code_segment,
        s1_offset = s1.offset,
        s1_dseg = s1.data_segment,
        s2_cseg = s2.code_segment,
        s2_offset = s2.offset,
        s2_dseg = s2.data_segment,
    )
}

fn pack_manifest(stage1_bytes: usize, s1_bytes: usize, s2_bytes: usize) -> String {
    let stage1 = STAGE1_LAYOUT.entry;
    let s1 = S1_APPLICATION_LAYOUT.entry;
    format!(
        "# Generated from {PROJECT_CONFIG}; paths are relative to this file.\n\
         format 1\n\
         target tang-nano-20k\n\
         stage1-section {stage1_name}\n\
         stage1-entry {stage1_cseg:#06x} {stage1_offset:#06x} {stage1_dseg:#06x} {stage1_sp:#06x}\n\
         application-entry {s1_cseg:#06x} {s1_offset:#06x} {s1_dseg:#06x} {s1_sp:#06x}\n\n\
         load {stage1_name} {stage1_destination:#010x} rx 32 {stage1_bytes} {stage1_asset}\n\
         load {s1_name} {s1_destination:#010x} rx 32 {s1_bytes} {s1_asset}\n\
         load {s2_name} {s2_destination:#010x} rx 32 {s2_bytes} {s2_asset}\n",
        stage1_name = STAGE1_LAYOUT.section_name,
        stage1_cseg = stage1.code_segment,
        stage1_offset = stage1.offset,
        stage1_dseg = stage1.data_segment,
        stage1_sp = stage1.stack_offset,
        stage1_destination = STAGE1_LAYOUT.destination().get(),
        stage1_asset = STAGE1_LAYOUT.asset_name,
        s1_name = S1_APPLICATION_LAYOUT.section_name,
        s1_cseg = s1.code_segment,
        s1_offset = s1.offset,
        s1_dseg = s1.data_segment,
        s1_sp = s1.stack_offset,
        s1_destination = S1_APPLICATION_LAYOUT.destination().get(),
        s1_asset = S1_APPLICATION_LAYOUT.asset_name,
        s2_name = S2_APPLICATION_LAYOUT.section_name,
        s2_destination = S2_APPLICATION_LAYOUT.destination().get(),
        s2_asset = S2_APPLICATION_LAYOUT.asset_name,
    )
}

fn project_map(
    project: &BootApplicationProject,
    stage1_bytes: &[u8],
    s1_bytes: &[u8],
    s2_bytes: &[u8],
) -> String {
    let mut text = format!("format=1\nconfig={PROJECT_CONFIG}\n");
    for (slot, source, layout, bytes) in [
        (
            "stage1",
            Path::new("rcc/stage1.rs"),
            STAGE1_LAYOUT,
            stage1_bytes,
        ),
        (
            "s1",
            project.s1_source.as_path(),
            S1_APPLICATION_LAYOUT,
            s1_bytes,
        ),
        (
            "s2",
            project.s2_source.as_path(),
            S2_APPLICATION_LAYOUT,
            s2_bytes,
        ),
    ] {
        let entry = layout.entry;
        writeln!(text, "{slot}.source={}", source.display()).unwrap();
        writeln!(text, "{slot}.asset={}", layout.asset_name).unwrap();
        writeln!(text, "{slot}.section={}", layout.section_name).unwrap();
        writeln!(
            text,
            "{slot}.entry={:04x}:{:04x} dseg={:04x} sp={:04x}",
            entry.code_segment, entry.offset, entry.data_segment, entry.stack_offset
        )
        .unwrap();
        writeln!(
            text,
            "{slot}.destination={:#010x}",
            layout.destination().get()
        )
        .unwrap();
        writeln!(text, "{slot}.bytes={}", bytes.len()).unwrap();
        writeln!(text, "{slot}.fnv1a64={:016x}", fnv1a64(bytes)).unwrap();
    }
    text
}

fn asset_bindings() -> String {
    let assets = [
        "stage0.v3bin",
        STAGE1_LAYOUT.asset_name,
        S1_APPLICATION_LAYOUT.asset_name,
        S2_APPLICATION_LAYOUT.asset_name,
        "cpu-v3-boot.bin",
        "cpu-v3-boot.map",
        "boot.cpu-v3-manifest",
        "boot-project.map",
        "boot-selection.generated.rs",
    ];
    let mut source = String::from("const ASSETS: &[(&str, &[u8])] = &[\n");
    for asset in assets {
        writeln!(
            source,
            "    ({asset:?}, include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{asset}\"))),"
        )
        .unwrap();
    }
    source.push_str("];\n");
    source
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let config_path = root.join(PROJECT_CONFIG);
    println!("cargo:rerun-if-changed={}", config_path.display());
    let config = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", config_path.display()));
    let project = BootApplicationProject::parse(&config)
        .unwrap_or_else(|error| panic!("parse {}: {error}", config_path.display()));

    let s1_path = root.join(&project.s1_source);
    let s2_path = root.join(&project.s2_source);
    let selection_path = output.join("boot-selection.generated.rs");
    write_artifact(
        &output,
        "boot-selection.generated.rs",
        selection_source().as_bytes(),
    );

    let stage0 = compile(
        &root.join("rcc/stage0.rs"),
        &CompilerOptions::default(),
        &[],
    );
    let s1_application = compile(&s1_path, &options(S1_APPLICATION_LAYOUT), &[]);
    let s2_application = compile(&s2_path, &options(S2_APPLICATION_LAYOUT), &[]);
    let stage1 = compile(
        &root.join("rcc/stage1.rs"),
        &options(STAGE1_LAYOUT),
        &[(SELECTION_MODULE, selection_path.as_path())],
    );
    let s2_simulator = compile(
        &s2_path,
        &CompilerOptions {
            stack_init: S2_APPLICATION_LAYOUT.entry.stack_offset,
            ..CompilerOptions::default()
        },
        &[],
    );

    let stage0_bytes = word_bytes(&stage0);
    let stage1_bytes = word_bytes(&stage1);
    let s1_bytes = word_bytes(&s1_application);
    let s2_bytes = word_bytes(&s2_application);
    let image = build_boot_image(BootImageSpec {
        target: BootTarget::TangNano20K,
        stage1_section: STAGE1_LAYOUT.section_name.into(),
        stage1_entry: STAGE1_LAYOUT.entry,
        application_entry: S1_APPLICATION_LAYOUT.entry,
        sections: vec![
            application_section(STAGE1_LAYOUT, stage1_bytes.clone()),
            application_section(S1_APPLICATION_LAYOUT, s1_bytes.clone()),
            application_section(S2_APPLICATION_LAYOUT, s2_bytes.clone()),
        ],
    })
    .expect("build boot image");

    // ISA 0.8 encoding migration rebaselined the Stage0 words; Step 5
    // re-validates the boot assets (including Stage0 < 1024 words) against
    // the final compiler and updates this baseline again if they change.
    assert_eq!(
        fnv1a64(&stage0_bytes),
        17_837_455_290_091_098_869,
        "Stage0 bytes changed from the CPU V3 boot-format baseline"
    );

    write_artifact(&output, "stage0.v3bin", &stage0_bytes);
    write_artifact(&output, STAGE1_LAYOUT.asset_name, &stage1_bytes);
    write_artifact(&output, S1_APPLICATION_LAYOUT.asset_name, &s1_bytes);
    write_artifact(&output, S2_APPLICATION_LAYOUT.asset_name, &s2_bytes);
    write_artifact(&output, "cpu-v3-boot.bin", &image.bytes);
    write_artifact(&output, "cpu-v3-boot.map", image.map().as_bytes());
    write_artifact(
        &output,
        "boot.cpu-v3-manifest",
        pack_manifest(stage1_bytes.len(), s1_bytes.len(), s2_bytes.len()).as_bytes(),
    );
    write_artifact(
        &output,
        "boot-project.map",
        project_map(&project, &stage1_bytes, &s1_bytes, &s2_bytes).as_bytes(),
    );
    write_artifact(
        &output,
        "boot_asset_bindings.rs",
        asset_bindings().as_bytes(),
    );

    let mut generated = String::new();
    writeln!(generated, "const STAGE0_PROGRAM: &[u16] = &{:?};", stage0).unwrap();
    writeln!(
        generated,
        "const FLASH_PACKAGE: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/cpu-v3-boot.bin\"));"
    )
    .unwrap();
    write_artifact(&output, "boot_images.rs", generated.as_bytes());
    write_artifact(
        &output,
        "display_image.rs",
        format!(
            "// Generated from the configured S2 application.\n\
             pub const DISPLAY_DEMO_PROGRAM: &[u16] = &{:?};\n",
            s2_simulator
        )
        .as_bytes(),
    );
}
