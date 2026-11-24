//! Count submitted native triangles and visible pixels on the static fixture.
use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::{
    system_sim::{CpuV3SystemSim, VBlankMode},
    GPU_COMMAND_COUNT, GPU_COMMAND_START, GPU_DEVICE,
};
use gpu_v2::system::{
    command::abi::{Command, DeviceSubmission},
    functional::Executor,
};
use std::{
    error::Error,
    path::{Path, PathBuf},
};

struct Workload {
    opaque_only: Vec<u16>,
    draws: [[u32; 2]; 2],
    submission: DeviceSubmission,
    geometry_fingerprint: u64,
}

impl Workload {
    fn observe(memory: &[u16], base: u16, count: Option<u16>) -> Result<Self, &'static str> {
        let bytes = (u32::from(count.ok_or("GPU START without COUNT")?) + 1) * 4;
        let address = u32::from(base) * 128;
        let submission = DeviceSubmission::new(address, bytes)?;
        let mut opaque_only = memory.to_vec();
        let mut draws = [[0; 2]; 2];
        let mut slots = [None; 2];
        let mut geometry_fingerprint = 0xcbf29ce484222325_u64;
        let first = address as usize / 2;
        for address in (first..first + bytes as usize / 2).step_by(2) {
            let low = memory[address];
            let high = memory[address + 1];
            let slot = (low & 1) as usize;
            match high >> 12 {
                2 => {
                    let sector = ((high & 4095) as usize) << 4 | (low >> 12) as usize;
                    slots[slot] = Some(sector * 64);
                }
                3 => {
                    let record = slots[slot].ok_or("DRAW without native meshlet DMA")?;
                    geometry_fingerprint = fingerprint(geometry_fingerprint, record as u16);
                    geometry_fingerprint = fingerprint(geometry_fingerprint, (record >> 16) as u16);
                    geometry_fingerprint = fingerprint(geometry_fingerprint, memory[record]);
                    let billboard = (memory[record + 4] & 1) as usize;
                    draws[billboard][0] += 1;
                    draws[billboard][1] += u32::from(memory[record] >> 8);
                    if billboard != 0 {
                        opaque_only[address] = Command::NOP.0 as u16;
                        opaque_only[address + 1] = (Command::NOP.0 >> 16) as u16;
                    }
                }
                _ => {}
            }
        }
        Ok(Self {
            opaque_only,
            draws,
            submission,
            geometry_fingerprint,
        })
    }

    #[allow(clippy::too_many_arguments)] // Explicit inputs to one frame's output record.
    fn report(
        mut self,
        frame: usize,
        machine: &CpuV3SystemSim,
        replay: &mut Executor,
        capture: Option<&Path>,
        cpu_instructions: u64,
        gpu_capture: bool,
        stack_high_water: usize,
    ) -> Result<(), Box<dyn Error>> {
        let report = replay.execute(&mut self.opaque_only, self.submission)?;
        let [color, depth] = report.effects.target.ok_or("submission has no target")?;
        let actual = machine.cpu().physical_memory_slice();
        let (heap_live, heap_largest_free) = heap_usage(actual)?;
        let mut opaque_pixels = 0;
        let mut billboard_pixels = 0;
        let mut color_fingerprint = 0xcbf29ce484222325_u64;
        let mut depth_fingerprint = 0xcbf29ce484222325_u64;
        let mut ppm = capture.map(|_| b"P6\n400 240\n255\n".to_vec());
        let mut depth_image = capture.map(|_| Vec::with_capacity(400 * 240 * 2));
        for y in 0..240 {
            for x in 0..400 {
                let index = ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16;
                let z = depth as usize / 2 + index;
                if actual[z] != self.opaque_only[z] {
                    return Err("billboards changed the depth plane".into());
                }
                opaque_pixels += usize::from(actual[z] != u16::MAX);
                let rgb = color as usize / 2 + index;
                billboard_pixels += usize::from(actual[rgb] != self.opaque_only[rgb]);
                color_fingerprint = fingerprint(color_fingerprint, actual[rgb]);
                depth_fingerprint = fingerprint(depth_fingerprint, actual[z]);
                if let Some(image) = &mut depth_image {
                    image.extend(actual[z].to_le_bytes());
                }
                if let Some(image) = &mut ppm {
                    let p = actual[rgb];
                    image.extend([
                        ((p >> 11) * 255 / 31) as u8,
                        (((p >> 5) & 63) * 255 / 63) as u8,
                        ((p & 31) * 255 / 31) as u8,
                    ]);
                }
            }
        }
        let [[meshlets, triangles], [billboard_meshlets, billboard_triangles]] = self.draws;
        if let Some(folder) = capture {
            std::fs::write(folder.join(format!("frame-{frame:03}.ppm")), ppm.unwrap())?;
            std::fs::write(
                folder.join(format!("frame-{frame:03}.d16")),
                depth_image.unwrap(),
            )?;
            let mut metadata = serde_json::json!({
                "frame": frame,
                "rgb_file": format!("frame-{frame:03}.ppm"),
                "depth_file": format!("frame-{frame:03}.d16"),
                "base": self.submission.base_sector as u32 * 128,
                "words": u32::from(self.submission.count_minus_one) + 1,
                "color": color, "depth": depth,
                "meshlets": meshlets + billboard_meshlets,
                "triangles": triangles + billboard_triangles,
                "billboard_triangles": billboard_triangles,
                "rgb565_fingerprint": format!("{color_fingerprint:016x}"),
                "d16_fingerprint": format!("{depth_fingerprint:016x}"),
                "cpu_instructions_since_submit": cpu_instructions,
                "geometry_order_fingerprint": format!("{:016x}", self.geometry_fingerprint),
                "heap_live_bytes": heap_live,
                "heap_largest_free_bytes": heap_largest_free,
                "stack_high_water_bytes": stack_high_water,
            });
            if gpu_capture {
                metadata["format"] = "tankracer.gpu-input.v1".into();
                metadata["image"] = format!("frame-{frame:03}.bin").into();
            } else {
                metadata["format"] = "tankracer.frame-output.v1".into();
            }
            std::fs::write(
                folder.join(format!("frame-{frame:03}.json")),
                serde_json::to_vec_pretty(&metadata)?,
            )?;
        }
        println!(
            "{frame},{meshlets},{triangles},{billboard_meshlets},{billboard_triangles},{opaque_pixels},{billboard_pixels},{color_fingerprint:016x},{depth_fingerprint:016x},{cpu_instructions},{:016x},{heap_live},{heap_largest_free},{stack_high_water}", self.geometry_fingerprint
        );
        Ok(())
    }
}

fn fingerprint(hash: u64, pixel: u16) -> u64 {
    (hash ^ u64::from(pixel)).wrapping_mul(0x100000001b3)
}

// The board's fixed boundary-tag heap occupies physical words [0x2000,0x9fff).
// Includes allocation tags; samples are taken at accepted submissions.
fn heap_usage(words: &[u16]) -> Result<(usize, usize), &'static str> {
    let mut at = 0x2000;
    let mut live = 0;
    let mut largest_free = 0;
    while at < 0x9fff {
        let flag = words[at];
        let size = usize::from(flag & 32767);
        if size < 4 || at + size > 0x9fff || words[at + size - 1] != flag {
            return Err("invalid board heap boundary tags at submission");
        }
        if flag & 32768 == 0 {
            live += size * 2;
        } else {
            largest_free = largest_free.max(size * 2);
        }
        at += size;
    }
    Ok((live, largest_free))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !(3..=5).contains(&args.len()) || (args.len() == 5 && args[4] != "--images-only") {
        return Err(
            "usage: tankracer-static-workload CB06 FRAMES MAX_CPU_STEPS [CAPTURE_DIRECTORY [--images-only]]".into(),
        );
    }
    let frame_limit: usize = args[1].parse()?;
    let step_limit: usize = args[2].parse()?;
    if frame_limit == 0 || step_limit == 0 {
        return Err("frame and instruction limits must be positive".into());
    }
    let capture = args.get(3).map(PathBuf::from);
    let gpu_capture = args.len() == 4;
    if let Some(folder) = &capture {
        let maximum = if gpu_capture { 16 } else { 192 };
        if frame_limit > maximum || folder.exists() {
            return Err(
                "Capture requires a fresh directory and at most 16 GPU / 192 image frames".into(),
            );
        }
        std::fs::create_dir_all(folder)?;
    }
    let program = std::fs::read(&args[0])?;
    let mut machine = CpuV3SystemSim::from_single_flash(&program, 0xd00d, VBlankMode::Manual)
        .map_err(|err| format!("{err:?}"))?
        .with_functional_gpu();
    let mut count = None;
    let mut seen = 0;
    let mut replay = Executor::new(3_000_000);
    let mut instructions = 0_u64;
    let mut previous_submit = 0_u64;
    let mut lowest_stack = 0xc000;
    let mut stack_ready = false;
    let mut pc_counts = gpu_capture.then(|| vec![0_u64; 65536]);
    println!(
        "frame,meshlets,triangles,billboard_meshlets,billboard_triangles,opaque_pixels,billboard_visible_pixels,rgb565_fingerprint,d16_fingerprint,cpu_instructions_since_submit,geometry_order_fingerprint,heap_live_bytes,heap_largest_free_bytes,stack_high_water_bytes"
    );
    for _ in 0..step_limit {
        if machine.blocked_on_vblank() {
            machine.advance_vblank();
        }
        let mut observation = None;
        let pc = usize::from(machine.cpu().pc());
        let step = machine
            .step_observing(|write, memory| {
                if write.device == GPU_DEVICE {
                    match write.channel {
                        GPU_COMMAND_COUNT => count = Some(write.value),
                        GPU_COMMAND_START => {
                            if gpu_capture {
                                if let Some(folder) = &capture {
                                    let mut bytes = Vec::with_capacity(memory.len() * 2);
                                    for word in memory {
                                        bytes.extend(word.to_le_bytes());
                                    }
                                    std::fs::write(
                                        folder.join(format!("frame-{:03}.bin", seen + 1)),
                                        bytes,
                                    )
                                    .expect("write bounded GPU capture");
                                }
                            }
                            observation =
                                Some(Workload::observe(memory, write.value, count.take()));
                        }
                        _ => {}
                    }
                }
                true
            })
            .map_err(|err| format!("{err:?}"))?;
        instructions += u64::from(step.is_some());
        let stack = usize::from(machine.cpu().registers()[13]);
        if (0x9fff..=0xc000).contains(&stack) {
            stack_ready = true;
            lowest_stack = lowest_stack.min(stack);
        } else if stack_ready {
            return Err("CPU stack left its resident board allocation".into());
        }
        if step.is_some() {
            if let Some(counts) = &mut pc_counts {
                counts[pc] += 1;
            }
        }
        if let Some(StepOutcome::Halted { signal }) = step {
            let memory = machine.cpu().physical_memory_slice();
            let fault = memory[..8192].windows(16).find(|words| words[0] == 0xfa17);
            if let Some(folder) = &capture {
                std::fs::write(
                    folder.join("cpu-failure.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "instructions":instructions,"registers":machine.cpu().registers(),
                        "fault":fault,"resident_words":&memory[..65536],
                    }))?,
                )?;
            }
            return Err(format!("CPU halted with {signal:#06x} after {instructions} instructions; GPU fault {fault:?}").into());
        }
        if let Some(error) = machine.gpu().last_error() {
            return Err(error.to_string().into());
        }
        if let Some(workload) = observation {
            seen += 1;
            if let (Some(folder), Some(counts)) = (&capture, &mut pc_counts) {
                let mut bytes = Vec::with_capacity(counts.len() * 8);
                for value in counts.iter_mut() {
                    bytes.extend(value.to_le_bytes());
                    *value = 0;
                }
                std::fs::write(folder.join(format!("frame-{seen:03}.cpu-pc64")), bytes)?;
            }
            workload?.report(
                seen,
                &machine,
                &mut replay,
                capture.as_deref(),
                instructions - previous_submit,
                gpu_capture,
                (0xc000 - lowest_stack) * 2,
            )?;
            previous_submit = instructions;
            if seen == frame_limit {
                return Ok(());
            }
        }
    }
    Err(format!("reached the instruction bound after {seen} submissions").into())
}
