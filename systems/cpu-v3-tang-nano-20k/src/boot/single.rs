//! CB06 sequential COPY/ZERO cold-boot image; layout validation belongs to the host.
use crate::gpu_layout as driver_layout;
use std::{
    fmt,
    path::{Path, PathBuf},
};

pub const SINGLE_BOOT_VERSION: u16 = 6;
pub const SINGLE_BOOT_HEADER_BYTES: usize = 4;
pub const SINGLE_BOOT_FLASH_BASE: usize = 0x10_0000;
pub const SINGLE_BOOT_CODE_WORD: usize = 0x7_0000;
pub const SINGLE_BOOT_MAX_CODE_BYTES: usize = 0x2_0000;
pub const SINGLE_BOOT_STATIC_WORDS: usize = 8192;
pub const SINGLE_BOOT_STACK: u16 = 0xc000;
pub const BOOT_MEMORY_BYTES: usize = 0x80_0000;
pub const BOOT_FRAMEBUFFER_BYTES: std::ops::Range<usize> = 0x40_0000..0x46_0000;
pub const BOOT_DEPTH_BYTES: std::ops::Range<usize> = 0x46_0000..0x49_0000;
const MAGIC: &[u8; 4] = b"CB06";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleBootError(pub &'static str);
impl fmt::Display for SingleBootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for SingleBootError {}

#[derive(Debug, Clone)]
pub struct BootResource {
    pub address: usize,
    pub data: Vec<u8>,
    pub zero_bytes: usize,
}
impl BootResource {
    pub fn copy(address: usize, mut data: Vec<u8>) -> Self {
        if data.len() & 1 != 0 {
            data.push(0);
        }
        Self {
            address,
            data,
            zero_bytes: 0,
        }
    }
    pub fn zero(address: usize, length: usize) -> Self {
        Self {
            address,
            data: Vec::new(),
            zero_bytes: length,
        }
    }
    fn length(&self) -> usize {
        if self.zero_bytes != 0 {
            self.zero_bytes
        } else {
            self.data.len()
        }
    }
}

#[derive(Debug, Clone)]
pub struct BootProject {
    pub application: PathBuf,
    /// (symbol, physical byte address, source path); paths are project-relative.
    pub copies: Vec<(String, usize, PathBuf)>,
    pub zeros: Vec<(usize, usize)>,
}
fn number(text: &str) -> Result<usize, SingleBootError> {
    let value = if let Some(hex) = text.strip_prefix("0x") {
        usize::from_str_radix(hex, 16)
    } else {
        text.parse()
    };
    value.map_err(|_| SingleBootError("invalid byte address or length"))
}
fn relative_source(text: &str) -> Result<PathBuf, SingleBootError> {
    let p = PathBuf::from(text);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(SingleBootError("source must be a normal relative path"));
    }
    Ok(p)
}
pub fn parse_boot_project(text: &str) -> Result<BootProject, SingleBootError> {
    let (mut format, mut application, mut copies, mut zeros) =
        (false, None, Vec::new(), Vec::new());
    for line in text
        .lines()
        .map(|l| l.split('#').next().unwrap().trim())
        .filter(|l| !l.is_empty())
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["format", "2"] if !format => format = true,
            ["application", p] if application.is_none() => {
                let p = relative_source(p)?;
                if p.extension().and_then(|x| x.to_str()) != Some("rs") {
                    return Err(SingleBootError("application must be a relative .rs source"));
                }
                application = Some(p);
            }
            ["copy", name, address, path] => {
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
                    || name.as_bytes()[0].is_ascii_digit()
                    || copies.iter().any(|(n, _, _)| n == name)
                {
                    return Err(SingleBootError(
                        "copy symbol must be unique upper snake case",
                    ));
                }
                copies.push((name.to_string(), number(address)?, relative_source(path)?));
            }
            ["zero", address, length] => zeros.push((number(address)?, number(length)?)),
            _ => {
                return Err(SingleBootError(
                    "expected format 2, application, copy NAME ADDRESS PATH or zero ADDRESS LENGTH",
                ))
            }
        }
    }
    if !format {
        return Err(SingleBootError("missing project format"));
    }
    Ok(BootProject {
        application: application.ok_or(SingleBootError("missing application source"))?,
        copies,
        zeros,
    })
}
pub fn single_application_source(text: &str) -> Result<PathBuf, SingleBootError> {
    Ok(parse_boot_project(text)?.application)
}
impl BootProject {
    pub fn resources(&self, root: &Path) -> Result<Vec<BootResource>, String> {
        let mut result = Vec::new();
        for (_, address, path) in &self.copies {
            let path = root.join(path);
            result.push(BootResource::copy(
                *address,
                std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?,
            ));
        }
        result.extend(self.zeros.iter().map(|&(a, n)| BootResource::zero(a, n)));
        Ok(result)
    }
    pub fn constants(&self) -> String {
        self.copies.iter().map(|(n,a,_)|format!(
            "pub const {n}_BYTE_ADDRESS_LOW: u16 = {};\npub const {n}_BYTE_ADDRESS_HIGH: u16 = {};\npub const {n}_WORD_ADDRESS_LOW: u16 = {};\npub const {n}_WORD_ADDRESS_HIGH: u16 = {};\n",
            a&0xffff,a>>16,(a/2)&0xffff,(a/2)>>16)).collect()
    }
}
fn command_range(address: usize, length: usize) -> Result<std::ops::Range<usize>, SingleBootError> {
    let end = address
        .checked_add(length)
        .ok_or(SingleBootError("DRAM address overflow"))?;
    if length == 0 || (address | length) & 1 != 0 || end > BOOT_MEMORY_BYTES {
        return Err(SingleBootError("nonempty even command must fit DRAM"));
    }
    if address < BOOT_DEPTH_BYTES.end && end > BOOT_DEPTH_BYTES.start {
        return Err(SingleBootError("boot cannot write Depth"));
    }
    Ok(address..end)
}
fn resource_range(resource: &BootResource) -> Result<std::ops::Range<usize>, SingleBootError> {
    if resource.zero_bytes != 0 && !resource.data.is_empty() {
        return Err(SingleBootError("ambiguous resource operation"));
    }
    let r = command_range(resource.address, resource.length())?;
    if ![
        (0x20000, 0xe0000),
        (0x100000, 0x400000),
        (
            usize::from(driver_layout::GPU_ASSET_FIRST_SECTOR) * 128,
            usize::from(driver_layout::GPU_ASSET_END_SECTOR) * 128,
        ),
    ]
    .into_iter()
    .any(|(a, b)| r.start >= a && r.end <= b)
    {
        return Err(SingleBootError("resource overlaps a protected region"));
    }
    Ok(r)
}
fn append_command(image: &mut Vec<u8>, address: usize, length: usize, zero: bool) {
    image.extend_from_slice(&(address as u32).to_le_bytes());
    image.extend_from_slice(&((length as u32) | if zero { 0x8000_0000 } else { 0 }).to_le_bytes());
}
pub fn pack_application_resources(
    code: &[u8],
    static_words: usize,
    resources: &[BootResource],
) -> Result<Vec<u8>, SingleBootError> {
    if static_words > SINGLE_BOOT_STATIC_WORDS {
        return Err(SingleBootError("statics exceed the 16-KiB arena"));
    }
    if code.is_empty() || code.len() & 1 != 0 || code.len() > SINGLE_BOOT_MAX_CODE_BYTES {
        return Err(SingleBootError(
            "code length must be nonzero, even, and at most 128 KiB",
        ));
    }
    let mut ranges = resources
        .iter()
        .map(resource_range)
        .collect::<Result<Vec<_>, _>>()?;
    ranges.sort_by_key(|r| r.start);
    if ranges.windows(2).any(|r| r[0].end > r[1].start) {
        return Err(SingleBootError("overlapping resources"));
    }
    let size = 4usize
        .checked_add((2 + usize::from(static_words != 0)) * 8 + code.len())
        .and_then(|n| {
            resources
                .iter()
                .try_fold(n, |n, r| n.checked_add(8)?.checked_add(r.data.len()))
        })
        .ok_or(SingleBootError("image size overflow"))?;
    if size > BOOT_MEMORY_BYTES - SINGLE_BOOT_FLASH_BASE {
        return Err(SingleBootError("image exceeds Flash"));
    }
    let mut image = Vec::with_capacity(size);
    image.extend_from_slice(MAGIC);
    if static_words != 0 {
        append_command(&mut image, 0, static_words * 2, true);
    }
    append_command(&mut image, SINGLE_BOOT_CODE_WORD * 2, code.len(), false);
    image.extend_from_slice(code);
    for r in resources {
        append_command(&mut image, r.address, r.length(), r.zero_bytes != 0);
        image.extend_from_slice(&r.data);
    }
    append_command(&mut image, 0, 0, false);
    Ok(image)
}
pub fn pack_single_application(
    code: &[u8],
    static_words: usize,
) -> Result<Vec<u8>, SingleBootError> {
    pack_application_resources(code, static_words, &[])
}
#[derive(Debug, Clone)]
pub struct BootCommand<'a> {
    pub address: usize,
    pub length: usize,
    pub payload: Option<&'a [u8]>,
}
/// Bounded functional decoder for the hardware's small checks, without a checksum.
pub fn single_boot_commands(package: &[u8]) -> Result<Vec<BootCommand<'_>>, SingleBootError> {
    if package.get(..4) != Some(MAGIC.as_slice()) {
        return Err(SingleBootError("invalid CB06 signature"));
    }
    if package.len() > BOOT_MEMORY_BYTES - SINGLE_BOOT_FLASH_BASE {
        return Err(SingleBootError("image exceeds Flash"));
    }
    let mut cursor = 4;
    let mut commands = Vec::new();
    // Every iteration consumes at least one complete command.
    while cursor < package.len() {
        let bytes = package
            .get(cursor..cursor + 8)
            .ok_or(SingleBootError("truncated command"))?;
        cursor += 8;
        let address = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        let control = u32::from_le_bytes(bytes[4..].try_into().unwrap());
        if address == 0 && control == 0 {
            if cursor != package.len() {
                return Err(SingleBootError("trailing bytes after END"));
            }
            return Ok(commands);
        }
        let length = (control & 0x7fff_ffff) as usize;
        command_range(address, length)?;
        let payload = if control >> 31 == 0 {
            let end = cursor
                .checked_add(length)
                .ok_or(SingleBootError("payload overflow"))?;
            let data = package
                .get(cursor..end)
                .ok_or(SingleBootError("truncated payload"))?;
            cursor = end;
            Some(data)
        } else {
            None
        };
        commands.push(BootCommand {
            address,
            length,
            payload,
        });
    }
    Err(SingleBootError("missing END"))
}
pub fn single_boot_code_length(package: &[u8]) -> Result<usize, SingleBootError> {
    single_boot_commands(package)?
        .into_iter()
        .find(|c| c.address == SINGLE_BOOT_CODE_WORD * 2 && c.payload.is_some())
        .map(|c| c.length)
        .ok_or(SingleBootError("missing code"))
}
pub fn single_boot_code(package: &[u8]) -> Result<&[u8], SingleBootError> {
    single_boot_commands(package)?
        .into_iter()
        .find(|c| c.address == SINGLE_BOOT_CODE_WORD * 2)
        .and_then(|c| c.payload)
        .ok_or(SingleBootError("missing code"))
}
pub fn boot_image_map(code: &[u8], image: &[u8]) -> String {
    let static_bytes = single_boot_commands(image)
        .expect("packer commands")
        .iter()
        .find(|c| c.address == 0 && c.payload.is_none())
        .map_or(0, |c| c.length);
    let mut map=format!("format=6\nsignature=CB06\ncode_segment=7\nentry=0\ndata_pages=0,1,2,3\nstack=0xc000\ncode_bytes={}\nstatic_zero_bytes={}\nflash_base=0x100000\nimage_bytes={}\n",code.len(),static_bytes,image.len());
    for c in single_boot_commands(image).expect("packer commands") {
        map.push_str(&format!(
            "{} byte_address=0x{:06x} bytes={}\n",
            if c.payload.is_some() { "COPY" } else { "ZERO" },
            c.address,
            c.length
        ));
    }
    map
}
/// Functional completion is synchronous here; RTL waits for each real response.
pub fn install_single_application(
    package: &[u8],
    memory: &mut [u16],
) -> Result<usize, SingleBootError> {
    let commands = single_boot_commands(package)?;
    let code_words = single_boot_code_length(package)? / 2;
    for c in &commands {
        if c.address / 2 + c.length / 2 > memory.len() {
            return Err(SingleBootError("insufficient physical memory"));
        }
    }
    for c in commands {
        let target = &mut memory[c.address / 2..(c.address + c.length) / 2];
        if let Some(data) = c.payload {
            for (w, p) in target.iter_mut().zip(data.as_chunks::<2>().0) {
                *w = u16::from_le_bytes([p[0], p[1]]);
            }
        } else {
            target.fill(0);
        }
    }
    Ok(code_words)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_zero_uses_the_allocated_prefix_and_keeps_the_arena_protected() {
        for words in [0, 1, 17, SINGLE_BOOT_STATIC_WORDS] {
            let image = pack_single_application(&[0x34, 0x12], words).unwrap();
            let mut memory = vec![0xd00d; BOOT_MEMORY_BYTES / 2];
            install_single_application(&image, &mut memory).unwrap();
            assert!(memory[..words].iter().all(|w| *w == 0));
            assert!(memory[words..SINGLE_BOOT_STATIC_WORDS + 1]
                .iter()
                .all(|w| *w == 0xd00d));
            let commands = single_boot_commands(&image).unwrap();
            assert_eq!(commands.len(), 1 + usize::from(words != 0));
            assert!(boot_image_map(&[0x34, 0x12], &image)
                .contains(&format!("static_zero_bytes={}\n", words * 2)));
            assert!(pack_application_resources(
                &[0, 0],
                words,
                &[BootResource::copy(0x3ffe, vec![1, 2])]
            )
            .is_err());
        }
        for words in [SINGLE_BOOT_STATIC_WORDS + 1, usize::MAX] {
            assert!(pack_single_application(&[0, 0], words).is_err());
        }
    }
    #[test]
    fn multi_megabyte_copy_zero_and_protected_holes() {
        let code = [0x34, 0x12, 0xcd, 0xab];
        let data: Vec<u8> = (0..0x20_0000).map(|i| (i ^ (i >> 8)) as u8).collect();
        let resources = [
            BootResource::copy(0x100000, data.clone()),
            BootResource::zero(0x4b0000, 34),
        ];
        let image = pack_application_resources(&code, 17, &resources).unwrap();
        let mut memory = vec![0xa55a; BOOT_MEMORY_BYTES / 2];
        assert_eq!(install_single_application(&image, &mut memory).unwrap(), 2);
        assert!(memory[..17].iter().all(|w| *w == 0));
        assert_eq!(memory[17], 0xa55a);
        assert_eq!(
            &memory[SINGLE_BOOT_CODE_WORD..SINGLE_BOOT_CODE_WORD + 2],
            &[0x1234, 0xabcd]
        );
        for (word, pair) in memory[0x80000..0x180000]
            .iter()
            .zip(data.as_chunks::<2>().0)
        {
            assert_eq!(*word, u16::from_le_bytes(*pair));
        }
        assert!(memory[0x200000..0x230000].iter().all(|w| *w == 0xa55a));
        assert!(memory[0x230000..0x248000].iter().all(|w| *w == 0xa55a));
        assert!(memory[0x248000..0x258000].iter().all(|w| *w == 0xa55a));
        assert!(memory[0x258000..0x258011].iter().all(|w| *w == 0));
        assert_eq!(memory[0x258011], 0xa55a);
        assert_eq!(
            pack_single_application(&code, 17).unwrap().len(),
            4 + 3 * 8 + code.len()
        );
    }
    #[test]
    fn host_rejects_bad_layout_and_truncation_but_not_changed_payload() {
        for a in [
            0,
            0x4000,
            0xe0000,
            0x400000,
            0x460000,
            0x48fffe,
            0x490000,
            0x498000,
            0x4a0000,
            0x4a8000,
            0x4afffe,
            0x7df800,
            0x7dfc00,
            0x7e0000,
            0x7f0000,
            0x7ffffe,
            0x800000,
            usize::MAX,
        ] {
            assert!(
                pack_application_resources(&[0, 0], 0, &[BootResource::copy(a, vec![1, 2])])
                    .is_err()
            );
        }
        assert!(pack_application_resources(&[0, 0], 0, &[BootResource::zero(0x20000, 3)]).is_err());
        let end = usize::from(driver_layout::GPU_ASSET_END_SECTOR) * 128;
        assert!(pack_application_resources(&[0, 0], 0, &[BootResource::zero(end - 2, 2)]).is_ok());
        assert!(pack_application_resources(&[0, 0], 0, &[BootResource::zero(end - 2, 4)]).is_err());
        assert!(pack_application_resources(
            &[0, 0],
            0,
            &[
                BootResource::zero(0x20000, 4),
                BootResource::zero(0x20002, 4)
            ]
        )
        .is_err());
        let mut image = pack_single_application(&[0x34, 0x12], 17).unwrap();
        for i in 0..image.len() {
            assert!(single_boot_commands(&image[..i]).is_err());
        }
        for i in 0..4 {
            let mut bad = image.clone();
            bad[i] ^= 1;
            assert!(single_boot_commands(&bad).is_err());
        }
        image[20] ^= 1; // First payload byte; no production checksum.
        assert!(single_boot_commands(&image).is_ok());
        for n in [0, 1, SINGLE_BOOT_MAX_CODE_BYTES + 2] {
            assert!(pack_single_application(&vec![0; n], 0).is_err());
        }
        assert!(pack_single_application(&vec![0; SINGLE_BOOT_MAX_CODE_BYTES], 0).is_ok());
        assert_eq!(BootResource::copy(0x20000, vec![1]).data, vec![1, 0]);
    }
    #[test]
    fn project_resource_constants_and_one_application() {
        let p = parse_boot_project(
            "format 2\napplication rcc/a.rs\ncopy MESH 0x100000 assets/m.bin\nzero 0x4b0000 32",
        )
        .unwrap();
        assert!(p.constants().contains("MESH_WORD_ADDRESS_HIGH: u16 = 8;"));
        assert_eq!(p.zeros, vec![(0x4b0000, 32)]);
        assert_eq!(
            single_application_source("format 2\napplication rcc/a.rs").unwrap(),
            PathBuf::from("rcc/a.rs")
        );
        for bad in [
            "format 1\napplication a.rs",
            "format 2\napplication ../a.rs",
            "format 2\napplication a.rs\napplication b.rs",
            "format 2\napplication a.rs\ncopy 1BAD 0 a.bin",
        ] {
            assert!(parse_boot_project(bad).is_err());
        }
    }
}
