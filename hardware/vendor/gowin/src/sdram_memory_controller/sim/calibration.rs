//! Bounded event-level experiment. Bridge transports remain explicit estimates.
use super::super::ports::*;
use super::{
    controller::{self, RowState},
    traffic::{ChainPolicy, Load},
};
use std::collections::BTreeMap;

pub fn bank_row(address: u64) -> (usize, u64) {
    (((address >> 7) & 3) as usize, address >> 12)
}
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub arrival: u64,
    pub client: Client,
    pub burst: Burst,
}
#[derive(Clone, Debug)]
pub struct Config {
    pub controller: controller::Config,
    pub load: Load,
    pub chain: ChainPolicy,
    pub recovery: u64,
    pub max_samples: usize,
    pub max_cycles: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            controller: Default::default(),
            load: Load::solo(),
            chain: ChainPolicy::ExistingUnchained,
            recovery: 1,
            max_samples: 65536,
            max_cycles: 100_000_000,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub samples: u64,
    pub sum_first: u64,
    pub sum_complete: u64,
    pub sum_queue: u64,
    pub sum_arbiter: u64,
    pub min_complete: u64,
    pub max_complete: u64,
    pub row_hits: u64,
    pub row_closed: u64,
    pub row_conflicts: u64,
    pub bank_changes: u64,
    pub hidden_prepare_core: u64,
    pub refresh_core: u64,
    pub priority_breaks: u64,
    pub refresh_breaks: u64,
    pub bank_breaks: u64,
    pub sector_first: [u64; 4],
    pub chained_sectors: u64,
    pub completions: Vec<u64>,
}
impl Stats {
    pub fn mean_first(&self) -> f64 {
        self.sum_first as f64 / self.samples.max(1) as f64
    }
    pub fn mean_complete(&self) -> f64 {
        self.sum_complete as f64 / self.samples.max(1) as f64
    }
    pub fn mean_arbiter(&self) -> f64 {
        self.sum_arbiter as f64 / self.samples.max(1) as f64
    }
    pub fn p95(&self) -> u64 {
        let mut values = self.completions.clone();
        values.sort_unstable();
        if values.is_empty() {
            0
        } else {
            values[(values.len() * 95).div_ceil(100) - 1]
        }
    }
}
#[derive(Clone, Debug)]
pub struct Observation {
    pub sample: Sample,
    /// Excludes same-client self-queue, includes background arbitration/refresh.
    pub eligible: u64,
    pub first: u64,
    pub complete: u64,
    pub sector_first: Vec<u64>,
    pub runs: Vec<controller::Run>,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub config: Config,
    pub classes: [Stats; 8],
    pub observations: Vec<Observation>,
    pub background_requests: u64,
}
struct Job {
    sample: Sample,
    target: Option<usize>,
    sectors: Vec<Burst>,
    next: usize,
    eligible: u64,
    first: Option<u64>,
    runs: Vec<controller::Run>,
    firsts: Vec<u64>,
    order: usize,
}

pub fn analyze(trace: &[Sample], config: Config) -> Result<Report, String> {
    config.load.validate()?;
    if trace.is_empty()
        || trace.len() > config.max_samples
        || config.max_samples > 65536
        || config.max_cycles == 0
        || config.max_cycles > 100_000_000
        || config.recovery == 0
        || config.recovery > 1000
        || trace.windows(2).any(|v| v[0].arrival > v[1].arrival)
        || trace.iter().any(|s| s.arrival >= config.max_cycles)
    {
        return Err("SDRAM calibration bounds/order".into());
    }
    let mut controller = controller::Controller::new(config.controller)?;
    let mut jobs = Vec::new();
    let mut queues: [BTreeMap<(u64, usize), usize>; 7] = std::array::from_fn(|_| BTreeMap::new());
    for (i, &sample) in trace.iter().enumerate() {
        queues[sample.client.index()].insert((sample.arrival, i), i);
        jobs.push(Job {
            sample,
            target: Some(i),
            sectors: sample.burst.sectors()?,
            next: 0,
            eligible: 0,
            first: None,
            runs: vec![],
            firsts: vec![],
            order: i,
        });
    }
    // Generate background only through the final target completion, rather than
    // running all configured streams indefinitely after the experiment.
    let mut stream_index = vec![0_u64; config.load.streams.len()];
    let mut now = 0;
    // One logical foreground at a time, matching the average runtime's FIFO.
    // Foreground self-queueing must not be baked into its service profile.
    let mut foreground_available = 0;
    let mut foreground_active: Option<usize> = None;
    let mut done = 0;
    let mut cursor = 1_usize;
    let mut ready = [0_u64; 7];
    let mut report = Report {
        config: config.clone(),
        classes: std::array::from_fn(|_| Stats::default()),
        observations: vec![],
        background_requests: 0,
    };
    let mut iterations = 0;
    while done < trace.len() {
        iterations += 1;
        if iterations > config.max_samples * 16 || now >= config.max_cycles {
            return Err("SDRAM calibration watchdog/overload".into());
        }
        for (stream, index) in config.load.streams.iter().zip(&mut stream_index) {
            while stream.arrival(*index).is_some_and(|arrival| arrival <= now) {
                if jobs.len() >= config.max_samples * 16 {
                    return Err("SDRAM background queue bound".into());
                }
                let sample = Sample {
                    arrival: stream.arrival(*index).unwrap(),
                    client: stream.client,
                    burst: stream.burst(*index),
                };
                queues[sample.client.index()].insert(
                    (
                        sample.arrival,
                        trace.len() + report.background_requests as usize,
                    ),
                    jobs.len(),
                );
                jobs.push(Job {
                    sample,
                    target: None,
                    sectors: sample.burst.sectors()?,
                    next: 0,
                    eligible: 0,
                    first: None,
                    runs: vec![],
                    firsts: vec![],
                    order: trace.len() + report.background_requests as usize,
                });
                report.background_requests += 1;
                *index += 1;
            }
        }
        let heads: [Option<usize>; 7] =
            std::array::from_fn(|client| queues[client].first_key_value().map(|(_, &i)| i));
        let eligible = |i: usize, c: usize| {
            jobs[i]
                .sample
                .arrival
                .max(ready[c])
                .max(if jobs[i].target.is_some() {
                    foreground_available
                } else {
                    0
                })
        };
        let permitted = |i: usize| {
            jobs[i].target.is_none() || foreground_active.is_none_or(|active| active == i)
        };
        let is_ready = |c: usize| heads[c].is_some_and(|i| permitted(i) && eligible(i, c) <= now);
        let chosen = if is_ready(0) {
            Some(0)
        } else {
            (0..6)
                .map(|n| 1 + (cursor - 1 + n) % 6)
                .find(|&c| is_ready(c))
        };
        let Some(client) = chosen else {
            let next_job = heads
                .iter()
                .enumerate()
                .filter_map(|(c, i)| i.filter(|&i| permitted(i)).map(|i| eligible(i, c)))
                .min();
            let next_background = config
                .load
                .streams
                .iter()
                .zip(&stream_index)
                .filter_map(|(s, &i)| s.arrival(i))
                .min();
            now = next_job
                .into_iter()
                .chain(next_background)
                .min()
                .ok_or("SDRAM missing pending job")?;
            continue;
        };
        let i = heads[client].unwrap();
        if jobs[i].target.is_some() {
            foreground_active = Some(i);
        }
        if client != 0 {
            cursor = 1 + client % 6;
        }
        // The front of a client queue becomes eligible only after its previous
        // request completes: its own queue is not charged again in a fixed profile.
        if jobs[i].first.is_none() && jobs[i].next == 0 {
            jobs[i].eligible =
                jobs[i]
                    .sample
                    .arrival
                    .max(ready[client])
                    .max(if jobs[i].target.is_some() {
                        foreground_available
                    } else {
                        0
                    });
        }
        let display_pending = heads[0].map(|i| jobs[i].sample.arrival);
        let next_display = config
            .load
            .streams
            .iter()
            .zip(&stream_index)
            .filter(|(s, _)| s.client == Client::Display)
            .filter_map(|(s, &i)| s.arrival(i))
            .chain(display_pending)
            .min();
        let run = controller.serve(
            &jobs[i].sectors[jobs[i].next..],
            now,
            config.chain == ChainPolicy::ChainedCandidate,
            |edge| client == 0 || next_display.is_none_or(|arrival| arrival * 2 > edge),
        )?;
        let complete = run.done_system;
        if complete >= config.max_cycles {
            return Err("SDRAM calibration completion watchdog".into());
        }
        if jobs[i].first.is_none() {
            jobs[i].first = Some(run.sectors[0].first_system);
        }
        jobs[i]
            .firsts
            .extend(run.sectors.iter().map(|s| s.first_system));
        jobs[i].next += run.sectors.len();
        jobs[i].runs.push(run);
        now = complete + config.recovery;
        if jobs[i].next == jobs[i].sectors.len() {
            queues[client].remove(&(jobs[i].sample.arrival, jobs[i].order));
            ready[client] = now;
            if jobs[i].target.is_some() {
                foreground_active = None;
                foreground_available = now;
                done += 1;
                let j = &jobs[i];
                let s = &mut report.classes[j.sample.burst.class()?];
                let elapsed = complete - j.eligible;
                s.samples += 1;
                s.sum_first += j.first.unwrap() - j.eligible;
                s.sum_complete += elapsed;
                s.sum_queue += j.eligible - j.sample.arrival;
                if s.samples == 1 {
                    s.min_complete = elapsed;
                } else {
                    s.min_complete = s.min_complete.min(elapsed);
                }
                s.max_complete = s.max_complete.max(elapsed);
                s.completions.push(elapsed);
                for (n, first) in j.firsts.iter().enumerate() {
                    s.sector_first[n] += first - j.eligible;
                }
                let mut prior_end = j.eligible;
                for run in &j.runs {
                    // grant = (accept - request transport - refresh wait)/2.
                    let grant = (run.sectors[0].accept_core
                        - config.controller.request_transport
                        - run.refresh_wait_core)
                        .div_ceil(2);
                    s.sum_arbiter += grant.saturating_sub(prior_end);
                    prior_end = run.done_system + config.recovery;
                    s.refresh_core += run.refresh_wait_core;
                    s.priority_breaks += u64::from(run.priority_break);
                    s.refresh_breaks += u64::from(run.refresh_break);
                    s.bank_breaks += u64::from(run.bank_break);
                    for sector in &run.sectors {
                        match sector.row_state {
                            RowState::Hit => s.row_hits += 1,
                            RowState::Closed => s.row_closed += 1,
                            RowState::Conflict => s.row_conflicts += 1,
                        }
                        s.bank_changes += u64::from(sector.bank_changed);
                        s.hidden_prepare_core += sector.hidden_prepare_core;
                        s.chained_sectors += u64::from(sector.chained);
                    }
                }
                report.observations.push(Observation {
                    sample: j.sample,
                    eligible: j.eligible,
                    first: j.first.unwrap(),
                    complete,
                    sector_first: j.firsts.clone(),
                    runs: j.runs.clone(),
                });
            }
        }
    }
    Ok(report)
}
pub fn representative_trace(per_class: usize) -> Result<Vec<Sample>, String> {
    if per_class == 0 || per_class > 8192 {
        return Err("SDRAM trace bound".into());
    }
    let mut trace = Vec::new();
    for n in 0..per_class {
        for class in 0..8 {
            let bytes = BURST_BYTES[class % 4];
            let block = if n % 4 == 3 {
                (n as u64 * 7919) % 4096
            } else {
                n as u64 % 4096
            };
            trace.push(Sample {
                arrival: trace.len() as u64 * 80,
                client: if class < 4 {
                    Client::GpuReadOnly
                } else {
                    Client::FramebufferWrite
                },
                burst: Burst {
                    address: block * 512,
                    bytes,
                    access: if class < 4 {
                        Access::Read
                    } else {
                        Access::Write
                    },
                },
            });
        }
    }
    Ok(trace)
}
