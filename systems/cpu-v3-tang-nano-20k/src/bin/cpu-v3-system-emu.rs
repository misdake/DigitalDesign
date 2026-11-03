//! Full-system cycle entry. See the fixed synthetic startup boundary in docs.
use cpu_v3_tang_nano_20k::{
    display::ACTIVE_DISPLAY_CONFIG,
    input_device::{ControllerSnapshot, InputMode},
    system_cycle_emu::{
        profiling::Recorder,
        scanout::{Frame, Scanout},
        startup::HANDOFF_LIMITS,
        System,
    },
};
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};
const HELP:&str="cpu-v3-system-emu <CB06-image> --entry preloaded-post-boot|cold-preloaded --max-logic-cycles N [--max-host-seconds N] [--presents N] [--frame-ppm PATH] [--memory-out PATH]\nRuns the production CPU/GPU/Display and one native SDRAM service at exact clock edges. cold-preloaded is a synthetic configuration-state prefix, qualified separately from real bootloader handoff. --presents waits for N applied presents and a subsequent complete scanout raster. Host deadline defaults to 600s. preloaded-post-boot completes the bounded cold preparation before starting the application cycle budget; it does not import real SPI-loader state.";
fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(2),
        Err(e) => {
            eprintln!("cpu-v3-system-emu: {e}");
            ExitCode::FAILURE
        }
    }
}
fn run() -> Result<bool, String> {
    let mut args = std::env::args().skip(1);
    let (mut input, mut entry, mut limit, mut presents, mut ppm, mut memory) =
        (None, None, None, None, None, None);
    let mut host_seconds = 600u64;
    let mut fixed_input = None;
    let mut profile_out = None;
    let mut profile_events = 2_000_000;
    let mut profile_frames = 2;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}\n--fixed-input digital|analog|pressure selects a fixed neutral snapshot peer; the default is disconnected.\n--profile-out DIR enables bounded passive full-system capture in a new directory. --profile-max-events N defaults to 2000000 (maximum 10000000); --profile-max-frames N defaults to 2 (0 disables images, maximum 1024). Event overflow marks partial coverage and leaves simulation budgets unchanged. Client64 and actual native-model32 slots have separate boundaries; neither is board pin bandwidth.");
                return Ok(true);
            }
            "--entry" => entry = Some(next(&mut args, &arg)?),
            "--max-logic-cycles" => limit = Some(number(&next(&mut args, &arg)?)?),
            "--max-host-seconds" => host_seconds = number(&next(&mut args, &arg)?)?,
            "--presents" => presents = Some(number(&next(&mut args, &arg)?)?),
            "--fixed-input" => {
                fixed_input = Some(match next(&mut args, &arg)?.as_str() {
                    "digital" => InputMode::Digital,
                    "analog" => InputMode::Analog,
                    "pressure" => InputMode::Pressure,
                    x => return Err(format!("unknown fixed input mode {x}")),
                })
            }
            "--frame-ppm" => ppm = Some(PathBuf::from(next(&mut args, &arg)?)),
            "--memory-out" => memory = Some(PathBuf::from(next(&mut args, &arg)?)),
            "--profile-out" => profile_out = Some(PathBuf::from(next(&mut args, &arg)?)),
            "--profile-max-events" => profile_events = number(&next(&mut args, &arg)?)?,
            "--profile-max-frames" => profile_frames = number(&next(&mut args, &arg)?)?,
            x if x.starts_with('-') => return Err(format!("unknown option {x}")),
            x if input.is_none() => input = Some(PathBuf::from(x)),
            x => return Err(format!("unexpected argument {x}")),
        }
    }
    if !matches!(
        entry.as_deref(),
        Some("cold-preloaded" | "preloaded-post-boot")
    ) {
        return Err(format!(
            "select --entry cold-preloaded or preloaded-post-boot explicitly; {HELP}"
        ));
    }
    let limit = limit.ok_or("--max-logic-cycles is required")?;
    if limit == 0 || host_seconds == 0 || presents == Some(0) {
        return Err("cycle, host and present bounds must be positive".into());
    }
    let input = input.ok_or("missing CB06 image")?;
    let bytes = std::fs::read(&input).map_err(|e| format!("read {}: {e}", input.display()))?;
    let started = Instant::now();
    let mut system =
        System::from_preloaded_cold(&bytes, 0xa55a).map_err(|e| format!("entry: {e:?}"))?;
    if let Some(mode) = fixed_input {
        system.publish_input(Some(ControllerSnapshot {
            mode,
            buttons_active_low: 0xffff,
            left: [128; 2],
            right: [128; 2],
        }));
    }
    let setup_seconds = started.elapsed().as_secs_f64();
    let prefix_started = Instant::now();
    if entry.as_deref() == Some("preloaded-post-boot") {
        while !system.application_ready() {
            if system.clock().logic_edges >= 1_000_000
                || system.clock().logic_edges % 4096 == 0
                    && started.elapsed() >= Duration::from_secs(host_seconds)
            {
                println!("stop=prefix-bound reached=false");
                return Ok(false);
            }
            system.step().map_err(|e| format!("prefix: {e:?}"))?;
        }
        assert_eq!(system.core().retired_words(), 0);
    }
    let prefix_seconds = prefix_started.elapsed().as_secs_f64();
    let prefix_logic = system.clock().logic_edges;
    let capture_started = Instant::now();
    let mut recorder = profile_out
        .as_ref()
        .map(|folder| {
            Recorder::create(
                folder,
                &bytes,
                system.clock().peek().ticks,
                profile_events,
                profile_frames,
            )
        })
        .transpose()
        .map_err(|e| format!("profile setup: {e}"))?;
    let capture_setup_seconds = capture_started.elapsed().as_secs_f64();
    let step_started = Instant::now();
    let mut raster = Scanout::default();
    let mut frame: Option<Frame> = None;
    let mut halt = None;
    let mut reached = false;
    let mut reason = "logic-bound";
    while system.clock().logic_edges - prefix_logic < limit {
        let step = system
            .step_observing(|o| {
                if let Some(r) = &mut recorder {
                    r.observe(o);
                }
            })
            .map_err(|e| {
                format!(
                    "logic {} tick {}: {e:?}",
                    system.clock().logic_edges,
                    system.clock().peek().ticks
                )
            })?;
        if let Some(r) = &mut recorder {
            r.after_step(&step, &system);
        }
        if let Some(cpu) = step.cpu {
            if cpu.fault {
                return Err(format!(
                    "CPU fault {} at {:#x}",
                    cpu.fault_code, cpu.fault_pc
                ));
            }
            if cpu.halted {
                halt = Some(cpu.halt_signal);
            }
        }
        if step.instant.pixel_rise {
            if let Some(complete) = raster.sample(step.display.video).map_err(str::to_owned)? {
                if let Some(r) = &mut recorder {
                    r.frame(&complete, step.instant.ticks)
                        .map_err(|e| format!("profile frame: {e}"))?;
                }
                let config = ACTIVE_DISPLAY_CONFIG;
                let pixel_count = ((config.h_active_end - config.h_active_start)
                    * (config.v_active_end - config.v_active_start))
                    as usize;
                let target_scanned = presents
                    .is_some_and(|n| system.statistics.presents_applied >= n)
                    && complete.effective_pixels == pixel_count;
                frame = Some(complete);
                if target_scanned {
                    reached = true;
                    reason = "present-raster";
                    break;
                }
            }
        }
        if halt.is_some() && presents.is_none() {
            reached = true;
            reason = "cpu-halt";
            break;
        }
        if step.instant.logic_rise
            && system.clock().logic_edges % 4096 == 0
            && started.elapsed() >= Duration::from_secs(host_seconds)
        {
            reason = "host-bound";
            break;
        }
    }
    println!(
        "source=system-cycle-emu entry={} input={}",
        entry.as_deref().unwrap(),
        input.display()
    );
    println!("boundary={HANDOFF_LIMITS}");
    println!("stop={reason} reached={reached} halt={halt:?} logic={} native={} pixel={} quantum_hz={} host_seconds={:.6}",system.clock().logic_edges,system.native_cycles(),system.clock().pixel_edges,system.clock().quantum_hz,started.elapsed().as_secs_f64());
    println!(
        "startup={:?} first_fetch={:?}",
        system.release,
        system.first_fetch()
    );
    println!("runtime={:?}", system.statistics);
    if let Some(release) = system.release {
        println!(
            "post_release_logic={} post_release_native={} post_release_pixel={}",
            system.clock().logic_edges - release.logic_edges,
            system.native_cycles() - release.native_cycles,
            system.clock().pixel_edges - release.pixel_edges
        );
    }
    let step_seconds = step_started.elapsed().as_secs_f64();
    let export_started = Instant::now();
    if let Some(path) = memory {
        let mut out = writer(&path)?;
        for word in system.backing() {
            out.write_all(&word.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
        out.flush().map_err(|e| e.to_string())?;
    }
    if let Some(path) = ppm {
        let frame = frame.ok_or("no complete registered scanout raster available")?;
        let mut out = writer(&path)?;
        let c = ACTIVE_DISPLAY_CONFIG;
        write!(
            out,
            "P6\n{} {}\n255\n",
            c.h_active_end - c.h_active_start,
            c.v_active_end - c.v_active_start
        )
        .map_err(|e| e.to_string())?;
        out.write_all(&frame.rgb).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
        println!(
            "scanout_frame={} effective_pixels={} path={}",
            frame.number,
            frame.effective_pixels,
            path.display()
        );
    }
    println!("prefix_logic={prefix_logic} runtime_logic={} host_setup_seconds={setup_seconds:.6} host_prefix_seconds={prefix_seconds:.6} host_step_seconds={step_seconds:.6} host_export_seconds={:.6}",system.clock().logic_edges-prefix_logic,export_started.elapsed().as_secs_f64());
    if let Some(r) = recorder {
        r.finish(serde_json::json!({"entry":entry,"seed":0xa55a,"fixed_input":format!("{fixed_input:?}"),"stop_reason":reason,"reached":reached,"max_logic_cycles":limit,"max_host_seconds":host_seconds,"prefix_logic":prefix_logic,"host_setup_seconds":setup_seconds,"host_prefix_seconds":prefix_seconds,"host_capture_setup_seconds":capture_setup_seconds,"host_step_seconds":step_seconds,"host_export_seconds":export_started.elapsed().as_secs_f64(),"startup_boundary":HANDOFF_LIMITS}),&system).map_err(|e|format!("profile finish: {e}"))?;
    }
    Ok(reached)
}
fn next(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{name} needs a value"))
}
fn number(value: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("invalid positive integer {value}"))
}
fn writer(path: &PathBuf) -> Result<BufWriter<File>, String> {
    File::create(path)
        .map(BufWriter::new)
        .map_err(|e| format!("create {}: {e}", path.display()))
}
