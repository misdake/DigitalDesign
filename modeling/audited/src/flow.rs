//! Bounded control-token execution: context leases, credits, CE and commit.
//! It carries no numerical Fixed payload and cannot construct numerical values.
use crate::Fault;
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextLease {
    pub bank: usize,
    pub epoch: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowConfig {
    pub context_banks: usize,
    pub fifo_capacity: usize,
    pub id_bits: u32,
    pub epoch_bits: u32,
    pub payload_bits: u32,
    pub phase_interval: u64,
    pub max_steps: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tick {
    pub ce: bool,
    pub load_context: Option<usize>,
    pub accept: Option<(u64, ContextLease)>,
    pub complete: Option<u64>,
    pub commit: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub id: u64,
    pub context: ContextLease,
    pub complete: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub advancing_cycles: u64,
    pub phase: u64,
    pub epochs: Vec<u64>,
    pub references: Vec<usize>,
    pub queue: VecDeque<Token>,
    pub committed: Vec<u64>,
    pub peak_fifo_bits: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickRecord {
    pub request: Tick,
    pub committed: Option<u64>,
    pub lease: Option<ContextLease>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowTrace {
    pub config: FlowConfig,
    pub ticks: Vec<TickRecord>,
    pub final_state: Snapshot,
}
pub struct FlowMachine {
    config: FlowConfig,
    state: Snapshot,
    ticks: Vec<TickRecord>,
    accepted: BTreeSet<u64>,
}
fn bad(s: &str) -> Fault {
    Fault::Audit(s.into())
}
impl FlowConfig {
    pub fn token_bits(&self) -> u32 {
        let bank_bits = usize::BITS - self.context_banks.saturating_sub(1).leading_zeros();
        self.id_bits
            .saturating_add(self.epoch_bits)
            .saturating_add(self.payload_bits)
            .saturating_add(bank_bits)
            .saturating_add(1)
    }
}
fn fits(value: u64, bits: u32) -> bool {
    bits == 64 || value < (1_u64 << bits)
}
impl FlowMachine {
    pub fn new(config: FlowConfig) -> Result<Self, Fault> {
        if config.context_banks == 0
            || config.context_banks > 64
            || config.fifo_capacity == 0
            || config.fifo_capacity > 65536
            || !(1..=64).contains(&config.id_bits)
            || !(1..=64).contains(&config.epoch_bits)
            || config.payload_bits > 3960
            || config.phase_interval == 0
            || config.max_steps == 0
            || config.max_steps > 1_000_000
        {
            return Err(bad("flow bounds"));
        }
        let state = Snapshot {
            advancing_cycles: 0,
            phase: 0,
            epochs: vec![0; config.context_banks],
            references: vec![0; config.context_banks],
            queue: VecDeque::new(),
            committed: Vec::new(),
            peak_fifo_bits: 0,
        };
        Ok(Self {
            config,
            state,
            ticks: Vec::new(),
            accepted: BTreeSet::new(),
        })
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.state
    }
    /// One wall-clock tick. CE=0 freezes tokens, epochs and the issue phase.
    /// Completion precedes commit; commit releases credit before acceptance.
    /// A failed transition is atomic and leaves state and trace unchanged.
    pub fn tick(&mut self, request: Tick) -> Result<TickRecord, Fault> {
        if self.ticks.len() >= self.config.max_steps {
            return Err(bad("flow step limit"));
        }
        let mut record = TickRecord {
            request: request.clone(),
            committed: None,
            lease: None,
        };
        if request.ce {
            let mut next = self.state.clone();
            if let Some(id) = request.complete {
                let token = next
                    .queue
                    .iter_mut()
                    .find(|t| t.id == id)
                    .ok_or_else(|| bad("completion without live token"))?;
                if std::mem::replace(&mut token.complete, true) {
                    return Err(bad("duplicate completion"));
                }
            }
            if request.commit {
                if !next.queue.front().is_some_and(|t| t.complete) {
                    return Err(bad("commit before head completion"));
                }
                let token = next.queue.pop_front().unwrap();
                next.references[token.context.bank] -= 1;
                next.committed.push(token.id);
                record.committed = Some(token.id);
            }
            if let Some(bank) = request.load_context {
                let refs = next
                    .references
                    .get(bank)
                    .ok_or_else(|| bad("context bank index"))?;
                if *refs != 0 {
                    return Err(bad("overwrite of leased context"));
                }
                next.epochs[bank] = next.epochs[bank]
                    .checked_add(1)
                    .ok_or_else(|| bad("context epoch overflow"))?;
                if !fits(next.epochs[bank], self.config.epoch_bits) {
                    return Err(bad("context epoch width"));
                }
                record.lease = Some(ContextLease {
                    bank,
                    epoch: next.epochs[bank],
                });
            }
            if let Some((id, context)) = request.accept {
                if !fits(id, self.config.id_bits) {
                    return Err(bad("token identity width"));
                }
                if next.phase != 0 {
                    return Err(bad("accept outside issue phase"));
                }
                if self.accepted.contains(&id) {
                    return Err(bad("duplicate token identity"));
                }
                if next.queue.len() >= self.config.fifo_capacity {
                    return Err(bad("FIFO credit exhausted"));
                }
                if context.epoch == 0 || next.epochs.get(context.bank) != Some(&context.epoch) {
                    return Err(bad("stale context lease"));
                }
                next.references[context.bank] += 1;
                next.queue.push_back(Token {
                    id,
                    context,
                    complete: false,
                });
                next.peak_fifo_bits = next
                    .peak_fifo_bits
                    .max(next.queue.len() as u64 * u64::from(self.config.token_bits()));
            }
            next.advancing_cycles = next
                .advancing_cycles
                .checked_add(1)
                .ok_or_else(|| bad("flow cycle overflow"))?;
            next.phase = if next.phase + 1 == self.config.phase_interval {
                0
            } else {
                next.phase + 1
            };
            self.state = next;
            if let Some((id, _)) = request.accept {
                self.accepted.insert(id);
            }
        }
        self.ticks.push(record.clone());
        Ok(record)
    }
    pub fn finish(self) -> FlowTrace {
        FlowTrace {
            config: self.config,
            ticks: self.ticks,
            final_state: self.state,
        }
    }
}
impl FlowTrace {
    /// Replay accepted requests, including stalls, and compare all observations.
    pub fn audit(&self) -> Result<(), Fault> {
        let mut machine = FlowMachine::new(self.config.clone())?;
        for tick in &self.ticks {
            if machine.tick(tick.request.clone())? != *tick {
                return Err(bad("flow observation tamper"));
            }
        }
        if machine.snapshot() != &self.final_state {
            return Err(bad("flow final-state tamper"));
        }
        Ok(())
    }
}
