//! Build immutable application snapshots and run bounded functional previews.
use cpu_v3_tools::{
    applications::{self, OptionValues},
    workflow::{self, Preview},
};
use std::{path::PathBuf, process::ExitCode};

const HELP: &str = "cpu-v3-app list\ncpu-v3-app build APPLICATION --out NEW-DIRECTORY [--gpu-debug 0|1] [application options]\ncpu-v3-app preview APPLICATION [application options] [--snapshot DIRECTORY] [--out NEW-RUN-DIRECTORY] [--presents N] [--max-steps N]\ncpu-v3-app run APPLICATION [application options] [--headless] [--samples N] [--max-steps N] [--seconds N]";
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
        Some(action @ ("build" | "preview" | "run")) => {
            let name = args.next().ok_or(HELP)?;
            let definition = applications::find(&name)?;
            let mut output = None;
            let mut snapshot = None;
            let mut seen = std::collections::BTreeSet::new();
            let mut preview = Preview {
                window: action == "run",
                samples: if action == "run" { 3600 } else { 120 },
                steps: if action == "run" {
                    100_000_000
                } else {
                    8_000_000
                },
                presents: if action == "run" { None } else { Some(2) },
                seconds: 120,
                xinput: None,
            };
            let mut settings = std::collections::BTreeMap::new();
            while let Some(flag) = args.next() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    println!(
                        "cpu-v3-app {action} {} {}",
                        definition.name,
                        if action == "build" {
                            "--out NEW-DIRECTORY"
                        } else {
                            "[--snapshot DIRECTORY] [--out NEW-RUN-DIRECTORY]"
                        }
                    );
                    if action != "build" {
                        println!("  --headless | --window\n  --presents N  --samples N  --max-steps N  --seconds N  --xinput-slot 0..3");
                    }
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
                        let default = option
                            .default_value()
                            .map(|v| format!(" (default {v})"))
                            .unwrap_or_default();
                        println!("  --{} {values}{default}", option.flag);
                    }
                    return Ok(());
                }
                if !seen.insert(flag.clone()) {
                    return Err(format!("duplicate {flag}"));
                }
                if action != "build" {
                    match flag.as_str() {
                        "--snapshot" => {
                            snapshot = Some(PathBuf::from(
                                args.next().ok_or("--snapshot needs a directory")?,
                            ));
                            continue;
                        }
                        "--headless" => {
                            preview.window = false;
                            continue;
                        }
                        "--window" => {
                            preview.window = true;
                            continue;
                        }
                        "--presents" | "--samples" | "--max-steps" | "--seconds"
                        | "--xinput-slot" => {
                            let n = args
                                .next()
                                .ok_or_else(|| format!("{flag} needs a value"))?
                                .parse::<usize>()
                                .map_err(|_| format!("invalid {flag}"))?;
                            if flag == "--xinput-slot" {
                                if n > 3 {
                                    return Err("--xinput-slot expects 0..3".into());
                                }
                                preview.xinput = Some(n as u32);
                            } else {
                                if n == 0 {
                                    return Err(format!("{flag} must be positive"));
                                }
                                match flag.as_str() {
                                    "--presents" => preview.presents = Some(n),
                                    "--samples" => preview.samples = n,
                                    "--max-steps" => preview.steps = n,
                                    _ => {
                                        if n > 7200 {
                                            return Err("--seconds must be at most 7200".into());
                                        }
                                        preview.seconds = n as u64;
                                    }
                                }
                            }
                            continue;
                        }
                        _ => (),
                    }
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
                value = option.validate(&value)?;
                if settings.insert(option.variable, value).is_some() {
                    return Err(format!("duplicate {flag}"));
                }
            }
            if seen.contains("--headless") && seen.contains("--window") {
                return Err("choose --headless or --window".into());
            }
            if snapshot.is_some() && !settings.is_empty() {
                return Err(
                    "--snapshot uses existing bytes; omit application build options".into(),
                );
            }
            if action == "build" {
                let output =
                    std::path::absolute(output.ok_or("build requires --out NEW-DIRECTORY")?)
                        .map_err(|e| e.to_string())?;
                cpu_v3_tools::require_empty_output(&output)?;
                let logs = workflow::unique(&workflow::app_root(definition)?.join("builds"))?;
                let result = workflow::build(definition, &output, &settings, &logs)?;
                println!(
                    "Snapshot: {}\nArtifact: {}\nLogs: {}",
                    workflow::show_path(&output),
                    result["artifact_id"].as_str().unwrap_or("?"),
                    workflow::show_path(&logs)
                );
            } else {
                let root = workflow::app_root(definition)?;
                let output = match output {
                    Some(path) => {
                        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
                        cpu_v3_tools::require_empty_output(&path)?;
                        std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
                        path
                    }
                    None => workflow::unique(&root.join("runs"))?,
                };
                let prepared = (|| -> Result<(), String> {
                    let snapshot = match snapshot {
                        Some(path) => path.canonicalize().map_err(|e| e.to_string())?,
                        None => {
                            let path = workflow::unique(&root.join("snapshots"))?;
                            workflow::build(
                                definition,
                                &path,
                                &settings,
                                &output.join("logs/build"),
                            )?;
                            path
                        }
                    };
                    if let Some(source) = definition.source {
                        let manifest: serde_json::Value = serde_json::from_slice(
                            &std::fs::read(snapshot.join("application.build.json"))
                                .map_err(|e| e.to_string())?,
                        )
                        .map_err(|e| e.to_string())?;
                        if !manifest["application"]["source"]["path"]
                            .as_str()
                            .is_some_and(|path| path.ends_with(source))
                        {
                            return Err("snapshot belongs to a different application".into());
                        }
                        if manifest["application"]["entry"] != definition.entry {
                            return Err("snapshot has a different application entry point".into());
                        }
                    }
                    if preview.window && !seen.contains("--xinput-slot") {
                        preview.xinput = Some(0);
                    }
                    workflow::preview(&snapshot, &output, &preview)?;
                    Ok(())
                })();
                if let Err(error) = prepared {
                    let receipt = output.join("run.json");
                    if !receipt.exists() {
                        let value = serde_json::json!({"schema":"digitaldesign.application-run","version":1,"success":false,
                            "application":definition.name,"stage":"preparation","error":error,"logs":"logs"});
                        workflow::write_receipt(&output, value)?;
                    }
                    return Err(format!("{error}\nRun: {}", output.display()));
                }
            }
            Ok(())
        }
        Some(action) => Err(format!("unknown action {action}; {HELP}")),
    }
}
