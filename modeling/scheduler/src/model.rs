//! Domain-independent DAG and resource description plus structural validation.

use crate::error::ScheduleError;
use crate::limits::Limits;
use std::collections::{BTreeMap, BTreeSet};

/// Index of a node inside [`Graph::nodes`].
pub type NodeId = usize;
/// Index of a resource inside [`Graph::resources`].
pub type ResourceId = usize;

/// One operation in the dependency graph.
///
/// A node with `resource == None` is a wiring node: it has zero latency and no
/// lane, but it still forwards dependencies. A node with a resource is an
/// operation whose latency, lane count and initiation interval come from that
/// resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// Human-readable label; it is never interpreted by the scheduler.
    pub name: String,
    /// Operand and control dependencies. Each id must be unique and point to an
    /// earlier or later node in the same graph.
    pub predecessors: Vec<NodeId>,
    /// Earliest cycle at which this node may issue (a release gate).
    pub earliest: u64,
    /// Resource this node occupies, or `None` for a wiring node.
    pub resource: Option<ResourceId>,
}

/// One schedulable unit: a set of identical, pipelined lanes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    /// Human-readable label; it is never interpreted by the scheduler.
    pub name: String,
    /// Number of identical execution lanes; must be non-zero.
    pub lanes: usize,
    /// Cycles from issue to result ready; must be non-zero.
    pub latency: u64,
    /// Minimum distance between successive issues on one lane; must be
    /// non-zero. `1` is a fully pipelined unit.
    pub initiation_interval: u64,
}

/// A dependency graph and the resources its nodes use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    /// All nodes, addressed by [`NodeId`].
    pub nodes: Vec<Node>,
    /// All resources, addressed by [`ResourceId`].
    pub resources: Vec<Resource>,
}

/// Validated graph plus precomputed facts shared by all candidate schedules.
#[derive(Debug)]
pub(crate) struct Prepared<'a> {
    pub graph: &'a Graph,
    /// Successor lists, parallel to `graph.nodes`.
    pub children: Vec<Vec<NodeId>>,
    /// Longest latency path from each node to any sink.
    pub critical: Vec<u64>,
    /// Per-node latency: zero for wiring nodes, otherwise the resource latency.
    pub latency: Vec<u64>,
    /// Lanes the scheduler may use per used resource. This is
    /// `min(resource.lanes, node_count)`; more lanes than nodes can never
    /// change a greedy earliest-lane schedule, so the cap only bounds memory.
    pub lane_capacity: BTreeMap<ResourceId, usize>,
}

impl Graph {
    /// Validate the graph against `limits` without scheduling it.
    ///
    /// Rejects invalid indices, duplicate predecessors, cycles, zero lane
    /// counts, zero latencies, zero initiation intervals, zero limits, graphs
    /// larger than `max_nodes`, release gates past `max_cycle`, nodes that can
    /// never meet the deadline, and arithmetic overflow.
    pub fn validate(&self, limits: &Limits) -> Result<(), ScheduleError> {
        self.prepare(limits).map(|_| ())
    }

    /// Validate and precompute. This is the only place graph structure is
    /// trusted; scheduling and checking consume the result.
    pub(crate) fn prepare(&self, limits: &Limits) -> Result<Prepared<'_>, ScheduleError> {
        if limits.max_nodes == 0 {
            return Err(ScheduleError::ZeroLimit("max_nodes"));
        }
        if limits.max_cycle == 0 {
            return Err(ScheduleError::ZeroLimit("max_cycle"));
        }
        if limits.max_candidates == 0 {
            return Err(ScheduleError::ZeroLimit("max_candidates"));
        }

        let node_count = self.nodes.len();
        if node_count > limits.max_nodes {
            return Err(ScheduleError::TooManyNodes {
                count: node_count,
                max: limits.max_nodes,
            });
        }

        for (resource, unit) in self.resources.iter().enumerate() {
            if unit.lanes == 0 {
                return Err(ScheduleError::ZeroLanes { resource });
            }
            if unit.latency == 0 {
                return Err(ScheduleError::ZeroLatency { resource });
            }
            if unit.initiation_interval == 0 {
                return Err(ScheduleError::ZeroInitiation { resource });
            }
        }

        let mut children = vec![Vec::new(); node_count];
        let mut indegree = vec![0usize; node_count];
        let mut latency = vec![0u64; node_count];
        for (node, spec) in self.nodes.iter().enumerate() {
            if spec.earliest > limits.max_cycle {
                return Err(ScheduleError::DeadlineBreach { node });
            }
            let node_latency = match spec.resource {
                None => 0,
                Some(resource) => {
                    self.resources
                        .get(resource)
                        .ok_or(ScheduleError::InvalidResourceIndex { node, resource })?
                        .latency
                }
            };
            latency[node] = node_latency;
            // The release gate is a lower bound on issue, so this is the
            // earliest possible result; if it already misses the deadline the
            // node can never be scheduled.
            match spec.earliest.checked_add(node_latency) {
                None => return Err(ScheduleError::ArithmeticOverflow { node: Some(node) }),
                Some(ready) if ready > limits.max_cycle => {
                    return Err(ScheduleError::DeadlineBreach { node })
                }
                Some(_) => {}
            }

            let mut seen = BTreeSet::new();
            for &pred in &spec.predecessors {
                if pred >= node_count {
                    return Err(ScheduleError::InvalidNodeIndex {
                        node,
                        predecessor: pred,
                    });
                }
                if !seen.insert(pred) {
                    return Err(ScheduleError::DuplicatePredecessor {
                        node,
                        predecessor: pred,
                    });
                }
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
                .ok_or(ScheduleError::ArithmeticOverflow { node: Some(node) })?;
        }

        let mut lane_capacity = BTreeMap::new();
        for spec in &self.nodes {
            if let Some(resource) = spec.resource {
                let lanes = self.resources[resource].lanes;
                lane_capacity
                    .entry(resource)
                    .or_insert_with(|| lanes.min(node_count).max(1));
            }
        }

        Ok(Prepared {
            graph: self,
            children,
            critical,
            latency,
            lane_capacity,
        })
    }
}

/// Kahn's algorithm. The initial and queue orders are deterministic, so the
/// resulting topological order is stable across runs.
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
