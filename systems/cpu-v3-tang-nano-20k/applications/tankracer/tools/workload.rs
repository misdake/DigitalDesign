//! Count submitted native triangles and visible pixels on the static fixture.
mod submission;
mod uv_probe;
use cpu_v3::StepOutcome;
use cpu_v3_tang_nano_20k::{
    system_sim::{CpuV3SystemSim, VBlankMode},
    GPU_COMMAND_COUNT, GPU_COMMAND_START, GPU_DEVICE,
};
use gpu_v2::system::{
    command::abi::{Command, DeviceSubmission},
    functional::Executor,
};
use sha2::{Digest, Sha256};
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
        let submission =
            DeviceSubmission::from_device_words(count.ok_or("GPU START without COUNT")?, base)?;
        let mut opaque_only = memory.to_vec();
        let mut draws = [[0; 2]; 2];
        let mut slots = [None; 2];
        let mut draw_kinds = std::collections::BTreeMap::new();
        let mut geometry_fingerprint = 0xcbf29ce484222325_u64;
        for address in submission::command_addresses(memory, submission)? {
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
                    let header = memory
                        .get(record..record + 5)
                        .ok_or("Truncated meshlet header")?;
                    geometry_fingerprint = fingerprint(geometry_fingerprint, header[0]);
                    let billboard = (header[4] & 1) as usize;
                    if let Some(previous) = draw_kinds.insert(address, billboard) {
                        if previous != billboard {
                            return Err("Aliased DRAW changes geometry kind across slices");
                        }
                    }
                    draws[billboard][0] += 1;
                    draws[billboard][1] += u32::from(header[0] >> 8);
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
        probe_uv: bool,
        probe_samples: bool,
        layout: CpuLayout,
    ) -> Result<(), Box<dyn Error>> {
        let report = replay.execute(&mut self.opaque_only, self.submission)?;
        let [color, depth] = report.effects.target.ok_or("submission has no target")?;
        let actual = machine.cpu().physical_memory_slice();
        if probe_uv {
            let mut full = self.opaque_only.clone();
            for at in submission::command_addresses(&full, self.submission)? {
                full[at..at + 2].copy_from_slice(&actual[at..at + 2]);
            }
            let mut executor = if probe_samples {
                Executor::new(6_000_000).with_texture_samples(48, 100_000)
            } else {
                Executor::new(6_000_000).with_texture_probe()
            };
            let observed = executor.execute(&mut full, self.submission)?;
            if let Some(folder) = capture {
                uv_probe::write(folder, frame, observed.textures.as_ref().unwrap())?;
                if let Some(samples) = &observed.samples {
                    uv_probe::write_samples(folder, frame, samples)?;
                }
            }
            for base in [color, depth] {
                if full[base as usize / 2..base as usize / 2 + 102400]
                    != actual[base as usize / 2..base as usize / 2 + 102400]
                {
                    return Err("UV probe changed the full replay output".into());
                }
            }
        }
        let (heap_live, heap_largest_free) = heap_usage(actual, layout)?;
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
                // Binary-alpha billboards now test alpha and write depth. Their
                // accepted pixels can only bring LessEqual depth nearer.
                if actual[z] > self.opaque_only[z] {
                    return Err("billboard depth moved behind the opaque reference".into());
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
                "count_word": self.submission.count_minus_one,
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
                "data_segments": machine.cpu().data_segments(),
            });
            if self.submission.segment_count().is_none() {
                metadata["words"] = (u32::from(self.submission.count_minus_one) + 1).into();
            }
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
#[derive(Clone, Copy)]
struct CpuLayout {
    heap_begin: usize,
    heap_end: usize,
    stack_top: usize,
}
impl CpuLayout {
    fn load(path: Option<&Path>, package: &[u8]) -> Result<Self, Box<dyn Error>> {
        let Some(path) = path else {
            return Ok(Self {
                heap_begin: 0x2000,
                heap_end: 0x9fff,
                stack_top: 0xc000,
            });
        };
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let file = manifest["package"]
            .as_str()
            .ok_or("layout package missing")?;
        let record = &manifest["files"][file];
        let hash = format!("{:x}", Sha256::digest(package));
        if record["bytes"].as_u64() != Some(package.len() as u64)
            || record["sha256"].as_str() != Some(hash.as_str())
        {
            return Err("layout does not describe this CB06 package".into());
        }
        let target = &manifest["target"];
        let value = |key: &str| -> Result<usize, Box<dyn Error>> {
            let n = target[key].as_u64().ok_or("layout field missing")?;
            if n > 65535 {
                return Err("layout field exceeds the CPU address space".into());
            }
            Ok(n as usize)
        };
        let heap_begin = value("heap_begin")?;
        let heap_end = heap_begin + value("heap_words")?;
        let stack_top = value("stack_top")?;
        if heap_end <= heap_begin + 3 || heap_end >= stack_top || stack_top > 65535 {
            return Err("invalid heap/stack layout".into());
        }
        Ok(Self {
            heap_begin,
            heap_end,
            stack_top,
        })
    }
}

// Includes allocation tags; samples are taken at accepted submissions.
fn heap_usage(words: &[u16], layout: CpuLayout) -> Result<(usize, usize), &'static str> {
    if layout.heap_end > words.len() || layout.heap_begin >= layout.heap_end {
        return Err("heap extent is outside captured memory");
    }
    let mut at = layout.heap_begin;
    let mut live = 0;
    let mut largest_free = 0;
    while at < layout.heap_end {
        let flag = words[at];
        let size = usize::from(flag & 32767);
        if size < 4 || at + size > layout.heap_end || words[at + size - 1] != flag {
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
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut layout_path = None;
    if let Some(index) = args.iter().position(|arg| arg == "--layout") {
        let value = args
            .get(index + 1)
            .ok_or("--layout needs an application manifest")?;
        layout_path = Some(PathBuf::from(value));
        args.drain(index..index + 2);
    }
    let probe_samples = args.last().is_some_and(|v| v == "--uv-samples");
    if probe_samples {
        args.pop();
    }
    let probe_uv = args.last().is_some_and(|v| v == "--uv-probe");
    if probe_uv {
        args.pop();
    }
    let probe_uv = probe_uv || probe_samples;
    if !(3..=5).contains(&args.len()) || (args.len() == 5 && args[4] != "--images-only") {
        return Err(
            "usage: tankracer-static-workload CB06 FRAMES MAX_CPU_STEPS [CAPTURE_DIRECTORY [--images-only]] [--uv-probe|--uv-samples] [--layout application.build.json]".into(),
        );
    }
    let frame_limit: usize = args[1].parse()?;
    let step_limit: usize = args[2].parse()?;
    if frame_limit == 0 || step_limit == 0 {
        return Err("frame and instruction limits must be positive".into());
    }
    let capture = args.get(3).map(PathBuf::from);
    if probe_uv && capture.is_none() {
        return Err("UV probes require a capture directory".into());
    }
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
    let layout = CpuLayout::load(layout_path.as_deref(), &program)?;
    if probe_uv {
        if let Some(folder) = &capture {
            let identity = program.iter().fold(0xcbf29ce484222325_u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
            std::fs::write(
                folder.join("uv-probe.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "format":"tankracer.uv-probe-run.v1","package_bytes":program.len(),
                    "package_fnv64":format!("{identity:016x}"),"requested_frames":frame_limit,
                    "max_cpu_steps":step_limit,
                "sample_minimum_lod_eighths":if probe_samples { Some(48) } else { None },
                }))?,
            )?;
        }
    }
    let mut machine = CpuV3SystemSim::from_single_flash(&program, 0xd00d, VBlankMode::Manual)
        .map_err(|err| format!("{err:?}"))?
        .with_functional_gpu();
    let mut count = None;
    let mut seen = 0;
    let mut replay = Executor::new(3_000_000);
    let mut instructions = 0_u64;
    let mut previous_submit = 0_u64;
    let mut lowest_stack = layout.stack_top;
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
        if (layout.heap_end..=layout.stack_top).contains(&stack) {
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
                (layout.stack_top - lowest_stack) * 2,
                probe_uv,
                probe_samples,
                layout,
            )?;
            previous_submit = instructions;
            if seen == frame_limit {
                return Ok(());
            }
        }
    }
    Err(format!("reached the instruction bound after {seen} submissions").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpu_v2::system::command::abi::CommandSlice;

    #[test]
    fn workload_keeps_dma_slots_across_slices_and_removes_only_billboard_draws() {
        let mut memory = vec![0; 1024];
        memory[512] = 2 << 8;
        memory[576] = 3 << 8;
        memory[580] = 1;
        let put = |memory: &mut [u16], at: usize, command: Command| {
            memory[at] = command.0 as u16;
            memory[at + 1] = (command.0 >> 16) as u16;
        };
        put(
            &mut memory,
            132,
            Command::dma_meshlet(1024, 0, 128).unwrap(),
        );
        put(&mut memory, 134, Command::END);
        put(&mut memory, 258, Command::draw_world_meshlet(0).unwrap());
        put(
            &mut memory,
            260,
            Command::dma_meshlet(1152, 1, 128).unwrap(),
        );
        put(
            &mut memory,
            262,
            Command::draw_world_billboard(1, true).unwrap(),
        );
        put(&mut memory, 264, Command::END);
        let (list, table) = DeviceSubmission::segment_table(
            128,
            &[
                CommandSlice::new(264, 8).unwrap(),
                CommandSlice::new(516, 16).unwrap(),
            ],
        )
        .unwrap();
        for (i, pair) in table.as_chunks::<2>().0.iter().enumerate() {
            memory[64 + i] = u16::from_le_bytes(*pair);
        }
        let result =
            Workload::observe(&memory, list.base_sector, Some(list.count_minus_one)).unwrap();
        assert_eq!(result.draws, [[1, 2], [1, 3]]);
        assert_eq!(result.opaque_only[259], memory[259]);
        assert_eq!(&result.opaque_only[262..264], &[0, 0]);
        assert_eq!(&result.opaque_only[64..128], &memory[64..128]);
    }
}

#[cfg(test)]
mod heap_tests {
    use super::*;
    fn heap() -> (Vec<u16>, CpuLayout) {
        let mut words = vec![0x7e7e; 32];
        words[8] = 6;
        words[13] = 6;
        words[14] = 0x8006;
        words[19] = 0x8006;
        (
            words,
            CpuLayout {
                heap_begin: 8,
                heap_end: 20,
                stack_top: 28,
            },
        )
    }
    #[test]
    fn measures_only_the_declared_heap() {
        let (words, layout) = heap();
        assert_eq!(heap_usage(&words, layout), Ok((12, 12)));
    }
    #[test]
    fn rejects_a_damaged_footer_and_a_block_crossing_the_heap_end() {
        let (mut words, layout) = heap();
        words[19] = 0x8005;
        assert!(heap_usage(&words, layout).is_err());
        words[14] = 0x8008;
        assert!(heap_usage(&words, layout).is_err());
    }
    #[test]
    fn rejects_an_extent_outside_the_captured_memory() {
        let (words, mut layout) = heap();
        layout.heap_end = 33;
        assert!(heap_usage(&words, layout).is_err());
    }
}
