//! Fixed-destination single-application Flash ABI. No section interpreter.
use std::{fmt, path::PathBuf};

pub const SINGLE_BOOT_VERSION: u16 = 5;
pub const SINGLE_BOOT_HEADER_BYTES: usize = 32;
pub const SINGLE_BOOT_PAYLOAD_OFFSET: usize = 128;
pub const SINGLE_BOOT_FLASH_BASE: usize = 0x10_0000;
pub const SINGLE_BOOT_CODE_WORD: usize = 0x7_0000;
pub const SINGLE_BOOT_MAX_CODE_BYTES: usize = 0x2_0000;
pub const SINGLE_BOOT_STATIC_WORDS: usize = 8192;
pub const SINGLE_BOOT_STACK: u16 = 0xc000;
const MAGIC: &[u8; 8] = b"CPU3BOOT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleBootError(pub &'static str);
impl fmt::Display for SingleBootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for SingleBootError {}

/// The project selects one source; destination and entry are fixed by the ABI.
pub fn single_application_source(text: &str) -> Result<PathBuf, SingleBootError> {
    let mut format = false;
    let mut source = None;
    for line in text
        .lines()
        .map(|l| l.split('#').next().unwrap().trim())
        .filter(|l| !l.is_empty())
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["format", "2"] if !format => format = true,
            ["application", path] if source.is_none() => {
                let path = PathBuf::from(path);
                if path.is_absolute()
                    || path.extension().and_then(|x| x.to_str()) != Some("rs")
                    || path
                        .components()
                        .any(|x| !matches!(x, std::path::Component::Normal(_)))
                {
                    return Err(SingleBootError("application must be a relative .rs source"));
                }
                source = Some(path);
            }
            _ => {
                return Err(SingleBootError(
                    "expected format 2 and exactly one application",
                ))
            }
        }
    }
    if !format {
        return Err(SingleBootError("missing project format"));
    }
    source.ok_or(SingleBootError("missing application source"))
}

pub fn single_boot_code_length(header: &[u8]) -> Result<usize, SingleBootError> {
    if header.len() < SINGLE_BOOT_HEADER_BYTES {
        return Err(SingleBootError("truncated header"));
    }
    if &header[..8] != MAGIC
        || u16::from_le_bytes([header[8], header[9]]) != SINGLE_BOOT_VERSION
        || u16::from_le_bytes([header[10], header[11]]) != SINGLE_BOOT_HEADER_BYTES as u16
        || header[16..32].iter().any(|b| *b != 0)
    {
        return Err(SingleBootError("invalid single-application header"));
    }
    let bytes = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    if bytes == 0 || bytes & 1 != 0 || bytes > SINGLE_BOOT_MAX_CODE_BYTES {
        return Err(SingleBootError(
            "code length must be nonzero, even, and at most 128 KiB",
        ));
    }
    if SINGLE_BOOT_FLASH_BASE + SINGLE_BOOT_PAYLOAD_OFFSET + bytes > 8 * 1024 * 1024 {
        return Err(SingleBootError("payload exceeds Flash"));
    }
    Ok(bytes)
}

pub fn pack_single_application(code: &[u8]) -> Result<Vec<u8>, SingleBootError> {
    let length = u32::try_from(code.len()).map_err(|_| SingleBootError("code exceeds u32"))?;
    let mut image = vec![0xff; SINGLE_BOOT_PAYLOAD_OFFSET];
    image[..32].fill(0);
    image[..8].copy_from_slice(MAGIC);
    image[8..10].copy_from_slice(&SINGLE_BOOT_VERSION.to_le_bytes());
    image[10..12].copy_from_slice(&(SINGLE_BOOT_HEADER_BYTES as u16).to_le_bytes());
    image[12..16].copy_from_slice(&length.to_le_bytes());
    single_boot_code_length(&image)?;
    image.extend_from_slice(code);
    Ok(image)
}

/// Functional oracle only: physical writes complete synchronously in this model.
/// The RTL loader independently waits for each actual memory response.
pub fn install_single_application(
    package: &[u8],
    memory: &mut [u16],
) -> Result<usize, SingleBootError> {
    let bytes = single_boot_code_length(package)?;
    let code = package
        .get(SINGLE_BOOT_PAYLOAD_OFFSET..SINGLE_BOOT_PAYLOAD_OFFSET + bytes)
        .ok_or(SingleBootError("truncated payload"))?;
    if memory.len() < SINGLE_BOOT_CODE_WORD + bytes / 2 {
        return Err(SingleBootError("insufficient physical memory"));
    }
    memory[..SINGLE_BOOT_STATIC_WORDS].fill(0);
    for (word, data) in memory[SINGLE_BOOT_CODE_WORD..]
        .iter_mut()
        .zip(code.as_chunks::<2>().0)
    {
        *word = u16::from_le_bytes([data[0], data[1]]);
    }
    Ok(bytes / 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_header_and_nonzero_memory_contract() {
        let image = pack_single_application(&[0x34, 0x12, 0xcd, 0xab]).unwrap();
        let mut memory = vec![0xa55a; SINGLE_BOOT_CODE_WORD + 8];
        assert_eq!(install_single_application(&image, &mut memory).unwrap(), 2);
        assert!(memory[..SINGLE_BOOT_STATIC_WORDS].iter().all(|w| *w == 0));
        assert_eq!(memory[SINGLE_BOOT_STATIC_WORDS], 0xa55a);
        assert_eq!(
            &memory[SINGLE_BOOT_CODE_WORD..SINGLE_BOOT_CODE_WORD + 3],
            &[0x1234, 0xabcd, 0xa55a]
        );
        for length in [0, 1, SINGLE_BOOT_MAX_CODE_BYTES + 2] {
            assert!(pack_single_application(&vec![0; length]).is_err());
        }
        assert!(pack_single_application(&vec![0; SINGLE_BOOT_MAX_CODE_BYTES]).is_ok());
        for index in (0..12).chain(16..32) {
            let mut bad = image.clone();
            bad[index] ^= 1;
            assert!(
                single_boot_code_length(&bad).is_err(),
                "header byte {index}"
            );
        }
        assert!(install_single_application(&image[..image.len() - 1], &mut memory).is_err());
    }
    #[test]
    fn project_has_one_application_and_no_selection() {
        assert_eq!(
            single_application_source("format 2\napplication rcc/display-demo.rs").unwrap(),
            PathBuf::from("rcc/display-demo.rs")
        );
        for bad in [
            "format 1\ns1 rcc/a.rs\ns2 rcc/b.rs",
            "format 2\napplication ../a.rs",
            "format 2\napplication rcc/a.rs\napplication rcc/b.rs",
        ] {
            assert!(single_application_source(bad).is_err());
        }
    }
}
