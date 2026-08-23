//! Dependency-free input manifest for the `cpu-v3-pack` host tool.

use std::fmt;
use std::path::{Path, PathBuf};

use super::{
    BootEntry, BootImageSpec, BootTarget, InputSection, SectionKind, SECTION_EXECUTE, SECTION_READ,
    SECTION_WRITE,
};
use crate::PhysicalWordAddress;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackManifest {
    pub target: BootTarget,
    pub stage1_section: String,
    pub stage1_entry: BootEntry,
    pub application_entry: BootEntry,
    pub sections: Vec<ManifestSection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestSection {
    pub name: String,
    pub kind: SectionKind,
    pub flags: u16,
    pub destination: PhysicalWordAddress,
    pub alignment_bytes: u32,
    pub memory_size_bytes: u32,
    pub source: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestError {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "manifest line {line}: {}", self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for ManifestError {}

impl PackManifest {
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let mut version = None;
        let mut target = None;
        let mut stage1_section = None;
        let mut stage1_entry = None;
        let mut application_entry = None;
        let mut sections = vec![];

        for (index, raw_line) in text.lines().enumerate() {
            let line_number = index + 1;
            let line = raw_line.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let command = fields[0];
            match command {
                "format" => {
                    expect_fields(&fields, 2, line_number)?;
                    let value = number(fields[1], line_number)?;
                    if value != 1 {
                        return Err(error(
                            line_number,
                            format!("unsupported manifest format {value}; expected 1"),
                        ));
                    }
                    set_once(&mut version, 1, "format", line_number)?;
                }
                "target" => {
                    expect_fields(&fields, 2, line_number)?;
                    let value = match fields[1] {
                        "tang-nano-20k" => BootTarget::TangNano20K,
                        value => {
                            return Err(error(line_number, format!("unsupported target `{value}`")))
                        }
                    };
                    set_once(&mut target, value, "target", line_number)?;
                }
                "stage1-section" => {
                    expect_fields(&fields, 2, line_number)?;
                    set_once(
                        &mut stage1_section,
                        fields[1].to_owned(),
                        "stage1-section",
                        line_number,
                    )?;
                }
                "stage1-entry" => {
                    expect_fields(&fields, 5, line_number)?;
                    set_once(
                        &mut stage1_entry,
                        parse_entry(&fields[1..], line_number)?,
                        "stage1-entry",
                        line_number,
                    )?;
                }
                "application-entry" => {
                    expect_fields(&fields, 5, line_number)?;
                    set_once(
                        &mut application_entry,
                        parse_entry(&fields[1..], line_number)?,
                        "application-entry",
                        line_number,
                    )?;
                }
                "load" => {
                    expect_fields(&fields, 7, line_number)?;
                    sections.push(ManifestSection {
                        name: fields[1].to_owned(),
                        kind: SectionKind::Load,
                        destination: PhysicalWordAddress::new(number(fields[2], line_number)?),
                        flags: flags(fields[3], line_number)?,
                        alignment_bytes: number(fields[4], line_number)?,
                        memory_size_bytes: number(fields[5], line_number)?,
                        source: Some(PathBuf::from(fields[6])),
                    });
                }
                "zero" => {
                    expect_fields(&fields, 6, line_number)?;
                    sections.push(ManifestSection {
                        name: fields[1].to_owned(),
                        kind: SectionKind::Zero,
                        destination: PhysicalWordAddress::new(number(fields[2], line_number)?),
                        flags: flags(fields[3], line_number)?,
                        alignment_bytes: number(fields[4], line_number)?,
                        memory_size_bytes: number(fields[5], line_number)?,
                        source: None,
                    });
                }
                value => {
                    return Err(error(
                        line_number,
                        format!("unknown manifest directive `{value}`"),
                    ))
                }
            }
        }

        required(version, "format")?;
        Ok(Self {
            target: required(target, "target")?,
            stage1_section: required(stage1_section, "stage1-section")?,
            stage1_entry: required(stage1_entry, "stage1-entry")?,
            application_entry: required(application_entry, "application-entry")?,
            sections,
        })
    }

    pub fn load(self, base: &Path) -> Result<BootImageSpec, ManifestError> {
        let mut sections = Vec::with_capacity(self.sections.len());
        for section in self.sections {
            let data = match &section.source {
                Some(path) => {
                    let path = base.join(path);
                    std::fs::read(&path).map_err(|source| ManifestError {
                        line: None,
                        message: format!(
                            "cannot read section `{}` from {}: {source}",
                            section.name,
                            path.display()
                        ),
                    })?
                }
                None => vec![],
            };
            sections.push(InputSection {
                name: section.name,
                kind: section.kind,
                flags: section.flags,
                destination: section.destination,
                data,
                memory_size_bytes: section.memory_size_bytes,
                alignment_bytes: section.alignment_bytes,
            });
        }
        Ok(BootImageSpec {
            target: self.target,
            stage1_section: self.stage1_section,
            stage1_entry: self.stage1_entry,
            application_entry: self.application_entry,
            sections,
        })
    }
}

fn parse_entry(fields: &[&str], line: usize) -> Result<BootEntry, ManifestError> {
    Ok(BootEntry {
        code_segment: word(fields[0], line)?,
        offset: word(fields[1], line)?,
        data_segment: word(fields[2], line)?,
        stack_offset: word(fields[3], line)?,
    })
}

fn flags(value: &str, line: usize) -> Result<u16, ManifestError> {
    let mut result = 0;
    for flag in value.bytes() {
        let bit = match flag {
            b'r' => SECTION_READ,
            b'w' => SECTION_WRITE,
            b'x' => SECTION_EXECUTE,
            _ => {
                return Err(error(
                    line,
                    format!("invalid section flag `{}` in `{value}`", char::from(flag)),
                ))
            }
        };
        if result & bit != 0 {
            return Err(error(line, format!("duplicate section flag in `{value}`")));
        }
        result |= bit;
    }
    Ok(result)
}

fn number(value: &str, line: usize) -> Result<u32, ManifestError> {
    let parsed = value
        .strip_prefix("0x")
        .map_or_else(|| value.parse(), |hex| u32::from_str_radix(hex, 16));
    parsed.map_err(|_| error(line, format!("invalid u32 number `{value}`")))
}

fn word(value: &str, line: usize) -> Result<u16, ManifestError> {
    let value = number(value, line)?;
    u16::try_from(value).map_err(|_| error(line, format!("value {value:#x} exceeds u16")))
}

fn expect_fields(fields: &[&str], count: usize, line: usize) -> Result<(), ManifestError> {
    if fields.len() == count {
        Ok(())
    } else {
        Err(error(
            line,
            format!(
                "directive `{}` expects {} fields, found {}",
                fields[0],
                count - 1,
                fields.len() - 1
            ),
        ))
    }
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    name: &str,
    line: usize,
) -> Result<(), ManifestError> {
    if slot.replace(value).is_some() {
        Err(error(line, format!("duplicate `{name}` directive")))
    } else {
        Ok(())
    }
}

fn required<T>(value: Option<T>, name: &str) -> Result<T, ManifestError> {
    value.ok_or_else(|| ManifestError {
        line: None,
        message: format!("manifest is missing required `{name}` directive"),
    })
}

fn error(line: usize, message: String) -> ManifestError {
    ManifestError {
        line: Some(line),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = "\
format 1
target tang-nano-20k
stage1-section loader
stage1-entry 0x1 0x100 0x2 0xf000
application-entry 3 0x200 4 0xe000
load loader 0x10100 rx 32 64 stage1.bin
load code 0x30200 rx 32 4 game.bin
zero bss 0x44000 rw 32 128
";

    #[test]
    fn parses_the_dependency_free_manifest_format() {
        let parsed = PackManifest::parse(EXAMPLE).unwrap();
        assert_eq!(parsed.stage1_section, "loader");
        assert_eq!(parsed.stage1_entry.code_segment, 1);
        assert_eq!(parsed.application_entry.offset, 0x200);
        assert_eq!(parsed.sections.len(), 3);
        assert_eq!(parsed.sections[0].flags, SECTION_READ | SECTION_EXECUTE);
        assert_eq!(parsed.sections[2].kind, SectionKind::Zero);
    }

    #[test]
    fn diagnostics_include_the_source_line() {
        let error = PackManifest::parse("format 1\ntarget mystery\n").unwrap_err();
        assert_eq!(error.line, Some(2));
        assert!(error.to_string().contains("unsupported target `mystery`"));
    }
}
