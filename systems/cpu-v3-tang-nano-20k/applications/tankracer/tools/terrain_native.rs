//! Passive native CPU/cache/SDRAM measurement of source-loaded collision pages.
use cpu_v3_tang_nano_20k::system_cycle_emu::System;
use serde_json::{json, Value};
use std::{
    error::Error,
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("terrain-native ARTIFACT BINDINGS OUTPUT CYCLES SECONDS".into());
    }
    let artifact = Path::new(&args[0]);
    let binding: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let manifest: Value =
        serde_json::from_slice(&fs::read(artifact.join("application.build.json"))?)?;
    if manifest["artifact_id"] != binding["artifact_id"] {
        return Err("artifact binding mismatch".into());
    }
    let out = Path::new(&args[2]);
    if out.exists() {
        return Err("new native output directory required".into());
    }
    fs::create_dir_all(out)?;
    let budget: u64 = args[3].parse()?;
    let seconds: u64 = args[4].parse()?;
    let package = fs::read(artifact.join(binding["package"].as_str().ok_or("package")?))?;
    let mut system = System::from_preloaded_cold(&package, 0xa55a)
        .map_err(|error| format!("preloaded entry {error:?}"))?;
    let address = |name: &str| binding["symbols"][name]["physical_word"].as_u64().unwrap() as u32;
    let phase_word = address("TERRAIN_PHASE");
    let sample_word = address("TERRAIN_SAMPLE");
    let started = Instant::now();
    let mut phase = 0;
    let mut active = None;
    let mut samples = Vec::new();
    let mut halted = None;
    let mut error = None;
    let mut cold_prefix = None;
    while system.clock().logic_edges < budget && started.elapsed() < Duration::from_secs(seconds) {
        let step = match system.step() {
            Ok(step) => step,
            Err(fault) => {
                error = Some(format!("{fault:?}"));
                break;
            }
        };
        if system.application_ready() && cold_prefix.is_none() {
            cold_prefix = Some(system.clock().logic_edges);
        }
        if step.instant.logic_rise {
            let next = system.inspect_cpu_word(phase_word).unwrap();
            if next != phase {
                let serial = system.inspect_cpu_word(sample_word).unwrap();
                if next == 2 {
                    active = Some((
                        serial,
                        system.clock().logic_edges,
                        system.native_cycles(),
                        system.statistics.clone(),
                    ));
                } else if next == 3 {
                    let (index, logic, native, traffic) =
                        active.take().ok_or("query end without start")?;
                    if index != serial {
                        return Err("query marker order changed".into());
                    }
                    let cycles = system.clock().logic_edges - logic;
                    samples.push(json!({"sample":serial,"logic_cycles":cycles,
                        "milliseconds_at_54mhz":cycles as f64/54000.0,
                        "native_cycles":system.native_cycles()-native,
                        "retired_words":system.statistics.cpu_retired_words-traffic.cpu_retired_words,
                        "memory_requests":system.statistics.accepts.iter().zip(traffic.accepts).map(|(b,a)|b-a).collect::<Vec<_>>(),
                        "response_beats":system.statistics.response_beats.iter().zip(traffic.response_beats).map(|(b,a)|b-a).collect::<Vec<_>>() }));
                }
                phase = next;
            }
        }
        if let Some(cpu) = step.cpu {
            if cpu.fault {
                error = Some(format!("CPU fault {} at {}", cpu.fault_code, cpu.fault_pc));
                break;
            }
            if cpu.halted {
                halted = Some(cpu.halt_signal);
                break;
            }
        }
    }
    let checks = system.inspect_cpu_word(address("TERRAIN_CHECKS")).unwrap();
    let switches = system
        .inspect_cpu_word(address("TERRAIN_SWITCHES"))
        .unwrap();
    let expected = binding["count"].as_u64().unwrap() as usize;
    let passed = halted == Some(0x600d)
        && error.is_none()
        && samples.len() == expected
        && u64::from(checks)
            == expected as u64 * binding["output_words"].as_u64().ok_or("output width")?;
    let result = json!({"artifact_id":binding["artifact_id"],"passed":passed,"halt":halted,"error":error,
        "query_records":samples.len(),"checked_words":checks,"collision_page_switches":switches,
        "logic_cycles":system.clock().logic_edges,"cold_prefix_logic_cycles":cold_prefix,
        "host_seconds":started.elapsed().as_secs_f64(),"samples":samples,
        "scope":"preloaded-post-boot native CPU/cache/SDRAM; phase2..3 includes cell selection/binding, output initialization, all four query variants, transformed auxiliary sampling and result packing. Sample loading and expected-word validation are outside each sample. SPI, GPU and Display contention are excluded."});
    fs::write(
        out.join("native-report.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!(
        "{}",
        json!({"passed":passed,"samples":samples.len(),"switches":switches,"host_seconds":started.elapsed().as_secs_f64()})
    );
    if !passed {
        return Err("native terrain qualification incomplete; inspect report".into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    std::thread::Builder::new()
        .name("terrain-native".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| run().map_err(|e| e.to_string()))?
        .join()
        .map_err(|_| "native worker panicked")?
        .map_err(Into::into)
}
