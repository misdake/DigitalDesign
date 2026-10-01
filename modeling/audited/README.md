# Audited numerical modeling

`audited` is a domain-independent fixed-point modeling tool. It has no GPU,
processor, compiler, hardware-framework or board dependency. IP models depend
on this crate; this crate never depends on an IP or a final system.

## Data boundary

`Fixed<B,F,S>` has compile-time literals but no runtime constructor, raw getter,
native arithmetic or division. Checked external integers enter an Input store
before a frame starts. Runtime values come from audited operations and typed
memory reads. Dynamic addresses and predicates preserve value provenance.
`finish()` consumes the frame before exposing host observations; `audit()`
independently replays values, control, memory and resource counts.

Numerical mode (`Model::numerical`, `compute`) records work and payload I/O
without hardware capacities or cycle estimates. Scheduled mode additionally
binds declared resources and uses an ASAP dependency schedule. Neither an audit
nor an ASAP result proves algorithm accuracy, a static RTL pipeline, area or fmax.

Products record both logical operand widths and physical lowering. Narrow
products use DSP18, wide-by-narrow products use two DSP18 operations plus a wide
reassembly adder, and wide products use DSP36. A 9-bit product is a logical
DSP18 operation; explicit target packing is checked separately. Rounding includes its
guard and increment adder. Variable right shifts truncate; RNE must be explicit.
`resize_exact` checks a contract and never implements saturation.

## Source ownership

| File | Responsibility |
| --- | --- |
| `src/lib.rs` | Closed types, formats, faults and resource declarations |
| `src/arithmetic.rs` | Audited numerical operations and product lowering |
| `src/model.rs` | Stores, event ledger, external inputs and independent audit |
| `src/scheduling.rs` | Optional resource scheduling and timing checks |
| `src/physical.rs` | Fused provenance, DSP topology, banks, replicas, ports and collision certificates |
| `src/lifecycle.rs` | Width-weighted retained values and periodic overlap |
| `src/flow.rs` | Bounded context leases, FIFO credits, CE and ordered commit |
| `src/tests.rs` | Framework semantic and tamper regressions |
| `examples/support/triangle.rs` | Bounded mathematical driver, outside private internals |

## Validation

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label audited -CargoArgs @('-p','audited')
& scripts/run-cargo.ps1 -Subcommand run -Label audited-driver -CargoArgs @('-p','audited','--example','audited_triangle','--','target/audited/driver')
```

The triangle example is a framework regression, not a triangle setup algorithm.
All frames have event limits; scheduled frames additionally have cycle limits.

## Explicit physical and state certificates

These modules consume finished reports without exposing a runtime `Fixed`
constructor. The numerical ledger stays immutable and independently replayable.
IP adapters supply a placement and timing certificate; checks recompute legality
rather than accepting an optimizer's resource counters.

`physical::FusedGroup` currently certifies exactly `(A0*B0+A1*B1)+C` with signed
18-bit-or-smaller operands, common product fraction and a signed 54-bit-or-smaller
accumulator/result. Absorbed intermediates may not escape. Bound dependencies
preserve external operands and every control gate; a certificate with different
operands, overlapping groups or an internal escape is rejected.

`physical::LogicCone` certifies a connected pure-logic subgraph with one result,
at most 64 absorbed events, an explicit width cap and a positive result latency.
Its sorted external operands and every control gate are preserved. Multipliers,
memory, publication and control effects cannot be absorbed; internal values must
not escape. Absorbed events become zero-time aliases at the result-ready edge.
`composed_dependencies`, the composed memory audits and
`lifecycle::analyze_composed_policy` combine these cones with DSP fusion, reject
overlap and account for the cone result's retained storage. Every original
numerical operation remains in the independently replayed ledger. This is a
declared circuit boundary; its width and latency do not prove a clock frequency
or its physical adder count. An IP must declare cone lanes and validate timing
through synthesis before treating the estimate as hardware evidence.

`DspInventory` describes two macros per tile. A macro holds four 9x9 lanes,
two 18x18 lanes or one paired/ALU/MAC mode; different kinds cannot share it.
A 36x36 instance owns both macros of its tile. Independent pre-add and ALU modes
also reserve their sites. `audit_issues` checks widths, fixed result latency and
unit II, including wraparound and a solitary operation's next-iteration conflict.
The half-slot inventory counts actual multiplier work; macro/tile occupancy also
captures unused slots and standalone ALU/pre-add reservations. This is a
conservative target policy, not synthesis, pin routing or fmax evidence.

`MemoryLayout` maps every ROM/RAM to copies of row/bit slices. Logical bits must
be covered exactly, physical regions fit capacity, and RAM owns whole rows
(arbitrary write masks are not assumed). Each access chooses a copy and a port
per slice; a RAM write must reach every replica. Shared RW ports share their II.
ReadFirst/WriteFirst requires the same-edge observation to agree with numerical
frame order, and two same-address writes are forbidden. Logical hazards include
all earlier reads even when independent reads change order. Effects happen at
the issue edge; declared latency delays the result/acknowledgment.
BSRAM requires registered reads; SSRAM permits asynchronous zero-cycle reads.
`audit_gowin_budget` checks supported BSRAM primitive geometries and counts
SSRAM RAM16 composition. Periodic access auditing accepts read-only bodies;
mutable iterations need a future stateful numerical replay. Representative
dynamic addresses do not prove conflict freedom for every possible address.

`lifecycle::analyze_bound_policy` counts half-open live intervals by bit width,
including overlap of repeated pixels. Wiring aliases share producer storage,
literals and fusion metadata do not allocate retained results. Input rows are
captured at body time zero even when their read events are later. Selected output
ports are held through commit; diagnostic goldens can be excluded explicitly.
Context inputs marked invariant count once across the periodic stream. Total
and optional per-width budgets are enforced. This report excludes DSP internal
pipeline registers, routing/mux registers, RAM cells and FIFO/control state;
it is not the design's total FF count or a register-allocation proof.

`flow::FlowMachine` models control tokens with bounded IDs/epochs and explicit
payload-bit accounting. A tick completes a token, commits a complete FIFO head,
loads an unused context bank, then accepts a token at its issue phase. Tokens
hold a context reference until commit; stale leases and exhausted credits fail
atomically. CE=0 freezes all modeled state and phase. A maximum wall-tick count
also bounds indefinitely stalled runs. `FlowTrace::audit` replays this control
transition model; it does not execute numerical payloads or provide an independent
arithmetic oracle. Uniform values, datapath stalls and payload queues still need
an IP-specific cycle executor.

The public physical/lifecycle/flow acceptance tests currently live in
`ip/gpu-v2/tests/physical.rs`; this milestone runs `cargo test -p gpu-v2` only.
