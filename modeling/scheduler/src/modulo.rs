//! Reusable modulo scheduling and an independent periodic checker.
//!
//! A modulo schedule statically reserves one position for a finite, acyclic
//! task graph that is then repeated every `II` cycles (the initiation
//! interval). Iteration `k` issues node `v` at `issue_v + k * II` on the same
//! lane. Because the graph carries no loop-carried dependency, whether a
//! resource legality is a property of circular issue spacing; dependencies
//! and release gates still use absolute body times:
//!
//! * circular spacing on every lane meets its resource initiation interval,
//!   including the next iteration of a solitary operation;
//! * a predecessor's result must be ready before its consumer issues in the
//!   same iteration: `issue_p + latency_p <= issue_v`.
//!
//! This module is domain independent: it consumes the same [`Graph`] as the
//! finite scheduler and adds a periodic legality checker that recomputes every
//! fact from the graph and the assignment alone. The search is bounded list
//! scheduling with a fixed number of deterministic/reproducible restarts; it
//! performs no exact or unbounded search and reports a necessary resource lower
//! bound so callers can see whether an II is even admissible.

use crate::error::ScheduleError;
use crate::limits::{Limits, SearchConfig};
use crate::model::{Graph, NodeId, ResourceId};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Representation bounds for the modulo search, mirroring the finite limits so
/// that u64 arithmetic and lane calendars stay small and checked.
const MAX_MODULO_NODES: usize = 100_000;
const MAX_MODULO_II: u64 = 65_536;
const MAX_MODULO_LANES: usize = 4_096;
/// Largest `lanes * II` reservation-calendar a single candidate may allocate.
/// This bounds host memory independently of the graph size.
const MAX_MODULO_CELLS: u64 = 8_388_608;
/// Bounded seeded restarts for one modulo request.
const MODULO_CANDIDATE_BUDGET: usize = 64;

/// Why a modulo graph or modulo request was rejected before scheduling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuloError {
    /// The underlying graph failed structural validation.
    Graph(ScheduleError),
    /// The graph exceeds [`MAX_MODULO_NODES`].
    TooManyNodes { count: usize, max: usize },
    /// A resource uses more lanes than [`MAX_MODULO_LANES`].
    TooManyLanes {
        resource: ResourceId,
        lanes: usize,
        max: usize,
    },
    /// The requested initiation interval is zero or exceeds [`MAX_MODULO_II`].
    InitiationInterval { ii: u64, max: u64 },
    /// A reservation calendar would allocate more than [`MAX_MODULO_CELLS`]
    /// lane-phase cells at this II.
    RepresentationTooLarge {
        resource: ResourceId,
        lanes: usize,
        ii: u64,
        max_cells: u64,
    },
    /// A checked arithmetic step overflowed `u64`.
    ArithmeticOverflow,
    /// No bounded candidate produced a legal calendar at the requested II.
    Infeasible {
        initiation_interval: u64,
        lower_bound: u64,
        /// Minimum observed span, or the necessary span bound if no complete
        /// construction exists. This is not a valid-plan certificate.
        best_span: u64,
    },
}

impl fmt::Display for ModuloError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuloError::Graph(error) => write!(f, "invalid graph: {error}"),
            ModuloError::TooManyNodes { count, max } => {
                write!(f, "graph has {count} nodes, modulo limit is {max}")
            }
            ModuloError::TooManyLanes {
                resource,
                lanes,
                max,
            } => write!(f, "resource {resource} has {lanes} lanes, limit is {max}"),
            ModuloError::InitiationInterval { ii, max } => {
                write!(f, "initiation interval {ii} is outside 1..={max}")
            }
            ModuloError::RepresentationTooLarge {
                resource,
                lanes,
                ii,
                max_cells,
            } => write!(
                f,
                "adding resource {resource} ({lanes} lanes x {ii} phases) exceeds {max_cells} total calendar cells"
            ),
            ModuloError::ArithmeticOverflow => {
                write!(f, "arithmetic overflow while scheduling modulo")
            }
            ModuloError::Infeasible {
                initiation_interval,
                lower_bound,
                best_span,
            } => write!(
                f,
                "bounded search found no legal modulo calendar at II={initiation_interval}; resource lower bound {lower_bound}, span diagnostic {best_span}"
            ),
        }
    }
}

impl std::error::Error for ModuloError {}

impl From<ScheduleError> for ModuloError {
    fn from(value: ScheduleError) -> Self {
        ModuloError::Graph(value)
    }
}

/// One recurring reservation: iteration `k` issues at `issue + k * ii`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuloNode {
    /// Absolute body issue cycle in `0..=span`; it may exceed II.
    pub issue: u64,
    /// Lane used, or `None` for a wiring node.
    pub lane: Option<usize>,
}

/// A modulo reservation for a graph repeated every `initiation_interval`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuloSchedule {
    /// Repetition distance between successive iterations; always non-zero.
    pub initiation_interval: u64,
    /// One reservation per graph node, in node order.
    pub nodes: Vec<ModuloNode>,
    /// Largest `issue + latency` inside one iteration (zero for an empty graph).
    pub span: u64,
}

/// Failure of an independent periodic check, computed from the graph and the
/// assignment alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuloViolation {
    /// The graph or the requested II is invalid.
    InvalidRequest(String),
    /// `nodes` does not have one entry per graph node.
    NodeCount { expected: usize, actual: usize },
    /// `initiation_interval` is zero.
    ZeroInitiationInterval,
    /// Body time may exceed II, but cannot precede its release gate.
    ReleaseGate {
        node: NodeId,
        issue: u64,
        earliest: u64,
    },
    /// A resource-backed node has no lane.
    MissingLane { node: NodeId },
    /// A wiring node carries a lane.
    UnexpectedLane { node: NodeId, lane: usize },
    /// A lane is outside the resource's lane count.
    LaneOutOfRange {
        node: NodeId,
        lane: usize,
        lanes: usize,
    },
    /// Two issues of one resource alias the same `(lane, issue mod II)` slot,
    /// which would overbook that physical lane across repeated iterations.
    InitiationCollision {
        resource: ResourceId,
        lane: usize,
        first: u64,
        second: u64,
        initiation: u64,
    },
    /// A predecessor result is not ready before the consumer issues.
    Dependency {
        node: NodeId,
        predecessor: NodeId,
        issue: u64,
        predecessor_ready: u64,
    },
    /// A wiring node has a non-zero latency or a lane.
    WiringTiming { node: NodeId },
    /// `issue + latency` overflowed `u64`.
    LatencyOverflow { node: NodeId },
    /// `span` disagrees with the recomputed maximum ready cycle.
    SpanMismatch { reported: u64, computed: u64 },
}

impl fmt::Display for ModuloViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuloViolation::InvalidRequest(message) => write!(f, "invalid request: {message}"),
            ModuloViolation::NodeCount { expected, actual } => {
                write!(f, "modulo schedule has {actual} rows, expected {expected}")
            }
            ModuloViolation::ZeroInitiationInterval => {
                write!(f, "initiation interval must be non-zero")
            }
            ModuloViolation::ReleaseGate { node, issue, earliest } => {
                write!(f, "node {node} issues at {issue}, before release {earliest}")
            }
            ModuloViolation::MissingLane { node } => write!(f, "node {node} has no lane"),
            ModuloViolation::UnexpectedLane { node, lane } => {
                write!(f, "wiring node {node} carries lane {lane}")
            }
            ModuloViolation::LaneOutOfRange { node, lane, lanes } => {
                write!(f, "node {node} uses lane {lane} of {lanes}")
            }
            ModuloViolation::InitiationCollision {
                resource,
                lane,
                first,
                second,
                initiation,
            } => write!(
                f,
                "resource {resource} lane {lane} aliases issues {first} and {second} at initiation {initiation}"
            ),
            ModuloViolation::Dependency {
                node,
                predecessor,
                issue,
                predecessor_ready,
            } => write!(
                f,
                "node {node} issues at {issue} before predecessor {predecessor} is ready at {predecessor_ready}"
            ),
            ModuloViolation::WiringTiming { node } => {
                write!(f, "wiring node {node} has non-zero latency or a lane")
            }
            ModuloViolation::LatencyOverflow { node } => {
                write!(f, "node {node} issue + latency overflowed")
            }
            ModuloViolation::SpanMismatch { reported, computed } => write!(
                f,
                "modulo schedule reports span {reported}, recomputed {computed}"
            ),
        }
    }
}

/// All periodic violations found for one modulo schedule; empty means legal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuloCheckReport {
    /// Rule breaks found, in node order followed by lane order.
    pub violations: Vec<ModuloViolation>,
}

impl ModuloCheckReport {
    /// Whether the calendar passed every periodic rule.
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Validated graph plus precomputed facts shared by modulo candidates.
///
/// `predecessors` includes every raw predecessor; `latency` is the resource
/// latency (zero for wiring nodes); `critical` is the longest latency path from
/// a node to any sink, used as a priority and as an honest span bound.
#[derive(Debug)]
pub struct ModuloGraph {
    node_count: usize,
    predecessors: Vec<Vec<NodeId>>,
    children: Vec<Vec<NodeId>>,
    latency: Vec<u64>,
    earliest: Vec<u64>,
    critical: Vec<u64>,
    resources: Vec<ModuloResource>,
    node_resource: Vec<Option<ResourceId>>,
    /// Longest latency chain in the whole graph, a necessary lower bound on
    /// `span` for any II.
    critical_path: u64,
}

#[derive(Clone, Debug)]
struct ModuloResource {
    lanes: usize,
    initiation_interval: u64,
    /// Number of graph nodes that use this resource.
    count: usize,
}

impl ModuloGraph {
    pub(crate) fn children(&self, id: NodeId) -> &[NodeId] {
        &self.children[id]
    }
    pub(crate) fn latency(&self, id: NodeId) -> u64 {
        self.latency[id]
    }
    pub(crate) fn topological_order(&self) -> Result<Vec<NodeId>, ScheduleError> {
        topological_order(
            &self.children,
            &self.predecessors.iter().map(Vec::len).collect::<Vec<_>>(),
        )
    }
    /// Validate `graph` and precompute its modulo facts.
    ///
    /// Cycles, invalid indices, duplicate dependencies, zero lane counts, zero
    /// latencies and zero initiation intervals are rejected exactly as in the
    /// finite scheduler. Representation bounds keep lane calendars small. No
    /// scheduling limits are required: modulo cycles are residues, not a
    /// deadline.
    pub fn from_graph(graph: &Graph) -> Result<Self, ModuloError> {
        let node_count = graph.nodes.len();
        if node_count > MAX_MODULO_NODES {
            return Err(ModuloError::TooManyNodes {
                count: node_count,
                max: MAX_MODULO_NODES,
            });
        }
        for (resource, unit) in graph.resources.iter().enumerate() {
            if unit.lanes == 0 {
                return Err(ScheduleError::ZeroLanes { resource }.into());
            }
            if unit.latency == 0 {
                return Err(ScheduleError::ZeroLatency { resource }.into());
            }
            if unit.initiation_interval == 0 {
                return Err(ScheduleError::ZeroInitiation { resource }.into());
            }
            if unit.lanes > MAX_MODULO_LANES {
                return Err(ModuloError::TooManyLanes {
                    resource,
                    lanes: unit.lanes,
                    max: MAX_MODULO_LANES,
                });
            }
        }

        let mut predecessors = vec![Vec::new(); node_count];
        let mut children = vec![Vec::new(); node_count];
        let mut indegree = vec![0usize; node_count];
        let mut latency = vec![0u64; node_count];
        let mut node_resource = vec![None; node_count];
        let mut counts = vec![0usize; graph.resources.len()];
        for (node, spec) in graph.nodes.iter().enumerate() {
            latency[node] = match spec.resource {
                None => 0,
                Some(resource) => {
                    let unit = graph
                        .resources
                        .get(resource)
                        .ok_or(ScheduleError::InvalidResourceIndex { node, resource })?;
                    counts[resource] += 1;
                    node_resource[node] = Some(resource);
                    unit.latency
                }
            };
            let mut seen = BTreeSet::new();
            for &pred in &spec.predecessors {
                if pred >= node_count {
                    return Err(ScheduleError::InvalidNodeIndex {
                        node,
                        predecessor: pred,
                    }
                    .into());
                }
                if !seen.insert(pred) {
                    return Err(ScheduleError::DuplicatePredecessor {
                        node,
                        predecessor: pred,
                    }
                    .into());
                }
                predecessors[node].push(pred);
                children[pred].push(node);
                indegree[node] += 1;
            }
        }

        let topo = topological_order(&children, &indegree)?;
        let mut critical = vec![0u64; node_count];
        for &node in topo.iter().rev() {
            let mut deepest = 0u64;
            for &child in &children[node] {
                deepest = deepest.max(critical[child]);
            }
            critical[node] = latency[node]
                .checked_add(deepest)
                .ok_or(ModuloError::ArithmeticOverflow)?;
        }
        let critical_path =
            graph
                .nodes
                .iter()
                .zip(&critical)
                .try_fold(0_u64, |span, (n, &tail)| {
                    n.earliest
                        .checked_add(tail)
                        .map(|ready| span.max(ready))
                        .ok_or(ModuloError::ArithmeticOverflow)
                })?;

        let resources = graph
            .resources
            .iter()
            .enumerate()
            .map(|(resource, unit)| ModuloResource {
                lanes: unit.lanes,
                initiation_interval: unit.initiation_interval,
                count: counts[resource],
            })
            .collect();

        Ok(ModuloGraph {
            node_count,
            predecessors,
            children,
            latency,
            earliest: graph.nodes.iter().map(|n| n.earliest).collect(),
            critical,
            resources,
            node_resource,
            critical_path,
        })
    }

    /// Number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Longest latency path from any source to any sink.
    pub fn critical_path(&self) -> u64 {
        self.critical_path
    }

    /// Necessary lower bound on the initiation interval.
    ///
    /// Each node of a resource occupies one `(lane, phase)` slot per period, so
    /// a resource with `count` nodes and `lanes` physical lanes needs
    /// `ceil(count / lanes)` phases. This is the classic resource-constrained
    /// modulo lower bound; it is necessary but not sufficient, because
    /// dependency latency can make an otherwise admissible II infeasible.
    pub fn resource_lower_bound(&self) -> u64 {
        let mut bound = 1u64;
        for unit in &self.resources {
            let issues = (unit.count as u64).div_ceil(unit.lanes as u64);
            if unit.count > 0 {
                bound = bound.max(issues.saturating_mul(unit.initiation_interval));
            }
        }
        bound
    }

    /// Whole-graph critical path, a necessary lower bound on the modulo span.
    pub fn span_lower_bound(&self) -> u64 {
        self.critical_path
    }
}

/// Kahn's algorithm with deterministic ordering.
fn topological_order(
    children: &[Vec<NodeId>],
    indegree: &[usize],
) -> Result<Vec<NodeId>, ScheduleError> {
    let mut remaining = indegree.to_vec();
    let mut queue: Vec<NodeId> = (0..children.len()).filter(|&n| remaining[n] == 0).collect();
    let mut order = Vec::with_capacity(children.len());
    let mut head = 0;
    while head < queue.len() {
        let node = queue[head];
        head += 1;
        order.push(node);
        for &child in &children[node] {
            remaining[child] -= 1;
            if remaining[child] == 0 {
                queue.push(child);
            }
        }
    }
    if order.len() != children.len() {
        let mut ordered = vec![false; children.len()];
        for &node in &order {
            ordered[node] = true;
        }
        let nodes = (0..children.len()).filter(|&n| !ordered[n]).collect();
        return Err(ScheduleError::Cycle { nodes });
    }
    Ok(order)
}

/// SplitMix64, matching the finite-search generator for reproducibility.
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

/// A per-lane reservation calendar over one initiation interval.
///
/// `busy[lane][phase]` marks phases the lane is occupied for. An issue at body
/// cycle `t` on a pipelined unit with initiation interval `d` (its reservation
/// distance) occupies phases `t, t+1, ..., t+min(d, ii)-1` modulo `ii`. A new
/// issue is legal on a lane only if none of those phases is already busy, which
/// is exactly the condition that every pair of repeated iterations is spaced by
/// at least `d` cycles and that the schedule period is `ii`.
struct Calendar {
    ii: u64,
    distance: u64,
    busy: Vec<Vec<bool>>,
}

impl Calendar {
    fn new(lanes: usize, ii: u64, distance: u64) -> Self {
        Calendar {
            ii,
            distance,
            busy: vec![vec![false; ii as usize]; lanes],
        }
    }

    /// Reserve the earliest issue at or after `start` on the first free lane.
    ///
    /// Returns `Some((issue, lane))`, or `None` when the resource is saturated
    /// for this II and distance. The scan advances at most `ii` cycles, so it is
    /// bounded.
    fn reserve(&mut self, start: u64) -> Result<Option<(u64, usize)>, ModuloError> {
        if self.distance > self.ii {
            return Ok(None);
        }
        let occupied = self.distance as usize;
        let mut issue = start;
        for _ in 0..self.ii {
            for lane in 0..self.busy.len() {
                let free = (0..occupied).all(|offset| {
                    let phase = ((issue % self.ii + offset as u64) % self.ii) as usize;
                    !self.busy[lane][phase]
                });
                if free {
                    for offset in 0..occupied {
                        let phase = ((issue % self.ii + offset as u64) % self.ii) as usize;
                        self.busy[lane][phase] = true;
                    }
                    return Ok(Some((issue, lane)));
                }
            }
            issue = issue
                .checked_add(1)
                .ok_or(ModuloError::ArithmeticOverflow)?;
        }
        Ok(None)
    }
}

enum ModuloPriority<'a> {
    Ready,
    CriticalPath,
    Random(&'a [u64]),
}

fn modulo_key(priority: &ModuloPriority<'_>, critical: &[u64], node: NodeId) -> u64 {
    match priority {
        // Primary key is readiness, so this only breaks ties deterministically.
        ModuloPriority::Ready => node as u64,
        ModuloPriority::CriticalPath => !critical[node],
        ModuloPriority::Random(keys) => keys[node],
    }
}

/// Build one modulo candidate at fixed `ii` and priority.
///
/// Returns `Ok(None)` if this ordering exhausts a lane's circular calendar or
/// exceeds the caller's span bound. Absolute body issue times may exceed II;
/// only reservations use phases. A lane with unit II greater than body II
/// cannot even reserve one recurring operation.
fn modulo_list_schedule(
    graph: &ModuloGraph,
    ii: u64,
    priority: ModuloPriority<'_>,
    max_span: u64,
) -> Result<Option<ModuloSchedule>, ModuloError> {
    let n = graph.node_count;
    let mut calendars: BTreeMap<ResourceId, Calendar> = BTreeMap::new();
    for (resource, unit) in graph.resources.iter().enumerate() {
        if unit.count > 0 {
            calendars.insert(
                resource,
                Calendar::new(unit.lanes, ii, unit.initiation_interval),
            );
        }
    }
    let mut deps_left: Vec<usize> = graph.predecessors.iter().map(|p| p.len()).collect();
    // Body readiness, i.e. the earliest issue cycle allowed by predecessors.
    let mut ready: Vec<u64> = graph.earliest.clone();
    let mut reserved: Vec<Option<ModuloNode>> = vec![None; n];
    let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(u64, u64, NodeId)>> =
        std::collections::BinaryHeap::new();
    for (node, &remaining) in deps_left.iter().enumerate() {
        if remaining == 0 {
            heap.push(std::cmp::Reverse((
                if matches!(priority, ModuloPriority::Ready) {
                    graph.earliest[node]
                } else {
                    modulo_key(&priority, &graph.critical, node)
                },
                graph.earliest[node],
                node,
            )));
        }
    }

    let mut span = 0u64;
    while let Some(std::cmp::Reverse((_, start, node))) = heap.pop() {
        // Keep absolute readiness while choosing a free recurring phase.
        let reservation = match graph.node_resource[node] {
            None => ModuloNode {
                issue: start,
                lane: None,
            },
            Some(resource) => {
                let calendar = calendars
                    .get_mut(&resource)
                    .ok_or(ModuloError::ArithmeticOverflow)?;
                let Some((issue, lane)) = calendar.reserve(start)? else {
                    return Ok(None);
                };
                ModuloNode {
                    issue,
                    lane: Some(lane),
                }
            }
        };
        let ready_cycle = reservation
            .issue
            .checked_add(graph.latency[node])
            .ok_or(ModuloError::ArithmeticOverflow)?;
        if ready_cycle > max_span {
            return Ok(None);
        }
        span = span.max(ready_cycle);
        for &child in &graph.children[node] {
            ready[child] = ready[child].max(ready_cycle);
            deps_left[child] -= 1;
            if deps_left[child] == 0 {
                heap.push(std::cmp::Reverse((
                    if matches!(priority, ModuloPriority::Ready) {
                        ready[child]
                    } else {
                        modulo_key(&priority, &graph.critical, child)
                    },
                    ready[child],
                    child,
                )));
            }
        }
        reserved[node] = Some(reservation);
    }

    let Some(nodes) = reserved.into_iter().collect::<Option<Vec<_>>>() else {
        return Ok(None);
    };
    Ok(Some(ModuloSchedule {
        initiation_interval: ii,
        nodes,
        span,
    }))
}

/// Return the best legal calendar found for `ii`, or [`ModuloError::Infeasible`].
///
/// Deterministic list scheduling runs first, then the critical-path priority,
/// then bounded seeded restarts derived from `config.base_seed` (at most
/// [`MODULO_CANDIDATE_BUDGET`] in total). Every candidate is validated by the
/// independent [`check_modulo`] before it can be returned; a candidate that the
/// checker rejects is never treated as a result. This is a bounded
/// construction, not exact optimization.
pub fn modulo_schedule(
    graph: &ModuloGraph,
    ii: u64,
    config: &SearchConfig,
) -> Result<ModuloSchedule, ModuloError> {
    modulo_schedule_bounded(
        graph,
        ii,
        &Limits::new(MAX_MODULO_NODES, u64::MAX, MODULO_CANDIDATE_BUDGET),
        config,
    )
}

pub fn modulo_schedule_bounded(
    graph: &ModuloGraph,
    ii: u64,
    limits: &Limits,
    config: &SearchConfig,
) -> Result<ModuloSchedule, ModuloError> {
    if limits.max_candidates == 0
        || limits.max_candidates > MODULO_CANDIDATE_BUDGET
        || limits.max_cycle == 0
        || limits.max_nodes == 0
    {
        return Err(ScheduleError::ZeroLimit("modulo request bounds").into());
    }
    if graph.node_count > limits.max_nodes {
        return Err(ScheduleError::TooManyNodes {
            count: graph.node_count,
            max: limits.max_nodes,
        }
        .into());
    }
    if ii == 0 || ii > MAX_MODULO_II {
        return Err(ModuloError::InitiationInterval {
            ii,
            max: MAX_MODULO_II,
        });
    }
    let mut total_cells = 0;
    for (resource, unit) in graph.resources.iter().enumerate() {
        if unit.count == 0 {
            continue;
        }
        // A physical lane must keep at least one occupied phase; with II>1 a
        // spanning unit needs up to `ii` phases per lane. Bound the calendar
        // before allocating it.
        total_cells += unit.lanes as u64 * ii;
        if total_cells > MAX_MODULO_CELLS {
            return Err(ModuloError::RepresentationTooLarge {
                resource,
                lanes: unit.lanes,
                ii,
                max_cells: MAX_MODULO_CELLS,
            });
        }
    }
    let lower_bound = graph.resource_lower_bound();
    if lower_bound > ii {
        return Err(ModuloError::Infeasible {
            initiation_interval: ii,
            lower_bound,
            best_span: graph.span_lower_bound(),
        });
    }
    let mut best: Option<ModuloSchedule> = None;
    // `Option` avoids colliding a legitimate `span == u64::MAX` with "unset".
    let mut best_span: Option<u64> = None;
    // Smallest span any construction produced, even one the checker rejected,
    // so an infeasibility report is honest about how close the search came.
    let mut observed_span: Option<u64> = None;

    let priorities = limits.max_candidates;
    let mut first_error = None;
    for candidate in 0..priorities {
        let seed = config.base_seed ^ (candidate as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut rng = Rng::new(seed);
        let keys: Vec<u64> = (0..graph.node_count).map(|_| rng.next()).collect();
        let priority = match candidate {
            0 => ModuloPriority::Ready,
            1 => ModuloPriority::CriticalPath,
            _ => ModuloPriority::Random(&keys),
        };
        let schedule = match modulo_list_schedule(graph, ii, priority, limits.max_cycle) {
            Ok(Some(s)) => s,
            Ok(None) => continue,
            Err(error) => {
                first_error.get_or_insert(error);
                continue;
            }
        };
        observed_span = Some(observed_span.map_or(schedule.span, |span| span.min(schedule.span)));
        if !check_modulo(graph, &schedule).is_ok() {
            continue;
        }
        if best_span.is_none_or(|span| schedule.span < span) {
            best_span = Some(schedule.span);
            best = Some(schedule);
        }
    }

    if best.is_none() {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    best.ok_or(ModuloError::Infeasible {
        initiation_interval: ii,
        lower_bound,
        // No positive span was produced; report the necessary lower bound
        // rather than a sentinel.
        best_span: observed_span.unwrap_or_else(|| graph.span_lower_bound()),
    })
}

/// Independently validate a modulo calendar.
///
/// Every fact is recomputed from `graph` and `schedule` alone; no search state
/// is trusted. The checker verifies that no physical lane is overbooked across
/// repeated iterations (distinct `(lane, issue mod II)` residues), that every
/// predecessor result is ready before its consumer issues under the real
/// dependency latency, and that `span` equals the largest `issue + latency`.
pub fn check_modulo(graph: &ModuloGraph, schedule: &ModuloSchedule) -> ModuloCheckReport {
    let mut violations = Vec::new();
    let ii = schedule.initiation_interval;
    if ii == 0 || ii > MAX_MODULO_II {
        return ModuloCheckReport {
            violations: vec![ModuloViolation::InvalidRequest(format!(
                "initiation interval {ii} outside 1..={MAX_MODULO_II}"
            ))],
        };
    }
    if schedule.nodes.len() != graph.node_count {
        violations.push(ModuloViolation::NodeCount {
            expected: graph.node_count,
            actual: schedule.nodes.len(),
        });
        return ModuloCheckReport { violations };
    }

    let mut lane_issues: BTreeMap<(ResourceId, usize), Vec<u64>> = BTreeMap::new();
    let mut computed_span = 0u64;

    for node in 0..graph.node_count {
        let assignment = schedule.nodes[node];
        if assignment.issue < graph.earliest[node] {
            violations.push(ModuloViolation::ReleaseGate {
                node,
                issue: assignment.issue,
                earliest: graph.earliest[node],
            });
        }
        match graph.node_resource[node] {
            None => {
                if let Some(lane) = assignment.lane {
                    violations.push(ModuloViolation::UnexpectedLane { node, lane });
                }
                if graph.latency[node] != 0 {
                    violations.push(ModuloViolation::WiringTiming { node });
                }
            }
            Some(resource) => {
                let unit = &graph.resources[resource];
                let Some(lane) = assignment.lane else {
                    violations.push(ModuloViolation::MissingLane { node });
                    continue;
                };
                if lane >= unit.lanes {
                    violations.push(ModuloViolation::LaneOutOfRange {
                        node,
                        lane,
                        lanes: unit.lanes,
                    });
                }
                lane_issues
                    .entry((resource, lane))
                    .or_default()
                    .push(assignment.issue);
            }
        }

        for &pred in &graph.predecessors[node] {
            let predecessor_ready =
                match schedule.nodes[pred].issue.checked_add(graph.latency[pred]) {
                    None => {
                        violations.push(ModuloViolation::LatencyOverflow { node: pred });
                        continue;
                    }
                    Some(ready) => ready,
                };
            if predecessor_ready > assignment.issue {
                violations.push(ModuloViolation::Dependency {
                    node,
                    predecessor: pred,
                    issue: assignment.issue,
                    predecessor_ready,
                });
            }
        }

        match assignment.issue.checked_add(graph.latency[node]) {
            None => violations.push(ModuloViolation::LatencyOverflow { node }),
            Some(ready) => computed_span = computed_span.max(ready),
        }
    }

    // Independent cross-iteration check: no physical lane is overbooked.
    //
    // Two issues `s1 < s2` on one lane repeat forever, so they collide unless
    // the circular gap is at least the unit's reservation distance in *both*
    // directions (the wrap-around gap is the spacing between the later issue of
    // one iteration and the earlier issue of the next).
    for ((resource, lane), mut issues) in lane_issues {
        let distance = graph.resources[resource].initiation_interval;
        let occupied = distance;
        for issue in &mut issues {
            *issue %= ii;
        }
        issues.sort_unstable();
        let collision = if occupied > ii {
            Some((issues[0], issues[0]))
        } else if let Some(pair) = issues.windows(2).find(|w| w[1] - w[0] < occupied) {
            Some((pair[0], pair[1]))
        } else if ii - (issues[issues.len() - 1] - issues[0]) < occupied {
            Some((issues[issues.len() - 1], issues[0]))
        } else {
            None
        };
        if let Some((first, second)) = collision {
            violations.push(ModuloViolation::InitiationCollision {
                resource,
                lane,
                first,
                second,
                initiation: distance,
            });
        }
    }

    let computed = if graph.node_count == 0 {
        0
    } else {
        computed_span
    };
    if schedule.span != computed {
        violations.push(ModuloViolation::SpanMismatch {
            reported: schedule.span,
            computed,
        });
    }

    ModuloCheckReport { violations }
}
