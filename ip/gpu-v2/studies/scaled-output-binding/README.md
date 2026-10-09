# Scaled output expression binding study

This is one finite Rust research unit. It compares the existing fine-grained
`scaled_output` ledger with a closed bit-window binding. It changes neither the
geometry numerical kernel nor its precision, record layout, DSP pool, WASM,
shared `audited` framework, or production GPU. No RTL or PnR was performed.

## Numerical and observation boundary

`reference.rs` freezes only the helper from geometry `counted.rs`, Git blob
`dacf7da0b72caa561ba89440b975a8e1f06b9647`, intake SHA256
`f472e2b1dc578364dd3abae35a10a23883a6cbbddd03e6d280da3285d9ce666f`.
The helper tokens match the source after whitespace removal. This labelled test
adapter is not a second production oracle. It calls the unchanged `audited`
operations. A separate integer rational quotient/remainder oracle checks the
new window's successful results without using the helper's jam/recover graph.

All eight actual `(input fraction, output width, output fraction)` combinations
are supported: `(46,36,28)`, `(60,36,28)`, `(28,36,24)`, `(60,36,24)`,
`(28,18,17)`, `(60,18,17)`, `(28,10,9)`, `(60,10,9)`. The last two exercise the
existing mixed study, without selecting that precision for production.

For valid signed72 input and signed18 exponent:

- `e >= 32` fails the original guard. `e < -126` fails negation/shift checks,
  including zero input. These failures are retained.
- `-126 <= e <= 0` requires the left-scaled intermediate to fit signed72.
  Recovery is exact, lost bits are zero, and the original eager `shifted + 1`
  still has to fit even when unused.
- `1 <= e <= 31` uses arithmetic floor. Its recovery fits signed72, and its
  lost remainder is nonnegative and less than `2^31`. A discarded-bit mask
  supplies sticky without a second wide shift/subtraction.
- The total scale is `F-G+e`. Floor, guard, sticky and parity produce RNE.
  Every supported format has `F-G >= 4`, so the original jam bit is below the
  guard bit. This claim is deliberately not extended to arbitrary formats.
- The original `RescaleFloor` intermediate narrowing, rounding-add range and
  final narrowing are checked separately. A final representable value cannot
  erase an earlier eager fault.

Tests cover 36,984 boundary/random cases and 24,624 exhaustive small signed
cases, with explicit loop bounds. Additional negative cases check eager unused
overflow, invalid exponents, input width, intermediate versus final narrowing,
and extra intermediate consumers/observations. Recognition requires the actual
operation/formats and exponent guard; it never derives ranges from trace values.

`binding::groups` verifies ledger audit, input/control closure, all consumers,
non-overlap, and the bound result for each recognized expression. An extra
consumer or Publish of an internal value rejects binding. Final observations
remain at the expression's final checked output. Failed numerical frames are
not scheduled, including fault reports whose recorded prefix passes ledger
audit. Extra arithmetic consumers are tested separately from Publish escapes.
This is bounded testing and an analytical domain argument,
not exhaustive enumeration of all signed72 values or a formal theorem proof.

## Fixed resource model and costs

The optional complete-trace replay starts from the existing packet calendar and
preserves every resource lane, initiation interval, latency and memory-port
declaration. A recognized expression reserves:

1. Existing `shift72`, latency 2: window, sticky and eager-range preparation.
2. Existing `round72`, latency 1: rounding decision.
3. Existing width-appropriate add lane, latency 1: add and final checked output.

This is a **latency assumption**, especially for the logic moved inside the
window stage. It does not prove that this cone fits the intended FPGA clock.
No combinational gate, mux or controller area is estimated. Moving checks into
the stage can increase local logic even while issue and transport decrease.

The first boundary carries the checked intermediate floor plus G/S, one valid
status bit and a three-bit format code: at most 43 model bits. The second carries
floor, increment, valid and format code. The final boundary carries the result
and valid. Input exponent/negation producers, existing guards, record addresses,
ROM, field barriers and all storage events remain charged. Destination control,
control ROM bits, physical valid/owner state and arbitration logic are still
unimplemented and cannot be inferred from these payload metrics.

Each expression replaces nine resource issues with three in this model. Its
round/add roles remain; the reverse shift and several general compare/select/
add issues move into the window cone. Lifetimes count actual data consumers,
not ordering/control edges. Dependency-bit totals are static signal-edge sums,
not routed wire count or measured memory traffic. Inclusive ready-to-last-use
bit-cycles and peak payload exclude RAM/ROM contents and actual allocation.

## Matched complete-trace results

The replay freshly executes six corpus cases, both clip methods and all three
existing plane profiles: 36 complete paths, covering all eight formats. Each
original numerical ledger is unchanged, and every bound result is checked.
An independent scheduler checker verifies both graphs under the same limits
and resources. Baseline cycles and peak data payload are cross-checked against
the existing geometry calendar. The older `probe.csv`/`stage-scan.csv` are
retained with hashes, not asserted to be matched-source evidence.

Representative **Conservative / direct attribute clip** results:

| Path | Baseline model cycles | Bound model cycles | Baseline peak payload bits | Bound peak payload bits |
| --- | ---: | ---: | ---: | ---: |
| Ordinary | 826 | 753 | 1,890 | 1,582 |
| Near crossing | 2,276 | 2,142 | 2,347 | 1,715 |
| Multiple clip planes | 4,631 | 4,321 | 2,383 | 1,751 |

For ordinary Conservative/direct clip, bit-cycles change from 587,652 to 475,455
and resource issues from 1,632 to 1,416. DSP work and RAM/ROM access counts stay
unchanged. Across all 36 paths model cycles decrease by 67 to 331 cycles.

**Negative result:** Conservative/weight clip's multi-plane peak stays at 5,212
bits despite fewer cycles and bit-cycles. Source attribute recovery creates the
peak outside this helper. This candidate does not solve every lifetime hotspot.

These traces lack an executed control ROM, source-capture/record-credit replay,
real backpressure, double-record ownership, CE and physical SRAM return state.
The reported cycles are finite dependency/resource schedules, not executed
geometry throughput. The model still retains the legacy scratch/output arrays;
it does not establish a physical storage allocation. There is no Logic/DSP-site
saving claim, full GPU resource result, hardware frequency or board proof.

## Reproduce and handoff

Core unit dependencies are only `modeling/audited` and `modeling/scheduler`.
Core tests do not require the untracked geometry study. From repository root:

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label scaled-binding-unit -CargoArgs @('--manifest-path','ip/gpu-v2/studies/scaled-output-binding/Cargo.toml','--offline','--','--nocapture')
& scripts/run-cargo.ps1 -Subcommand clippy -Label scaled-binding-clippy -CargoArgs @('--manifest-path','ip/gpu-v2/studies/scaled-output-binding/Cargo.toml','--offline','--all-targets','--','-D','warnings')
```

The optional `replay/` is a separate diagnostic package. It additionally needs
the frozen **existing** `ip/gpu-v2/studies/geometry` package, including its
`Cargo.toml`, `build.rs`, `src/lib.rs` and declared source modules. Its build
reads and checks vertex/triangle reference blobs at pinned commit
`acdd994eca42fb4afd7fd055b6d789541ef9b655`; that object must be available.
Neither old geometry sources nor WASM/web/output trees are part of this unit's
delivery. Do not cherry-pick or stage the whole old study as an implicit
dependency. Integrator decides whether/where to retain that study; no public
primitive or production integration is needed for the core unit.

```powershell
& ip/gpu-v2/studies/scaled-output-binding/replay/verify-source.ps1
& scripts/run-cargo.ps1 -Subcommand run -Label scaled-binding-replay -CargoArgs @('--manifest-path','ip/gpu-v2/studies/scaled-output-binding/replay/Cargo.toml','--offline')
```

Original intake SHA256 inventory, fresh complete-path CSV, role histograms and
validation logs are under `target/gpu-v2-scaled-binding/` in the producing
worktree. The final delivery inventory records exact source and dependency
hashes. All original intake files/dependencies/reports were checked unchanged.
No existing source file was edited. This candidate stops at the Rust study;
whether to fund physical implementation is a separate integration decision.
