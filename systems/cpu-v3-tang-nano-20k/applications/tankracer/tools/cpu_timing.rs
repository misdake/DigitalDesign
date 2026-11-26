//! Bounded CPU/cache/native SDRAM timing with passive execute-PC occupancy.
use cpu_v3::Device;
use cpu_v3_tang_nano_20k::{
    system_emu::CpuV3SystemEmu, system_sim::VBlankMode, GpuDevice, GPU_COMMAND_START, GPU_DEVICE,
};
use std::{
    any::Any,
    cell::Cell,
    error::Error,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

// Controlled overlap experiment only: actual rendering remains functional and
// synchronous, while BUSY retires at a fixed CPU clock deadline. No bandwidth
// or end-to-end system timing claim follows from this artificial service time.
struct TimedGpu {
    inner: GpuDevice,
    clock: Rc<Cell<u64>>,
    service: u64,
    ready: u64,
}
impl Device for TimedGpu {
    fn read(&mut self, memory: &mut [u16], channel: u8) -> u16 {
        if channel == 2 && self.clock.get() < self.ready {
            return 1;
        }
        self.inner.read(memory, channel)
    }
    fn write(&mut self, memory: &mut [u16], channel: u8, value: u16) {
        assert!(
            self.clock.get() >= self.ready,
            "submission before timed GPU retirement"
        );
        self.inner.write(memory, channel, value);
        if channel == GPU_COMMAND_START {
            self.ready = self.clock.get() + 1 + self.service;
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn gpu(machine: &CpuV3SystemEmu, service: u64) -> &GpuDevice {
    if service == 0 {
        machine.gpu()
    } else {
        &machine
            .device::<TimedGpu>(GPU_DEVICE)
            .expect("timed GPU peer")
            .inner
    }
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 5 && !(args.len() == 7 && args[5] == "--gpu-cycles") {
        return Err(
            "usage: tankracer-cpu-timing CB06 FRAMES MAX_CYCLES MAX_SECONDS OUTPUT_DIRECTORY [--gpu-cycles N]"
                .into(),
        );
    }
    let frames: usize = args[1].parse()?;
    let maximum: u64 = args[2].parse()?;
    let seconds: u64 = args[3].parse()?;
    let service: u64 = if args.len() == 7 { args[6].parse()? } else { 0 };
    let output = Path::new(&args[4]);
    if !(1..=16).contains(&frames) || maximum == 0 || seconds == 0 || output.exists() {
        return Err(
            "require 1..16 frames, positive cycle/time bounds and a fresh output directory".into(),
        );
    }
    std::fs::create_dir_all(output)?;
    let bytes = std::fs::read(&args[0])?;
    let identity = bytes.iter().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    let mut machine = CpuV3SystemEmu::from_single_flash(&bytes, 0xd00d, VBlankMode::Manual)?
        .with_functional_gpu();
    let clock = Rc::new(Cell::new(0));
    if service != 0 {
        machine.attach_device(
            GPU_DEVICE,
            Box::new(TimedGpu {
                inner: GpuDevice::functional(),
                clock: clock.clone(),
                service,
                ready: 0,
            }),
        );
    }
    let mut histogram = vec![0_u64; 65536];
    let mut unassigned = 0_u64;
    let mut previous_cycle = 0;
    let mut previous_retired = 0;
    let mut samples = Vec::new();
    let started = Instant::now();
    while machine.cycles() < maximum {
        clock.set(machine.cycles());
        if machine.cycles() % 4096 == 0 && started.elapsed() >= Duration::from_secs(seconds) {
            break;
        }
        if machine.waiting_for_vblank() || machine.blocked_on_vblank() {
            machine.advance_vblank();
        }
        let pc = machine.core().executing_instruction_address();
        if let Some((7, address)) = pc {
            histogram[usize::from(address)] += 1;
        } else {
            unassigned += 1;
        }
        let mut submitted = false;
        let value = machine
            .step_observing(|write, _| {
                submitted = write.device == GPU_DEVICE && write.channel == GPU_COMMAND_START;
                true
            })
            .map_err(|error| format!("CPU/native-memory step: {error:?}"))?
            .expect("observer always advances");
        if value.fault || value.halted {
            return Err(format!(
                "CPU stopped: fault={} pc={} halt={}",
                value.fault_code, value.fault_pc, value.halt_signal
            )
            .into());
        }
        if let Some(error) = gpu(&machine, service).last_error() {
            return Err(error.to_string().into());
        }
        if submitted {
            let frame = samples.len() + 1;
            let cycle = machine.cycles();
            let retired = u64::from(machine.core().retired_words());
            let [color, depth] = gpu(&machine, service)
                .last_effects()
                .target
                .ok_or("submission has no target")?;
            let memory = machine.physical_memory();
            let mut rgb = 0xcbf29ce484222325_u64;
            let mut d16 = rgb;
            for y in 0..240 {
                for x in 0..400 {
                    let at = ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16;
                    rgb = (rgb ^ u64::from(memory[color as usize / 2 + at]))
                        .wrapping_mul(0x100000001b3);
                    d16 = (d16 ^ u64::from(memory[depth as usize / 2 + at]))
                        .wrapping_mul(0x100000001b3);
                }
            }
            drop(memory);
            let sample = serde_json::json!({"frame":frame,"cycles":cycle-previous_cycle,
                "milliseconds_at_54mhz":(cycle-previous_cycle) as f64/54000.0,
                "retired_words":retired-previous_retired,"execute_occupancy":histogram.iter().sum::<u64>(),
                "start_cycle":cycle,"gap_after_service_cycles":if frame > 1 && service != 0 { Some(cycle.saturating_sub(previous_cycle+service)) } else { None },
                "rgb565_fingerprint":format!("{rgb:016x}"),"d16_fingerprint":format!("{d16:016x}"),
                "unassigned_cycles":unassigned});
            println!("{sample}");
            let data = histogram
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            std::fs::write(output.join(format!("frame-{frame:03}.cpu-execute64")), data)?;
            samples.push(sample);
            previous_cycle = cycle;
            previous_retired = retired;
            histogram.fill(0);
            unassigned = 0;
            if frame == frames {
                break;
            }
        }
    }
    let complete = samples.len() == frames;
    std::fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format":"tankracer.cpu-timing.v1","package_fnv64":format!("{identity:016x}"),
            "synthetic_gpu_service_cycles":service,
            "scope":"CPU core, I/D caches, arbiter and native SDRAM; functional GPU/Display without memory competition; manual vblank release; excludes SPI boot time and board FPS",
            "window":"consecutive GPU START edges; first includes initialization; execute histogram counts actual scalar execute occupancy only, not retirement or fetch/FPU-only cycles",
            "complete":complete,"max_cycles":maximum,"max_host_seconds":seconds,
            "host_seconds":started.elapsed().as_secs_f64(),"samples":samples
        }))?,
    )?;
    if !complete {
        return Err("bounded CPU timing did not reach all requested submissions".into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(run)?
        .join()
        .map_err(|_| "CPU timing worker panicked")?
}
