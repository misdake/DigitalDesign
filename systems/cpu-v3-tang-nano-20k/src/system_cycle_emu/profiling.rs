//! Bounded passive capture. No clock, mutable component, or memory write access.
use super::{scanout::Frame, Observation, Step, System};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

pub const PORTS: [&str; 7] = [
    "display",
    "instruction",
    "data",
    "dma",
    "gpu_ro",
    "gpu_fb_r",
    "gpu_fb_w",
];
pub struct Recorder {
    folder: PathBuf,
    writer: BufWriter<File>,
    limit: u64,
    count: u64,
    event_bytes: u64,
    byte_limit: u64,
    truncated: bool,
    error: Option<io::Error>,
    state: Option<[u64; 10]>,
    retired: Option<u64>,
    busy: bool,
    submits: u64,
    presents: u64,
    applied: u64,
    frame_limit: u64,
    profile_frame: Option<u64>,
    frames: Vec<Value>,
    origin: u64,
    last_tick: u64,
    cp_phase: Option<u8>,
    owners: [VecDeque<bool>; 7],
    raster_start: Option<(u64, u64, u32)>,
    vsync: bool,
    frames_seen: u64,
    executed_pc: Option<u32>,
    gpu_load: Option<[u64; 39]>,
    gpu_cache: Option<gpu_v2::system::vliw_cycle::trace::Cache>,
    preimage: bool,
    gpu_load_dirty: bool,
    gpu_control: Option<[u64; 9]>,
    submission_stopped: Option<&'static str>,
    latched_command: Option<(u32, u32)>,
    gpu_control_word: Option<u32>,
}

impl Recorder {
    pub fn create(
        folder: &Path,
        program: &[u8],
        origin: u64,
        max_events: u64,
        max_frames: u64,
    ) -> io::Result<Self> {
        Self::create_bounded(folder, program, origin, max_events, max_frames, 10_000_000)
    }
    /// Explicit bounded ordinary-game runner tier; legacy callers retain10M.
    pub fn create_runner(
        folder: &Path,
        program: &[u8],
        origin: u64,
        max_events: u64,
    ) -> io::Result<Self> {
        Self::create_bounded(folder, program, origin, max_events, 12, 112_000_000)
    }
    fn create_bounded(
        folder: &Path,
        program: &[u8],
        origin: u64,
        max_events: u64,
        max_frames: u64,
        ceiling: u64,
    ) -> io::Result<Self> {
        if max_events == 0 || max_events > ceiling || max_frames > 1024 {
            return Err(io::Error::other(
                "event/frame budget outside the selected recorder tier",
            ));
        }
        fs::create_dir(folder)?;
        fs::write(folder.join("program.bin"), program)?;
        // Preserve the exact running image once, outside simulated time. The importer hashes it.
        fs::copy(
            std::env::current_exe()?,
            folder.join("source-executable.bin"),
        )?;
        Ok(Self {
            folder: folder.into(),
            writer: BufWriter::new(File::create(folder.join("events.jsonl"))?),
            limit: max_events,
            count: 0,
            event_bytes: 0,
            byte_limit: if ceiling > 10_000_000 {
                4_294_967_296
            } else {
                u64::MAX
            },
            truncated: false,
            error: None,
            state: None,
            retired: None,
            busy: false,
            submits: 0,
            presents: 0,
            applied: 0,
            frame_limit: max_frames,
            profile_frame: None,
            frames: Vec::new(),
            origin,
            last_tick: origin,
            cp_phase: None,
            owners: Default::default(),
            raster_start: None,
            vsync: false,
            frames_seen: 0,
            executed_pc: None,
            gpu_load: None,
            gpu_cache: None,
            preimage: false,
            gpu_load_dirty: false,
            gpu_control: None,
            submission_stopped: None,
            latched_command: None,
            gpu_control_word: None,
        })
    }
    /// Select one application presentation, retaining its complete warmup journal.
    /// Call before observing; no modeled state, cache or timing is changed.
    pub fn select_frame(&mut self, number: u64) -> io::Result<()> {
        if number == 0 || self.count != 0 || !self.frames.is_empty() {
            return Err(io::Error::other(
                "select a positive frame before observation",
            ));
        }
        self.profile_frame = Some(number);
        Ok(())
    }
    /// One host-side copy of the unique model backing, before any observation.
    /// No modeled memory request, clock, flush or cache mutation is performed.
    pub fn initialize_memory(&mut self, system: &System) -> io::Result<()> {
        if self.count != 0
            || self.preimage
            || system.clock().peek().ticks != self.origin
            || system.statistics.gpu_submits != 0
            || system.statistics.present_requests != 0
        {
            return Err(io::Error::other(
                "capture preimage must precede observation and the first GPU submit/present",
            ));
        }
        let bytes: Vec<u8> = system
            .backing()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        fs::write(self.folder.join("initial-memory.bin"), bytes)?;
        self.preimage = true;
        Ok(())
    }
    fn emit(&mut self, tick: u64, kind: &str, data: Value) {
        if self.error.is_some() || self.truncated {
            return;
        }
        if self.count == self.limit {
            self.truncated = true;
            return;
        }
        let time = if matches!(kind, "native" | "physical_write") {
            json!([tick, 1])
        } else {
            json!(tick)
        };
        let line = match serde_json::to_vec(&json!([time, kind, data])) {
            Ok(line) => line,
            Err(e) => {
                self.error = Some(io::Error::other(e));
                return;
            }
        };
        if self.event_bytes + line.len() as u64 + 1 > self.byte_limit {
            self.truncated = true;
            return;
        }
        let result = self
            .writer
            .write_all(&line)
            .and_then(|_| self.writer.write_all(b"\n"));
        if let Err(e) = result {
            self.error = Some(e);
        }
        self.count += 1;
        self.event_bytes += line.len() as u64 + 1;
        self.last_tick = tick;
    }
    pub fn observe(&mut self, o: Observation<'_>) {
        if self.truncated || self.error.is_some() {
            return;
        }
        let (Some(cpu), Some(gpu), Some(m)) = (o.cpu, o.gpu, o.memory) else {
            return;
        };
        let tick = o.instant.ticks;
        let c = m.clients;
        let a = &m.arbiter;
        let valid = [
            c.display_valid,
            c.instruction.valid,
            c.data.valid,
            false,
            c.gpu[0].request.valid,
            c.gpu[1].request.valid,
            c.gpu[2].request.valid,
        ];
        let ready = [
            a.display_request_ready,
            a.instruction_request_ready,
            a.data_request_ready,
            a.dma_request_ready,
            a.gpu_ro_request_ready,
            a.gpu_fb_r_request_ready,
            a.gpu_fb_w_request_ready,
        ];
        let response = [
            a.display_response_valid,
            a.instruction_response_valid && c.instruction.response_ready,
            a.data_response_valid && c.data.response_ready,
            false,
            a.gpu_ro_response_valid,
            a.gpu_fb_r_response_valid,
            a.gpu_fb_w_response_valid,
        ];
        let terminal = m.endpoint.cpu_response_valid
            && m.endpoint.cpu_response_last
            && a.memory_response_ready;
        let pending = valid
            .iter()
            .enumerate()
            .fold(0, |v, (n, &b)| v | (u64::from(b) << n));
        let waiting = valid
            .iter()
            .zip(ready)
            .enumerate()
            .fold(0, |v, (n, (&b, r))| v | (u64::from(b && !r) << n));
        let state = [
            u64::from(o.startup_hold),
            u64::from(cpu.system.cpu_hold),
            u64::from(cpu.dcache.valid_sweep),
            u64::from(cpu.core.halted),
            u64::from(cpu.core.fault),
            u64::from(gpu.control.processor.busy),
            u64::from(!gpu.render.producer.idle),
            u64::from(!gpu.render.backend.idle),
            pending,
            waiting,
        ];
        if self.state != Some(state) {
            self.emit(tick, "state", json!(state));
            self.state = Some(state);
        }
        if self.retired != Some(cpu.core.retired_words) {
            self.emit(tick, "cpu", json!([cpu.core.retired_words, cpu.core.pc]));
            self.retired = Some(cpu.core.retired_words);
        }
        if self.busy && !gpu.control.processor.busy {
            if let Some(reason) = self.submission_stopped.take() {
                self.emit(tick, "gpu_stopped", json!([self.submits, reason]));
            } else {
                self.emit(tick, "gpu_done", json!(self.submits));
            }
        }
        self.busy = gpu.control.processor.busy;
        for n in 0..7 {
            if valid[n] && ready[n] {
                let (address, writing) = match n {
                    0 => (c.display_address, false),
                    1 => (c.instruction.address, false),
                    2 => (c.data.address, c.data.writing),
                    4..=6 => (c.gpu[n - 4].request.address_halfword, c.gpu[n - 4].writing),
                    _ => (0, false),
                };
                let beats = match n {
                    0 | 1 => 4,
                    2 => {
                        if c.data.line {
                            4
                        } else {
                            1
                        }
                    }
                    4..=6 => (u16::from(c.gpu[n - 4].request.count) + 1) * 4,
                    _ => 0,
                };
                self.emit(tick, "accept", json!([n, address, writing, beats]));
                self.owners[n].push_back(writing);
            }
            let read = response[n] && !self.owners[n].front().copied().unwrap_or(n == 6);
            let write =
                (n == 2 && a.data_write_data_ready) || (n == 6 && a.gpu_fb_w_write_data_ready);
            if read {
                self.emit(tick, "payload", json!([n, 8, 0]));
            }
            if write {
                self.emit(tick, "payload", json!([n, 0, 8]));
            }
            if terminal && response[n] {
                let error = [
                    a.display_error,
                    a.instruction_error,
                    a.data_error,
                    false,
                    a.gpu_ro_error,
                    a.gpu_fb_r_error,
                    a.gpu_fb_w_error,
                ][n];
                self.emit(tick, "terminal", json!([n, error]));
                self.owners[n].pop_front();
            }
        }
        if let Some(bus) = o.bus {
            if bus.write && bus.index == 4 && bus.channel == 1 && gpu.control.processor.submit_ready
            {
                self.submits += 1;
                self.emit(tick, "submit", json!(self.submits));
            }
            if bus.write && bus.index == 3 && bus.channel == 3 && bus.data == 1 {
                self.presents += 1;
                self.emit(tick, "present_request", json!(self.presents));
            }
        }
        // Phase numbers are production CP state, not decoded instruction lifetimes.
        if self.cp_phase != Some(o.gpu_phase) {
            self.emit(tick, "phase", json!(o.gpu_phase));
            self.cp_phase = Some(o.gpu_phase);
        }
    }
    pub fn after_step(&mut self, step: &Step, system: &System) {
        if self.truncated || self.error.is_some() {
            return;
        }
        if let Some(edge) = step.gpu_commands {
            let tick = step.instant.ticks;
            if edge.aborted {
                self.submission_stopped = Some("abort");
            }
            if edge.reset && self.submission_stopped.is_none() {
                self.submission_stopped = Some("reset");
            }
            if let Some((base, bytes)) = edge.submit {
                self.submission_stopped = None;
                self.executed_pc = None;
                self.latched_command = None;
                self.emit(tick, "cb_submit", json!([self.submits, base, bytes]));
            }
            if let Some((pc, word)) = edge.latched {
                self.latched_command = Some((pc, word));
                self.emit(tick, "cb_latch", json!([self.submits, pc, word]));
            }
            if let Some((pc, word)) = edge.execute {
                if self.executed_pc != Some(pc) {
                    self.emit(tick, "cb_execute", json!([self.submits, pc, word]));
                    self.executed_pc = Some(pc);
                }
            }
            if let Some((pc, word)) = edge.issued {
                self.emit(tick, "cb_issue", json!([self.submits, pc, word]));
            }
            if let Some(pc) = edge.released {
                if let Some((latched, word)) =
                    self.latched_command.filter(|(latched, _)| *latched == pc)
                {
                    self.emit(tick, "cb_release", json!([self.submits, latched, word]));
                } else {
                    self.error = Some(io::Error::other(
                        "command release has no observed RAM latch",
                    ));
                }
            }
            for (kind, active) in [
                ("cb_drain", edge.drained),
                ("cb_abort", edge.aborted),
                ("cb_reset", edge.reset),
            ] {
                if active {
                    self.emit(tick, kind, json!([self.submits]));
                }
            }
        }
        if let Some(trace) = &step.gpu_trace {
            let tick = step.instant.ticks;
            if let Some(control) = trace.control {
                if self.gpu_control != Some(control) || self.gpu_control_word != trace.control_word
                {
                    self.emit(
                        tick,
                        "gpu_control",
                        json!([self.submits, control, trace.control_word]),
                    );
                    self.gpu_control = Some(control);
                    self.gpu_control_word = trace.control_word;
                }
            }
            // Rate samples are impulses, not held levels. Owner changes also
            // require the next old-state sample even when inventories match.
            if self.gpu_load != Some(trace.load)
                || self.gpu_load_dirty
                || trace.load[15..=20].iter().any(|&v| v != 0)
                || !trace.events.is_empty()
            {
                self.emit(tick, "gpu_load", json!(trace.load.as_slice()));
                self.gpu_load = Some(trace.load);
            }
            self.gpu_load_dirty = !trace.events.is_empty();
            if let Some(cache) = &trace.cache {
                if self.gpu_cache.as_ref() != Some(cache) {
                    self.emit(
                        tick,
                        "gpu_cache",
                        json!([
                            cache.valid,
                            cache.valid_tags(),
                            cache.color_base,
                            cache.depth_base
                        ]),
                    );
                    self.gpu_cache = Some(cache.clone());
                }
            }
            for event in &trace.events {
                self.emit(
                    tick,
                    "gpu_event",
                    json!([self.submits, event.kind, event.values]),
                );
            }
        }
        if let Some(n) = step.native {
            if n.read_bytes != 0 || n.write_slot_bytes != 0 {
                self.emit(
                    step.instant.ticks,
                    "native",
                    json!([n.read_bytes, n.write_slot_bytes, n.write_unmasked_bytes]),
                );
            }
            if self.preimage {
                if let Some(write) = n.write {
                    self.emit(
                        step.instant.ticks,
                        "physical_write",
                        json!([write.address_bytes, write.data, write.unmasked]),
                    );
                }
            }
        }
        if self.applied != system.statistics.presents_applied {
            self.applied = system.statistics.presents_applied;
            self.emit(
                step.instant.ticks,
                "present_applied",
                json!([
                    self.applied,
                    system.display_register(1) as u32 | ((system.display_register(2) as u32) << 16)
                ]),
            );
        }
        if step.instant.pixel_rise {
            let p = step.display.video;
            if p.vsync && !self.vsync {
                self.raster_start = None;
            }
            self.vsync = p.vsync;
            if p.de && self.raster_start.is_none() {
                let front =
                    system.display_register(1) as u32 | ((system.display_register(2) as u32) << 16);
                self.raster_start = Some((step.instant.ticks, self.applied, front));
            }
        }
    }
    pub fn frame(&mut self, frame: &Frame, tick: u64) -> io::Result<()> {
        self.frame_selected(frame, tick, true)
    }
    pub fn raster_descriptor(&self) -> Option<(u64, u64, u32)> {
        self.raster_start
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    /// Retain only explicitly selected stable presentations, while observing
    /// every raster start and every write from the original preimage onward.
    pub fn frame_selected(&mut self, frame: &Frame, tick: u64, selected: bool) -> io::Result<()> {
        self.frames_seen += 1;
        let selected = selected
            && self.profile_frame.is_none_or(|number| {
                self.applied == number
                    && self
                        .raster_start
                        .is_some_and(|(_, count, _)| count == number)
            });
        if !selected || self.frames.len() as u64 >= self.frame_limit || self.truncated {
            return Ok(());
        }
        let name = format!("scanout-{:06}.rgb", self.frames.len());
        fs::write(self.folder.join(&name), &frame.rgb)?;
        let (start, count, front) = self
            .raster_start
            .take()
            .ok_or_else(|| io::Error::other("scanout has no observed active start"))?;
        self.frames.push(json!({"file":name,"frame":frame.number,"start_tick":start,"end_tick":tick,"effective_pixels":frame.effective_pixels,"applied_present_count":self.applied,"front_at_start_halfword":front,"present_id":null,"present_changed_during_scanout":count!=self.applied,"source":"registered-rgb"}));
        Ok(())
    }
    pub fn finish(mut self, mut metadata: Value, system: &System) -> io::Result<()> {
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        self.writer.flush()?;
        let contract: Value =
            serde_json::from_str(gpu_v2::system::vliw_cycle::trace::CAPTURE_CONTRACT)
                .map_err(io::Error::other)?;
        for (key, value) in contract.as_object().unwrap() {
            metadata[key] = value.clone();
        }
        metadata["source_kind"] = json!("system-cycle-emu");
        metadata["origin_tick"] = json!(self.origin);
        metadata["end_tick"] = json!(system.clock().peek().ticks);
        metadata["quantum_hz"] = json!(system.clock().quantum_hz);
        metadata["logic_period"] = json!(148);
        metadata["clock_hz"] = json!(54_000_000);
        metadata["domains"] = json!({"gpu":true,"cpu":true,"display":true,"shared_service64":true,"native_model32":true,"physical_write":self.preimage});
        metadata["events"] = json!(self.count);
        metadata["truncated"] = json!(self.truncated);
        metadata["coverage_end_tick"] = json!(if self.truncated {
            self.last_tick
        } else {
            system.clock().peek().ticks
        });
        metadata["scanouts"] = json!(self.frames);
        metadata["frame_limit"] = json!(self.frame_limit);
        metadata["frames_seen"] = json!(self.frames_seen);
        metadata["event_limit"] = json!(self.limit);
        metadata["event_bytes"] = json!(self.event_bytes);
        if self.byte_limit != u64::MAX {
            metadata["event_byte_limit"] = json!(self.byte_limit);
        }
        metadata["ports"] = json!(PORTS);
        metadata["gpu_load_columns"] =
            json!(gpu_v2::system::vliw_cycle::trace::LOAD_COLUMNS.as_slice());
        metadata["image_journal"] = json!({"available":self.preimage,
            "preimage":if self.preimage { Some("initial-memory.bin") } else { None },
            "layout":"rgb565-macro16x16-column-pad16","width":400,"height":240,
            "plane_bytes":204800,"allocation_bytes":262144,
            "event_limit":self.limit,"truncated":self.truncated,
            "completed_target_budget":2,"proven_partial_prefix":true,
            "scope":"Actual native32 masked physical writes and GPU cache changes; registered scanout is separate"});
        if let Some(selection) = metadata.get("runner_selected_presentations").cloned() {
            metadata["image_journal"]["selected_presentations"] = selection;
            metadata["image_journal"]["completed_target_budget"] = json!(4);
            metadata["image_journal"]["max_command_images"] =
                metadata["runner_max_native_command_images"].clone();
        }
        if let Some(number) = self.profile_frame {
            metadata["profile_frame"] = json!(number);
            metadata["image_journal"]["selected_presentations"] = json!([number]);
        }
        metadata["capabilities"] = json!({"client_payload64":true,"native_model32":true,"shared_service64":true,"pin_electrical":false,"command_control_lineage":true,"gpu_signal_journal":true,"gpu_master_requests":true,"gpu_master_payloads":true,"instruction_owner_lineage":true,"device5":"fixed-snapshot-peer","loader":"synthetic-preload"});
        if metadata.get("runner_version").is_some() {
            metadata["capabilities"]["device5"] = json!("scripted-snapshot-peer");
        }
        fs::write(
            self.folder.join("capture.json"),
            serde_json::to_vec_pretty(&metadata).map_err(io::Error::other)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_truncation_keeps_a_complete_prefix_and_stops_all_later_events() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ordinary-runner-short");
        fs::create_dir_all(&root).unwrap();
        let folder = root.join(format!("byte-limit-{}", std::process::id()));
        let mut r = Recorder::create_runner(&folder, b"synthetic-recorder-unit", 0, 20).unwrap();
        r.byte_limit = 30;
        r.emit(0, "cpu", json!([0, 0]));
        assert_eq!(r.count, 1);
        r.emit(148, "too-large", json!(vec![123456789; 8]));
        assert!(r.truncated());
        r.emit(296, "cpu", json!([1, 1]));
        assert_eq!(r.count, 1);
        r.writer.flush().unwrap();
        let raw = fs::read_to_string(folder.join("events.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(raw.trim()).unwrap(),
            json!([0, "cpu", [0, 0]])
        );
    }
}
