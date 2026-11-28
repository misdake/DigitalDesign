//! Passive core-physics timing on a qualified direct driving artifact.
use cpu_v3_tang_nano_20k::{system_emu::CpuV3SystemEmu, system_sim::VBlankMode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn read_bound(app: &Path, manifest: &Value, name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let relative = Path::new(name);
    if relative.is_absolute()
        || relative
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err("artifact file must remain within its directory".into());
    }
    let record = &manifest["files"][name];
    let bytes = fs::read(app.join(relative))?;
    if record["bytes"].as_u64() != Some(bytes.len() as u64)
        || record["sha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str())
    {
        return Err(format!("artifact file identity differs: {name}").into());
    }
    Ok(bytes)
}
fn symbol(debug: &str, name: &str) -> Result<usize, Box<dyn Error>> {
    let mut result = None;
    for line in debug.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        if words.len() == 4 && words[0] == "global" && words[1] == name && words[2] == "u16" {
            if result.is_some() {
                return Err(format!("ambiguous symbol: {name}").into());
            }
            result = Some(usize::from(u16::from_str_radix(
                words[3].trim_start_matches("0x"),
                16,
            )?));
        }
    }
    result.ok_or_else(|| format!("missing symbol: {name}").into())
}
fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err(
            "usage: tankracer-driving-profile ARTIFACT MAX_LOGIC_CYCLES MAX_HOST_SECONDS OUTPUT"
                .into(),
        );
    }
    let app = Path::new(&args[1]);
    let budget = args[2].parse::<u64>()?;
    let limit = Duration::from_secs(args[3].parse()?);
    let output = Path::new(&args[4]);
    if budget == 0 || limit.is_zero() || output.exists() {
        return Err("positive limits and a fresh report path are required".into());
    }
    let manifest: Value = serde_json::from_slice(&fs::read(app.join("application.build.json"))?)?;
    let assets: Value = serde_json::from_slice(&read_bound(
        app,
        &manifest,
        "tankracer-driving-assets/assets.json",
    )?)?;
    let cases = assets["cases"].as_array().ok_or("driving cases missing")?;
    if cases.len() != 1 || cases[0]["world_pass"].as_bool() != Some(false) {
        return Err(
            "select one direct trajectory; ordered worlds need per-frame timing boundaries".into(),
        );
    }
    let debug = String::from_utf8(read_bound(
        app,
        &manifest,
        manifest["debug"].as_str().ok_or("debug file missing")?,
    )?)?;
    let phase_word = symbol(&debug, "DRIVING_PHASE")?;
    let frame_word = symbol(&debug, "DRIVING_FRAME")?;
    if phase_word >= 0x8000 || frame_word >= 0x8000 {
        return Err("timing markers must use the persistent CPU data windows".into());
    }
    let package = read_bound(
        app,
        &manifest,
        manifest["package"].as_str().ok_or("package missing")?,
    )?;
    let mut system = CpuV3SystemEmu::from_single_flash(&package, 0xd00d, VBlankMode::Manual)?
        .with_functional_gpu();
    let started = Instant::now();
    let mut previous_phase = 0;
    let mut current = None;
    let mut rows = Vec::new();
    let mut halted = false;
    while system.cycles() < budget && started.elapsed() < limit {
        let cpu = system
            .step()
            .map_err(|error| format!("native system: {error:?}"))?;
        if cpu.fault {
            return Err(format!("CPU fault {} at {}", cpu.fault_code, cpu.fault_pc).into());
        }
        let phase = system.inspect_cpu_words(phase_word, 1)?[0];
        if phase != previous_phase {
            if phase == 2 {
                if current.is_some() {
                    return Err("overlapping physical timing markers".into());
                }
                current = Some((
                    system.inspect_cpu_words(frame_word, 1)?[0],
                    system.cycles(),
                    system.native_cycles(),
                    system.core().retired_words(),
                ));
            }
            if phase == 3 {
                let (serial, logic, native, retired) =
                    current.take().ok_or("end marker without start")?;
                let cycles = system.cycles() - logic;
                let row = json!({"serial":serial,"logic_cycles":cycles,
                    "native_cycles":system.native_cycles()-native,
                    "retired_words":system.core().retired_words()-retired,
                    "milliseconds_at_54mhz":cycles as f64 / 54000.0});
                println!("{row}");
                rows.push(row);
            }
            previous_phase = phase;
        }
        if cpu.halted {
            if cpu.halt_signal != 0x600d {
                return Err(format!("qualification halted with {:#x}", cpu.halt_signal).into());
            }
            halted = true;
            break;
        }
    }
    let passed =
        halted && current.is_none() && Some(rows.len() as u64) == assets["updates"].as_u64();
    let report = json!({"passed":passed,"artifact_id":manifest["artifact_id"],
        "backend":"native CPU cache/SDRAM with functional GPU/Display; no memory competition",
        "scope":"CPU-visible phase2 through phase3: contact sampling, suspension/power, collisions, ground response, motion, effects and tail. Excludes input, prefix/control preparation, installation and target assertions. One visible direct trajectory is required.",
        "clock_hz":54000000,"case":cases[0]["name"],"host_seconds":started.elapsed().as_secs_f64(),
        "total_logic_cycles_with_loading_and_assertions":system.cycles(),"halted":halted,
        "input_records":assets["updates"],"rows":rows});
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    if !passed {
        return Err("bounded run or marker coverage is incomplete; see report".into());
    }
    println!("Saved {}", output.display());
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("tankracer-driving-profile: {error}");
        std::process::exit(1);
    }
}
