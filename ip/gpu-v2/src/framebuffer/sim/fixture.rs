//! Deterministic backing-store fixture. Its delays are not SDRAM measurements.
use super::super::ports::*;
#[derive(Clone, Debug)]
pub struct Fixture {
    pub bytes: Vec<u8>,
    pub cycle: u64,
    pub request_period: u64,
    pub beat_period: u64,
    pub ack_delay: u64,
    pub fail_request: Option<u64>,
    pub requests: u64,
    active: Option<(Request, u8, u64, bool)>,
}
impl Fixture {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            cycle: 0,
            request_period: 1,
            beat_period: 1,
            ack_delay: 3,
            fail_request: None,
            requests: 0,
            active: None,
        }
    }
    pub fn idle(&self) -> bool {
        self.active.is_none()
    }
}
impl MemoryPort for Fixture {
    fn cycle(&mut self, request: Option<Request>, write: Option<u64>) -> Result<Response, String> {
        self.cycle += 1;
        let mut r = Response::default();
        if let Some((q, index, due, fail)) = self.active.as_mut() {
            if *index < 16 && self.cycle.is_multiple_of(self.beat_period.max(1)) {
                let address = q.address_bytes as usize + usize::from(*index) * 8;
                if q.write {
                    if let Some(data) = write {
                        self.bytes[address..address + 8].copy_from_slice(&data.to_le_bytes());
                        r.write_accepted = true;
                        *index += 1;
                    }
                } else {
                    r.read = Some((
                        *index,
                        u64::from_le_bytes(self.bytes[address..address + 8].try_into().unwrap()),
                    ));
                    *index += 1;
                }
                if *index == 16 {
                    *due = self.cycle + self.ack_delay.max(1);
                }
            } else if *index == 16 && self.cycle >= *due {
                r.complete = Some(!*fail);
                self.active = None;
            }
        } else if let Some(q) = request {
            if self.cycle.is_multiple_of(self.request_period.max(1)) {
                assert_eq!(q.address_bytes % 128, 0);
                assert!(q.address_bytes + 128 <= self.bytes.len() as u64);
                self.requests += 1;
                self.active = Some((q, 0, 0, self.fail_request == Some(self.requests)));
                r.accepted = true;
            }
        }
        Ok(r)
    }
}

/// Host stimulus/trace storage is outside the bounded hardware model.
pub fn replay(
    surface: MaterializedSurface,
    context: Context,
    quads: &[Quad],
    memory: Fixture,
    ce_period: u64,
    max_cycles: u64,
) -> Result<(super::bounded::Model, Fixture, Vec<super::bounded::Cycle>), String> {
    replay_model(
        super::bounded::Model::new(surface, context)?,
        quads,
        memory,
        ce_period,
        max_cycles,
    )
}

pub fn replay_pipelined(
    surface: MaterializedSurface,
    context: Context,
    quads: &[Quad],
    memory: Fixture,
    ce_period: u64,
    max_cycles: u64,
) -> Result<(super::bounded::Model, Fixture, Vec<super::bounded::Cycle>), String> {
    replay_model(
        super::bounded::Model::new_pipelined(surface, context)?,
        quads,
        memory,
        ce_period,
        max_cycles,
    )
}

pub fn replay_forwarding(
    surface: MaterializedSurface,
    context: Context,
    quads: &[Quad],
    memory: Fixture,
    ce_period: u64,
    max_cycles: u64,
) -> Result<(super::bounded::Model, Fixture, Vec<super::bounded::Cycle>), String> {
    replay_model(
        super::bounded::Model::new_forwarding(surface, context)?,
        quads,
        memory,
        ce_period,
        max_cycles,
    )
}

fn replay_model(
    mut model: super::bounded::Model,
    quads: &[Quad],
    mut memory: Fixture,
    ce_period: u64,
    max_cycles: u64,
) -> Result<(super::bounded::Model, Fixture, Vec<super::bounded::Cycle>), String> {
    let mut cursor = 0;
    let mut flushing = false;
    let mut trace = Vec::new();
    for cycle in 0..max_cycles {
        let input = quads.get(cursor / 8).map(|q| super::bounded::OutputRow {
            header: q.header,
            row: (cursor % 8) as u8,
            data: q.rows()[cursor % 8],
        });
        let t = model.step(ce_period == 0 || cycle % ce_period != 0, input, &mut memory)?;
        if t.input_accepted {
            cursor += 1;
        }
        trace.push(t);
        if cursor == quads.len() * 8 && !flushing && !model.fault {
            model.request_flush()?;
            flushing = true;
        }
        if model.flush_complete || model.drained() {
            return Ok((model, memory, trace));
        }
    }
    Err(format!(
        "framebuffer replay exceeded {max_cycles} wall cycles"
    ))
}
