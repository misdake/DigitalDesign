//! Run compiled CB06 code to its bound Prepared gate; export driver ABI bytes.
use cpu_v3_tang_nano_20k::profiling_capture::{
    capture_flash, capture_flash_with_functional_prefix, capture_native_flash,
    capture_native_flash_with_functional_prefix, CapturePoint, FrozenData,
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
    let (mut native, mut cycle_limit) = (false, None);
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
            "--max-cycles" => cycle_limit = Some(number(&value)?),
            "--cpu-mode" => {
                native = match value.as_str() {
                    "instruction" => false,
                    "native" => true,
                    _ => return Err("--cpu-mode must be instruction or native".into()),
                }
            }
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
    if native && limit.is_some() || !native && cycle_limit.is_some() {
        return Err("use --max-steps for instruction; --max-cycles for native".into());
    }
    let budget = if native {
        cycle_limit.ok_or("--max-cycles is required for native")?
    } else {
        limit.ok_or("--max-steps is required")?
    };
    let bytes = fs::read(flash).map_err(|e| e.to_string())?;
    let (data, counters, vblank_events, physical, metric) = if native {
        let capture = if functional_prefix {
            capture_native_flash_with_functional_prefix
        } else {
            capture_native_flash
        };
        let frozen = capture(&bytes, point, seed, budget)?;
        (
            frozen.data,
            format!(
                "\"cpu_mode\":\"native\",\"cpu_steps\":null,\"cpu_logic_cycles\":{},\"native_cycles\":{}",
                frozen.logic_cycles, frozen.native_cycles
            ),
            frozen.vblank_events,
            Some(frozen.physical_memory),
            format!("{} CPU logic cycles", frozen.logic_cycles),
        )
    } else {
        let capture = if functional_prefix {
            capture_flash_with_functional_prefix
        } else {
            capture_flash
        };
        let frozen = capture(&bytes, point, seed, budget)?;
        (
            FrozenData {
                header: frozen.header,
                ranges: frozen.ranges,
                payloads: frozen.payloads,
            },
            format!(
                "\"cpu_mode\":\"instruction\",\"cpu_steps\":{}",
                frozen.cpu_steps
            ),
            frozen.vblank_events,
            None,
            format!("{} CPU steps", frozen.cpu_steps),
        )
    };
    // A failed/limited run produces no complete dump directory.
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    for (name, data) in [("header.bin", &data.header), ("ranges.bin", &data.ranges)] {
        fs::write(
            output.join(name),
            data.iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .map_err(|e| e.to_string())?;
    }
    for (index, data) in data.payloads.iter().enumerate() {
        fs::write(output.join(format!("range-{index:03}.bin")), data).map_err(|e| e.to_string())?;
    }
    if let Some(memory) = physical {
        use std::io::Write;
        let mut writer = std::io::BufWriter::new(
            fs::File::create(output.join("physical.bin")).map_err(|e| e.to_string())?,
        );
        for word in memory {
            writer
                .write_all(&word.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
        writer.flush().map_err(|e| e.to_string())?;
    }
    let prefix_origin = if functional_prefix {
        ",\"prefix_gpu\":\"functional\""
    } else {
        ""
    };
    let origin = format!(
        "{{\"schema\":1,\"source\":\"system-simulation\",\"boundary\":\"before-profiler-gate\",{},\"vblank_events\":{},\"memory_seed\":{},\"gate_pc\":{},\"gpu_consumed\":false,\"range_count\":{}{}}}\n",
        counters, vblank_events, seed, point.gate_pc, data.payloads.len(), prefix_origin
    );
    fs::write(output.join("origin.json"), origin).map_err(|e| e.to_string())?;
    println!(
        "Frozen {} driver ranges before GPU consumption after {}",
        data.payloads.len(),
        metric
    );
    Ok(())
}
