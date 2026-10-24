//! Pack the one fixed-destination application; no section/entry manifest.
use cpu_v3_tang_nano_20k::boot::{
    boot_image_map, pack_application_resources, parse_boot_project, SINGLE_BOOT_FLASH_BASE,
};
use std::{path::PathBuf, process::ExitCode};
const HELP:&str="cpu-v3-pack <application.v3bin> [--static-words N] [-o package.bin] [--map package.map] [--project boot-applications.conf] [--configuration-bin design.bin] [--flash-image complete.bin]; reads adjacent application.static-words unless --static-words is given";
fn main() -> ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cpu-v3-pack: {e}");
            ExitCode::FAILURE
        }
    }
}
fn run(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let (mut input, mut output, mut map, mut configuration, mut flash_image) =
        (None, None, None, None, None);
    let mut project = None;
    let mut static_words = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            "-o" => output = Some(next(&mut args, "-o")?),
            "--map" => map = Some(next(&mut args, "--map")?),
            "--project" => project = Some(next(&mut args, "--project")?),
            "--static-words" => {
                static_words = Some(
                    args.next()
                        .ok_or("--static-words needs a decimal count")?
                        .parse::<usize>()
                        .map_err(|_| "invalid --static-words count")?,
                );
            }
            "--configuration-bin" => configuration = Some(next(&mut args, "--configuration-bin")?),
            "--flash-image" => flash_image = Some(next(&mut args, "--flash-image")?),
            x if x.starts_with('-') => return Err(format!("unknown option {x}")),
            x if input.is_none() => input = Some(PathBuf::from(x)),
            x => return Err(format!("unexpected input {x}")),
        }
    }
    let input = input.ok_or_else(|| format!("missing application; {HELP}"))?;
    let output = output.unwrap_or_else(|| input.with_extension("cpu-v3-boot"));
    let map = map.unwrap_or_else(|| output.with_extension("map"));
    let code = std::fs::read(&input).map_err(|e| format!("read {}: {e}", input.display()))?;
    let static_words = match static_words {
        Some(words) => words,
        None => {
            let path = input.with_extension("static-words");
            std::fs::read_to_string(&path)
                .map_err(|e| {
                    format!(
                        "read {}: {e}; supply compiler metadata or --static-words",
                        path.display()
                    )
                })?
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("invalid static word count in {}", path.display()))?
        }
    };
    let resources = if let Some(path) = project {
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        parse_boot_project(&text)
            .map_err(|e| e.to_string())?
            .resources(path.parent().unwrap_or_else(|| std::path::Path::new(".")))?
    } else {
        Vec::new()
    };
    let package =
        pack_application_resources(&code, static_words, &resources).map_err(|e| e.to_string())?;
    if flash_image.is_some() && configuration.is_none() {
        return Err("--flash-image requires --configuration-bin".into());
    }
    let placed = if let Some(configuration) = configuration {
        let bytes = std::fs::read(configuration).map_err(|e| e.to_string())?;
        if bytes.len() > SINGLE_BOOT_FLASH_BASE {
            return Err("FPGA configuration exceeds its 1 MiB reserve".into());
        }
        let mut full = vec![0xff; SINGLE_BOOT_FLASH_BASE];
        full[..bytes.len()].copy_from_slice(&bytes);
        full.extend_from_slice(&package);
        Some(full)
    } else {
        None
    };
    std::fs::write(&output, &package).map_err(|e| e.to_string())?;
    std::fs::write(&map, boot_image_map(&code, &package)).map_err(|e| e.to_string())?;
    if let Some(bytes) = placed {
        std::fs::write(
            flash_image.unwrap_or_else(|| output.with_extension("flash.bin")),
            bytes,
        )
        .map_err(|e| e.to_string())?;
    }
    println!(
        "packed one application, {} code bytes, {} package bytes -> {}",
        code.len(),
        package.len(),
        output.display()
    );
    Ok(())
}
fn next(args: &mut impl Iterator<Item = String>, option: &str) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{option} needs a path"))
}
