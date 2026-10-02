//! Candidate generation: deterministic baseline, critical-path list schedule,
//! backfilling insertion schedules, and bounded seeded random multi-start.
//!
//! Two construction methods share the same fixed hardware and graph:
//!
//! * *List scheduling* (baseline and the seeded restarts) walks every node in
//!   priority order and appends it after the earliest free lane time. It can
//!   never backfill an earlier idle hole.
//! * *Insertion scheduling* considers every lane of the node's resource and
//!   places the node at the earliest hole that is legal for its release gate,
//!   its predecessors and the lane's initiation interval, so earlier gaps are
//!   reused.

use crate::error::ScheduleError;
use crate::limits::{Limits, SearchConfig};
use crate::model::{Graph, NodeId, Prepared, ResourceId};
use crate::schedule::{NodeSchedule, Schedule};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

/// One evaluated schedule and the priority that produced it.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// Stable label, e.g. `baseline`, `critical-path`, `random-3`.
    pub label: String,
    /// Seed for a randomized candidate; `None` for the deterministic ones.
    pub seed: Option<u64>,
    /// The produced schedule.
    pub schedule: Schedule,
    /// Objective: makespan in cycles (`schedule.cycles`), repeated for
    /// convenience of comparison code.
    pub makespan: u64,
    /// Peak number of results live at once. Secondary tie-breaker only.
    pub live_pressure: u64,
    /// Whether `makespan <= Limits::max_cycle`.
    pub within_deadline: bool,
}

/// All candidates evaluated for a request, plus the best feasible one.
#[derive(Clone, Debug)]
pub struct SearchOutcome {
    /// Every candidate, in evaluation order.
    pub candidates: Vec<Candidate>,
    /// Index into `candidates` of the best feasible schedule.
    pub best: usize,
}

impl SearchOutcome {
    /// The best schedule by `(makespan, live_pressure, evaluation order)`.
    pub fn best_candidate(&self) -> &Candidate {
        &self.candidates[self.best]
    }
}

/// Priority used to order nodes that are ready to issue.
enum Priority<'a> {
    /// Greedy earliest-result list schedule (the deterministic baseline).
    Release,
    /// Longest path to a sink first (critical-path list schedule).
    CriticalPath,
    /// Seeded random total order.
    Random(&'a [u64]),
}

fn priority_key(priority: &Priority<'_>, critical: &[u64], node: NodeId, earliest: u64) -> u64 {
    match priority {
        Priority::Release => earliest,
        // Bitwise inversion reverses the order so the longest path wins a
        // min-heap.
        Priority::CriticalPath => !critical[node],
        Priority::Random(keys) => keys[node],
    }
}

/// Primary ordering key for insertion scheduling.
///
/// Unlike the append-only list scheduler, `CriticalPath` and `Random` order by
/// their key *first* and by readiness second. Placing a later-critical node
/// before an earlier-ready one can open a hole in a lane calendar that the
/// insertion walk then backfills, which a strictly readiness-first order can
/// never exploit. `Release` keeps the readiness-first order and therefore
/// degenerates to the baseline list schedule.
fn insertion_primary(priority: &Priority<'_>, critical: &[u64], node: NodeId) -> u64 {
    match priority {
        Priority::Release => 0,
        // Longest path first (inverted for the min-heap).
        Priority::CriticalPath => !critical[node],
        Priority::Random(keys) => keys[node],
    }
}

/// Schedule every node exactly once under fixed hardware.
///
/// The priority only changes the order in which already-ready nodes are
/// considered; the graph, resources and capacities are identical for every
/// candidate.
fn list_schedule(
    prepared: &Prepared<'_>,
    priority: Priority<'_>,
) -> Result<Schedule, ScheduleError> {
    let node_count = prepared.graph.nodes.len();
    let mut lane_free: BTreeMap<ResourceId, Vec<u64>> = prepared
        .lane_capacity
        .iter()
        .map(|(&resource, &capacity)| (resource, vec![0u64; capacity]))
        .collect();
    let mut deps_left: Vec<usize> = prepared
        .graph
        .nodes
        .iter()
        .map(|node| node.predecessors.len())
        .collect();
    let mut earliest: Vec<u64> = prepared
        .graph
        .nodes
        .iter()
        .map(|node| node.earliest)
        .collect();
    let mut placed: Vec<Option<NodeSchedule>> = vec![None; node_count];
    let mut heap: BinaryHeap<Reverse<(u64, u64, NodeId)>> = BinaryHeap::new();

    for node in 0..node_count {
        if deps_left[node] == 0 {
            let key = priority_key(&priority, &prepared.critical, node, earliest[node]);
            heap.push(Reverse((earliest[node], key, node)));
        }
    }

    let mut cycles = 0u64;
    while let Some(Reverse((_, _, node))) = heap.pop() {
        let spec = &prepared.graph.nodes[node];
        let assignment = match spec.resource {
            None => {
                let issue = earliest[node];
                NodeSchedule {
                    issue,
                    ready: issue,
                    lane: None,
                }
            }
            Some(resource) => {
                let slots = lane_free
                    .get_mut(&resource)
                    .ok_or(ScheduleError::InvalidResourceIndex { node, resource })?;
                if slots.is_empty() {
                    return Err(ScheduleError::ZeroLanes { resource });
                }
                let mut lane = 0usize;
                let mut free = u64::MAX;
                for (index, &candidate) in slots.iter().enumerate() {
                    if candidate < free {
                        free = candidate;
                        lane = index;
                    }
                }
                let issue = earliest[node].max(free);
                let ready = issue
                    .checked_add(prepared.latency[node])
                    .ok_or(ScheduleError::ArithmeticOverflow { node: Some(node) })?;
                let initiation = prepared.graph.resources[resource].initiation_interval;
                slots[lane] = issue
                    .checked_add(initiation)
                    .ok_or(ScheduleError::ArithmeticOverflow { node: Some(node) })?;
                NodeSchedule {
                    issue,
                    ready,
                    lane: Some(lane),
                }
            }
        };

        // Over-deadline assignments are recorded as-is; feasibility is decided
        // per candidate and the complete schedule stays independently
        // checkable.
        cycles = cycles.max(assignment.ready);
        for &child in &prepared.children[node] {
            earliest[child] = earliest[child].max(assignment.ready);
            deps_left[child] -= 1;
            if deps_left[child] == 0 {
                let key = priority_key(&priority, &prepared.critical, child, earliest[child]);
                heap.push(Reverse((earliest[child], key, child)));
            }
        }
        placed[node] = Some(assignment);
    }

    let nodes = placed
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or(ScheduleError::Cycle { nodes: Vec::new() })?;
    Ok(Schedule { nodes, cycles })
}

/// The earliest legal hole for a new issue on one lane.
///
/// `start` is the lower bound from gate and predecessors. `times` holds the
/// already-placed issue times on this lane in ascending order. The candidate
/// issue must keep at least `initiation` cycles from both neighbours, and the
/// returned value is the smallest such cycle at or after `start`.
fn earliest_hole(start: u64, times: &[u64], initiation: u64) -> Result<u64, ScheduleError> {
    let mut issue = start;
    // `times` is short (bounded by the node count) and the walk terminates
    // because each conflicting neighbour either precedes `issue` or pushes it
    // past itself.
    for &other in times {
        if other > issue && other - issue >= initiation {
            return Ok(issue);
        }
        if issue.abs_diff(other) < initiation {
            issue = other
                .checked_add(initiation)
                .ok_or(ScheduleError::ArithmeticOverflow { node: None })?;
        }
    }
    Ok(issue)
}

/// Schedule every node exactly once, inserting each ready node at the earliest
/// legal hole on any lane of its resource.
///
/// Wiring nodes still issue at their readiness time. Resource nodes choose the
/// lane (and issue cycle) that finishes first among all legal holes, which lets
/// them reuse gaps the greedy append-only list scheduler would skip. The
/// resulting schedule is checked by the same independent [`crate::check`] as
/// every other candidate.
fn insertion_schedule(
    prepared: &Prepared<'_>,
    priority: Priority<'_>,
) -> Result<Schedule, ScheduleError> {
    let node_count = prepared.graph.nodes.len();
    let mut lane_times: BTreeMap<ResourceId, Vec<Vec<u64>>> = prepared
        .lane_capacity
        .iter()
        .map(|(&resource, &capacity)| (resource, vec![Vec::new(); capacity]))
        .collect();
    let mut deps_left: Vec<usize> = prepared
        .graph
        .nodes
        .iter()
        .map(|node| node.predecessors.len())
        .collect();
    let mut earliest: Vec<u64> = prepared
        .graph
        .nodes
        .iter()
        .map(|node| node.earliest)
        .collect();
    let mut placed: Vec<Option<NodeSchedule>> = vec![None; node_count];
    // Primary priority key first, then readiness, then node id for determinism.
    let mut heap: BinaryHeap<Reverse<(u64, u64, NodeId)>> = BinaryHeap::new();

    for node in 0..node_count {
        if deps_left[node] == 0 {
            let key = insertion_primary(&priority, &prepared.critical, node);
            heap.push(Reverse((key, earliest[node], node)));
        }
    }

    let mut cycles = 0u64;
    while let Some(Reverse((_, start, node))) = heap.pop() {
        let spec = &prepared.graph.nodes[node];
        let assignment = match spec.resource {
            None => NodeSchedule {
                issue: start,
                ready: start,
                lane: None,
            },
            Some(resource) => {
                let slots = lane_times
                    .get_mut(&resource)
                    .ok_or(ScheduleError::InvalidResourceIndex { node, resource })?;
                if slots.is_empty() {
                    return Err(ScheduleError::ZeroLanes { resource });
                }
                let initiation = prepared.graph.resources[resource].initiation_interval;
                let mut best: Option<(u64, u64, usize)> = None;
                for (lane, times) in slots.iter().enumerate() {
                    let issue = earliest_hole(start, times, initiation)?;
                    let ready = issue
                        .checked_add(prepared.latency[node])
                        .ok_or(ScheduleError::ArithmeticOverflow { node: Some(node) })?;
                    // Prefer the earliest finish, then the earliest issue, then
                    // the lowest lane id for determinism.
                    let better = match best {
                        None => true,
                        Some((best_ready, best_issue, best_lane)) => {
                            (ready, issue, lane) < (best_ready, best_issue, best_lane)
                        }
                    };
                    if better {
                        best = Some((ready, issue, lane));
                    }
                }
                let (ready, issue, lane) = best.ok_or(ScheduleError::ZeroLanes { resource })?;
                slots[lane].push(issue);
                slots[lane].sort_unstable();
                NodeSchedule {
                    issue,
                    ready,
                    lane: Some(lane),
                }
            }
        };

        cycles = cycles.max(assignment.ready);
        for &child in &prepared.children[node] {
            earliest[child] = earliest[child].max(assignment.ready);
            deps_left[child] -= 1;
            if deps_left[child] == 0 {
                let key = insertion_primary(&priority, &prepared.critical, child);
                heap.push(Reverse((key, earliest[child], child)));
            }
        }
        placed[node] = Some(assignment);
    }

    let nodes = placed
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or(ScheduleError::Cycle { nodes: Vec::new() })?;
    Ok(Schedule { nodes, cycles })
}

/// Lower bound on the makespan from resource capacity alone.
///
/// For every used resource the total issue demand is `node_count` slots across
/// `lanes` physical lanes, and each lane may start a new operation only every
/// `initiation_interval` cycles. A final result also needs its resource latency
/// after the last issue, so the bound is
/// `max_r ((ceil(count_r / lanes_r) - 1) * II_r + latency_r + release_floor)`.
/// It is a necessary bound only: dependencies and release gates can push the
/// real makespan higher. It is reported for honest comparison, never used to
/// prune the search.
fn resource_lower_bound(prepared: &Prepared<'_>) -> u64 {
    let mut bound = 0u64;
    let mut per_resource: BTreeMap<ResourceId, (u64, u64, u64)> = BTreeMap::new();
    for spec in &prepared.graph.nodes {
        if let Some(resource) = spec.resource {
            let entry = per_resource.entry(resource).or_insert((0, 0, u64::MAX));
            entry.0 += 1;
            entry.1 = prepared.graph.resources[resource].latency;
            entry.2 = entry.2.min(spec.earliest);
        }
    }
    for (&resource, &(count, latency, release_floor)) in &per_resource {
        let lanes = prepared
            .lane_capacity
            .get(&resource)
            .copied()
            .unwrap_or(1)
            .max(1) as u64;
        let initiation = prepared.graph.resources[resource].initiation_interval;
        let issues = count.div_ceil(lanes);
        let span = issues
            .saturating_sub(1)
            .saturating_mul(initiation)
            .saturating_add(latency);
        // The first issue cannot precede the resource's earliest release gate.
        bound = bound.max(release_floor.saturating_add(span));
    }
    bound
}

/// Peak number of results live at the same time.
///
/// A result is live from its `ready` cycle until the last issue of any
/// consumer; a node with no consumer holds its result until the makespan. This
/// is a cheap secondary objective, not a storage model.
fn live_pressure(prepared: &Prepared<'_>, schedule: &Schedule) -> u64 {
    let mut events: Vec<(u64, i64)> = Vec::new();
    for node in 0..prepared.graph.nodes.len() {
        let ready = schedule.nodes[node].ready;
        let mut end = if prepared.children[node].is_empty() {
            schedule.cycles
        } else {
            ready
        };
        for &child in &prepared.children[node] {
            end = end.max(schedule.nodes[child].issue);
        }
        let end = end.max(ready);
        if end > ready {
            events.push((ready, 1));
            events.push((end, -1));
        }
    }
    events.sort_unstable();
    let mut live = 0i64;
    let mut peak = 0i64;
    for (_, delta) in events {
        live += delta;
        peak = peak.max(live);
    }
    peak.max(0) as u64
}

/// SplitMix64: small, dependency-free, deterministic.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn restart_seed(base: u64, index: usize) -> u64 {
    let mut rng = Rng::new(base ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    rng.next()
}

fn candidate_from(
    label: String,
    seed: Option<u64>,
    schedule: Schedule,
    limits: &Limits,
    prepared: &Prepared<'_>,
) -> Candidate {
    Candidate {
        label,
        seed,
        makespan: schedule.cycles,
        live_pressure: live_pressure(prepared, &schedule),
        within_deadline: schedule.cycles <= limits.max_cycle,
        schedule,
    }
}

/// Validate, then evaluate bounded candidates and return the best one.
///
/// The candidate set always starts with the deterministic `baseline`. With
/// `max_candidates >= 2` the deterministic `critical-path` schedule follows.
/// From six candidates onward the backfilling `insertion` and
/// `insertion-critical-path` schedules are added, and every remaining slot
/// becomes a seeded random restart (list schedule up to five candidates, then
/// insertion schedules so the seeded alternatives can also backfill). The best
/// feasible candidate is chosen by `(makespan, live_pressure, evaluation
/// order)`.
///
/// Every candidate is a complete, independently checkable schedule; the search
/// never mutates the graph, never adds hardware, and performs no unbounded
/// exact search.
///
/// Returns [`ScheduleError::NoFeasibleSchedule`] if every evaluated candidate
/// exceeds `Limits::max_cycle`, and a structural error before evaluating
/// anything if the graph itself is invalid.
pub fn plan(
    graph: &Graph,
    limits: &Limits,
    config: &SearchConfig,
) -> Result<SearchOutcome, ScheduleError> {
    let prepared = graph.prepare(limits)?;

    let baseline = list_schedule(&prepared, Priority::Release)?;
    let mut candidates = vec![candidate_from(
        "baseline".to_owned(),
        None,
        baseline,
        limits,
        &prepared,
    )];

    if limits.max_candidates >= 2 {
        let critical = list_schedule(&prepared, Priority::CriticalPath)?;
        candidates.push(candidate_from(
            "critical-path".to_owned(),
            None,
            critical,
            limits,
            &prepared,
        ));
    }

    // Insertion candidates are deterministic and free of any seed. They are
    // skipped for the smallest budgets so the historical candidate sequence
    // (`baseline`, `critical-path`, seeded restarts) is unchanged there.
    if limits.max_candidates >= 6 {
        let insertion = insertion_schedule(&prepared, Priority::Release)?;
        candidates.push(candidate_from(
            "insertion".to_owned(),
            None,
            insertion,
            limits,
            &prepared,
        ));
        let insertion_critical = insertion_schedule(&prepared, Priority::CriticalPath)?;
        candidates.push(candidate_from(
            "insertion-critical-path".to_owned(),
            None,
            insertion_critical,
            limits,
            &prepared,
        ));
    }

    while candidates.len() < limits.max_candidates {
        let index = candidates.len();
        let seed = restart_seed(config.base_seed, index);
        let mut rng = Rng::new(seed);
        let keys: Vec<u64> = (0..graph.nodes.len()).map(|_| rng.next()).collect();
        // Small budgets keep the original seeded list schedules; larger budgets
        // use seeded insertion schedules so restarts can backfill as well.
        let schedule = if limits.max_candidates < 6 {
            list_schedule(&prepared, Priority::Random(&keys))?
        } else {
            insertion_schedule(&prepared, Priority::Random(&keys))?
        };
        candidates.push(candidate_from(
            format!("random-{index}"),
            Some(seed),
            schedule,
            limits,
            &prepared,
        ));
    }

    // Deadline failures remain observable, but every assignment must be legal.
    let audit_limits = Limits::new(limits.max_nodes, u64::MAX, limits.max_candidates);
    for candidate in &candidates {
        let report = crate::check(graph, &audit_limits, &candidate.schedule);
        if !report.is_ok() {
            return Err(ScheduleError::InvalidCandidate(format!(
                "{}: {:?}",
                candidate.label, report.violations
            )));
        }
    }
    let best = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.within_deadline)
        .min_by_key(|(index, candidate)| (candidate.makespan, candidate.live_pressure, *index))
        .map(|(index, _)| index)
        .ok_or_else(|| ScheduleError::NoFeasibleSchedule {
            best_cycle: candidates
                .iter()
                .map(|candidate| candidate.makespan)
                .min()
                .unwrap_or(0),
        })?;

    Ok(SearchOutcome { candidates, best })
}

/// Necessary makespan lower bound from resource capacity and initiation
/// intervals.
///
/// The bound is recomputed from the graph alone and the graph is validated
/// first, so it is safe to compare against any candidate. See
/// [`resource_lower_bound`] for the derivation; it ignores dependencies and is
/// therefore a lower bound, not a guarantee.
pub fn lower_bound(graph: &Graph, limits: &Limits) -> Result<u64, ScheduleError> {
    let prepared = graph.prepare(limits)?;
    Ok(resource_lower_bound(&prepared))
}
