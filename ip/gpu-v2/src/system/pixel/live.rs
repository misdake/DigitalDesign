//! Lighting-live connection unit: the real cycle emulator, not injected results.
//! It drives `LightingEmu` from controlled quad stimulus and feeds accepted
//! results into the existing J1 light store. Sampling remains controlled external
//! stimulus (`LiveTick::sample`), exactly as in J1.

use super::*;
use crate::lighting::{
    emu::LightingEmu,
    ports::{LightingContext, LightingRequest, LightingTick, PixelInput},
    LightingProfile,
};

/// Controlled quad: J1 attributes plus the per-lane lighting payload that a
/// future attribute→lighting stream would supply. Four lanes are held only from
/// admission until the last covered, non-default lane is issued; this is bounded
/// input stimulus, not a result copy or a second quad payload store.
#[derive(Clone, Copy, Debug)]
pub struct LiveQuad {
    pub quad: QuadInput,
    pub light: [PixelInput; 4],
}

/// One edge of external stimulus. `finish` and `final_ready` keep J1 semantics.
#[derive(Clone, Copy, Debug)]
pub struct LiveTick {
    pub ce: bool,
    pub final_ready: bool,
    /// Permit the real light-store write. A blocked result stays in LightingEmu.
    pub light_ready: bool,
    pub quad: Option<LiveQuad>,
    pub sample: Option<SampleWrite>,
    pub finish: bool,
}
impl Default for LiveTick {
    fn default() -> Self {
        Self {
            ce: true,
            final_ready: true,
            light_ready: true,
            quad: None,
            sample: None,
            finish: false,
        }
    }
}

/// Observed lighting handshakes on one edge, plus the wrapped J1 cycle.
#[derive(Clone, Debug)]
pub struct LiveCycle {
    pub wall: u64,
    pub ce: bool,
    pub model: Cycle,
    /// Request accepted by the emulator this edge, with its encoded identity.
    pub light_issued: Option<(PixelKey, u32)>,
    /// Result returned and stored this edge. `epoch` is checked against context.
    pub light_returned: Option<(PixelKey, LightingOutput, u16)>,
    /// The caller's offered quad was not accepted and remains caller-owned.
    pub input_held: bool,
}

/// One input snapshot, with no pre-admission quad copy or added result FIFO.
/// `pending` holds four 84-bit pixels (normal48 + NDC36), mask4, cursor3,
/// quad4 and valid1: 348 logical bits until the last covered lane issues.
/// Serial64 and `ticket_for_quad[16]` are host ownership witnesses, excluded
/// from that logical bill. An upstream source holds its offered quad until
/// `model.quad_accepted`, including through CE and admission backpressure.
///
/// The input snapshot is explicit additional state, not fitted resource evidence.
/// It changes neither the 16 global slots nor the store ports or six-bit key.
/// LightingEmu is stallable and holds its output until the real store write.
pub struct LightingLive {
    model: Model,
    emu: LightingEmu,
    context: LightingContext,
    context_loaded: bool,
    pending: Option<Issue>,
    ticket_for_quad: [Option<Ticket>; 16],
    pub stats: Stats,
}
#[derive(Clone, Copy)]
struct Issue {
    ticket: Ticket,
    mask: u8,
    light: [PixelInput; 4],
    lane: u8,
}
fn encode(quad: u8, lane: u8) -> u32 {
    u32::from(quad) << 2 | u32::from(lane)
}
impl LightingLive {
    pub fn new(pixel: Context, lighting: LightingContext, max_cycles: u64) -> Result<Self, String> {
        if max_cycles == 0 {
            return Err("lighting-live cycle bound".into());
        }
        lighting
            .validate()
            .map_err(|e| format!("lighting context: {e:?}"))?;
        let model = Model::new(pixel, max_cycles)?;
        let emu = LightingEmu::with_profile(
            LightingProfile::Fast,
            max_cycles
                .checked_add(8)
                .ok_or("lighting-live cycle bound overflow")?,
        )?;
        Ok(Self {
            model,
            emu,
            context: lighting,
            context_loaded: false,
            pending: None,
            ticket_for_quad: [None; 16],
            stats: Stats::default(),
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        self.model.snapshot()
    }
    /// Reuse the existing allocation witness for the sibling sampling adapter.
    /// This does not add another owner/tag table or change the six-bit key.
    pub(super) fn allocated_ticket(&self, quad: u8) -> Option<Ticket> {
        self.ticket_for_quad
            .get(usize::from(quad))
            .copied()
            .flatten()
    }
    pub fn complete(&self) -> bool {
        self.model.complete()
    }
    pub fn drained(&self) -> bool {
        self.model.drained() && self.pending.is_none() && self.emu.in_flight() == 0
    }
    pub fn abort(&mut self) {
        self.model.abort();
        self.pending = None;
        self.ticket_for_quad = [None; 16];
    }
    pub fn in_flight(&self) -> usize {
        self.emu.in_flight()
    }
    pub fn latency(&self) -> usize {
        self.emu.latency()
    }
    pub fn initiation_interval(&self) -> usize {
        self.emu.initiation_interval()
    }
    fn first_lane(mask: u8) -> u8 {
        (0..4).find(|&lane| mask >> lane & 1 != 0).unwrap_or(4)
    }
    fn decode(&self, id: u32, epoch: u16) -> Result<PixelKey, String> {
        if epoch != self.context.epoch {
            return Err("lighting result epoch mismatch".into());
        }
        if id > 63 {
            return Err("lighting result identity".into());
        }
        let quad = (id >> 2) as u8;
        let lane = (id & 3) as u8;
        let ticket =
            self.ticket_for_quad[usize::from(quad)].ok_or("lighting result for released quad")?;
        Ok(PixelKey { ticket, lane })
    }
    fn next_request(&self) -> Option<(PixelKey, LightingRequest)> {
        let issue = self.pending?;
        let ticket = self.ticket_for_quad[usize::from(issue.ticket.quad)]?;
        if ticket.serial != issue.ticket.serial || issue.lane >= 4 {
            return None;
        }
        Some((
            PixelKey {
                ticket,
                lane: issue.lane,
            },
            LightingRequest {
                pixel: issue.light[usize::from(issue.lane)],
                id: encode(issue.ticket.quad, issue.lane),
            },
        ))
    }
    fn advance_pending(&mut self) {
        if let Some(issue) = &mut self.pending {
            issue.lane += 1;
            while issue.lane < 4 && issue.mask >> issue.lane & 1 == 0 {
                issue.lane += 1;
            }
            if issue.lane >= 4 {
                self.pending = None;
            }
        }
    }
    /// One edge: exactly one emulator tick and one J1 tick. CE pauses the whole
    /// lighting datapath; a held return stays inside the emulator until the light
    /// store has a real write edge. The caller owns any unaccepted quad.
    pub fn step(
        &mut self,
        tick: LiveTick,
        memory: &mut impl MemoryPort,
    ) -> Result<LiveCycle, String> {
        let result = self.step_inner(tick, memory);
        if result.is_err() {
            self.abort();
        }
        result
    }
    fn step_inner(
        &mut self,
        tick: LiveTick,
        memory: &mut impl MemoryPort,
    ) -> Result<LiveCycle, String> {
        let model_running = matches!(self.model.snapshot().phase, Phase::Running | Phase::Closing);
        let loading_context = !self.context_loaded && tick.ce;
        let mut emu_tick = LightingTick {
            reset: !model_running,
            ce: tick.ce,
            context: loading_context.then_some(self.context),
            input: None,
            output_ready: false,
        };
        let probe = self.emu.signals(emu_tick);
        // The return is popped only on an edge that can actually store it.
        let take = !loading_context
            && tick.ce
            && tick.light_ready
            && model_running
            && probe.output.is_some();
        emu_tick.output_ready = take;
        let sig = self.emu.signals(emu_tick);
        let request = if model_running && !loading_context && sig.input_ready {
            self.next_request()
        } else {
            None
        };
        emu_tick.input = request.map(|(_, r)| r);
        let mut light_write = None;
        let mut returned = None;
        if let Some(result) = probe.output.filter(|_| take) {
            let key = self.decode(result.id, result.epoch)?;
            light_write = Some(LightWrite {
                key,
                value: result.output,
            });
            returned = Some((key, result.output, result.epoch));
        }
        if let Some((key, request)) = request {
            debug_assert_eq!(request.id, encode(key.ticket.quad, key.lane));
        }
        let present_quad = self
            .pending
            .is_none()
            .then(|| tick.quad.map(|h| h.quad))
            .flatten();
        let cycle = self.model.step(
            Tick {
                ce: tick.ce,
                final_ready: tick.final_ready,
                quad: present_quad,
                light: light_write,
                sample: tick.sample,
                finish: tick.finish,
            },
            memory,
        )?;
        let fault = matches!(cycle.snapshot.phase, Phase::FaultDraining | Phase::Faulted);
        if !fault && cycle.light_accepted != light_write.is_some() {
            return Err("lighting result lost by light store".into());
        }
        if fault {
            self.abort();
            emu_tick = LightingTick {
                reset: true,
                ce: false,
                context: None,
                input: None,
                output_ready: false,
            };
            returned = None;
        }
        let signals = self.emu.tick(emu_tick)?;
        if signals.context_ready {
            self.context_loaded = true;
        }
        if cycle.quad_accepted {
            let held = tick
                .quad
                .ok_or("J1 accepted a quad without offered lighting input")?;
            if let Some(ticket) = cycle.ticket {
                self.ticket_for_quad[usize::from(ticket.quad)] = Some(ticket);
                if !held.quad.default_light && held.quad.header.mask != 0 {
                    self.pending = Some(Issue {
                        ticket,
                        mask: held.quad.header.mask,
                        light: held.light,
                        lane: Self::first_lane(held.quad.header.mask),
                    });
                }
            }
        }
        if request.is_some() {
            self.advance_pending();
        }
        for event in &cycle.events {
            if let Event::Retired(ticket) = event {
                if self.ticket_for_quad[usize::from(ticket.quad)] == Some(*ticket) {
                    self.ticket_for_quad[usize::from(ticket.quad)] = None;
                }
            }
        }
        let light_issued = request.filter(|_| !fault).map(|(key, r)| (key, r.id));
        let input_held = tick.quad.is_some() && !cycle.quad_accepted;
        self.stats = self.model.stats.clone();
        Ok(LiveCycle {
            wall: cycle.wall,
            ce: tick.ce,
            model: cycle,
            light_issued,
            light_returned: returned,
            input_held,
        })
    }
}
