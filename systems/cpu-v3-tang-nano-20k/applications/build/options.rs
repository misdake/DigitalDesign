//! One declaration drives CLI validation, build dependencies and optional RCC constants.
use std::path::Path;
#[derive(Clone, Copy)]
pub struct BuildOption {
    pub flag: &'static str,
    pub variable: &'static str,
    pub values: OptionValues,
    constant_default: Option<u16>,
}
#[derive(Clone, Copy)]
pub enum OptionValues {
    Choices(&'static [&'static str]),
    Integer { minimum: u16, maximum: u16 },
    InputFile,
}
impl BuildOption {
    pub const fn environment(
        flag: &'static str,
        variable: &'static str,
        values: OptionValues,
    ) -> Self {
        Self {
            flag,
            variable,
            values,
            constant_default: None,
        }
    }
    pub const fn integer_constant(
        flag: &'static str,
        name: &'static str,
        default: u16,
        minimum: u16,
        maximum: u16,
    ) -> Self {
        assert!(minimum <= default && default <= maximum);
        Self {
            flag,
            variable: name,
            values: OptionValues::Integer { minimum, maximum },
            constant_default: Some(default),
        }
    }
    pub fn default_value(self) -> Option<u16> {
        self.constant_default
    }
    pub fn validate(self, value: &str) -> Result<String, String> {
        let flag = self.flag;
        match self.values {
            OptionValues::Integer { minimum, maximum } => {
                let number = value
                    .parse::<u16>()
                    .map_err(|_| format!("--{flag} expects {minimum}..{maximum}"))?;
                if !(minimum..=maximum).contains(&number) {
                    return Err(format!("--{flag} expects {minimum}..{maximum}"));
                }
                Ok(number.to_string())
            }
            OptionValues::Choices(choices) => {
                if !choices.contains(&value) {
                    return Err(format!("--{flag} expects {}", choices.join("|")));
                }
                Ok(value.into())
            }
            OptionValues::InputFile => {
                let path = Path::new(value)
                    .canonicalize()
                    .map_err(|error| format!("--{flag}: {error}"))?;
                if !path.is_file() {
                    return Err(format!("--{flag} needs a file"));
                }
                Ok(path.to_string_lossy().into_owned())
            }
        }
    }
    pub(super) fn constant_source(self, value: Option<&str>) -> Result<String, String> {
        let value = value.map(|v| self.validate(v)).transpose()?;
        Ok(match self.constant_default {
            Some(default) => format!(
                "pub const {}:u16={};\n",
                self.variable,
                value.unwrap_or_else(|| default.to_string())
            ),
            None => String::new(),
        })
    }
}
pub const COMMON_OPTIONS: &[BuildOption] = &[
    BuildOption::environment(
        "hook",
        "GPU_PROFILER_HOOK",
        OptionValues::Choices(&["0", "1"]),
    ),
    BuildOption::environment(
        "gpu-debug",
        "RCC_GPU_DEBUG",
        OptionValues::Choices(&["0", "1"]),
    ),
    BuildOption::environment("manifest", "GPU_PROFILER_MANIFEST", OptionValues::InputFile),
];
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_declaration_checks_cli_and_direct_build_values_and_emits_defaults() {
        let option = BuildOption::integer_constant("page", "PAGE", 0, 0, 5);
        assert_eq!(
            option.constant_source(None).unwrap(),
            "pub const PAGE:u16=0;\n"
        );
        assert_eq!(
            option.constant_source(Some("05")).unwrap(),
            "pub const PAGE:u16=5;\n"
        );
        for bad in ["6", "-1", "65536", "no"] {
            assert_eq!(
                option.validate(bad).unwrap_err(),
                option.constant_source(Some(bad)).unwrap_err()
            );
        }
        let option =
            BuildOption::environment("scene", "SCENE", OptionValues::Choices(&["micro", "full"]));
        assert_eq!(option.constant_source(None).unwrap(), "");
        assert!(option.constant_source(Some("full")).unwrap().is_empty());
        assert!(option.constant_source(Some("typo")).is_err());
    }
}
