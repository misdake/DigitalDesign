//! Source-bound frame-loop timing and architectural data footprints.
use cpu_v3::{decode, decode_fpu_pair, FpuAuxKind, Instruction, StepOutcome};
use cpu_v3_tang_nano_20k::{
    system_emu::CpuV3SystemEmu,
    system_sim::{CpuV3SystemSim, VBlankMode},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    path::Path,
    time::{Duration, Instant},
};

mod driving_pool_profile;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
struct Boundary {
    name: &'static str,
    source: String,
    segment: u16,
    pc: u16,
    line: usize,
}
fn bound(app: &Path, manifest: &Value, name: &str) -> Result<Vec<u8>> {
    let path = Path::new(name);
    if path.is_absolute()
        || path
            .components()
            .any(|v| matches!(v, std::path::Component::ParentDir))
    {
        return Err("artifact path must stay within its directory".into());
    }
    let bytes = fs::read(app.join(path))?;
    let record = &manifest["files"][name];
    if record["bytes"].as_u64() != Some(bytes.len() as u64)
        || record["sha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str())
    {
        return Err(format!("artifact identity differs: {name}").into());
    }
    Ok(bytes)
}
fn segments(manifest: &Value) -> Result<Vec<Value>> {
    let mut rows = vec![
        json!({"cseg":manifest["target"]["code_segment"],"code":manifest["code"],"debug":manifest["debug"]}),
    ];
    rows.extend(
        manifest["code_segments"]
            .as_array()
            .ok_or("code segments missing")?
            .iter()
            .cloned(),
    );
    Ok(rows)
}
fn boundaries(app: &Path, manifest: &Value, stages: bool) -> Result<Vec<Boundary>> {
    let inputs = manifest["declared_inputs"]
        .as_array()
        .ok_or("source inputs missing")?;
    let split = inputs.iter().any(|r| {
        r["origin"]["kind"] == "repository"
            && r["origin"]["path"]
                .as_str()
                .is_some_and(|p| p.replace('\\', "/").ends_with("/driving/game_update.rs"))
    });
    let update_file = if split { "game_update.rs" } else { "main.rs" };
    let anchors = [
        (
            "world-control",
            "main.rs",
            if split {
                "let vehicles: Buf<tank_response::Vehicle, 3> = Buf::new(["
            } else {
                "let saved_slot = attitude::get(state.runtime.slot, 0);"
            },
            0,
        ),
        ("contact-power", update_file, "mark(2);", 0),
        (
            "scenery-contact",
            update_file,
            if split {
                "if runtime.scenery_enabled {"
            } else {
                "if meta[21u16] != 0 {"
            },
            if split { 0 } else { 1 },
        ),
        (
            "vehicle-contact",
            update_file,
            "effects.frame = attitude::get(data, 26);",
            0,
        ),
        (
            "deformation",
            update_file,
            "let mut contact = deform_contact::Vehicle {",
            0,
        ),
        (
            "ground-response",
            update_file,
            "let mut response = ground_response::State {",
            0,
        ),
        (
            "motion-effects",
            update_file,
            if split {
                "let motion_status ="
            } else {
                "let status = complete_check::advance("
            },
            0,
        ),
        (
            "world-tail",
            update_file,
            "if own == 2 && player.finish_stage != 0 {",
            1,
        ),
        ("end", "main.rs", "mark(3);", 0),
    ];
    let mut debug = Vec::new();
    for row in segments(manifest)? {
        debug.push((
            row["cseg"].as_u64().ok_or("CSEG missing")? as u16,
            String::from_utf8(bound(
                app,
                manifest,
                row["debug"].as_str().ok_or("debug missing")?,
            )?)?,
        ));
    }
    let mut result = Vec::new();
    for (name, filename, anchor, occurrence) in anchors {
        if !stages && name != "world-control" && name != "end" {
            continue;
        }
        let suffix = format!("/driving/{filename}");
        let source = inputs
            .iter()
            .find(|r| {
                r["origin"]["kind"] == "repository"
                    && r["origin"]["path"]
                        .as_str()
                        .is_some_and(|p| p.replace('\\', "/").ends_with(&suffix))
            })
            .ok_or_else(|| format!("source missing: {filename}"))?;
        let text = String::from_utf8(bound(
            app,
            manifest,
            source["file"].as_str().ok_or("source file missing")?,
        )?)?;
        let source_lines: Vec<_> = text.lines().collect();
        let mut lines = Vec::new();
        for (index, line) in source_lines.iter().enumerate() {
            if line.trim().starts_with(anchor) {
                lines.push(index + 1);
            }
        }
        let source_line = *lines
            .get(occurrence)
            .ok_or_else(|| format!("source boundary missing: {name}"))?;
        let mut location = None;
        for (segment, text) in &debug {
            let mut ids = BTreeSet::new();
            for row in text.lines() {
                let fields: Vec<_> = row.splitn(3, ' ').collect();
                if fields.len() == 3 && fields[0] == "file" {
                    let path = fields[2].replace('\\', "/");
                    if path == filename || path.ends_with(&suffix) {
                        ids.insert(fields[1]);
                    }
                }
            }
            for row in text.lines() {
                let fields: Vec<_> = row.split_whitespace().collect();
                if fields.len() == 4
                    && fields[0] == "line"
                    && ids.contains(fields[2])
                    && fields[3].parse::<usize>()? == source_line
                {
                    let pc = u16::from_str_radix(fields[1].trim_start_matches("0x"), 16)?;
                    if let Some((old_segment, old_pc)) = location {
                        if *segment != old_segment {
                            return Err("ambiguous boundary segment".into());
                        }
                        location = Some((*segment, pc.min(old_pc)));
                    } else {
                        location = Some((*segment, pc));
                    }
                }
            }
        }
        let (segment, pc) = location.ok_or_else(|| format!("debug boundary missing: {name}"))?;
        result.push(Boundary {
            name,
            source: filename.to_owned(),
            segment,
            pc,
            line: source_line,
        });
    }
    for (i, entry) in result.iter().enumerate() {
        for earlier in &result[..i] {
            if earlier.source == entry.source && earlier.line >= entry.line {
                return Err("source boundaries must follow the update loop".into());
            }
        }
    }
    Ok(result)
}
fn rows_json(
    parts: &[[u64; 4]],
    reads: &[BTreeSet<u32>],
    writes: &[BTreeSet<u32>],
    tags: &[Boundary],
    instruction: bool,
) -> Value {
    let mut result = Vec::new();
    for i in 0..tags.len() - 1 {
        let mut row = json!({"stage":tags[i].name,"ticks":parts[i][0],"retired_words":parts[i][1]});
        if instruction {
            let union: BTreeSet<_> = reads[i].union(&writes[i]).copied().collect();
            let cpu_lines = union.iter().filter(|v| **v < 2048).count();
            row["data_read_words"] = json!(parts[i][2]);
            row["data_write_words"] = json!(parts[i][3]);
            row["data_line_bytes"] = json!(union.len() * 32);
            row["cpu_line_bytes"] = json!(cpu_lines * 32);
        }
        result.push(row);
    }
    json!(result)
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err("usage: tankracer-driving-loop-profile ARTIFACT native|instruction|pools MAX_TICKS MAX_HOST_SECONDS OUTPUT".into());
    }
    if args[2] == "pools" {
        return driving_pool_profile::run(&args);
    }
    let instruction = match args[2].as_str() {
        "instruction" => true,
        "native" => false,
        _ => return Err("unknown backend".into()),
    };
    let budget = args[3].parse::<u64>()?;
    let limit = Duration::from_secs(args[4].parse()?);
    let output = Path::new(&args[5]);
    if budget == 0 || limit.is_zero() || output.exists() {
        return Err("positive bounds and a fresh report are required".into());
    }
    let app = Path::new(&args[1]);
    let manifest: Value = serde_json::from_slice(&fs::read(app.join("application.build.json"))?)?;
    let assets: Value = serde_json::from_slice(&bound(
        app,
        &manifest,
        "tankracer-driving-assets/assets.json",
    )?)?;
    let cases = assets["cases"].as_array().ok_or("cases missing")?;
    if cases.len() != 1 {
        return Err("select one driving trajectory".into());
    }
    let expected = if cases[0]["world_pass"] == true {
        assets["world_frames"].as_u64()
    } else {
        assets["updates"].as_u64()
    }
    .ok_or("frame count missing")?;
    let tags = boundaries(app, &manifest, true)?;
    let package = bound(
        app,
        &manifest,
        manifest["package"].as_str().ok_or("package missing")?,
    )?;
    let mut code = vec![Vec::new(); 8];
    if instruction {
        for row in segments(&manifest)? {
            let segment = row["cseg"].as_u64().ok_or("CSEG missing")? as usize;
            if segment >= code.len() {
                return Err("CSEG outside profiling range".into());
            }
            let bytes = bound(app, &manifest, row["code"].as_str().ok_or("code missing")?)?;
            if bytes.len() & 1 != 0 {
                return Err("odd code size".into());
            }
            for word in bytes.chunks_exact(2) {
                code[segment].push(u16::from_le_bytes([word[0], word[1]]));
            }
        }
    }
    let mut parts = vec![[0u64; 4]; tags.len()];
    let mut reads = vec![BTreeSet::<u32>::new(); tags.len()];
    let mut writes = reads.clone();
    let mut active: Option<usize> = None;
    let mut stage_first = (0u64, 0u64);
    let mut frame_first = stage_first;
    let mut previous = None;
    let mut prefix = None;
    let mut frames = Vec::new();
    let mut halted = false;
    let mut ticks = 0;
    let started = Instant::now();
    let mut native = None;
    let mut sim = None;
    if instruction {
        sim = Some(CpuV3SystemSim::from_single_flash(
            &package,
            0xd00d,
            VBlankMode::Manual,
        )?);
    } else {
        native = Some(
            CpuV3SystemEmu::from_single_flash(&package, 0xd00d, VBlankMode::Manual)?
                .with_functional_gpu(),
        );
    }
    while ticks < budget && started.elapsed() < limit {
        let (pc, retired) = if let Some(s) = &sim {
            (
                Some((s.cpu().code_segment(), s.cpu().pc())),
                s.cpu().retired_words(),
            )
        } else {
            let core = native.as_ref().unwrap().core();
            (
                core.executing_instruction_address(),
                u64::from(core.retired_words()),
            )
        };
        if instruction || pc != previous {
            if let Some((segment, pc)) = pc {
                if let Some(index) = tags.iter().position(|p| p.segment == segment && p.pc == pc) {
                    if index == 0 {
                        if active.is_some() {
                            return Err("overlapping frame boundaries".into());
                        }
                        frame_first = (ticks, retired);
                    }
                    if let Some(old) = active.take() {
                        parts[old][0] += ticks - stage_first.0;
                        parts[old][1] += retired - stage_first.1;
                        if index == tags.len() - 1 {
                            if parts.iter().map(|p| p[0]).sum::<u64>() != ticks - frame_first.0
                                || parts.iter().map(|p| p[1]).sum::<u64>()
                                    != retired - frame_first.1
                            {
                                return Err("stage timing coverage differs".into());
                            }
                            let row = json!({"serial":frames.len(),"ticks":ticks-frame_first.0,"retired_words":retired-frame_first.1,"stages":rows_json(&parts,&reads,&writes,&tags,instruction)});
                            println!("{row}");
                            frames.push(row);
                            parts.fill([0; 4]);
                            for r in &mut reads {
                                r.clear();
                            }
                            for w in &mut writes {
                                w.clear();
                            }
                        } else {
                            active = Some(index);
                        }
                    } else if index == 0 {
                        active = Some(index);
                    }
                    if active.is_some() {
                        stage_first = (ticks, retired);
                    }
                }
            }
        }
        previous = pc;
        if let Some(s) = &mut sim {
            let mut decoded = None;
            let mut access = None;
            let pages = s.cpu().data_segments();
            if active.is_some() {
                let (segment, pc) = pc.unwrap();
                let word = code[usize::from(segment)][usize::from(pc)];
                let op = if matches!(word >> 12, 12..=14) {
                    decode_fpu_pair(word, code[usize::from(segment)][usize::from(pc) + 1])
                } else {
                    decode(word)
                };
                let registers = s.cpu().registers();
                match op {
                    Instruction::Load { base, offset, .. }
                    | Instruction::Store { base, offset, .. } => {
                        let offset =
                            prefix.map_or(offset as u16, |v: u16| (v << 4) | (offset as u16 & 15));
                        access = Some((
                            registers[usize::from(base)].wrapping_add(offset),
                            1,
                            matches!(op, Instruction::Store { .. }),
                        ));
                    }
                    Instruction::FpuAux {
                        kind: FpuAuxKind::IntegerRegister,
                        x,
                        subop,
                        mode,
                        ..
                    } if subop <= 1 => {
                        access = Some((
                            registers[usize::from(x)],
                            2 * u16::from(1 + (mode & 3)),
                            subop == 1,
                        ));
                    }
                    Instruction::LineCopy { .. } => {
                        return Err("LCOPY traffic needs a separate measurement".into())
                    }
                    _ => {}
                }
                decoded = Some(op);
            }
            let outcome = s.step().map_err(|e| format!("instruction: {e:?}"))?;
            if pc != Some((s.cpu().code_segment(), s.cpu().pc())) {
                prefix = match decoded {
                    Some(Instruction::Prefix { payload }) => Some(payload),
                    _ => None,
                };
            }
            if s.cpu().retired_words() != retired {
                if let (Some(index), Some((address, count, write))) = (active, access) {
                    parts[index][if write { 3 } else { 2 }] += u64::from(count);
                    for j in 0..count {
                        let logical = address.wrapping_add(j);
                        let physical = u32::from(pages[usize::from(logical >> 14)]) * 16384
                            + u32::from(logical & 16383);
                        if write {
                            writes[index].insert(physical >> 4);
                        } else {
                            reads[index].insert(physical >> 4);
                        }
                    }
                }
            }
            ticks += 1;
            if let StepOutcome::Halted { signal } = outcome {
                if signal != 0x600d {
                    return Err(format!("halt {signal:#x}").into());
                }
                halted = true;
                break;
            }
        } else {
            let s = native.as_mut().unwrap();
            let cpu = s.step().map_err(|e| format!("native: {e:?}"))?;
            ticks = s.cycles();
            if cpu.fault {
                return Err(format!("CPU fault {} at {}", cpu.fault_code, cpu.fault_pc).into());
            }
            if cpu.halted {
                if cpu.halt_signal != 0x600d {
                    return Err(format!("halt {:#x}", cpu.halt_signal).into());
                }
                halted = true;
                break;
            }
        }
    }
    let passed = halted && active.is_none() && frames.len() as u64 == expected;
    let report = json!({"passed":passed,"artifact_id":manifest["artifact_id"],"profiling_tool_sha256":format!("{:x}",Sha256::digest(include_bytes!("driving_loop_profile.rs"))),"backend":args[2],"clock_hz":54000000,"case":cases[0]["name"],"unit":if cases[0]["world_pass"] == true {"world_frame"} else {"vehicle_update"},"host_seconds":started.elapsed().as_secs_f64(),"ticks_with_loading_and_assertions":ticks,"frames":frames,
        "scope":"Ordered update loop and Stop drain. Input installation, clock/camera preparation, assertions and loading are outside. Native ticks use first Execute presence, including boundary holds, with functional GPU/Display; GPU work and shared GPU memory competition are outside CPU cycle costs. Instruction ticks are step calls; load/store words and unique physical 32-byte lines exclude instruction fetch, cache refill and SDRAM bus traffic. A skipped branch remains in its enclosing stage.",
        "boundaries":tags.iter().map(|p| json!({"stage":p.name,"cseg":p.segment,"pc":p.pc,"source_file":p.source,"source_line":p.line})).collect::<Vec<_>>()});
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    if !passed {
        return Err("bounded profile or frame coverage is incomplete; see report".into());
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("tankracer-driving-loop-profile: {error}");
        std::process::exit(1);
    }
}
