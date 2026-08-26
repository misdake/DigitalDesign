use cpu_v3::Machine;
use cpu_v3_tang_nano_20k::display::{render_frame, write_ppm};
#[cfg(feature = "display-window")]
use cpu_v3_tang_nano_20k::display::{HDMI_HEIGHT, HDMI_WIDTH};
use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/display_image.rs"));

struct Options {
    max_cpu_steps: usize,
    frames: usize,
    ppm: bool,
    window: bool,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        max_cpu_steps: 5_000_000,
        frames: 120,
        ppm: false,
        window: false,
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
            "--window" => options.window = true,
            _ => return Err(format!("unknown argument `{arg}`")),
        }
    }
    if options.max_cpu_steps == 0 || options.frames == 0 {
        return Err("step and frame limits must be non-zero".into());
    }
    Ok(options)
}

fn main() -> Result<(), String> {
    let options = options()?;
    let mut machine = Machine::default();
    machine
        .load_program(0, DISPLAY_DEMO_PROGRAM)
        .map_err(|error| format!("cannot load display demo: {error:?}"))?;

    #[cfg(feature = "display-window")]
    let mut window = options.window.then(|| {
        minifb::Window::new(
            "CPU V3 320x240 framebuffer (3x HDMI preview)",
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

    let chunk = options.max_cpu_steps.div_ceil(options.frames);
    let mut pixels = Vec::new();
    let mut executed = 0;
    for _ in 0..options.frames {
        let next = (executed + chunk).min(options.max_cpu_steps);
        while executed < next {
            machine
                .step()
                .map_err(|error| format!("CPU fault after {executed} steps: {error:?}"))?;
            executed += 1;
        }
        pixels = render_frame(&machine);
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
    println!(
        "simulated {executed} CPU steps and {} display samples",
        options.frames
    );
    Ok(())
}
