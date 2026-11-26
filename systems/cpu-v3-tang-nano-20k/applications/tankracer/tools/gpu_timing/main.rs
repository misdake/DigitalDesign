//! Bounded cold command replay through the GPU and native SDRAM cycle models.
mod memory;
#[path = "../submission.rs"]
mod submission;
mod timeline;
mod traffic;
use gpu_v2::system::vliw_cycle::{
    command::DeviceBus,
    draw::{Gpu, Input},
    postquad::Profile,
    router,
};
use std::{
    error::Error,
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 5 && !(args.len() == 6 && args[5] == "--trace") {
        return Err("usage: tankracer-gpu-timing FRAME.json MAX_CYCLES MAX_SECONDS TRAFFIC OUTPUT_DIRECTORY [--trace]".into());
    }
    let maximum: usize = args[1].parse()?;
    let seconds: u64 = args[2].parse()?;
    if maximum == 0 || maximum > 20_000_000 || seconds == 0 || seconds > 1800 {
        return Err("Cycle and host limits must be positive and bounded".into());
    }
    if !["idle", "display", "display_cpu", "display_cpu_heavy"].contains(&args[3].as_str()) {
        return Err("Unknown synthetic contention profile".into());
    }
    let input = Path::new(&args[0]);
    let source: serde_json::Value = serde_json::from_slice(&fs::read(input)?)?;
    if source["format"] != "tankracer.gpu-input.v1" {
        return Err("GPU input format".into());
    }
    let image = source["image"].as_str().ok_or("Missing preimage")?;
    if image.contains(['/', '\\', ':']) || !image.ends_with(".bin") {
        return Err("Invalid preimage path".into());
    }
    let bytes = fs::read(input.parent().ok_or("Missing input folder")?.join(image))?;
    if bytes.len() != 8 * 1024 * 1024 {
        return Err("GPU preimage must cover 8 MiB".into());
    }
    let color = usize::try_from(source["color"].as_u64().ok_or("Missing color target")?)?;
    let depth = usize::try_from(source["depth"].as_u64().ok_or("Missing depth target")?)?;
    if !color.is_multiple_of(65536)
        || !depth.is_multiple_of(65536)
        || color > bytes.len() - 204800
        || depth > bytes.len() - 204800
    {
        return Err("Framebuffer extent".into());
    }
    let submission = submission::from_capture(&source)?;
    let output = Path::new(&args[4]);
    if output.exists() {
        return Err("GPU timing needs a fresh output directory".into());
    }
    fs::create_dir_all(output)?;
    let mut backing = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        backing.push(u16::from_le_bytes(*pair));
    }
    let addresses = submission::command_addresses(&backing, submission)?;
    let mut timeline = (args.len() == 6).then(|| timeline::Timeline::new(&backing, addresses));
    let mut memory = memory::Memory::new(backing, traffic::Traffic::new(&args[3], color, 0));
    let mut gpu = Gpu::with_all_commands(Profile::Native);
    let mut start = None;
    let mut fragments = 0;
    let mut meshlets = 0;
    let mut texture_beats = 0;
    let clock_start = Instant::now();
    for cycle in 0..maximum {
        if cycle & 4095 == 0 && clock_start.elapsed() > Duration::from_secs(seconds) {
            return Err(format!("Host deadline at cycle {cycle}").into());
        }
        let mut i = Input {
            reset: cycle < 2,
            ce: true,
            bus: DeviceBus {
                index: 4,
                channel: 2,
                read: true,
                ..Default::default()
            },
            ..Default::default()
        };
        if cycle == 16384 || cycle == 16385 {
            let count = cycle == 16384;
            i.bus = DeviceBus {
                index: 4,
                channel: u8::from(!count),
                write: true,
                data: if count {
                    submission.count_minus_one
                } else {
                    submission.base_sector
                },
                ..Default::default()
            };
        }
        let mut feedback_pass = 0;
        let o = loop {
            let o = gpu.outputs(i);
            let feedback = memory.feedback(
                router::Output {
                    physical: o.physical,
                    write_data: o.write_data,
                    ..Default::default()
                },
                i.reset,
            );
            if i.physical == feedback {
                break o;
            }
            i.physical = feedback;
            feedback_pass += 1;
            if feedback_pass >= 16 {
                return Err("GPU/memory feedback did not settle".into());
            }
        };
        if !i.reset {
            if o.control.device_read_data & 2 != 0 {
                return Err(format!("GPU error at cycle {cycle}").into());
            }
            if o.control.processor.busy && start.is_none() {
                start = Some(cycle);
            }
            if let Some(first) = start {
                if !o.control.processor.busy {
                    if !o.render.idle || !o.physical_drained {
                        return Err("Completion precedes memory retirement".into());
                    }
                    let mut rgb = 0xcbf29ce484222325_u64;
                    let mut d16 = rgb;
                    let mut ppm = b"P6\n400 240\n255\n".to_vec();
                    let plane = memory.backing();
                    for (name, base) in [("color.bin", color), ("depth.bin", depth)] {
                        let mut bytes = Vec::with_capacity(204800);
                        for &word in &plane[base / 2..base / 2 + 102400] {
                            bytes.extend(word.to_le_bytes());
                        }
                        fs::write(output.join(name), bytes)?;
                    }
                    for y in 0..240 {
                        for x in 0..400 {
                            let at = ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16;
                            let p = plane[color / 2 + at];
                            rgb = (rgb ^ u64::from(p)).wrapping_mul(0x100000001b3);
                            d16 = (d16 ^ u64::from(plane[depth / 2 + at]))
                                .wrapping_mul(0x100000001b3);
                            ppm.extend([
                                ((p >> 11) * 255 / 31) as u8,
                                (((p >> 5) & 63) * 255 / 63) as u8,
                                ((p & 31) * 255 / 31) as u8,
                            ]);
                        }
                    }
                    let report = serde_json::json!({"format":"tankracer.gpu-timing.v1", "frame":source["frame"],
                        "gpu_cycles":cycle-first, "gpu_ms_54mhz":(cycle-first) as f64 / 54000.,
                        "clock_hz":54000000, "meshlet_starts":meshlets,
                        "fragments_final":fragments, "texture_beats":texture_beats,
                        "rgb565_fingerprint":format!("{rgb:016x}"), "d16_fingerprint":format!("{d16:016x}"),
                        "functional_rgb_match":source["rgb565_fingerprint"] == format!("{rgb:016x}"),
                        "functional_d16_match":source["d16_fingerprint"] == format!("{d16:016x}"),
                        "traffic":args[3], "traffic_grants":memory.traffic.grants,
                        "native_memory_cycles":memory.native_cycles(),
                        "scope":"Cold GPU + production arbiter/native SDRAM cycle replay; optional traffic is synthetic. CPU execution and presentation are measured separately."});
                    fs::write(output.join("frame.ppm"), ppm)?;
                    fs::write(
                        output.join("result.json"),
                        serde_json::to_vec_pretty(&report)?,
                    )?;
                    if let Some(timeline) = timeline {
                        timeline.finish(output, first, cycle)?;
                    }
                    println!("{report}");
                    return Ok(());
                }
                fragments += usize::from(o.render.backend.final_result);
                texture_beats += usize::from(o.control.transport.router.texture_read);
                meshlets += usize::from(
                    o.control.processor.start && o.render.producer.frontend.meshlet.ready,
                );
            }
        }
        if let Some(timeline) = &mut timeline {
            let phase = gpu.control.processor.phase();
            let edge = gpu
                .clock_observing(i)
                .map_err(|e| format!("GPU cycle {cycle}: {e}"))?;
            if start.is_some() {
                timeline.observe(cycle, phase, &o, edge)?;
            }
        } else {
            gpu.clock(i)
                .map_err(|e| format!("GPU cycle {cycle}: {e}"))?;
        }
        memory.clock();
    }
    Err(format!("Cycle limit {maximum}; meshlets={meshlets}, fragments={fragments}").into())
}
