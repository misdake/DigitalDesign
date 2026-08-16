//! Builds a validated G16 boot package from an offline section manifest.

use cpu_v2::g16::boot::{build_boot_image, PackManifest};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
g16-pack: build a versioned G16 Flash payload

usage: g16-pack <manifest.g16manifest> [-o image.g16boot] [--map image.map]

manifest format:
  format 1
  target tang-nano-20k
  stage1-section <name>
  stage1-entry <cseg> <offset> <dseg> <stack-offset>
  application-entry <cseg> <offset> <dseg> <stack-offset>
  load <name> <physical-word> <rwx-flags> <alignment-bytes> <memory-bytes> <file>
  zero <name> <physical-word> <rw-flags> <alignment-bytes> <memory-bytes>

Numbers are decimal or 0x-prefixed hexadecimal. File paths are relative to the
manifest and may not contain whitespace. The output contains package-relative
Flash offsets; placement into a board Flash is a separate checked step.";

fn main() -> ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("g16-pack: error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut input = None;
    let mut output = None;
    let mut map = None;
    let mut args = args;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "-o" => output = Some(next_path(&mut args, "-o")?),
            "--map" => map = Some(next_path(&mut args, "--map")?),
            value if value.starts_with('-') => return Err(format!("unknown option `{value}`")),
            value if input.is_none() => input = Some(PathBuf::from(value)),
            value => return Err(format!("unexpected second input `{value}`")),
        }
    }

    let input = input.ok_or_else(|| "missing manifest input; see --help".to_owned())?;
    let output = output.unwrap_or_else(|| input.with_extension("g16boot"));
    let map = map.unwrap_or_else(|| output.with_extension("map"));
    let text = std::fs::read_to_string(&input)
        .map_err(|source| format!("cannot read {}: {source}", input.display()))?;
    let manifest = PackManifest::parse(&text).map_err(|error| error.to_string())?;
    let base = input.parent().unwrap_or_else(|| Path::new("."));
    let spec = manifest.load(base).map_err(|error| error.to_string())?;
    let image = build_boot_image(spec).map_err(|error| error.to_string())?;
    std::fs::write(&output, &image.bytes)
        .map_err(|source| format!("cannot write {}: {source}", output.display()))?;
    std::fs::write(&map, image.map())
        .map_err(|source| format!("cannot write {}: {source}", map.display()))?;
    println!(
        "packed {} sections, {} bytes -> {} (+ {})",
        image.sections.len(),
        image.bytes.len(),
        output.display(),
        map.display(),
    );
    Ok(())
}

fn next_path(args: &mut impl Iterator<Item = String>, option: &str) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{option} needs a path"))
}
