# Resource-constrained scheduling experiment

`resource-scheduler` is a dependency-free offline tool beside `audited`.
Domain adapters provide DAG nodes, release gates and resources with lane count,
result latency and initiation interval. It does not know lighting or fixed-point
math and never changes the graph or adds hardware to improve a candidate.

The bounded search always includes earliest-ready list scheduling, then a
critical-path tie-break candidate, then seeded randomized ties. Earliest-ready
time takes priority in every candidate. Greedy per-lane clocks do not backfill
earlier holes or backtrack. The objective is makespan, with a live-node-count
tie-break. That count includes wiring/control nodes and sinks held to completion;
it is not a register-bit or storage-capacity estimate.

The API exposes `Graph::validate`, `plan`, and an independent assignment
`check`. Explicit limits bound nodes, candidates and completion cycles; they
are not a wall-clock watchdog. Invalid indices, cycles, duplicate dependencies,
zero capacities/latencies/II, deadlines and arithmetic overflow fail explicitly.
The checker reconstructs dependencies, lanes, II spacing, latency, release gates
and makespan rather than trusting search state.

Public API regressions are currently hosted in `ip/gpu-v2/tests/scheduler.rs`
so this milestone's test command stays `cargo test -p gpu-v2`. The lighting
adapter and reproducible experiment are described in
[lighting](../../ip/gpu-v2/docs/lighting.md). The example `compare` is an optional
generic driver, not the GPU acceptance command.
