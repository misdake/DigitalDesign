//! Ownership-store tracing for transient weapon-pool occupancy.
use super::*;

struct Pool {
    name: &'static str,
    base: u32,
    stride: u32,
    per_page: u32,
    pages: u32,
    occupied: Vec<bool>,
    live: usize,
    peak: usize,
    allocations: usize,
    releases: usize,
    largest_slot: usize,
    header_probes: u64,
}
impl Pool {
    fn slot(&self, address: u32) -> Option<usize> {
        let delta = address.checked_sub(self.base)?;
        let page = delta / 16384;
        let word = delta % 16384;
        if page < self.pages && word % self.stride == 0 && word / self.stride < self.per_page {
            Some((page * self.per_page + word / self.stride) as usize)
        } else {
            None
        }
    }
    fn sample(&mut self, memory: &[u16], limit: u16) -> Result<()> {
        self.live = 0;
        for i in 0..self.occupied.len() {
            let address = self.base
                + (i as u32 / self.per_page) * 16384
                + (i as u32 % self.per_page) * self.stride;
            let value = memory[address as usize];
            if value > limit {
                return Err(format!("uninitialized {} slot {i}: {value}", self.name).into());
            }
            self.occupied[i] = value != 0;
            self.live += usize::from(value != 0);
        }
        self.peak = self.peak.max(self.live);
        Ok(())
    }
    fn store(&mut self, slot: usize, value: u16) -> Result<()> {
        let used = value != 0;
        if used && !self.occupied[slot] {
            let mut first = 0;
            while first < self.occupied.len() && self.occupied[first] {
                first += 1;
            }
            if first != slot {
                return Err(format!("{} allocation order at {slot}", self.name).into());
            }
            self.allocations += 1;
            self.live += 1;
            self.largest_slot = self.largest_slot.max(slot);
        } else if !used && self.occupied[slot] {
            self.releases += 1;
            self.live -= 1;
        }
        self.occupied[slot] = used;
        self.peak = self.peak.max(self.live);
        Ok(())
    }
}
pub(super) fn run(args: &[String]) -> Result<()> {
    let app = Path::new(&args[1]);
    let budget = args[3].parse::<u64>()?;
    let limit = Duration::from_secs(args[4].parse()?);
    let output = Path::new(&args[5]);
    if output.exists() || budget == 0 || limit.is_zero() {
        return Err("fresh output and positive bound required".into());
    }
    let manifest: Value = serde_json::from_slice(&fs::read(app.join("application.build.json"))?)?;
    let assets: Value = serde_json::from_slice(&bound(
        app,
        &manifest,
        "tankracer-driving-assets/assets.json",
    )?)?;
    let cases = assets["cases"].as_array().ok_or("cases")?;
    if cases.is_empty() {
        return Err("trajectory missing".into());
    }
    for case in cases {
        if case["world_pass"] != cases[0]["world_pass"] {
            return Err("select either world frames or vehicle updates".into());
        }
    }
    let expected = if cases[0]["world_pass"] == true {
        assets["world_frames"].as_u64()
    } else {
        assets["updates"].as_u64()
    };
    let tags = boundaries(app, &manifest, false)?;
    let buffers: Value =
        serde_json::from_slice(&bound(app, &manifest, "application.buffers.json")?)?;
    let weapons = buffers
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "DRIVING_WEAPONS")
        .ok_or("weapons")?;
    let base = (weapons["address"].as_u64().unwrap() / 2) as u32;
    let creation = frozen_source(app, &manifest, "scene_creation.rs")?;
    let player = frozen_source(app, &manifest, "/driving/player_world.rs")?;
    let generated = String::from_utf8(bound(
        app,
        &manifest,
        "tankracer-driving-assets/driving_assets.rs",
    )?)?;
    let capacity = generated
        .split("pub const DRIVING_OBJECT_CAPACITY:u16=")
        .nth(1)
        .ok_or("object capacity missing")?
        .split(';')
        .next()
        .unwrap()
        .parse::<u16>()?;
    let initializer = player
        .split("weapons: scene_creation::Weapons {")
        .nth(1)
        .ok_or("weapon owner missing")?
        .split("\n        },")
        .next()
        .ok_or("weapon owner boundary")?;
    let mut pools = Vec::new();
    for (name, stem, kind) in [
        ("projectiles", "projectile", 1),
        ("mines", "mine", 2),
        ("explosions", "explosion", 3),
        ("ufos", "ufo", 4),
    ] {
        let field = format!("{stem}_page: ");
        let expression = initializer
            .lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix(&field))
            .ok_or("pool page missing")?
            .trim_end_matches(',');
        let offset = if expression == "page" {
            0
        } else {
            expression
                .strip_prefix("page + ")
                .ok_or("constant pool page required")?
                .parse::<u32>()?
        };
        let field = format!("{stem}_pages: ");
        let pages = initializer
            .lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix(&field))
            .ok_or("pool pages missing")?
            .trim_end_matches(',')
            .parse::<u32>()?;
        let stride = kind_constant(&creation, "stride", kind)?;
        let per_page = kind_constant(&creation, "slots", kind)?;
        if pages == 0
            || per_page == 0
            || per_page * stride > 16384
            || (offset + pages) * 32768 > weapons["bytes"].as_u64().ok_or("weapon bytes")? as u32
        {
            return Err("pool layout outside bank".into());
        }
        pools.push(Pool {
            name,
            base: base + offset * 16384,
            stride,
            per_page,
            pages,
            occupied: vec![false; (per_page * pages) as usize],
            live: 0,
            peak: 0,
            allocations: 0,
            releases: 0,
            largest_slot: 0,
            header_probes: 0,
        });
    }
    let mut insert_range = None;
    let mut insert_steps = 0u64;
    let mut code = vec![Vec::new(); 8];
    for row in segments(&manifest)? {
        let debug = String::from_utf8(bound(
            app,
            &manifest,
            row["debug"].as_str().ok_or("debug missing")?,
        )?)?;
        for line in debug.lines() {
            if let Some(tail) = line.strip_prefix("func scene_creation::insert ") {
                let range = tail.split_whitespace().next().ok_or("insert range")?;
                let (first, last) = range.split_once("..").ok_or("insert bounds")?;
                if insert_range.is_some() {
                    return Err("ambiguous insertion function".into());
                }
                insert_range = Some((
                    row["cseg"].as_u64().unwrap() as u16,
                    u16::from_str_radix(first.trim_start_matches("0x"), 16)?,
                    u16::from_str_radix(last.trim_start_matches("0x"), 16)?,
                ));
            }
        }
        let bytes = bound(app, &manifest, row["code"].as_str().ok_or("code")?)?;
        for w in bytes.chunks_exact(2) {
            code[row["cseg"].as_u64().unwrap() as usize].push(u16::from_le_bytes([w[0], w[1]]));
        }
    }
    let (insert_segment, insert_first, insert_last) =
        insert_range.ok_or("pool insertion function missing")?;
    let image = bound(app, &manifest, manifest["package"].as_str().unwrap())?;
    let mut sim = CpuV3SystemSim::from_single_flash(&image, 0xd00d, VBlankMode::Manual)?;
    let started = Instant::now();
    let mut active = false;
    let mut prefix = None;
    let mut frames = 0;
    let mut peak_total = 0;
    for step in 0..budget {
        if started.elapsed() >= limit {
            return Err("host time bound exceeded".into());
        }
        let cpu = sim.cpu();
        let pc = (cpu.code_segment(), cpu.pc());
        if pc == (tags[0].segment, tags[0].pc) {
            if active {
                return Err("nested frame".into());
            }
            active = true;
            prefix = None;
            for p in &mut pools {
                p.sample(cpu.physical_memory_slice(), capacity)?;
            }
            peak_total = peak_total.max(pools.iter().map(|p| p.live).sum::<usize>());
        }
        if pc == (tags.last().unwrap().segment, tags.last().unwrap().pc) {
            for p in &pools {
                for i in 0..p.occupied.len() {
                    let address = p.base
                        + (i as u32 / p.per_page) * 16384
                        + (i as u32 % p.per_page) * p.stride;
                    if p.occupied[i] != (cpu.physical_memory_slice()[address as usize] != 0) {
                        return Err("pool ownership changed outside traced stores".into());
                    }
                }
            }
            active = false;
            frames += 1;
        }
        let in_insert =
            active && pc.0 == insert_segment && pc.1 >= insert_first && pc.1 < insert_last;
        if in_insert {
            insert_steps += 1;
        }
        let retired = cpu.retired_words();
        let pages = cpu.data_segments();
        let mut access = None;
        let mut op = None;
        if active {
            let word = code[pc.0 as usize][pc.1 as usize];
            let ins = if matches!(word >> 12, 12..=14) {
                decode_fpu_pair(word, code[pc.0 as usize][pc.1 as usize + 1])
            } else {
                decode(word)
            };
            match ins {
                Instruction::Store { base, offset, .. }
                | Instruction::Load { base, offset, .. }
                    if in_insert || matches!(ins, Instruction::Store { .. }) =>
                {
                    let offset =
                        prefix.map_or(offset as u16, |v: u16| (v << 4) | (offset as u16 & 15));
                    access = Some((
                        cpu.registers()[base as usize].wrapping_add(offset),
                        1,
                        matches!(ins, Instruction::Store { .. }),
                    ));
                }
                Instruction::FpuAux {
                    kind: FpuAuxKind::IntegerRegister,
                    x,
                    subop,
                    mode,
                    ..
                } if subop <= 1 && (in_insert || subop == 1) => {
                    access = Some((
                        cpu.registers()[x as usize],
                        2 * u16::from(1 + (mode & 3)),
                        subop == 1,
                    ));
                }
                Instruction::LineCopy { .. } => return Err("active LCOPY must be traced".into()),
                _ => {}
            }
            op = Some(ins);
        }
        let result = sim.step().map_err(|e| format!("{e:?}"))?;
        if active {
            prefix = match op {
                Some(Instruction::Prefix { payload }) => Some(payload),
                _ => None,
            };
            if sim.cpu().retired_words() != retired {
                if let Some((address, count, write)) = access {
                    for j in 0..count {
                        let logical = address.wrapping_add(j);
                        let physical = u32::from(pages[usize::from(logical >> 14)]) * 16384
                            + u32::from(logical & 16383);
                        for pool in &mut pools {
                            if let Some(slot) = pool.slot(physical) {
                                if !write {
                                    pool.header_probes += 1;
                                    continue;
                                }
                                pool.store(
                                    slot,
                                    sim.cpu().physical_memory_slice()[physical as usize],
                                )?;
                            }
                        }
                    }
                    peak_total = peak_total.max(pools.iter().map(|p| p.live).sum::<usize>());
                }
            }
        }
        if let StepOutcome::Halted { signal } = result {
            if signal != 0x600d || active || Some(frames) != expected {
                return Err(format!("halt {signal:#x},frames {frames}").into());
            }
            let rows=pools.iter().map(|p|json!({"name":p.name,"capacity":p.occupied.len(),"stride_bytes":p.stride*2,"reserved_bytes":p.pages*32768,"peak":p.peak,"allocations":p.allocations,"releases":p.releases,"largest_allocated_slot":p.largest_slot,"allocation_header_probes":p.header_probes})).collect::<Vec<_>>();
            let report = json!({"passed":true,"artifact_id":manifest["artifact_id"],"tracing_source_sha256":format!("{:x}",Sha256::digest(include_bytes!("driving_pool_profile.rs"))),"boundary_source_sha256":format!("{:x}",Sha256::digest(include_bytes!("driving_loop_profile.rs"))),"frames":frames,"steps_with_loading_and_assertions":step+1,"host_seconds":started.elapsed().as_secs_f64(),"peak_total":peak_total,"insertion_body_steps":insert_steps,"pools":rows,"scope":"Instruction stores to pool ownership words during ordered updates, including transient creation before retirement; existing occupancy sampled at each frame start. First free slot order checked. Pool layout and object limit read from the frozen RCC input; end-of-frame ownership checked against memory. CPU timing and capacity guarantees require separate workloads."});
            fs::write(output, serde_json::to_vec_pretty(&report)?)?;
            println!("{report}");
            return Ok(());
        }
    }
    Err("instruction budget exceeded".into())
}

fn frozen_source(app: &Path, manifest: &Value, suffix: &str) -> Result<String> {
    let rows = manifest["declared_inputs"]
        .as_array()
        .ok_or("source inputs missing")?;
    let mut selected = None;
    for row in rows {
        let path = row["origin"]["path"]
            .as_str()
            .ok_or("input path missing")?
            .replace('\\', "/");
        if path.ends_with(suffix) {
            if selected.is_some() {
                return Err(format!("ambiguous source: {suffix}").into());
            }
            selected = Some(row["file"].as_str().ok_or("input file missing")?);
        }
    }
    Ok(String::from_utf8(bound(
        app,
        manifest,
        selected.ok_or_else(|| format!("source missing: {suffix}"))?,
    )?)?)
}
// These two runtime lookup functions contain only literal branches. Reject a
// richer implementation rather than silently measuring a different layout.
fn kind_constant(source: &str, function: &str, kind: u32) -> Result<u32> {
    let header = format!("pub fn {function}(kind: u16) -> u16 {{");
    let body = source
        .split(&header)
        .nth(1)
        .ok_or("pool lookup missing")?
        .split("\n}")
        .next()
        .ok_or("pool lookup boundary")?;
    let branch = format!("if kind == {kind} {{");
    if let Some(tail) = body.split(&branch).nth(1) {
        return Ok(tail
            .trim()
            .strip_prefix("return ")
            .ok_or("literal pool return required")?
            .split(';')
            .next()
            .unwrap()
            .parse()?);
    }
    Ok(body
        .lines()
        .last()
        .ok_or("pool default missing")?
        .trim()
        .parse()?)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> Pool {
        Pool {
            name: "test",
            base: 16,
            stride: 8,
            per_page: 2,
            pages: 2,
            occupied: vec![false; 4],
            live: 0,
            peak: 0,
            allocations: 0,
            releases: 0,
            largest_slot: 0,
            header_probes: 0,
        }
    }

    #[test]
    fn ownership_words_cover_both_pages_and_exclude_padding() {
        let p = pool();
        assert_eq!(p.slot(16), Some(0));
        assert_eq!(p.slot(24), Some(1));
        assert_eq!(p.slot(16 + 16384), Some(2));
        assert_eq!(p.slot(24 + 16384), Some(3));
        for address in [15, 17, 32, 16383, 16 + 32768] {
            assert_eq!(p.slot(address), None);
        }
    }

    #[test]
    fn first_free_order_handles_retirement_and_reuse_within_a_frame() {
        let mut p = pool();
        for i in 0..4 {
            p.store(i, (i + 1) as u16).unwrap();
        }
        p.store(2, 0).unwrap();
        p.store(0, 0).unwrap();
        assert!(p.store(2, 5).is_err());
        p.store(0, 5).unwrap();
        p.store(2, 6).unwrap();
        assert_eq!((p.live, p.peak, p.allocations, p.releases), (4, 4, 6, 2));
    }

    #[test]
    fn initial_occupancy_checks_seeded_memory_and_owner_range() {
        let mut p = pool();
        let mut memory = vec![0; 32784];
        memory[16] = 7;
        memory[24 + 16384] = 455;
        p.sample(&memory, 455).unwrap();
        assert_eq!((p.live, p.peak), (2, 2));
        memory[16] = 0xd00d;
        assert!(p.sample(&memory, 455).is_err());
    }
}
