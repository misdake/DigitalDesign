//! Independent validation of a produced schedule.
//!
//! The checker recomputes every fact from the graph, the limits and the
//! assignment alone. It does not read any counter the scheduler maintained.

use crate::limits::Limits;
use crate::model::{Graph, NodeId, ResourceId};
use crate::schedule::Schedule;
use std::collections::BTreeMap;
use std::fmt;

/// One rule the schedule broke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    InvalidGraph(String),
    /// `Schedule::nodes` does not have one entry per graph node.
    NodeCount {
        expected: usize,
        actual: usize,
    },
    /// A predecessor id is outside the graph.
    InvalidPredecessor {
        node: NodeId,
        predecessor: NodeId,
    },
    /// A resource id is outside the graph.
    InvalidResource {
        node: NodeId,
        resource: ResourceId,
    },
    /// A resource-backed node has no lane.
    MissingLane {
        node: NodeId,
    },
    /// A wiring node carries a lane.
    UnexpectedLane {
        node: NodeId,
        lane: usize,
    },
    /// A lane is outside the resource's lane count.
    LaneOutOfRange {
        node: NodeId,
        lane: usize,
        lanes: usize,
    },
    /// `ready != issue + latency`.
    LatencyMismatch {
        node: NodeId,
        issue: u64,
        ready: u64,
        expected: u64,
    },
    /// `issue + latency` overflowed `u64`.
    LatencyOverflow {
        node: NodeId,
    },
    /// A wiring node has `ready != issue`.
    WiringTiming {
        node: NodeId,
        issue: u64,
        ready: u64,
    },
    /// Issue precedes the node's release gate.
    ReleaseGate {
        node: NodeId,
        issue: u64,
        earliest: u64,
    },
    /// A predecessor result is not ready by the consumer's issue.
    Dependency {
        node: NodeId,
        predecessor: NodeId,
        issue: u64,
        predecessor_ready: u64,
    },
    /// Two issues on one lane are closer than the initiation interval.
    InitiationCollision {
        resource: ResourceId,
        lane: usize,
        first: u64,
        second: u64,
        initiation: u64,
    },
    /// A result is ready after the deadline.
    Deadline {
        node: NodeId,
        ready: u64,
        max_cycle: u64,
    },
    /// `Schedule::cycles` disagrees with the recomputed makespan.
    MakespanMismatch {
        reported: u64,
        computed: u64,
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::InvalidGraph(message) => write!(f, "invalid graph: {message}"),
            Violation::NodeCount { expected, actual } => {
                write!(f, "schedule has {actual} node rows, expected {expected}")
            }
            Violation::InvalidPredecessor { node, predecessor } => {
                write!(f, "node {node} names invalid predecessor {predecessor}")
            }
            Violation::InvalidResource { node, resource } => {
                write!(f, "node {node} names invalid resource {resource}")
            }
            Violation::MissingLane { node } => write!(f, "node {node} has no lane"),
            Violation::UnexpectedLane { node, lane } => {
                write!(f, "wiring node {node} carries lane {lane}")
            }
            Violation::LaneOutOfRange { node, lane, lanes } => {
                write!(f, "node {node} uses lane {lane} of {lanes}")
            }
            Violation::LatencyMismatch {
                node,
                issue,
                ready,
                expected,
            } => write!(
                f,
                "node {node} issue {issue} + latency should be ready {expected}, got {ready}"
            ),
            Violation::LatencyOverflow { node } => {
                write!(f, "node {node} issue + latency overflowed")
            }
            Violation::WiringTiming { node, issue, ready } => {
                write!(f, "wiring node {node} issue {issue} != ready {ready}")
            }
            Violation::ReleaseGate {
                node,
                issue,
                earliest,
            } => write!(f, "node {node} issues at {issue} before gate {earliest}"),
            Violation::Dependency {
                node,
                predecessor,
                issue,
                predecessor_ready,
            } => write!(
                f,
                "node {node} issues at {issue} but predecessor {predecessor} is ready at {predecessor_ready}"
            ),
            Violation::InitiationCollision {
                resource,
                lane,
                first,
                second,
                initiation,
            } => write!(
                f,
                "resource {resource} lane {lane} issues at {first} and {second}, closer than initiation {initiation}"
            ),
            Violation::Deadline {
                node,
                ready,
                max_cycle,
            } => write!(f, "node {node} ready at {ready} exceeds deadline {max_cycle}"),
            Violation::MakespanMismatch { reported, computed } => write!(
                f,
                "schedule reports makespan {reported}, recomputed {computed}"
            ),
        }
    }
}

/// All violations found for one schedule; empty means the schedule is valid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// Rule breaks found, in node order followed by lane order.
    pub violations: Vec<Violation>,
}

impl CheckReport {
    /// Whether the schedule passed every rule.
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Validate `schedule` against `graph` and `limits` independently.
///
/// Checks node identity, predecessor readiness, release gates, lane ranges,
/// latency, initiation-interval collisions, the deadline and the makespan. It
/// never reads a scheduler counter and never mutates its inputs.
pub fn check(graph: &Graph, limits: &Limits, schedule: &Schedule) -> CheckReport {
    let mut violations = Vec::new();
    if let Err(e) = graph.validate(limits) {
        return CheckReport {
            violations: vec![Violation::InvalidGraph(format!("{e:?}"))],
        };
    }
    let node_count = graph.nodes.len();
    if schedule.nodes.len() != node_count {
        violations.push(Violation::NodeCount {
            expected: node_count,
            actual: schedule.nodes.len(),
        });
        return CheckReport { violations };
    }

    let mut lane_issues: BTreeMap<(ResourceId, usize), Vec<u64>> = BTreeMap::new();
    let mut computed_makespan = 0u64;

    for (node, spec) in graph.nodes.iter().enumerate() {
        let assignment = schedule.nodes[node];

        for &pred in &spec.predecessors {
            if pred >= node_count {
                violations.push(Violation::InvalidPredecessor {
                    node,
                    predecessor: pred,
                });
            } else if schedule.nodes[pred].ready > assignment.issue {
                violations.push(Violation::Dependency {
                    node,
                    predecessor: pred,
                    issue: assignment.issue,
                    predecessor_ready: schedule.nodes[pred].ready,
                });
            }
        }

        if assignment.issue < spec.earliest {
            violations.push(Violation::ReleaseGate {
                node,
                issue: assignment.issue,
                earliest: spec.earliest,
            });
        }

        match spec.resource {
            None => {
                if let Some(lane) = assignment.lane {
                    violations.push(Violation::UnexpectedLane { node, lane });
                }
                if assignment.ready != assignment.issue {
                    violations.push(Violation::WiringTiming {
                        node,
                        issue: assignment.issue,
                        ready: assignment.ready,
                    });
                }
            }
            Some(resource) => {
                let Some(unit) = graph.resources.get(resource) else {
                    violations.push(Violation::InvalidResource { node, resource });
                    continue;
                };
                let Some(lane) = assignment.lane else {
                    violations.push(Violation::MissingLane { node });
                    continue;
                };
                if lane >= unit.lanes {
                    violations.push(Violation::LaneOutOfRange {
                        node,
                        lane,
                        lanes: unit.lanes,
                    });
                }
                match assignment.issue.checked_add(unit.latency) {
                    None => violations.push(Violation::LatencyOverflow { node }),
                    Some(expected) if expected != assignment.ready => {
                        violations.push(Violation::LatencyMismatch {
                            node,
                            issue: assignment.issue,
                            ready: assignment.ready,
                            expected,
                        });
                    }
                    Some(_) => {}
                }
                lane_issues
                    .entry((resource, lane))
                    .or_default()
                    .push(assignment.issue);
            }
        }

        if assignment.ready > limits.max_cycle {
            violations.push(Violation::Deadline {
                node,
                ready: assignment.ready,
                max_cycle: limits.max_cycle,
            });
        }
        computed_makespan = computed_makespan.max(assignment.ready);
    }

    for ((resource, lane), mut issues) in lane_issues {
        issues.sort_unstable();
        let initiation = graph
            .resources
            .get(resource)
            .map_or(1, |unit| unit.initiation_interval);
        for pair in issues.windows(2) {
            if pair[1] < pair[0].saturating_add(initiation) {
                violations.push(Violation::InitiationCollision {
                    resource,
                    lane,
                    first: pair[0],
                    second: pair[1],
                    initiation,
                });
            }
        }
    }

    let computed = if node_count == 0 {
        0
    } else {
        computed_makespan
    };
    if schedule.cycles != computed {
        violations.push(Violation::MakespanMismatch {
            reported: schedule.cycles,
            computed,
        });
    }

    CheckReport { violations }
}
