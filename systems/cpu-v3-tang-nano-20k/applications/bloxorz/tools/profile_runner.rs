//! Bloxorz level 29: exact-model time, real Device5 input, passive sidecar.
mod selection;
use cpu_v3_tang_nano_20k::{
    input_device::{ControllerSnapshot, InputMode},
    system_cycle_emu::{profiling::Recorder, runner, scanout::Scanout, System},
};
use selection::Selection;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};

fn main() {
    let result = std::env::args()
        .nth(1)
        .ok_or("usage: bloxorz-profile-runner <verified-plan.json>".to_string())
        .and_then(|p| runner::host_worker(move || run(Path::new(&p))));
    match result {
        Ok(true) => {}
        Ok(false) => std::process::exit(2),
        Err(e) => {
            eprintln!("bloxorz-profile-runner: {e}");
            std::process::exit(1)
        }
    }
}
fn number(v: &Value, key: &str, maximum: u64) -> Result<u64, String> {
    v[key]
        .as_u64()
        .filter(|n| *n > 0 && *n <= maximum)
        .ok_or(format!("invalid {key} ceiling {maximum}"))
}
fn publish(system: &mut System, buttons: u16) {
    system.publish_input(Some(ControllerSnapshot {
        mode: InputMode::Digital,
        buttons_active_low: buttons,
        left: [128; 2],
        right: [128; 2],
    }));
}
fn log(writer: &mut impl Write, event: &Value, count: &mut u64) -> Result<(), String> {
    if *count >= 100_000 {
        return Err("sidecar event ceiling100000".into());
    }
    serde_json::to_writer(&mut *writer, event).map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())?;
    *count += 1;
    Ok(())
}
fn run(plan_path: &Path) -> Result<bool, String> {
    let wall = Instant::now();
    let plan_raw = fs::read(plan_path).map_err(|e| e.to_string())?;
    let p: Value = serde_json::from_slice(&plan_raw).map_err(|e| e.to_string())?;
    if p["schema"] != "digitaldesign.ordinary-profile-plan" || p["version"] != 1 {
        return Err("plan schema".into());
    }
    if p["site_observation"] != "first-unheld-execute" {
        return Err("explicit first-unheld-execute site policy required".into());
    }
    let logic = number(&p, "max_logic_cycles", runner::MAX_LOGIC)?;
    let host = number(&p, "max_host_seconds", runner::MAX_HOST)?;
    let events = number(&p, "max_events", runner::MAX_EVENTS)?;
    let native_images = number(&p, "max_native_command_images", 2048)?;
    let root = plan_path.parent().unwrap_or(Path::new("."));
    let program = root.join(p["program"]["file"].as_str().ok_or("program file")?);
    let folder = root.join(p["output"].as_str().ok_or("output folder")?);
    let bytes = fs::read(program).map_err(|e| e.to_string())?;
    if Some(bytes.len() as u64) != p["program"]["bytes"].as_u64() {
        return Err("program byte length".into());
    }
    // SHA and linked-source closure are checked by the production Python launcher.
    let words: BTreeMap<String, u32> = p["words"]
        .as_object()
        .ok_or("probe words")?
        .iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                v.as_u64()
                    .filter(|n| *n < 4194304)
                    .ok_or("probe word address")? as u32,
            ))
        })
        .collect::<Result<_, String>>()?;
    if words.len() > 64 {
        return Err("at most64 named probe words".into());
    }
    for name in [
        "phase",
        "moving",
        "level",
        "x",
        "z",
        "orientation",
        "mask",
        "transition",
        "motion_age",
        "epoch",
    ] {
        if !words.contains_key(name) {
            return Err(format!("missing CPU probe {name}"));
        }
    }
    for i in 1..=9 {
        if !words.contains_key(&format!("switch_age_{i}")) {
            return Err("switch ages".into());
        }
    }
    let mut gates = BTreeMap::new();
    for name in ["before_input", "consumed", "sample"] {
        let a = p["gates"][name]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("gate PC pair")?;
        let pair = (
            a[0].as_u64().filter(|n| *n < 65536).ok_or("gate segment")? as u16,
            a[1].as_u64().filter(|n| *n < 65536).ok_or("gate PC")? as u16,
        );
        if gates.insert(pair, name).is_some() {
            return Err("gates must be distinct".into());
        }
    }
    let mut sites: BTreeMap<(u16, u16), Vec<String>> = BTreeMap::new();
    for name in ["vblank_return", "final_wait_return"] {
        let a = p["sites"][name]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("timing return PC pair")?;
        let pair = (
            a[0].as_u64().filter(|n| *n < 65536).ok_or("site segment")? as u16,
            a[1].as_u64().filter(|n| *n < 65536).ok_or("site PC")? as u16,
        );
        sites.entry(pair).or_default().push(name.into());
    }
    let mut system =
        System::from_preloaded_cold(&bytes, 0xa55a).map_err(|e| format!("entry {e:?}"))?;
    publish(&mut system, 0xffff);
    while !system.application_ready() {
        if system.clock().logic_edges >= 1_000_000 || wall.elapsed().as_secs() >= host {
            return Err("bounded prefix incomplete".into());
        }
        system.step().map_err(|e| format!("prefix {e:?}"))?;
    }
    let prefix = system.clock().logic_edges;
    let origin = system.clock().peek().ticks;
    let mut recorder =
        Recorder::create_runner(&folder, &bytes, origin, events).map_err(|e| e.to_string())?;
    recorder
        .initialize_memory(&system)
        .map_err(|e| e.to_string())?;
    fs::write(folder.join("runner-plan.json"), &plan_raw).map_err(|e| e.to_string())?;
    let mut sidecar = BufWriter::new(
        fs::File::create(folder.join("runner-events.jsonl")).map_err(|e| e.to_string())?,
    );
    let mut side_count = 0;
    log(
        &mut sidecar,
        &json!({"kind":"input","tick":origin,"buttons_active_low":65535,"reason":"initial-neutral"}),
        &mut side_count,
    )?;
    let mut selection = Selection::default();
    let mut raster = Scanout::default();
    let mut reason = "logic-bound".to_string();
    let mut watch_edges = 0u64;
    let mut execute_arrival = runner::ExecuteArrival::default();
    let mut pending_gate = None;
    let start = Instant::now();
    while system.clock().logic_edges - prefix < logic {
        // One scheduler instant can emit at most seven sidecar records. Stop
        // before exhausting that reserve so finish can retain a partial capture.
        if side_count >= 99_992 {
            reason = "sidecar-bound".into();
            break;
        }
        let mut device = None;
        let mut gate = None;
        let mut site = None;
        let mut site_r10 = 0;
        let mut retired = 0;
        let step = system.step_observing(|o| {
            recorder.observe(o);
            device = runner::cpu_device(o).map(|mut event| {
                // Exact linked ordinary-wait ABI, not a System/device contract.
                event["wait_started_r8"] = json!(o.cpu_registers[8]);
                event["wait_start_epoch_r9"] = json!(o.cpu_registers[9]);
                event["finish_r10"] = json!(o.cpu_registers[10]);
                event
            });
            if let Some(c) = o.cpu {
                retired = c.core.retired_words;
                let arrival = execute_arrival.observe(o);
                gate = arrival.and_then(|pc| gates.get(&pc).map(|name| (*name, pc)));
                site = arrival.filter(|pc| sites.contains_key(pc));
                site_r10 = o.cpu_registers[10];
                watch_edges += u64::from(c.system.watch_read_enable);
            }
        });
        let step = match step {
            Ok(s) => s,
            Err(e) => {
                reason = format!("model-error {e:?}");
                break;
            }
        };
        recorder.after_step(&step, &system);
        if recorder.truncated() {
            reason = "journal-truncated".into();
            break;
        }
        if let Some(mut e) = device {
            e["committed"] = json!(system.core().retired_words() as u64 > retired);
            e["epoch"] = json!(system.inspect_cpu_word(words["epoch"]));
            selection.device(&e);
            if let Err(e) = log(&mut sidecar, &e, &mut side_count) {
                reason = e;
                break;
            }
        }
        if let Some(pc) = site {
            for name in &sites[&pc] {
                if !selection::site_enabled(name, site_r10) {
                    continue;
                }
                log(
                    &mut sidecar,
                    &json!({"kind":"site","name":name,"tick":step.instant.ticks,"pc":pc,
                        "observation":"first-unheld-execute",
                        "finish_r10":site_r10,
                        "next_present":system.statistics.present_requests+1,
                        "epoch":system.inspect_cpu_word(words["epoch"])}),
                    &mut side_count,
                )?;
            }
        }
        if let Some(gate) = gate {
            if pending_gate.replace(gate).is_some() {
                return Err("new gate before the previous instruction retired".into());
            }
        }
        // A return-site LOAD can retire in WaitLoad, after its Execute arrival.
        // Keep the exact gate until that in-order retirement, rather than losing it.
        if system.core().retired_words() as u64 > retired {
            if let Some((name, pc)) = pending_gate.take() {
                let state = runner::snapshot(&system, &words)?;
                log(
                    &mut sidecar,
                    &json!({"kind":"gate","name":name,"tick":step.instant.ticks,"pc":pc,"state":state,
                    "next_present":system.statistics.present_requests+1}),
                    &mut side_count,
                )?;
                let input = match name {
                    "before_input" if selection.input_gate() => {
                        Some((0xffdf, "right-press-after-A-raster"))
                    }
                    "consumed" if selection.consumed_gate() => {
                        Some((0xffff, "release-after-real-read-and-consumption"))
                    }
                    "sample" => {
                        match selection.sample(&state, system.statistics.present_requests + 1) {
                            Ok(Some(label)) => log(
                                &mut sidecar,
                                &json!({"kind":"selected","label":label,"tick":step.instant.ticks,
                                "presentation":system.statistics.present_requests+1,"state":state}),
                                &mut side_count,
                            )?,
                            Ok(None) => {}
                            Err(e) => {
                                reason = e;
                                break;
                            }
                        }
                        None
                    }
                    _ => None,
                };
                if let Some((buttons, why)) = input {
                    publish(&mut system, buttons);
                    log(
                        &mut sidecar,
                        &json!({"kind":"input","tick":system.clock().peek().ticks,
                        "buttons_active_low":buttons,"reason":why}),
                        &mut side_count,
                    )?;
                }
            }
        }
        if step.cpu.is_some_and(|c| c.fault || c.halted) {
            reason = "cpu-fault-or-halt".into();
            break;
        }
        if step.instant.pixel_rise {
            if let Some(frame) = raster.sample(step.display.video).map_err(str::to_string)? {
                let descriptor = recorder.raster_descriptor();
                let valid = descriptor.is_some_and(|(_, n, front)| {
                    n > 0
                        && n == system.statistics.presents_applied
                        && front
                            == (system.display_register(1) as u32
                                | ((system.display_register(2) as u32) << 16))
                        && frame.effective_pixels == 384000
                        && selection.selected().contains(&n)
                        && !selection.complete.contains(&n)
                });
                recorder
                    .frame_selected(&frame, step.instant.ticks, valid)
                    .map_err(|e| e.to_string())?;
                if valid {
                    let (lo, n, front) = descriptor.unwrap();
                    selection.complete.insert(n);
                    log(
                        &mut sidecar,
                        &json!({"kind":"raster","tick":step.instant.ticks,"start_tick":lo,
                        "presentation":n,"front_halfword":front,"effective_pixels":frame.effective_pixels}),
                        &mut side_count,
                    )?;
                }
                if selection.done() {
                    reason = "four-selected-rasters".into();
                    break;
                }
            }
        }
        if step.instant.logic_rise
            && system.clock().logic_edges.is_multiple_of(4096)
            && wall.elapsed().as_secs() >= host
        {
            reason = "host-bound".into();
            break;
        }
    }
    sidecar.flush().map_err(|e| e.to_string())?;
    let selected: Vec<_> = selection
        .selected()
        .into_iter()
        .filter(|n| *n <= system.statistics.present_requests)
        .collect();
    let reached = selection.done();
    let runtime_logic = system.clock().logic_edges - prefix;
    recorder.finish(json!({"entry":"preloaded-post-boot","seed":0xa55a,"fixed_input":"scripted-digital",
        "reached":reached,"stop_reason":reason,"prefix_logic":prefix,"max_logic_cycles":logic,"max_host_seconds":host,
        "host_step_seconds":start.elapsed().as_secs_f64(),"runtime_logic":runtime_logic,"runner_version":1,
        "runner_files":["runner-plan.json","runner-events.jsonl"],"runner_selected_presentations":selected,
        "runner_sidecar_events":side_count,"runner_watch_edges":watch_edges,"native_group_limit":4,
        "selected_rasters":selection.complete,"stable_present":selection.stable,"rolling_present":selection.rolling,
        "runner_max_native_command_images":native_images,"host_worker_stack_bytes":runner::HOST_STACK_BYTES,
        "right_read":selection.right_read,"input_ack":selection.ack,"input_released":selection.released}),&system).map_err(|e|e.to_string())?;
    println!(
        "reached={reached} stop={reason} logic={runtime_logic} host={:.3} capture={}",
        wall.elapsed().as_secs_f64(),
        folder.display()
    );
    Ok(reached)
}
