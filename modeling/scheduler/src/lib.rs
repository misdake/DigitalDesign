//! Bounded offline resource-constrained DAG scheduling.
//!
//! This crate is a small, dependency-free, domain-independent tool for
//! scheduling an acyclic task graph onto a fixed set of pipelined resources.
//! It is a learning experiment, not an exact optimizer: it compares priority
//! orderings under exactly the same hardware and reports the best makespan it
//! finds.
//!
//! # Model
//!
//! A [`Graph`] is a list of [`Node`]s plus a list of [`Resource`]s. Each node
//! lists the predecessor nodes whose results it consumes and an optional
//! resource. A node without a resource is a wiring node: zero latency, no lane.
//! A resource has a lane count, a result latency and an initiation interval.
//! Real operand and control dependencies are preserved exactly; the scheduler
//! never merges, removes or simplifies nodes.
//!
//! # Usage
//!
//! ```
//! use resource_scheduler::{check, plan, Graph, Limits, Node, Resource, SearchConfig};
//!
//! let graph = Graph {
//!     nodes: vec![
//!         Node {
//!             name: "a".into(),
//!             predecessors: vec![],
//!             earliest: 0,
//!             resource: Some(0),
//!         },
//!         Node {
//!             name: "b".into(),
//!             predecessors: vec![0],
//!             earliest: 0,
//!             resource: Some(0),
//!         },
//!     ],
//!     resources: vec![Resource {
//!         name: "adder".into(),
//!         lanes: 1,
//!         latency: 2,
//!         initiation_interval: 1,
//!     }],
//! };
//! let limits = Limits::new(64, 1_000, 16);
//! let outcome = plan(&graph, &limits, &SearchConfig::default()).unwrap();
//! let best = outcome.best_candidate();
//! assert!(check(&graph, &limits, &best.schedule).is_ok());
//! ```
//!
//! # Boundaries
//!
//! The scheduler is a greedy list scheduler; it does not backtrack within a
//! candidate and never adds resources to improve a result. It does not do
//! exact optimization or ILP, does not model a runtime arbiter, and does not
//! model storage or a numerical framework. All search is bounded by
//! [`Limits`].

#![forbid(unsafe_code)]

mod check;
mod error;
mod limits;
mod model;
mod schedule;
mod search;

pub use check::{check, CheckReport, Violation};
pub use error::ScheduleError;
pub use limits::{Limits, SearchConfig};
pub use model::{Graph, Node, NodeId, Resource, ResourceId};
pub use schedule::{NodeSchedule, Schedule};
pub use search::{plan, Candidate, SearchOutcome};
