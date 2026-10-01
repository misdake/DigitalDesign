//! Finite scheduling budgets.

/// Hard, caller-supplied bounds for one scheduling request.
///
/// All three fields are capacities and must be non-zero. `max_candidates`
/// bounds both the number of schedules evaluated and the host time spent:
/// the search never produces more candidates than this, and each candidate
/// uses finite graph walks, heap operations and lane scans. These bounds do
/// not provide a wall-clock timeout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum number of nodes accepted in a graph.
    pub max_nodes: usize,
    /// Deadline in cycles; no returned candidate may have a larger makespan.
    pub max_cycle: u64,
    /// Maximum number of candidate schedules the search may evaluate.
    pub max_candidates: usize,
}

impl Limits {
    /// Build a bounded request.
    pub const fn new(max_nodes: usize, max_cycle: u64, max_candidates: usize) -> Self {
        Self {
            max_nodes,
            max_cycle,
            max_candidates,
        }
    }
}

/// Deterministic randomized-search configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchConfig {
    /// Base seed for the deterministic random restarts. Baseline and
    /// critical-path candidates do not depend on it.
    pub base_seed: u64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            base_seed: 0x9E37_79B9_7F4A_7C15,
        }
    }
}
