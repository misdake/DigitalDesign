//! Declarative selection and derived memory layout for the two FPGA applications.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use super::BootEntry;
use crate::PhysicalWordAddress;

pub const BOOT_APPLICATION_PROJECT_FORMAT_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootApplicationProject {
    pub s1_source: PathBuf,
    pub s2_source: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationLayout {
    pub section_name: &'static str,
    pub asset_name: &'static str,
    pub entry: BootEntry,
}

impl ApplicationLayout {
    pub const fn destination(self) -> PhysicalWordAddress {
        self.entry.physical_entry()
    }
}

/// Reset-time selection `01` (the S1 button held during reset) uses this
/// slider-diagnostic slot.
pub const S1_APPLICATION_LAYOUT: ApplicationLayout = ApplicationLayout {
    section_name: "application-s1",
    asset_name: "application-s1.v3bin",
    entry: BootEntry {
        code_segment: 3,
        offset: 0x0200,
        data_segment: 4,
        stack_offset: 0xe000,
    },
};

/// The board-level selection latch powers up at `10`, so this display
/// application is the default. Data segment zero is retained for programs that
/// temporarily switch DSEG for framebuffer stores and restore the reset
/// segment afterward.
pub const S2_APPLICATION_LAYOUT: ApplicationLayout = ApplicationLayout {
    section_name: "application-s2",
    asset_name: "application-s2.v3bin",
    entry: BootEntry {
        code_segment: 7,
        offset: 0x0200,
        data_segment: 0,
        stack_offset: 0xf000,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectConfigError {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ProjectConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "boot application config line {line}: {}", self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for ProjectConfigError {}

impl BootApplicationProject {
    pub fn parse(text: &str) -> Result<Self, ProjectConfigError> {
        let mut version = None;
        let mut s1_source = None;
        let mut s2_source = None;

        for (index, raw_line) in text.lines().enumerate() {
            let line_number = index + 1;
            let line = raw_line.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 2 {
                return Err(error(
                    line_number,
                    format!("`{}` expects exactly one value", fields[0]),
                ));
            }
            match fields[0] {
                "format" => {
                    if fields[1] != BOOT_APPLICATION_PROJECT_FORMAT_VERSION.to_string() {
                        return Err(error(
                            line_number,
                            format!(
                                "unsupported format {}; expected {}",
                                fields[1], BOOT_APPLICATION_PROJECT_FORMAT_VERSION
                            ),
                        ));
                    }
                    set_once(&mut version, (), "format", line_number)?;
                }
                "s1" => set_once(
                    &mut s1_source,
                    source_path(fields[1], line_number)?,
                    "s1",
                    line_number,
                )?,
                "s2" => set_once(
                    &mut s2_source,
                    source_path(fields[1], line_number)?,
                    "s2",
                    line_number,
                )?,
                directive => {
                    return Err(error(
                        line_number,
                        format!("unknown directive `{directive}`"),
                    ))
                }
            }
        }

        required(version, "format")?;
        let project = Self {
            s1_source: required(s1_source, "s1")?,
            s2_source: required(s2_source, "s2")?,
        };
        if project.s1_source == project.s2_source {
            return Err(ProjectConfigError {
                line: None,
                message: "s1 and s2 must name two distinct source programs".into(),
            });
        }
        Ok(project)
    }
}

fn source_path(value: &str, line: usize) -> Result<PathBuf, ProjectConfigError> {
    let path = Path::new(value);
    if path.extension().and_then(|value| value.to_str()) != Some("rs") {
        return Err(error(line, "application source must be a .rs file"));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return Err(error(
            line,
            "application source must be a relative path inside the system project",
        ));
    }
    Ok(path.to_owned())
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    name: &str,
    line: usize,
) -> Result<(), ProjectConfigError> {
    if slot.replace(value).is_some() {
        Err(error(line, format!("duplicate `{name}` directive")))
    } else {
        Ok(())
    }
}

fn required<T>(slot: Option<T>, name: &str) -> Result<T, ProjectConfigError> {
    slot.ok_or_else(|| ProjectConfigError {
        line: None,
        message: format!("missing required `{name}` directive"),
    })
}

fn error(line: usize, message: impl Into<String>) -> ProjectConfigError {
    ProjectConfigError {
        line: Some(line),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exactly_two_application_sources() {
        let project =
            BootApplicationProject::parse("format 1\ns1 rcc/boot-demo.rs\ns2 rcc/boot-alt.rs\n")
                .unwrap();
        assert_eq!(project.s1_source, Path::new("rcc/boot-demo.rs"));
        assert_eq!(project.s2_source, Path::new("rcc/boot-alt.rs"));
    }

    #[test]
    fn rejects_missing_duplicate_or_external_sources() {
        assert!(BootApplicationProject::parse("format 1\ns1 rcc/a.rs\n").is_err());
        assert!(
            BootApplicationProject::parse("format 1\ns1 rcc/a.rs\ns1 rcc/b.rs\ns2 rcc/c.rs\n")
                .is_err()
        );
        assert!(BootApplicationProject::parse("format 1\ns1 ../a.rs\ns2 rcc/b.rs\n").is_err());
    }

    #[test]
    fn derived_slots_are_disjoint_and_keep_existing_entries() {
        assert_ne!(
            S1_APPLICATION_LAYOUT.destination(),
            S2_APPLICATION_LAYOUT.destination()
        );
        assert_eq!(S1_APPLICATION_LAYOUT.entry.code_segment, 3);
        assert_eq!(S2_APPLICATION_LAYOUT.entry.code_segment, 7);
        assert_eq!(S2_APPLICATION_LAYOUT.entry.data_segment, 0);
    }
}
