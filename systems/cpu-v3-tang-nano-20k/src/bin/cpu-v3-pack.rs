//! Pack the one fixed-destination application; no section/entry manifest.
use cpu_v3_tang_nano_20k::boot::{pack_single_application, SINGLE_BOOT_FLASH_BASE};
use std::{path::PathBuf, process::ExitCode};
const HELP:&str="cpu-v3-pack <application.v3bin> [-o package.bin] [--map package.map] [--configuration-bin design.bin] [--flash-image complete.bin]";
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
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            "-o" => output = Some(next(&mut args, "-o")?),
            "--map" => map = Some(next(&mut args, "--map")?),
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
    let package = pack_single_application(&code).map_err(|e| e.to_string())?;
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
    std::fs::write(&map,format!("format=5\ncode_bytes={}\nflash_base=0x100000\npayload_offset=128\ncode_segment=7\nentry=0\ndata_pages=0,1,2,3\nstack=0xc000\nstatic_zero_bytes=16384\n",code.len())).map_err(|e|e.to_string())?;
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
