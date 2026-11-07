//! Build one explicit application snapshot through the authoritative build script.
use cpu_v3_tools::applications::{self, OptionValues};
use std::{
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

const HELP: &str = "cpu-v3-app list\ncpu-v3-app build APPLICATION --out NEW-DIRECTORY [--gpu-debug 0|1] [--manifest FILE] [application options]";
fn main() -> ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cpu-v3-app: {error}");
            ExitCode::FAILURE
        }
    }
}
fn run(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    match args.next().as_deref() {
        None | Some("--help" | "-h") => {
            println!("{HELP}");
            Ok(())
        }
        Some("list") => {
            if args.next().is_some() {
                return Err("list takes no arguments".into());
            }
            for definition in applications::definitions() {
                println!("{}", definition.name);
            }
            Ok(())
        }
        Some("build") => {
            let name = args.next().ok_or(HELP)?;
            let definition = applications::find(&name)?;
            let mut output = None;
            let mut settings = std::collections::BTreeMap::new();
            let inherited_hook = std::env::var("GPU_PROFILER_HOOK").ok();
            while let Some(flag) = args.next() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    println!("cpu-v3-app build {} --out NEW-DIRECTORY", definition.name);
                    for option in applications::COMMON_OPTIONS
                        .iter()
                        .chain(definition.options)
                    {
                        let values = match option.values {
                            OptionValues::Choices(values) => values.join("|"),
                            OptionValues::Integer { minimum, maximum } => {
                                format!("{minimum}..{maximum}")
                            }
                            OptionValues::InputFile => "FILE".into(),
                        };
                        let hook = settings
                            .get("GPU_PROFILER_HOOK")
                            .map(String::as_str)
                            .or(inherited_hook.as_deref());
                        let required = if option.variable == "GPU_PROFILER_MANIFEST"
                            && definition.capture.requires_manifest(hook)
                        {
                            " (required with hook)"
                        } else {
                            ""
                        };
                        println!("  --{} {values}{required}", option.flag);
                    }
                    return Ok(());
                }
                if flag == "--out" {
                    if output.is_some() {
                        return Err("duplicate --out".into());
                    }
                    output = Some(PathBuf::from(args.next().ok_or("--out needs a directory")?));
                    continue;
                }
                let option = applications::COMMON_OPTIONS
                    .iter()
                    .chain(definition.options)
                    .find(|option| flag.strip_prefix("--") == Some(option.flag))
                    .ok_or_else(|| format!("unknown option {flag} for {}", definition.name))?;
                let mut value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
                match option.values {
                    OptionValues::Integer { minimum, maximum } => {
                        let number = value
                            .parse::<u16>()
                            .map_err(|_| format!("{flag} expects {minimum}..{maximum}"))?;
                        if !(minimum..=maximum).contains(&number) {
                            return Err(format!("{flag} expects {minimum}..{maximum}"));
                        }
                        value = number.to_string();
                    }
                    OptionValues::Choices(choices) => {
                        if !choices.contains(&value.as_str()) {
                            return Err(format!("{flag} expects {}", choices.join("|")));
                        }
                    }
                    OptionValues::InputFile => {
                        let path = Path::new(&value)
                            .canonicalize()
                            .map_err(|error| format!("{flag}: {error}"))?;
                        if !path.is_file() {
                            return Err(format!("{flag} needs a file"));
                        }
                        value = path.to_string_lossy().into_owned();
                    }
                }
                if settings.insert(option.variable, value).is_some() {
                    return Err(format!("duplicate {flag}"));
                }
            }
            let hook = settings
                .get("GPU_PROFILER_HOOK")
                .map(String::as_str)
                .or(inherited_hook.as_deref());
            if definition.capture.requires_manifest(hook)
                && !settings.contains_key("GPU_PROFILER_MANIFEST")
                && !std::env::var_os("GPU_PROFILER_MANIFEST").is_some_and(|value| !value.is_empty())
            {
                return Err(format!(
                    "{} requires --manifest FILE with its capture hook enabled",
                    definition.name
                ));
            }
            let output = std::path::absolute(output.ok_or("build requires --out NEW-DIRECTORY")?)
                .map_err(|error| error.to_string())?;
            cpu_v3_tools::require_empty_output(&output)?;
            let system = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let repository = system
                .join("../..")
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let mut build = Command::new("cargo");
            build
                .current_dir(repository)
                .args([
                    "run",
                    "--release",
                    "-p",
                    "cpu-v3-application",
                    "--bin",
                    "cpu-v3-boot-assets",
                    "--",
                ])
                .arg(&output)
                .envs(settings);
            if let Some(selection) = definition.selection {
                build.env("CPU_V3_BOOT_EXAMPLE", selection);
            } else {
                build.env_remove("CPU_V3_BOOT_EXAMPLE");
            }
            let status = build
                .status()
                .map_err(|error| format!("start application build: {error}"))?;
            if !status.success() {
                return Err(format!("application build exited {status}"));
            }
            let status = Command::new("python")
                .arg("-B")
                .arg(system.join("tools/application_artifact.py"))
                .arg("check")
                .arg(&output)
                .status()
                .map_err(|error| format!("start artifact validation: {error}"))?;
            if !status.success() {
                return Err(format!("application artifact validation exited {status}"));
            }
            Ok(())
        }
        Some(action) => Err(format!("unknown action {action}; {HELP}")),
    }
}
