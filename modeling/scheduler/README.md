# Resource-constrained scheduling experiment

`resource-scheduler` is a dependency-free offline tool beside `audited`.
Domain adapters provide DAG nodes, release gates and resources with lane count,
result latency and initiation interval. It does not know lighting or fixed-point
math and never changes the graph or adds hardware to improve a candidate.

The bounded search always includes earliest-ready list scheduling (`baseline`),
then a critical-path candidate. From six candidates onward it also adds a
backfilling `insertion` calendar and an `insertion-critical-path` calendar, and
every remaining slot becomes a seeded random restart. Insertion scheduling
tracks the real issue times on each lane and places a node at the earliest legal
hole, so earlier gaps are reused; with the critical-path priority used as the
primary order rather than only a tie-break this finds shorter makespans than the
append-only list scheduler. `lower_bound` reports a necessary makespan bound
from lane counts, initiation intervals and result latency. The objective is
makespan, with a live-node-count tie-break. That count includes wiring/control
nodes and sinks held to completion; it is not a register-bit or storage-capacity
estimate.

The API exposes `Graph::validate`, `plan`, `lower_bound`, and an independent
assignment `check`. Explicit limits bound nodes, candidates and completion
cycles; they are not a wall-clock watchdog. Invalid indices, cycles, duplicate
dependencies, zero capacities/latencies/II, deadlines and arithmetic overflow
fail explicitly. The checker reconstructs dependencies, lanes, II spacing,
latency, release gates and makespan rather than trusting search state.

## Periodic (modulo) scheduling

`ModuloGraph::from_graph`, `modulo_schedule_bounded` and the independent `check_modulo`
add a reusable periodic tool for a finite acyclic graph repeated every requested
initiation interval. `ModuloSchedule` reserves one issue cycle and lane per node;
iteration `k` issues at `issue + k * II`. The checker proves a legal result by
recomputing, from the graph and assignment alone, that no physical lane is
overbooked across repeated iterations (issues on one lane stay at least the
resource initiation interval apart in both circular directions), that every
predecessor result is ready before its consumer issues under the real dependency
latency, and that `span` equals the largest `issue + latency`. Initiation
intervals greater than one, release gates and actual dependency latency are modeled.
Absolute body times may exceed II; only resource reservations use residues.
Even a single operation is rejected when its own unit II exceeds the body II.
`ModuloGraph::resource_lower_bound` reports the necessary resource bound and
`ModuloError::Infeasible::best_span` is the minimum observed construction span,
or the necessary span bound if no complete construction exists. It does not
claim a valid schedule or prove global infeasibility. Limits bound node count,
completion and up to 64 candidates. Hard representation bounds also cap the
sum of all lane/phase calendars before allocation. `modulo_schedule` is a
convenience wrapper with the documented representation bounds. The search uses
ready, critical-path and seeded priorities without exact or unbounded search.

`compact_modulo` is an optional reverse-topological ALAP pass. Resource operations
move by whole periods, preserving lane, phase, II and completion; wiring moves
to its latest legal time. Input/output certificates are checked independently.
It does not guarantee fewer retained bits: later consumers can keep captured
inputs alive longer. An adapter must evaluate width-weighted lifetimes and keep
the original when compaction worsens its objective. Lighting does so.

Finite search independently checks every assignment, including candidates that
miss the deadline. Width/capacity, fusion and bank legality belong to audited/IP
certificates; the scheduler itself only sees scalar resource reservations.

Public API regressions are currently hosted in `ip/gpu-v2/tests/scheduler.rs`
so this milestone's test command stays `cargo test -p gpu-v2`. The lighting
adapter and reproducible experiment are described in
[lighting](../../ip/gpu-v2/docs/lighting.md). The example `compare` is an
optional generic driver, and `scheduler_upgrade_probe` in `ip/gpu-v2` is the
upgrade-specific probe; neither is the GPU acceptance command.
