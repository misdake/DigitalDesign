//! Passive bounded production System run. The CB06 installer owns all loading.
use cpu_v3_tang_nano_20k::system_cycle_emu::{startup::HANDOFF_LIMITS, Statistics, System};
use serde_json::{json, Value};
use std::{
    error::Error,
    fs::{self, File},
    io::Write,
    path::Path,
    time::{Duration, Instant},
};
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    assert_eq!(fs::read(path)?, bytes);
    Ok(())
}
fn stats(a: &Statistics, b: &Statistics) -> Value {
    fn delta(a: &[u64; 7], b: &[u64; 7]) -> Vec<u64> {
        a.iter().zip(b).map(|(a, b)| b - a).collect()
    }
    json!({"retired_words":b.cpu_retired_words-a.cpu_retired_words,"accepts":delta(&a.accepts,&b.accepts),"wait_cycles":delta(&a.wait_cycles,&b.wait_cycles),"response_beats":delta(&a.response_beats,&b.response_beats),"write_beats":delta(&a.write_beats,&b.write_beats),"gpu_submits":b.gpu_submits-a.gpu_submits})
}
fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err("usage: tankracer-native-check ARTIFACT BINDINGS OUTPUT MAX_LOGIC_CYCLES MAX_HOST_SECONDS".into());
    }
    let snapshot = Path::new(&args[1]);
    let binding: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let manifest: Value =
        serde_json::from_slice(&fs::read(snapshot.join("application.build.json"))?)?;
    assert_eq!(manifest["artifact_id"], binding["artifact_id"]);
    let output = Path::new(&args[3]);
    fs::create_dir_all(output)?;
    let budget = args[4].parse::<u64>()?;
    let host = Duration::from_secs(args[5].parse()?);
    let package = fs::read(snapshot.join(binding["package"].as_str().unwrap()))?;
    atomic(&output.join("consumed.cb06"), &package)?;
    let mut system = System::from_preloaded_cold(&package, 0xa55a)
        .map_err(|e| format!("package entry {e:?}"))?;
    let address = |name: &str| binding["symbols"][name]["physical_word"].as_u64().unwrap() as u32;
    let phase_address = address("PHYSICS_PHASE");
    let sample_address = address("PHYSICS_SAMPLE");
    let checks_address = address("PHYSICS_CHECKS");
    let failure_address = address("PHYSICS_FAILURE");
    let mut loaded = Vec::new();
    for (name, file) in [
        ("PHYSICS_SINE", "loaded-sine.bin"),
        ("PHYSICS_ANGLE", "loaded-angle.bin"),
    ] {
        let root = address(name) as usize;
        let words = &system.backing()[root..root + 4];
        let first = usize::from(words[0]) * 64;
        let bytes = usize::from(words[2]) + (usize::from(words[3]) << 16);
        let payload: Vec<u8> = system.backing()[first..first + bytes / 2]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        atomic(&output.join(file), &payload)?;
        loaded.push(json!({"root":name,"root_word":root,"first_physical_byte":first*2,"payload_bytes":bytes,"readback":file}));
    }
    let started = Instant::now();
    while !system.application_ready() {
        if system.clock().logic_edges >= 1_000_000 || started.elapsed() >= host {
            return Err("bounded cold prefix did not finish".into());
        }
        system.step().map_err(|e| format!("cold prefix {e:?}"))?;
    }
    assert_eq!(system.core().retired_words(), 0);
    let prefix_logic = system.clock().logic_edges;
    let prefix_native = system.native_cycles();
    let prefix_seconds = started.elapsed().as_secs_f64();
    let mut previous_phase = 0;
    let mut current = None;
    let mut captures = Vec::new();
    let mut halt = None;
    let mut reason = "logic-bound";
    let mut error = None;
    while system.clock().logic_edges - prefix_logic < budget {
        if started.elapsed() >= host {
            reason = "host-bound";
            break;
        }
        let step = match system.step() {
            Ok(step) => step,
            Err(e) => {
                error = Some(format!("System {e:?}"));
                reason = "system-error";
                break;
            }
        };
        if step.instant.logic_rise {
            let phase = system.inspect_cpu_word(phase_address).unwrap();
            if phase != previous_phase {
                let serial = system.inspect_cpu_word(sample_address).unwrap() as usize;
                if phase == 2 {
                    assert!(current.is_none());
                    current = Some((
                        serial,
                        system.clock().logic_edges,
                        system.native_cycles(),
                        system.statistics.clone(),
                    ));
                }
                if phase == 3 {
                    let (serial, start_logic, start_native, start_stats) =
                        current.take().expect("tick end without start");
                    let expected = &binding["samples"][serial % 3]["expected"];
                    let mut actual = serde_json::Map::new();
                    for (name, address, count) in [
                        ("attitude", 0x1100, 29),
                        ("suspension", 0x1180, 44),
                        ("drive", 0x1300, 16),
                        ("motion", 0x1380, 16),
                        ("boost", 0x1400, 4),
                    ] {
                        let values: Vec<i32> = (0..count)
                            .map(|i| {
                                let lo = system.inspect_cpu_word(address + i * 2).unwrap();
                                let hi = system.inspect_cpu_word(address + i * 2 + 1).unwrap();
                                (u32::from(lo) | (u32::from(hi) << 16)) as i32
                            })
                            .collect();
                        assert_eq!(json!(values), expected[name], "sample {serial} {name}");
                        actual.insert(name.into(), json!(values));
                    }
                    let cycles = system.clock().logic_edges - start_logic;
                    captures.push(json!({"serial":serial,"repeat":serial/3,"label":binding["samples"][serial%3]["label"],"reference_frame":binding["samples"][serial%3]["frame"],"dt":binding["samples"][serial%3]["dt"],"logic_cycles":cycles,"native_cycles":system.native_cycles()-start_native,"milliseconds_at_54mhz":cycles as f64/54000.0,"remaining_of_30ms_cycles":1620000i64-cycles as i64,"remaining_of_30ms_ms":30.0-cycles as f64/54000.0,"traffic":stats(&start_stats,&system.statistics),"checked_i32_values":109,"maximum_integer_error":0,"actual":actual}));
                }
                previous_phase = phase;
            }
        }
        if let Some(cpu) = step.cpu {
            if cpu.fault {
                error = Some(format!("CPU fault {} at {}", cpu.fault_code, cpu.fault_pc));
                reason = "cpu-fault";
                break;
            }
            if cpu.halted {
                halt = Some(cpu.halt_signal);
                reason = "cpu-halt";
                break;
            }
        }
    }
    let checks = system.inspect_cpu_word(checks_address).unwrap();
    let failure = system.inspect_cpu_word(failure_address).unwrap();
    let reached = halt == Some(0x600d) && captures.len() == 6 && checks == 654 && error.is_none();
    let image = json!({"code_words":system.image.code_words,"copied_bytes":system.image.copied_bytes,"zeroed_bytes":system.image.zeroed_bytes,"memory_seed":system.image.memory_seed});
    let report = json!({"artifact_id":binding["artifact_id"],"package_sha256_bound_by_preflight":binding["package_sha256"],"entry":"preloaded-post-boot","boundary":HANDOFF_LIMITS,"load_implementation":"System::from_preloaded_cold -> startup::image -> boot::install_single_application on the consumed CB06; no host resource/state writes","image":image,"loaded_assets":loaded,"cold_prefix_logic_cycles":prefix_logic,"cold_prefix_native_cycles":prefix_native,"cold_prefix_host_seconds":prefix_seconds,"application_logic_cycles":system.clock().logic_edges-prefix_logic,"application_native_cycles":system.native_cycles()-prefix_native,"clock_hz":54000000,"stop":reason,"reached":reached,"halt":halt,"error":error,"target_checks":checks,"failure_word_index":failure,"samples":captures,"gpu_submits":system.statistics.gpu_submits,"host_seconds":started.elapsed().as_secs_f64(),"limit_logic_cycles":budget,"limit_host_seconds":host.as_secs(),"timing_window":"CPU-visible phase2 marker through phase3 marker; includes tick call/return and marker boundary, excludes sample loading and target verification","cache_context":"First tick follows package installation, real cold preparation and target record loading; caches are not manually primed/cleared. Second round has the prior three real invocations/loads/verifications as history, not an all-hit guarantee.","budget_scope":"30ms remainder must also fund contact queries and other integration work; geometry sampling, collisions, AI, GPU work and rendering integration remain excluded."});
    atomic(
        &output.join("native-report.json"),
        &(serde_json::to_vec_pretty(&report)?),
    )?;
    println!(
        "{}",
        json!({"reached":reached,"stop":reason,"halt":halt,"logic":system.clock().logic_edges-prefix_logic,"samples":report["samples"].as_array().unwrap().len(),"host_seconds":started.elapsed().as_secs_f64()})
    );
    if !reached {
        return Err("native qualification incomplete; bounded report saved".into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let result = std::thread::Builder::new()
        .name("tankracer-native".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| run().map_err(|e| e.to_string()))?
        .join()
        .map_err(|_| "native worker panicked")?;
    result.map_err(Into::into)
}
