//! Run compiled CB06 code to its bound Prepared gate; export driver ABI bytes.
use cpu_v3_tang_nano_20k::profiling_capture::{
    capture_flash, capture_flash_with_functional_prefix, CapturePoint,
};
use std::{env, fs, path::PathBuf};

fn number(value: &str) -> Result<usize, String> {
    if let Some(hex) = value.strip_prefix("0x") {
        usize::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        value.parse::<usize>().map_err(|e| e.to_string())
    }
}

fn main() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let (mut flash, mut output, mut gate, mut header, mut ranges, mut seed, mut limit) =
        (None, None, None, None, None, None, None);
    let mut functional_prefix = false;
    while let Some(arg) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--flash" => flash = Some(PathBuf::from(value)),
            "--out" => output = Some(PathBuf::from(value)),
            "--gate-pc" => gate = Some(u16::try_from(number(&value)?).map_err(|e| e.to_string())?),
            "--header-word" => header = Some(number(&value)?),
            "--ranges-word" => ranges = Some(number(&value)?),
            "--memory-seed" => {
                seed = Some(u16::try_from(number(&value)?).map_err(|e| e.to_string())?)
            }
            "--max-steps" => limit = Some(number(&value)?),
            "--prefix-gpu" => match value.as_str() {
                "reject" => functional_prefix = false,
                "functional" => functional_prefix = true,
                _ => return Err("--prefix-gpu must be reject or functional".into()),
            },
            _ => return Err(format!("Unknown argument {arg}")),
        }
    }
    let flash = flash.ok_or("--flash is required")?;
    let output = output.ok_or("--out is required")?;
    let point = CapturePoint {
        gate_pc: gate.ok_or("--gate-pc from compilation metadata is required")?,
        header_word: header.ok_or("--header-word from compilation metadata is required")?,
        ranges_word: ranges.ok_or("--ranges-word from compilation metadata is required")?,
    };
    let seed = seed.ok_or("--memory-seed is an explicit simulation assumption")?;
    let limit = limit.ok_or("--max-steps is required")?;
    let bytes = fs::read(flash).map_err(|e| e.to_string())?;
    let capture = if functional_prefix {
        capture_flash_with_functional_prefix
    } else {
        capture_flash
    };
    let frozen = capture(&bytes, point, seed, limit)?;
    // A failed/limited run produces no complete dump directory.
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    for (name, data) in [
        ("header.bin", &frozen.header),
        ("ranges.bin", &frozen.ranges),
    ] {
        fs::write(
            output.join(name),
            data.iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .map_err(|e| e.to_string())?;
    }
    for (index, data) in frozen.payloads.iter().enumerate() {
        fs::write(output.join(format!("range-{index:03}.bin")), data).map_err(|e| e.to_string())?;
    }
    let prefix_origin = if functional_prefix {
        ",\"prefix_gpu\":\"functional\""
    } else {
        ""
    };
    let origin = format!(
        "{{\"schema\":1,\"source\":\"system-simulation\",\"boundary\":\"before-profiler-gate\",\"cpu_steps\":{},\"vblank_events\":{},\"memory_seed\":{},\"gate_pc\":{},\"gpu_consumed\":false,\"range_count\":{}{}}}\n",
        frozen.cpu_steps, frozen.vblank_events, seed, point.gate_pc, frozen.payloads.len(), prefix_origin
    );
    fs::write(output.join("origin.json"), origin).map_err(|e| e.to_string())?;
    println!(
        "Frozen {} driver ranges before GPU consumption after {} CPU steps",
        frozen.payloads.len(),
        frozen.cpu_steps
    );
    Ok(())
}
