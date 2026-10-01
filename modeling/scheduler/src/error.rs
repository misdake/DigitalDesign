//! Rejected inputs and infeasible planning outcomes.

use crate::model::{NodeId, ResourceId};
use std::fmt;

/// Why a graph or a scheduling request was rejected.
///
/// Structural problems (indices, cycles, zero capacities, guaranteed deadline
/// breaches, arithmetic overflow) are reported before any schedule is
/// produced. [`ScheduleError::NoFeasibleSchedule`] is reported only after every
/// candidate was evaluated and none met `Limits::max_cycle`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// A `Limits` field that is a capacity was zero.
    ZeroLimit(&'static str),
    /// The graph has more nodes than `Limits::max_nodes`.
    TooManyNodes { count: usize, max: usize },
    /// A predecessor id is not a valid node id.
    InvalidNodeIndex { node: NodeId, predecessor: NodeId },
    /// A node names a resource id that is not present.
    InvalidResourceIndex { node: NodeId, resource: ResourceId },
    /// A node lists the same predecessor twice.
    DuplicatePredecessor { node: NodeId, predecessor: NodeId },
    /// The predecessor relation is not acyclic.
    Cycle { nodes: Vec<NodeId> },
    /// A resource declares zero lanes.
    ZeroLanes { resource: ResourceId },
    /// A resource declares zero result latency.
    ZeroLatency { resource: ResourceId },
    /// A resource declares a zero initiation interval.
    ZeroInitiation { resource: ResourceId },
    /// A node cannot be issued before `Limits::max_cycle` even at its release
    /// gate, so the deadline is unreachable for it.
    DeadlineBreach { node: NodeId },
    /// A checked arithmetic step overflowed `u64`.
    ArithmeticOverflow { node: Option<NodeId> },
    /// Every candidate exceeded `Limits::max_cycle`; `best_cycle` is the
    /// smallest makespan actually observed.
    NoFeasibleSchedule { best_cycle: u64 },
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleError::ZeroLimit(name) => write!(f, "limit {name} must be non-zero"),
            ScheduleError::TooManyNodes { count, max } => {
                write!(f, "graph has {count} nodes, limit is {max}")
            }
            ScheduleError::InvalidNodeIndex { node, predecessor } => {
                write!(f, "node {node} names invalid predecessor {predecessor}")
            }
            ScheduleError::InvalidResourceIndex { node, resource } => {
                write!(f, "node {node} names invalid resource {resource}")
            }
            ScheduleError::DuplicatePredecessor { node, predecessor } => {
                write!(f, "node {node} repeats predecessor {predecessor}")
            }
            ScheduleError::Cycle { nodes } => write!(f, "dependency cycle over nodes {nodes:?}"),
            ScheduleError::ZeroLanes { resource } => {
                write!(f, "resource {resource} has zero lanes")
            }
            ScheduleError::ZeroLatency { resource } => {
                write!(f, "resource {resource} has zero latency")
            }
            ScheduleError::ZeroInitiation { resource } => {
                write!(f, "resource {resource} has zero initiation interval")
            }
            ScheduleError::DeadlineBreach { node } => {
                write!(f, "node {node} cannot meet the cycle deadline")
            }
            ScheduleError::ArithmeticOverflow { node } => match node {
                Some(node) => write!(f, "arithmetic overflow while handling node {node}"),
                None => write!(f, "arithmetic overflow while preparing the graph"),
            },
            ScheduleError::NoFeasibleSchedule { best_cycle } => {
                write!(
                    f,
                    "no schedule meets the deadline; best makespan was {best_cycle}"
                )
            }
        }
    }
}

impl std::error::Error for ScheduleError {}
