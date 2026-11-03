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
    truncated: bool,
    error: Option<io::Error>,
    state: Option<[u64; 10]>,
    retired: Option<u64>,
    busy: bool,
    submits: u64,
    presents: u64,
    applied: u64,
    frame_limit: u64,
    frames: Vec<Value>,
    origin: u64,
    last_tick: u64,
    cp_phase: Option<u8>,
    owners: [VecDeque<bool>; 7],
    raster_start: Option<(u64, u64, u32)>,
    vsync: bool,
    frames_seen: u64,
}
impl Recorder {
    pub fn create(
        folder: &Path,
        program: &[u8],
        origin: u64,
        max_events: u64,
        max_frames: u64,
    ) -> io::Result<Self> {
        if max_events == 0 || max_events > 10_000_000 || max_frames > 1024 {
            return Err(io::Error::other(
                "1..10000000 events and at most 1024 frames required",
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
            writer: BufWriter::new(File::create(folder.join("system-events.jsonl"))?),
            limit: max_events,
            count: 0,
            truncated: false,
            error: None,
            state: None,
            retired: None,
            busy: false,
            submits: 0,
            presents: 0,
            applied: 0,
            frame_limit: max_frames,
            frames: Vec::new(),
            origin,
            last_tick: origin,
            cp_phase: None,
            owners: Default::default(),
            raster_start: None,
            vsync: false,
            frames_seen: 0,
        })
    }
    fn emit(&mut self, tick: u64, kind: &str, data: Value) {
        if self.error.is_some() {
            return;
        }
        if self.count == self.limit {
            self.truncated = true;
            return;
        }
        let result = serde_json::to_writer(&mut self.writer, &json!([tick, kind, data]))
            .map_err(io::Error::other)
            .and_then(|_| self.writer.write_all(b"\n"));
        if let Err(e) = result {
            self.error = Some(e);
        }
        self.count += 1;
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
            self.emit(tick, "gpu_done", json!(self.submits));
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
        if let Some(n) = step.native {
            if n.read_bytes != 0 || n.write_slot_bytes != 0 {
                self.emit(
                    step.instant.ticks,
                    "native",
                    json!([n.read_bytes, n.write_slot_bytes, n.write_unmasked_bytes]),
                );
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
        self.frames_seen += 1;
        if self.frames.len() as u64 >= self.frame_limit || self.truncated {
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
        metadata["schema"] = json!("digitaldesign.system-emu-capture");
        metadata["version"] = json!(1);
        metadata["source_kind"] = json!("system-cycle-emu");
        metadata["origin_tick"] = json!(self.origin);
        metadata["end_tick"] = json!(system.clock().peek().ticks);
        metadata["quantum_hz"] = json!(system.clock().quantum_hz);
        metadata["logic_period"] = json!(148);
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
        metadata["ports"] = json!(PORTS);
        metadata["capabilities"] = json!({"client_payload64":true,"native_model32":true,"pin_electrical":false,"instruction_owner_lineage":false,"device5":"fixed-snapshot-peer","loader":"synthetic-preload"});
        fs::write(
            self.folder.join("capture.json"),
            serde_json::to_vec_pretty(&metadata).map_err(io::Error::other)?,
        )
    }
}
