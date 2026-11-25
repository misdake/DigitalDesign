//! Host display simulator consuming an explicit, already built CB06 package.

use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::display::{render_frame_at, write_ppm};
#[cfg(feature = "display-window")]
use cpu_v3_tang_nano_20k::display::{ACTIVE_DISPLAY_CONFIG, HDMI_HEIGHT, HDMI_WIDTH};
use cpu_v3_tang_nano_20k::host_io::{HostIo, RealtimeHostIo};
use cpu_v3_tang_nano_20k::system_emu::{CpuV3SystemEmu, DeviceWrite};
use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
use cpu_v3_tang_nano_20k::{DeviceSubmission, GPU_COMMAND_COUNT, GPU_COMMAND_START, GPU_DEVICE};
use std::io::{BufWriter, Write};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[cfg(feature = "display-window")]
type PreviewWindow = minifb::Window;
#[cfg(not(feature = "display-window"))]
type PreviewWindow = ();

fn window_focus(window: &mut Option<PreviewWindow>) -> bool {
    #[cfg(feature = "display-window")]
    if let Some(window) = window {
        window.update();
        return window.is_active();
    }
    #[cfg(not(feature = "display-window"))]
    let _ = window;
    true
}

enum CpuBackend {
    Instruction(Box<CpuV3SystemSim>),
    Native(Box<CpuV3SystemEmu>),
}
#[derive(Default)]
struct SubmissionPause {
    target: Option<usize>,
    seen: usize,
    count: Option<u16>,
    capture: Option<(DeviceWrite, Option<u16>, Vec<u16>)>,
    halt: Option<(u16, u16)>,
    fault_pc: Option<u16>,
}
impl SubmissionPause {
    fn observe(&mut self, write: DeviceWrite, memory: &[u16]) -> bool {
        if write.device != GPU_DEVICE {
            return true;
        }
        if write.channel == GPU_COMMAND_COUNT {
            self.count = Some(write.value);
        }
        if write.channel == GPU_COMMAND_START {
            self.seen += 1;
            let count = self.count.take();
            if self.target == Some(self.seen) {
                self.capture = Some((write, count, memory.to_vec()));
                return false;
            }
        }
        true
    }
}
impl CpuBackend {
    fn frame_counters(&self, presentation: usize, advances: usize) -> serde_json::Value {
        let (retired, logic, native) = match self {
            Self::Instruction(s) => (s.cpu().retired_words(), None, None),
            Self::Native(s) => (
                s.core().retired_words() as u64,
                Some(s.cycles()),
                Some(s.native_cycles()),
            ),
        };
        serde_json::json!({"presentation":presentation,"cpu_advances":advances,
            "retired_words":retired,"logic_cycles":logic,"native_cycles":native})
    }
    fn host_event(&mut self, host: &mut HostIo, elapsed: Duration, focused: bool) {
        match self {
            Self::Instruction(s) => host.advance(s, elapsed, focused),
            Self::Native(s) => host.advance_input(s.as_mut(), elapsed, focused),
        }
    }
    fn realtime_input(&mut self, host: &mut RealtimeHostIo, focused: bool) {
        match self {
            Self::Instruction(s) => host.pump(s, focused),
            Self::Native(s) => host.pump_input(s.as_mut(), focused),
        }
    }
    fn step(&mut self, pause: &mut SubmissionPause) -> Result<Option<bool>, String> {
        match self {
            Self::Instruction(s) => {
                let pc = s.cpu().pc();
                let result = s.step_observing(|w, m| pause.observe(w, m)).map_err(|e| {
                    pause.fault_pc = Some(e.address);
                    format!("instruction CPU fault: {e:?}")
                })?;
                Ok(result.map(|outcome| {
                    if let StepOutcome::Halted { signal } = outcome {
                        pause.halt = Some((signal, pc));
                        true
                    } else {
                        false
                    }
                }))
            }
            Self::Native(s) => {
                let Some(o) = s.step_observing(|w, m| pause.observe(w, m)).map_err(|e| {
                    format!("native CPU fault at logic cycle {}: {e:?}", s.cycles())
                })?
                else {
                    return Ok(None);
                };
                if o.fault {
                    pause.fault_pc = Some(o.fault_pc as u16);
                    return Err(format!(
                        "native core fault {} at logic cycle {}, PC {:#06x}",
                        o.fault_code,
                        s.cycles(),
                        s.core().program_counter()
                    ));
                }
                if o.halted {
                    pause.halt = Some((o.halt_signal as u16, (o.pc as u16).wrapping_sub(1)));
                }
                Ok(Some(o.halted))
            }
        }
    }
    fn diagnostic_state(
        &self,
        halt: Option<(u16, u16)>,
        logical: Option<u16>,
        stack_top: u16,
        fault: Option<&str>,
        fault_pc: Option<u16>,
    ) -> serde_json::Value {
        let (pc, segment, dseg, registers) = match self {
            Self::Instruction(s) => (
                s.cpu().pc(),
                s.cpu().code_segment(),
                s.cpu().data_segments(),
                s.cpu().registers().to_owned(),
            ),
            Self::Native(s) => (
                s.core().program_counter(),
                s.core().segments().0,
                s.core().data_segments(),
                s.core().architectural_gprs(),
            ),
        };
        let read = |logical: u16, count: usize| -> Vec<u16> {
            let mut words = Vec::new();
            let mut address = usize::from(logical);
            while words.len() < count && address < 65536 {
                let chunk = (0x4000 - (address & 0x3fff)).min(count - words.len());
                let physical = (usize::from(dseg[address >> 14]) << 14) | (address & 0x3fff);
                let values = match self {
                    Self::Instruction(s) => s
                        .cpu()
                        .physical_memory_slice()
                        .get(physical..physical + chunk)
                        .map(|w| w.to_vec()),
                    Self::Native(s) => s.inspect_cpu_words(physical, chunk).ok(),
                };
                let Some(values) = values else {
                    break;
                };
                words.extend(values);
                address += chunk;
            }
            words
        };
        let sp = registers[13];
        let stack_count = usize::from(stack_top.saturating_sub(sp)).min(4096);
        serde_json::json!({"pc":pc,"code_segment":segment,"data_segments":dseg,
            "halt_signal":halt.map(|h|h.0),"halt_pc":halt.map(|h|h.1),"fault_words":logical.map(|l|read(l,16)).unwrap_or_default(),
            "sp":sp,"lr":registers[14],"stack_words":read(sp,stack_count),"stack_truncated":usize::from(stack_top.saturating_sub(sp))>stack_count,
            "cpu_fault":fault,"fault_pc":fault_pc,"gpu_error":self.gpu_error()})
    }
    fn gpu_error(&self) -> Option<String> {
        match self {
            Self::Instruction(s) => s.gpu().last_error().map(|e| format!("{e:?}")),
            Self::Native(s) => s.gpu().last_error().map(|e| format!("{e:?}")),
        }
    }
    fn vblank(&mut self) -> bool {
        match self {
            Self::Instruction(s) => s.advance_vblank(),
            Self::Native(s) => s.advance_vblank(),
        }
    }
    fn can_yield_vblank(&self) -> bool {
        // Only the architectural backend has an explicit host-event time base.
        // Native Device0 and memory need every clock; a CPU hold is insufficient
        // proof that those owners can be skipped.
        matches!(self, Self::Instruction(s) if s.blocked_on_vblank())
    }
    fn render(&self) -> Vec<u32> {
        match self {
            Self::Native(s) => s.render_frame(),
            Self::Instruction(s) => {
                if s.cpu()
                    .device::<cpu_v3_tang_nano_20k::DisplayDevice>(
                        cpu_v3_tang_nano_20k::DISPLAY_DEVICE,
                    )
                    .is_some_and(cpu_v3_tang_nano_20k::DisplayDevice::front_valid)
                {
                    render_frame_at(s.cpu(), s.display_state().active_base)
                } else {
                    vec![
                        0;
                        cpu_v3_tang_nano_20k::display::HDMI_WIDTH
                            * cpu_v3_tang_nano_20k::display::HDMI_HEIGHT
                    ]
                }
            }
        }
    }
}

struct Options {
    vblank_fast_forward: bool,
    max_cpu_steps: Option<usize>,
    max_cpu_cycles: Option<usize>,
    native: bool,
    package: Option<PathBuf>,
    artifact: Option<PathBuf>,
    freeze_submit: Option<usize>,
    stop_after_present: Option<usize>,
    output: PathBuf,
    frames: usize,
    ppm: bool,
    window: bool,
    realtime: bool,
    host_tick_us: u32,
    xinput_slot: Option<u32>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        vblank_fast_forward: true,
        max_cpu_steps: None,
        max_cpu_cycles: None,
        native: false,
        package: None,
        artifact: None,
        freeze_submit: None,
        stop_after_present: None,
        output: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/display-sim"),
        frames: 120,
        ppm: false,
        window: false,
        realtime: false,
        host_tick_us: 16_667,
        xinput_slot: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-vblank-fast-forward" => options.vblank_fast_forward = false,
            "--max-cpu-steps" => {
                options.max_cpu_steps = Some(
                    args.next()
                        .ok_or("--max-cpu-steps requires a value")?
                        .parse()
                        .map_err(|_| "invalid --max-cpu-steps")?,
                );
            }
            "--cpu-mode" => {
                options.native = match args.next().as_deref() {
                    Some("instruction") => false,
                    Some("native") => true,
                    _ => return Err("--cpu-mode requires instruction or native".into()),
                }
            }
            "--max-cpu-cycles" => {
                options.max_cpu_cycles = Some(
                    args.next()
                        .ok_or("--max-cpu-cycles requires a value")?
                        .parse()
                        .map_err(|_| "invalid --max-cpu-cycles")?,
                )
            }
            "--artifact" => {
                options.artifact =
                    Some(args.next().ok_or("--artifact requires a directory")?.into());
            }
            "--package" => {
                options.package = Some(args.next().ok_or("--package requires a path")?.into())
            }
            "--out-dir" => options.output = args.next().ok_or("--out-dir requires a path")?.into(),
            "--freeze-submit" => {
                options.freeze_submit = Some(
                    args.next()
                        .ok_or("--freeze-submit requires a one-based index")?
                        .parse()
                        .map_err(|_| "invalid --freeze-submit")?,
                )
            }
            "--stop-after-present" => {
                options.stop_after_present = Some(
                    args.next()
                        .ok_or("--stop-after-present requires a count")?
                        .parse()
                        .map_err(|_| "invalid --stop-after-present")?,
                )
            }
            "--frames" => {
                options.frames = args
                    .next()
                    .ok_or("--frames requires a value")?
                    .parse()
                    .map_err(|_| "invalid --frames")?;
            }
            "--ppm" => options.ppm = true,
            "--window" => {
                options.window = true;
                options.realtime = true;
            }
            "--realtime" => options.realtime = true,
            "--host-tick-us" => {
                options.host_tick_us = args
                    .next()
                    .ok_or("--host-tick-us requires a value")?
                    .parse()
                    .map_err(|_| "invalid --host-tick-us")?
            }
            "--xinput-slot" => {
                options.xinput_slot = Some(
                    args.next()
                        .ok_or("--xinput-slot requires a value")?
                        .parse()
                        .map_err(|_| "invalid --xinput-slot")?,
                );
                options.realtime = true;
            }
            _ => return Err(format!("unknown argument `{arg}`")),
        }
    }
    if options.native && options.max_cpu_steps.is_some()
        || !options.native && options.max_cpu_cycles.is_some()
    {
        return Err(
            "use --max-cpu-steps with instruction mode; --max-cpu-cycles with native mode".into(),
        );
    }
    if options.stop_after_present == Some(0) {
        return Err("--stop-after-present must be positive".into());
    }
    if options.stop_after_present.is_some() && options.freeze_submit.is_some() {
        return Err("choose --stop-after-present or --freeze-submit".into());
    }
    if options.freeze_submit == Some(0) {
        return Err("--freeze-submit is one-based".into());
    }
    if options.max_cpu_steps == Some(0) || options.max_cpu_cycles == Some(0) || options.frames == 0
    {
        return Err("step and frame limits must be non-zero".into());
    }
    if options.host_tick_us == 0 || options.host_tick_us > 1_000_000 {
        return Err("--host-tick-us must be between 1 and 1000000".into());
    }
    Ok(options)
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn run() -> Result<(), String> {
    if std::env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("cpu-v3-display-sim --package EXISTING-CB06 [--cpu-mode instruction|native] [--frames N] [--max-cpu-steps N | --max-cpu-cycles N] [--out-dir DIR] [--ppm] [--window]");
        return Ok(());
    }
    let options = options()?;
    let package_path = options.package.as_ref().ok_or(
        "--package requires an existing CB06 image; build one with cpu-v3-app build APPLICATION --out NEW-DIRECTORY",
    )?;
    let package =
        std::fs::read(package_path).map_err(|e| format!("read application package: {e}"))?;
    std::fs::create_dir_all(&options.output).map_err(|e| e.to_string())?;
    let frozen_package = options.output.join("application.cb06");
    std::fs::write(&frozen_package, &package).map_err(|e| e.to_string())?;
    let diagnostics = options
        .artifact
        .as_ref()
        .map(|artifact| {
            cpu_v3_tang_nano_20k::runtime_diagnostics::RuntimeDiagnostics::new(
                artifact,
                &frozen_package,
                &options.output,
            )
        })
        .transpose()?;
    let mut system = if options.native {
        CpuBackend::Native(Box::new(
            CpuV3SystemEmu::from_single_flash(&package, 0xd00d, VBlankMode::Manual)
                .map_err(|e| format!("load native application: {e}"))?
                .with_functional_gpu(),
        ))
    } else {
        CpuBackend::Instruction(Box::new(
            CpuV3SystemSim::from_single_flash(&package, 0xd00d, VBlankMode::Manual)
                .map_err(|e| format!("load application: {e}"))?
                .with_functional_gpu(),
        ))
    };
    let budget = if options.native {
        options.max_cpu_cycles
    } else {
        options.max_cpu_steps
    }
    .unwrap_or(5_000_000);
    let mut pause = SubmissionPause {
        target: options.freeze_submit,
        ..Default::default()
    };
    let started = Instant::now();

    #[cfg(feature = "display-window")]
    let mut window = options.window.then(|| {
        minifb::Window::new(
            &format!(
                "CPU V3 400x240 framebuffer ({} HDMI preview)",
                ACTIVE_DISPLAY_CONFIG.name
            ),
            HDMI_WIDTH,
            HDMI_HEIGHT,
            minifb::WindowOptions::default(),
        )
        .expect("create display preview window")
    });
    #[cfg(not(feature = "display-window"))]
    if options.window {
        return Err("--window requires --features display-window".into());
    }

    #[cfg(not(feature = "display-window"))]
    let mut window: Option<PreviewWindow> = None;
    let io = if let Some(slot) = options.xinput_slot {
        HostIo::with_xinput(slot).map_err(|error| error.to_string())?
    } else {
        HostIo::default()
    };
    let mut realtime = options.realtime.then(|| RealtimeHostIo::new(io));
    // This explicit deterministic event schedule is independent of CPU progress.
    // The per-event CPU budget is a work limit, never a clock conversion.
    let mut deterministic = HostIo::default();
    let chunk = budget.div_ceil(options.frames);
    let mut pixels = Vec::new();
    let mut executed = 0_usize;
    let mut halted = false;
    let mut cpu_fault = None;
    let mut samples = 0;
    let mut presented = 0;
    let mut frame_counters = vec![system.frame_counters(0, 0)];
    let mut vblank_yields = 0_usize;
    let mut focus_checked = Instant::now();
    'samples: for _ in 0..options.frames {
        let mut focused = window_focus(&mut window);
        if let Some(host) = realtime.as_mut() {
            system.realtime_input(host, focused);
        } else {
            system.host_event(
                &mut deterministic,
                Duration::from_micros(u64::from(options.host_tick_us)),
                focused,
            );
        }
        let next = executed.saturating_add(chunk).min(budget);
        while executed < next && !halted {
            if options.vblank_fast_forward && system.can_yield_vblank() {
                vblank_yields += 1;
                break;
            }
            if let Some(host) = realtime.as_mut() {
                if focus_checked.elapsed() >= Duration::from_millis(8) {
                    focused = window_focus(&mut window);
                    focus_checked = Instant::now();
                }
                system.realtime_input(host, focused);
            }
            let stopped = match system.step(&mut pause) {
                Ok(Some(stopped)) => stopped,
                Ok(None) => break 'samples,
                Err(error) => {
                    cpu_fault = Some(error);
                    break 'samples;
                }
            };
            halted = stopped;
            if options.freeze_submit.is_some() {
                if let Some(error) = system.gpu_error() {
                    return Err(format!(
                        "GPU prefix failed after {executed} CPU advances: {error}"
                    ));
                }
            }
            executed += 1;
        }
        if system.vblank() {
            presented += 1;
            frame_counters.push(system.frame_counters(presented, executed));
        }
        samples += 1;
        pixels = system.render();
        if options.stop_after_present.is_some_and(|n| presented >= n) {
            break;
        }
        #[cfg(feature = "display-window")]
        if let Some(window) = window.as_mut() {
            if !window.is_open() {
                break;
            }
            window
                .update_with_buffer(&pixels, HDMI_WIDTH, HDMI_HEIGHT)
                .map_err(|error| format!("window update failed: {error}"))?;
        }
    }
    let diagnostic = diagnostics
        .as_ref()
        .map(|diagnostics| {
            diagnostics.finish(&system.diagnostic_state(
                pause.halt,
                diagnostics.fault_logical,
                diagnostics.stack_top,
                cpu_fault.as_deref(),
                pause.fault_pc,
            ))
        })
        .transpose()?;
    let present_error = options
        .stop_after_present
        .filter(|&target| presented < target)
        .map(|target| {
            format!(
                "only {presented} real swaps within {executed} CPU advances; requested {target}"
            )
        });
    if let Some((write, count, memory)) = pause.capture.as_ref() {
        std::fs::create_dir_all(&options.output).map_err(|e| e.to_string())?;
        let count = count.ok_or("target START has no preceding COUNT")?;
        let descriptor = DeviceSubmission::from_device_words(count, write.value)?;
        let raw = options.output.join("physical-pre-submit.bin");
        let mut writer = BufWriter::new(std::fs::File::create(&raw).map_err(|e| e.to_string())?);
        for word in memory {
            writer
                .write_all(&word.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
        writer.flush().map_err(|e| e.to_string())?;
        std::fs::write(options.output.join("application.cb06"), &package)
            .map_err(|e| e.to_string())?;
        // Diagnostic architectural registers are not a resumable CPU/cache checkpoint.
        let architecture=match &system{
            CpuBackend::Instruction(s)=>format!("{{\"pc\":{},\"code_segment\":{},\"gprs\":{:?},\"data_segments\":{:?},\"retired_words\":{}}}",s.cpu().pc(),s.cpu().code_segment(),s.cpu().registers(),s.cpu().data_segments(),s.cpu().retired_words()),
            CpuBackend::Native(s)=>{let c=s.core();format!("{{\"pc\":{},\"code_segment\":{},\"gprs\":{:?},\"data_segments\":{:?},\"retired_words\":{}}}",c.program_counter(),c.segments().0,c.architectural_gprs(),c.data_segments(),c.retired_words())}
        };
        std::fs::write(options.output.join("cpu-architecture.json"), architecture)
            .map_err(|e| e.to_string())?;
        let (mode, logic, native) = match &system {
            CpuBackend::Native(s) => (
                "native",
                s.cycles().to_string(),
                s.native_cycles().to_string(),
            ),
            CpuBackend::Instruction(_) => ("instruction", "null".into(), "null".into()),
        };
        let metadata = format!(
            r#"{{
 "kind":"physical-gpu-preimage","boundary":"before-external-device-write",
 "cpu_checkpoint":false,"implicit_cache_maintenance":false,"backend":"{}",
 "submission_ordinal":{},"functional_prefix_submissions":{},"cpu_step_calls":{},
 "logic_cycles":{},"native_cycles":{},"device":{},"channel":{},"value":{},
 "base_sector":{},"count_minus_one":{},"memory_words":{},"byte_order":"little-endian",
 "gpu_display":"functional-without-memory-competition"
}}"#,
            mode,
            pause.seen,
            pause.seen - 1,
            executed,
            logic,
            native,
            write.device,
            write.channel,
            write.value,
            descriptor.base_sector,
            descriptor.count_minus_one,
            memory.len()
        );
        std::fs::write(options.output.join("pre-submit.json"), metadata)
            .map_err(|e| e.to_string())?;
        pixels = system.render();
        println!(
            "paused before GPU START {} after {} CPU advances, wrote {}",
            pause.seen,
            executed,
            raw.display()
        );
    } else if let Some(target) = options.freeze_submit {
        return Err(format!("GPU START {target} not reached within {executed} CPU advances; observed {} START writes",pause.seen));
    }
    if options.ppm {
        let path = options.output.join("frame-final.ppm");
        write_ppm(&path, &pixels).map_err(|error| format!("write {}: {error}", path.display()))?;
        println!("wrote {}", path.display());
    }
    match &system{
        CpuBackend::Instruction(_)=>println!("simulated {executed} CPU step calls and {samples} explicit display samples"),
        CpuBackend::Native(s)=>println!("simulated {} CPU logic cycles / {} native SDRAM cycles, {} retired words and {samples} explicit display samples; CSEG {}, PC {:#06x}; GPU/Display functional without memory competition; host wall {:.3}s",s.cycles(),s.native_cycles(),s.core().retired_words(),s.core().segments().0,s.core().program_counter(),started.elapsed().as_secs_f64()),
    }
    if let CpuBackend::Native(s) = &system {
        let display = s
            .device::<cpu_v3_tang_nano_20k::DisplayDevice>(cpu_v3_tang_nano_20k::DISPLAY_DEVICE)
            .unwrap();
        let markers = s
            .device::<cpu_v3_tang_nano_20k::ObservationDevice>(7)
            .unwrap();
        let accepted = pause.seen - usize::from(pause.capture.is_some());
        let stop = if pause.capture.is_some() {
            "pre-submit"
        } else if options.stop_after_present.is_some_and(|n| presented >= n) {
            "present-limit"
        } else if halted {
            "halt"
        } else {
            "cycle-limit"
        };
        std::fs::create_dir_all(&options.output).map_err(|e| e.to_string())?;
        std::fs::write(options.output.join("application.cb06"), &package)
            .map_err(|e| e.to_string())?;
        let summary = format!(
            r#"{{
 "backend":"native","logic_cycles":{},"native_cycles":{},"retired_words":{},
 "accepted_gpu_submissions":{},"gpu_busy":{},"gpu_error":{},
 "front_valid":{},"presented_swaps":{},"active_base_word":{},"linear_rgb565":{},
 "samples":{},"host_wall_seconds":{},"stop_reason":"{}","observation_phase":{},"observation_frame":{},
 "gpu_display":"functional-without-memory-competition","flash_install":"preloaded-cb06-not-spi-time","input":"no-controller-unless-explicit-xinput"
}}"#,
            s.cycles(),
            s.native_cycles(),
            s.core().retired_words(),
            accepted,
            s.gpu().busy(),
            system.gpu_error().is_some(),
            display.front_valid(),
            presented,
            s.display_state().active_base,
            display.linear_rgb565(),
            samples,
            started.elapsed().as_secs_f64(),
            stop,
            markers.last(1).unwrap_or(0),
            markers.last(2).unwrap_or(0)
        );
        std::fs::write(options.output.join("summary-native.json"), summary)
            .map_err(|e| e.to_string())?;
        println!("accepted GPU submissions {accepted}, valid front {}, real swaps {presented}, stop {stop}",display.front_valid());
    }
    if let CpuBackend::Instruction(s) = &system {
        let display = s
            .cpu()
            .device::<cpu_v3_tang_nano_20k::DisplayDevice>(cpu_v3_tang_nano_20k::DISPLAY_DEVICE)
            .unwrap();
        let accepted = pause.seen - usize::from(pause.capture.is_some());
        let stop = if pause.capture.is_some() {
            "pre-submit"
        } else if options.stop_after_present.is_some_and(|n| presented >= n) {
            "present-limit"
        } else if halted {
            "halt"
        } else if executed < budget {
            "sample-limit"
        } else {
            "step-limit"
        };
        std::fs::create_dir_all(&options.output).map_err(|e| e.to_string())?;
        std::fs::write(options.output.join("application.cb06"), &package)
            .map_err(|e| e.to_string())?;
        let summary = format!(
            r#"{{"backend":"instruction","cpu_step_calls":{},"logic_cycles":null,"native_cycles":null,"accepted_gpu_submissions":{},"gpu_busy":{},"gpu_error":{},"front_valid":{},"presented_swaps":{},"active_base_word":{},"linear_rgb565":{},"samples":{},"host_wall_seconds":{},"stop_reason":"{}","vblank_yields":{},"vblank_fast_forward":{},"flash_install":"preloaded-cb06-not-spi-time","input":"no-controller-unless-explicit-xinput","gpu_display":"functional-without-memory-competition"}}"#,
            executed,
            accepted,
            s.gpu().busy(),
            system.gpu_error().is_some(),
            display.front_valid(),
            presented,
            s.display_state().active_base,
            display.linear_rgb565(),
            samples,
            started.elapsed().as_secs_f64(),
            stop,
            vblank_yields,
            options.vblank_fast_forward
        );
        std::fs::write(options.output.join("summary-instruction.json"), summary)
            .map_err(|e| e.to_string())?;
        println!("accepted GPU submissions {accepted}, valid front {}, real swaps {presented}, stop {stop}; instruction host wall {:.3}s",display.front_valid(),started.elapsed().as_secs_f64());
    }
    let summary_path = options.output.join(if options.native {
        "summary-native.json"
    } else {
        "summary-instruction.json"
    });
    let mut summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summary_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    summary["halt_signal"] = serde_json::json!(pause.halt.map(|h| h.0));
    summary["cpu_fault"] = serde_json::json!(cpu_fault);
    std::fs::write(summary_path, serde_json::to_vec_pretty(&summary).unwrap())
        .map_err(|e| e.to_string())?;
    if let Some(error) = system.gpu_error() {
        println!("GPU retained execution error: {error}");
    }
    std::fs::write(
        options.output.join("frame-counters.json"),
        serde_json::to_vec_pretty(&frame_counters).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if let Some(diagnostic) = diagnostic.as_ref() {
        if matches!(
            diagnostic["kind"].as_str(),
            Some("assertion" | "gpu_fault" | "cpu_fault" | "gpu_error")
        ) {
            return Err(cpu_v3_tang_nano_20k::runtime_diagnostics::render(
                diagnostic,
            ));
        }
    }
    if let Some(error) = cpu_fault.or_else(|| system.gpu_error()) {
        return Err(error);
    }
    if let Some(error) = present_error {
        let detail = diagnostic
            .as_ref()
            .filter(|d| d["kind"] == "halt")
            .map(cpu_v3_tang_nano_20k::runtime_diagnostics::render)
            .unwrap_or_default();
        return Err(format!("{error}\n{detail}"));
    }
    Ok(())
}
