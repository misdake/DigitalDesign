//! Result-level native GPU simulator used by the shared dump/replay frontend.
use gpu_v2::system::{functional::Executor, vliw_command::DeviceSubmission};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 5 && (args.len() != 7 || args[5] != "--access") {
        return Err("Expected input.bin base_bytes count_minus_one maximum_work output.bin [--access byte-flags.bin]".into());
    }
    let parse = |at: usize| -> Result<usize, Box<dyn std::error::Error>> {
        Ok(args[at]
            .to_str()
            .ok_or("non-UTF8 numeric argument")?
            .parse()?)
    };
    let bytes = std::fs::read(PathBuf::from(&args[0]))?;
    if bytes.len() != 0x800000 {
        return Err("Expected one complete8MiB physical image".into());
    }
    let base = u32::try_from(parse(1)?)?;
    let count = u32::try_from(parse(2)?)?;
    let length = count
        .checked_add(1)
        .and_then(|v| v.checked_mul(4))
        .ok_or("command count overflow")?;
    let submission = DeviceSubmission::new(base, length)?;
    let mut memory: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|word| u16::from_le_bytes(*word))
        .collect();
    let mut access = if args.len() == 7 {
        Some(std::fs::read(PathBuf::from(&args[6]))?)
    } else {
        None
    };
    let mut executor = Executor::new(parse(3)?);
    #[cfg(feature = "functional-batch")]
    if let Ok(mode) = std::env::var("GPU_SIM_REBIN") {
        use gpu_v2::system::functional::RebinMode;
        executor = executor.with_rebin_mode(match mode.as_str() {
            "A" => RebinMode::TriangleTiles,
            "B" => RebinMode::TileScan,
            "C" => RebinMode::ParallelSort,
            "D" => RebinMode::LocalBuckets,
            _ => return Err("GPU_SIM_REBIN must be A, B, C or D".into()),
        });
    }
    let outcome = match &mut access {
        Some(flags) => executor.execute_captured(&mut memory, flags, submission),
        None => executor.execute(&mut memory, submission),
    };
    #[cfg(feature = "functional-batch")]
    if let Some(path) = std::env::var_os("GPU_SIM_BATCH_STATS") {
        std::fs::write(PathBuf::from(path), executor.batch_stats().to_json())?;
    }
    let effects = match &outcome {
        Ok(report) => report.effects,
        Err(error) => error.effects,
    };
    let bytes: Vec<_> = memory.into_iter().flat_map(u16::to_le_bytes).collect();
    std::fs::write(PathBuf::from(&args[4]), bytes)?;
    if let Some(flags) = access {
        let mut output = args[4].clone();
        output.push(".access");
        std::fs::write(PathBuf::from(output), flags)?;
    }
    let written = effects.written_bytes.map_or_else(
        || "null".to_owned(),
        |(first, end)| format!("[{first},{end}]"),
    );
    let target = effects.target.map_or_else(
        || "null".to_owned(),
        |[color, depth]| format!("[{color},{depth}]"),
    );
    let mut completion = args[4].clone();
    completion.push(".completion.json");
    std::fs::write(
        PathBuf::from(completion),
        format!(
            "{{\"complete\":{},\"target\":{target},\"written_bytes\":{written}}}\n",
            outcome.is_ok()
        ),
    )?;
    outcome?;
    println!("GPU_SIM_COMPLETE");
    Ok(())
}
