//! One RCC fixture shared by CPU execution and the offline pack CLI.
use cpu_v3::rcc_backend::{self, CompilerOptions};
use cpu_v3_boot_format::*;
pub fn fixture() -> Vec<u8> {
    let source = include_str!("../../rcc/boot-contract.rs");
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = SINGLE_BOOT_STACK;
    let parsed =
        rcc::frontend::compile_program_named("boot-contract.rs", source, &options, &mut |name| {
            Err(format!("unexpected module {name}"))
        })
        .unwrap();
    let static_words = parsed.debug.static_data_end;
    assert_eq!(static_words, 17);
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let bytes: Vec<u8> = program.words.iter().flat_map(|x| x.to_le_bytes()).collect();
    pack_single_application(&bytes, static_words).unwrap()
}
