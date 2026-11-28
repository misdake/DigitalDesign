//! Passive function occupancy and source-bound frame-output milestones.
use super::*;

struct Function {
    name: String,
    file: String,
    segment: u16,
    first: usize,
    end: usize,
    selected: bool,
}

struct Call {
    owner: usize,
    return_pc: (u16, u16),
    stack: u16,
    begin: u64,
    selection: bool,
}

pub struct Detail {
    functions: Vec<Function>,
    owners: Vec<u16>,
    counts: Vec<u64>,
    inclusive: Vec<[u64; 2]>,
    selection_inclusive: Vec<[u64; 2]>,
    selection_begin: Option<u64>,
    calls: Vec<Call>,
    milestones: Vec<Boundary>,
    pub events: Vec<Value>,
    pub selections: Vec<Value>,
}

impl Detail {
    pub fn new(app: &Path, manifest: &Value) -> Result<Self> {
        let mut functions = Vec::new();
        let mut owners = vec![0; 8 * 65536];
        let mut debug = Vec::new();
        for row in segments(manifest)? {
            let segment = row["cseg"].as_u64().ok_or("CSEG missing")? as u16;
            let text = String::from_utf8(bound(
                app,
                manifest,
                row["debug"].as_str().ok_or("debug missing")?,
            )?)?;
            for line in text.lines().filter(|line| line.starts_with("func ")) {
                let head = line
                    .split_once(" frame ")
                    .ok_or("function frame missing")?
                    .0;
                let fields: Vec<_> = head.splitn(4, ' ').collect();
                let (first, end) = fields[2].split_once("..").ok_or("function range missing")?;
                let first = usize::from_str_radix(first.trim_start_matches("0x"), 16)?;
                let end = usize::from_str_radix(end.trim_start_matches("0x"), 16)?;
                if segment >= 8 || first >= end || end > 65536 {
                    return Err("function profiling range is invalid".into());
                }
                let id = u16::try_from(functions.len() + 1)?;
                for owner in &mut owners
                    [usize::from(segment) * 65536 + first..usize::from(segment) * 65536 + end]
                {
                    if *owner != 0 {
                        return Err("function profiling ranges overlap".into());
                    }
                    *owner = id;
                }
                functions.push(Function {
                    name: fields[1].into(),
                    file: fields[3].into(),
                    segment,
                    first,
                    end,
                    selected: [
                        "weapon_world.rs",
                        "weapon_emitters.rs",
                        "scene_emitter.rs",
                        "scene_creation.rs",
                        "scene_world.rs",
                        "projectile_flight.rs",
                        "projectile_trail.rs",
                        "explosion.rs",
                        "explosion_blasts.rs",
                        "quad_pool.rs",
                        "particle_sprite.rs",
                        "scene_projectile_impact.rs",
                        "scene_explosions.rs",
                        "scene_query.rs",
                        "integer.rs",
                        "trig_lookup.rs",
                        "gpu_window.rs",
                        "game_update.rs",
                        "player_world.rs",
                        "player_input.rs",
                        "player_route.rs",
                        "terrain.rs",
                        "sampling.rs",
                        "ground.rs",
                        "driving.rs",
                        "suspension.rs",
                        "vehicle.rs",
                        "player.rs",
                        "world.rs",
                        "pvs.rs",
                        "prefix.rs",
                        "car_runtime.rs",
                        "scenery_contact.rs",
                        "lifecycle.rs",
                        "scene_player.rs",
                        "temporary.rs",
                        "player_control.rs",
                        "car_motion.rs",
                        "car_effects.rs",
                        "collision.rs",
                        "attitude.rs",
                        "body.rs",
                        "vehicle_pose.rs",
                        "vehicle_debris.rs",
                        "vehicle_shield.rs",
                        "vehicle_smoke.rs",
                        "drive.rs",
                        "damage.rs",
                        "tank_response.rs",
                        "ground_response.rs",
                        "deform_contact.rs",
                        "projectile.rs",
                        "projectile_sweep.rs",
                        "vehicle_transfer.rs",
                        "track_effects.rs",
                        "inverse_basis.rs",
                        "powerup.rs",
                        "pickup_light.rs",
                    ]
                    .contains(&fields[3].rsplit('/').next().unwrap())
                        || (fields[3].ends_with("/main.rs") && !fields[1].starts_with('<')),
                });
            }
            debug.push((segment, text));
        }
        let mut milestones = Vec::new();
        if let Some((filename, source)) = manifest["source_map"]
            .as_object()
            .and_then(|map| map.iter().find(|(name, _)| name.ends_with("/range_map.rs")))
        {
            let source = String::from_utf8(bound(
                app,
                manifest,
                source["file"].as_str().ok_or("render source missing")?,
            )?)?;
            let lines: Vec<_> = source.lines().collect();
            let begin = lines
                .iter()
                .position(|line| line.trim().starts_with("pub fn draw(&mut self, frame:"))
                .ok_or("frame-output draw missing")?;
            let end = begin
                + 1
                + lines[begin + 1..]
                    .iter()
                    .position(|line| line.trim().starts_with("pub fn finish("))
                    .ok_or("frame-output finish missing")?;
            for (name, anchor) in [
                ("draw-begin", "let frame = frame % CAMERA_COUNT;"),
                ("select-end", "let mut back ="),
                ("record-begin", "let mut commands ="),
                ("record-end", "let list = commands.finish_list();"),
                ("output-ready", "if self.active {"),
            ] {
                let line = begin
                    + 1
                    + lines[begin..end]
                        .iter()
                        .position(|line| line.trim().starts_with(anchor))
                        .ok_or_else(|| format!("frame-output milestone missing: {name}"))?;
                let mut location = None;
                for (segment, text) in &debug {
                    let file = text.lines().find_map(|row| {
                        row.strip_prefix("file ")
                            .and_then(|row| row.split_once(' '))
                            .filter(|(_, file)| *file == filename)
                            .map(|(id, _)| id)
                    });
                    let Some(file) = file else { continue };
                    for row in text.lines() {
                        let fields: Vec<_> = row.split_whitespace().collect();
                        if fields.len() == 4
                            && fields[0] == "line"
                            && fields[2] == file
                            && fields[3].parse::<usize>()? == line
                        {
                            let pc = u16::from_str_radix(fields[1].trim_start_matches("0x"), 16)?;
                            if let Some((old_segment, old_pc)) = location {
                                if old_segment != *segment {
                                    return Err("milestone CSEG is ambiguous".into());
                                }
                                location = Some((*segment, pc.min(old_pc)));
                            } else {
                                location = Some((*segment, pc));
                            }
                        }
                    }
                }
                let (segment, mut pc) =
                    location.ok_or_else(|| format!("milestone debug line missing: {name}"))?;
                if name == "draw-begin" {
                    pc = functions
                        [usize::from(owners[usize::from(segment) * 65536 + usize::from(pc)]) - 1]
                        .first as u16;
                }
                milestones.push(Boundary {
                    name,
                    source: filename.clone(),
                    segment,
                    pc,
                    line,
                });
            }
        }
        for (name, function, at_return) in [
            ("present-begin", "present_frame", false),
            ("present-end", "present_frame", true),
            (
                "swap-queue-begin",
                "gpu_present_set_next_with_format",
                false,
            ),
            ("vblank-begin", "gpu_present_wait_vblank", false),
            ("vblank-end", "gpu_present_wait_vblank", true),
            ("submit-begin", "gpu_execute_submit_list", false),
            ("submit-end", "gpu_execute_submit_list", true),
        ] {
            for f in functions.iter().filter(|f| f.name == function) {
                milestones.push(Boundary {
                    name,
                    source: f.file.clone(),
                    segment: f.segment,
                    pc: if at_return { f.end - 1 } else { f.first } as u16,
                    line: 0,
                });
            }
        }
        let counts = vec![0; functions.len() + 1];
        let inclusive = vec![[0; 2]; functions.len() + 1];
        let selection_inclusive = vec![[0; 2]; functions.len() + 1];
        Ok(Self {
            functions,
            owners,
            counts,
            inclusive,
            selection_inclusive,
            selection_begin: None,
            calls: Vec::new(),
            milestones,
            events: Vec::new(),
            selections: Vec::new(),
        })
    }

    // The update endpoint can be the caller continuation of its outer function.
    pub fn return_at(&mut self, pc: (u16, u16), registers: &[u16; 16], ticks: u64) {
        while self
            .calls
            .last()
            .is_some_and(|call| call.return_pc == pc && call.stack == registers[13])
        {
            let call = self.calls.pop().unwrap();
            let counters = if call.selection {
                &mut self.selection_inclusive
            } else {
                &mut self.inclusive
            };
            counters[call.owner][0] += ticks - call.begin;
            counters[call.owner][1] += 1;
        }
    }

    #[allow(clippy::too_many_arguments)] // One passive CPU/calendar observation.
    pub fn observe(
        &mut self,
        pc: Option<(u16, u16)>,
        active: bool,
        changed: bool,
        ticks: u64,
        frame: usize,
        registers: &[u16; 16],
        logic_edge: bool,
    ) {
        if active && logic_edge {
            let owner = pc.map_or(0, |(segment, pc)| {
                self.owners[usize::from(segment) * 65536 + usize::from(pc)]
            });
            self.counts[usize::from(owner)] += 1;
        }
        if changed {
            if let Some((segment, pc)) = pc {
                self.return_at((segment, pc), registers, ticks);
                if self
                    .milestones
                    .iter()
                    .any(|tag| tag.name == "draw-begin" && tag.segment == segment && tag.pc == pc)
                {
                    self.selection_begin = Some(ticks);
                }
                let owner =
                    usize::from(self.owners[usize::from(segment) * 65536 + usize::from(pc)]);
                if (active || self.selection_begin.is_some()) && owner != 0 {
                    let f = &self.functions[owner - 1];
                    if f.selected && usize::from(pc) == f.first {
                        self.calls.push(Call {
                            owner,
                            return_pc: (segment, registers[14]),
                            stack: registers[13],
                            begin: ticks,
                            selection: self.selection_begin.is_some(),
                        });
                    }
                }
                for tag in &self.milestones {
                    if tag.segment == segment && tag.pc == pc {
                        if tag.name == "select-end" {
                            let mut indices: Vec<_> = (1..self.selection_inclusive.len())
                                .filter(|i| self.selection_inclusive[*i][1] != 0)
                                .collect();
                            indices.sort_by_key(|i| {
                                std::cmp::Reverse(self.selection_inclusive[*i][0])
                            });
                            let functions: Vec<_> = indices.iter().map(|i| {
                                let f = &self.functions[*i - 1];
                                json!({"function":f.name,"file":f.file,"cseg":f.segment,"ticks":self.selection_inclusive[*i][0],"calls":self.selection_inclusive[*i][1]})
                            }).collect();
                            self.selections.push(json!({"updates_completed":frame,"begin_cycle":self.selection_begin.take(),"end_cycle":ticks,"inclusive_functions":functions}));
                            self.selection_inclusive.fill([0; 2]);
                        }
                        self.events.push(
                            json!({"name":tag.name,"cycle":ticks,"updates_completed":frame,
                            "cseg":segment,"pc":pc,"source":tag.source,"line":tag.line}),
                        );
                    }
                }
            }
        }
    }

    pub fn finish_frame(&mut self, expected: u64) -> Result<Value> {
        if self.counts.iter().sum::<u64>() != expected {
            return Err("function occupancy does not cover update interval".into());
        }
        if !self.calls.is_empty() {
            return Err(format!(
                "selected function call crosses update boundary: {:?}",
                self.calls
                    .iter()
                    .map(|c| (
                        &self.functions[c.owner - 1].name,
                        c.return_pc,
                        c.stack,
                        c.selection
                    ))
                    .collect::<Vec<_>>()
            )
            .into());
        }
        let mut indices: Vec<_> = (1..self.counts.len())
            .filter(|i| self.counts[*i] != 0)
            .collect();
        indices.sort_by_key(|i| std::cmp::Reverse(self.counts[*i]));
        let omitted: u64 = indices.iter().skip(32).map(|i| self.counts[*i]).sum();
        let functions: Vec<_> = indices.iter().take(32).map(|i| {
            let f = &self.functions[*i - 1];
            json!({"function":f.name,"file":f.file,"cseg":f.segment,"first":f.first,"end":f.end,"ticks":self.counts[*i]})
        }).collect();
        let mut inclusive_indices: Vec<_> = (1..self.inclusive.len())
            .filter(|i| self.inclusive[*i][1] != 0)
            .collect();
        inclusive_indices.sort_by_key(|i| std::cmp::Reverse(self.inclusive[*i][0]));
        let inclusive:Vec<_>=inclusive_indices.iter().take(120).map(|i|{
            let f=&self.functions[*i-1];json!({"function":f.name,"file":f.file,"cseg":f.segment,"ticks":self.inclusive[*i][0],"calls":self.inclusive[*i][1]})
        }).collect();
        let result = json!({"functions":functions,"omitted_function_ticks":omitted,"without_execute_address_or_symbol":self.counts[0],
            "inclusive_functions":inclusive,"inclusive_scope":"Selected function entry to caller continuation at the same stack depth, including nested calls and waits. Inclusive rows overlap."});
        self.counts.fill(0);
        self.inclusive.fill([0; 2]);
        Ok(result)
    }
}
