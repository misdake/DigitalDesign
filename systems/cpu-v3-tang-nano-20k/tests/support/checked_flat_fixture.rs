//! Explicit legacy SDK unit scope, without resource-library authority.
use rcc::frontend::{self, CompileError, ModuleNamespace, ModuleSource, Program};
use std::path::PathBuf;

pub fn compile(
    name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    check: &frontend::rustc_check::RustcCheck,
    loader: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Program, CompileError> {
    frontend::compile_program_named_with_libraries_checked(
        name,
        source,
        options,
        &[],
        check,
        &mut |name| {
            loader(name).map(|source| ModuleSource {
                source,
                origin: None,
                namespace: ModuleNamespace::Flat,
            })
        },
    )
}

pub fn output_root() -> PathBuf {
    std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/sdk-host-checks")
        })
}

/// Compare only push words fetched by native DMA commands. Scratch gaps and
/// alignment holes are CPU staging, not GPU inputs or deterministic padding.
pub fn consumed_payload(code: &[u16], push: &[u16], sector: u16) -> Vec<(u16, Vec<u16>)> {
    assert_eq!(code.len() % 2, 0);
    let base = usize::from(sector) * 64;
    let mut records = Vec::new();
    for pair in code.as_chunks::<2>().0 {
        let command = u32::from(pair[0]) | (u32::from(pair[1]) << 16);
        if command >> 28 == 15 {
            break;
        }
        if command >> 28 != 1 {
            continue;
        }
        let first = ((command >> 10) & 0x3ffff) as usize * 16;
        let words = 16 << ((command >> 1) & 3);
        let offset = first
            .checked_sub(base)
            .expect("DMA source precedes push storage");
        records.push((
            ((command >> 3) & 127) as u16,
            push[offset..offset + words].to_vec(),
        ));
    }
    records
}

#[test]
fn payload_oracle_ignores_only_unreferenced_words() {
    let sector = 0x9a00;
    let base = u32::from(sector) * 64;
    let command = 0x1000_0000 | (((base + 16) / 16) << 10) | (66 << 3);
    let code = [command as u16, (command >> 16) as u16, 0, 0xf000];
    let mut push: Vec<u16> = (0..64).collect();
    let expected = consumed_payload(&code, &push, sector);
    assert_eq!(expected, vec![(66, push[16..32].to_vec())]);
    push[0] ^= 1;
    assert_eq!(consumed_payload(&code, &push, sector), expected);
    push[17] ^= 1;
    assert_ne!(consumed_payload(&code, &push, sector), expected);
}
