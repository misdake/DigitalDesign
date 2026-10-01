//! A completed schedule for one candidate priority ordering.

/// Issue/ready/lane assignment for one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeSchedule {
    /// Cycle at which the node begins. Wiring nodes issue and are ready in the
    /// same cycle.
    pub issue: u64,
    /// Cycle at which the result is available: `issue + latency`.
    pub ready: u64,
    /// Lane used, or `None` for a wiring node.
    pub lane: Option<usize>,
}

/// Per-node assignments plus the total makespan.
///
/// `nodes` is indexed by [`crate::NodeId`], so it has exactly one entry per
/// graph node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// One assignment per graph node, in node order.
    pub nodes: Vec<NodeSchedule>,
    /// Makespan: the largest `ready` cycle (zero for an empty graph).
    pub cycles: u64,
}

impl Schedule {
    /// The objective value: total makespan in cycles.
    pub const fn objective(&self) -> u64 {
        self.cycles
    }
}
