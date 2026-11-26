//! Optional read-only command, DMA and pipeline observations from committed edges.
use gpu_v2::system::vliw_cycle::{
    command::Phase,
    draw::{Edge, Output},
    trace::LOAD_COLUMNS,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, error::Error, fs, path::Path};

pub struct Timeline {
    expected: Vec<(usize, u32)>,
    commands: Vec<Value>,
    active: Option<usize>,
    phases: Vec<Value>,
    last_phase: Option<(Phase, usize, Option<usize>)>,
    phase_cycles: BTreeMap<String, usize>,
    load_sum: [u64; 39],
    events: Vec<Value>,
    event_counts: BTreeMap<&'static str, usize>,
    cache: Vec<Value>,
    pending_cache: Option<usize>,
}

fn kind(word: u32) -> &'static str {
    match word >> 28 {
        0 => "NOP",
        1 => "DMA_LOCAL",
        2 => "DMA_MESH",
        3 => "DRAW",
        4 => "WAIT",
        5 if word & 128 != 0 => "SET_SCENE",
        5 => "SET_GLOBAL",
        6 if word & 128 != 0 => "SET_MATERIAL",
        6 => "SET_RENDER",
        7 => "FLUSH",
        8 => "CLEAR",
        9 => "TEX_INVALIDATE",
        10 => "SET_TARGET",
        11 => "UI_GRID",
        12 => "MEMCPY",
        15 => "END",
        _ => "UNKNOWN",
    }
}

impl Timeline {
    pub fn new(memory: &[u16], addresses: Vec<usize>) -> Self {
        Self {
            expected: addresses
                .into_iter()
                .map(|at| {
                    (
                        at * 2,
                        u32::from(memory[at]) | u32::from(memory[at + 1]) << 16,
                    )
                })
                .collect(),
            commands: Vec::new(),
            active: None,
            phases: Vec::new(),
            last_phase: None,
            phase_cycles: BTreeMap::new(),
            load_sum: [0; 39],
            events: Vec::new(),
            event_counts: BTreeMap::new(),
            cache: Vec::new(),
            pending_cache: None,
        }
    }

    fn close_phase(&mut self, end: usize) {
        if let Some((phase, start, command)) = self.last_phase.take() {
            self.phases.push(
                json!({"phase":format!("{phase:?}"),"start":start,"end":end,"command":command}),
            );
        }
    }

    pub fn observe(
        &mut self,
        cycle: usize,
        phase: Phase,
        o: &Output,
        edge: Edge,
    ) -> Result<(), Box<dyn Error>> {
        // All timestamps are absolute GPU clocks. Intervals are [start,end).
        *self.phase_cycles.entry(format!("{phase:?}")).or_default() += 1;
        if self
            .last_phase
            .is_some_and(|(p, _, c)| p != phase || c != self.active)
        {
            self.close_phase(cycle);
        }
        if self.last_phase.is_none() {
            self.last_phase = Some((phase, cycle, self.active));
        }
        for (sum, load) in self.load_sum.iter_mut().zip(edge.trace.load) {
            *sum += load;
        }
        for event in edge.trace.events {
            *self.event_counts.entry(event.kind).or_default() += 1;
            if matches!(
                event.kind,
                "dma_job"
                    | "dma_request"
                    | "dma_return"
                    | "dma_done"
                    | "draw"
                    | "vt_end"
                    | "r_start"
                    | "r_end"
                    | "master_request"
                    | "master_terminal"
                    | "target"
                    | "target_unbind"
                    | "write_owner"
            ) {
                self.events
                    .push(json!({"cycle":cycle,"kind":event.kind,"values":event.values}));
            }
        }
        let p = o.control.processor;
        let d = o.control.transport.dma;
        if d.cache_done {
            let at = self
                .pending_cache
                .take()
                .ok_or("Unowned cache DMA completion")?;
            self.cache[at]["end"] = json!(cycle + 1);
        }
        if p.cache.valid && d.cache_ready {
            if self.pending_cache.is_some() {
                return Err("Overlapping cache DMA observations".into());
            }
            self.pending_cache = Some(self.cache.len());
            self.cache.push(json!({"start":cycle,"end":null,
                "source":(p.cache.source_halfword & !1)*2,"bytes":128,
                "table":p.cache.source_halfword & 1 != 0,"bank":p.cache.bank}));
        }
        let c = edge.command;
        if let Some((pc, word)) = c.latched {
            if self.active.is_some() {
                return Err("New command latch before prior release".into());
            }
            let index = self.commands.len();
            let &(address, expected) = self.expected.get(index).ok_or("Extra latched command")?;
            if word != expected {
                return Err(
                    format!("Latched command {index}: {word:08x} != {expected:08x}").into(),
                );
            }
            self.active = Some(index);
            self.commands
                .push(json!({"index":index,"address":address,"pc":pc,
                "word":format!("{word:08x}"),"kind":kind(word),
                "latch":cycle,"execute":null,"issue":null,"end":null}));
        }
        if let Some((_, word)) = c.execute {
            let at = self.active.ok_or("Exec without latched command")?;
            if self.commands[at]["execute"].is_null() {
                self.commands[at]["execute"] = json!(cycle);
            }
            if word >> 28 == 15 && self.commands[at]["word"] != format!("{word:08x}") {
                return Err("END command identity".into());
            }
        }
        if c.issued.is_some() {
            let at = self.active.ok_or("Issue without latched command")?;
            self.commands[at]["issue"] = json!(cycle);
        }
        if c.released.is_some() {
            let at = self
                .active
                .take()
                .ok_or("Release without latched command")?;
            self.commands[at]["end"] = json!(cycle + 1);
        }
        Ok(())
    }

    pub fn finish(mut self, output: &Path, first: usize, end: usize) -> Result<(), Box<dyn Error>> {
        self.close_phase(end);
        if self.commands.len() != self.expected.len()
            || self.active.is_some()
            || self.pending_cache.is_some()
        {
            return Err("Incomplete command/cache timeline".into());
        }
        let load: BTreeMap<_, _> = LOAD_COLUMNS.into_iter().zip(self.load_sum).collect();
        let report = json!({"format":"tankracer.gpu-timeline.v1",
            "scope":"Read-only committed-edge observations. Absolute 54 MHz GPU clocks; intervals [start,end). Command end releases CP, not all downstream pixels. Phase cycles partition busy time; load counters overlap.",
            "start":first,"end":end,"phase_cycles":self.phase_cycles,"load_sum":load,
            "event_counts":self.event_counts,"commands":self.commands,"phases":self.phases,
            "cache_dma":self.cache,"events":self.events});
        fs::write(output.join("timeline.json"), serde_json::to_vec(&report)?)?;
        Ok(())
    }
}
