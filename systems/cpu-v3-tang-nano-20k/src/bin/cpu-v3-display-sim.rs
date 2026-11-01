//! Host display simulator for the configured single application.
//!
//! `boot-applications.conf` selects the source. The build script generates the
//! same CB06 Flash package as the FPGA; the default CPU-only diagnostic fills
//! and presents alternating RGB565 surfaces.

use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::display::{render_frame_at, write_ppm};
#[cfg(feature = "display-window")]
use cpu_v3_tang_nano_20k::display::{ACTIVE_DISPLAY_CONFIG, HDMI_HEIGHT, HDMI_WIDTH};
use cpu_v3_tang_nano_20k::host_io::{HostIo, RealtimeHostIo};
use cpu_v3_tang_nano_20k::system_sim::{CpuV3SystemSim, VBlankMode};
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

const GENERATED_APPLICATION_PACKAGE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/cpu-v3-boot.bin"));

struct Options {
    max_cpu_steps: usize,
    frames: usize,
    ppm: bool,
    window: bool,
    realtime: bool,
    host_tick_us: u32,
    xinput_slot: Option<u32>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        max_cpu_steps: 5_000_000,
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
            "--max-cpu-steps" => {
                options.max_cpu_steps = args
                    .next()
                    .ok_or("--max-cpu-steps requires a value")?
                    .parse()
                    .map_err(|_| "invalid --max-cpu-steps")?;
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
    if options.max_cpu_steps == 0 || options.frames == 0 {
        return Err("step and frame limits must be non-zero".into());
    }
    if options.host_tick_us == 0 || options.host_tick_us > 1_000_000 {
        return Err("--host-tick-us must be between 1 and 1000000".into());
    }
    Ok(options)
}

fn main() -> Result<(), String> {
    let options = options()?;
    let mut system = CpuV3SystemSim::from_single_flash(
        GENERATED_APPLICATION_PACKAGE,
        0xd00d,
        VBlankMode::Manual,
    )
    .map_err(|error| format!("cannot cold-boot display application: {error}"))?
    .with_functional_gpu();

    #[cfg(feature = "display-window")]
    let mut window = options.window.then(|| {
        minifb::Window::new(
            &format!(
                "CPU V3 320x240 framebuffer ({} HDMI preview)",
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
    let chunk = options.max_cpu_steps.div_ceil(options.frames);
    let mut pixels = Vec::new();
    let mut executed = 0_usize;
    let mut halted = false;
    let mut samples = 0;
    let mut focus_checked = Instant::now();
    for _ in 0..options.frames {
        let mut focused = window_focus(&mut window);
        if let Some(host) = realtime.as_mut() {
            host.pump(&mut system, focused);
        } else {
            deterministic.advance(
                &mut system,
                Duration::from_micros(u64::from(options.host_tick_us)),
                focused,
            );
        }
        let next = executed.saturating_add(chunk).min(options.max_cpu_steps);
        while executed < next && !halted {
            if let Some(host) = realtime.as_mut() {
                if focus_checked.elapsed() >= Duration::from_millis(8) {
                    focused = window_focus(&mut window);
                    focus_checked = Instant::now();
                }
                host.pump(&mut system, focused);
            }
            halted = matches!(
                system
                    .step()
                    .map_err(|error| format!("CPU fault after {executed} steps: {error:?}"))?,
                StepOutcome::Halted { .. }
            );
            executed += 1;
        }
        let _ = system.advance_vblank();
        samples += 1;
        pixels = if system
            .cpu()
            .device::<cpu_v3_tang_nano_20k::DisplayDevice>(cpu_v3_tang_nano_20k::DISPLAY_DEVICE)
            .is_some_and(cpu_v3_tang_nano_20k::DisplayDevice::front_valid)
        {
            render_frame_at(system.cpu(), system.display_state().active_base)
        } else {
            vec![
                0;
                cpu_v3_tang_nano_20k::display::HDMI_WIDTH
                    * cpu_v3_tang_nano_20k::display::HDMI_HEIGHT
            ]
        };
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
    if options.ppm {
        let path = PathBuf::from("target/display-sim/frame-final.ppm");
        write_ppm(&path, &pixels).map_err(|error| format!("write {}: {error}", path.display()))?;
        println!("wrote {}", path.display());
    }
    println!("simulated {executed} CPU step calls and {samples} explicit display samples");
    Ok(())
}
