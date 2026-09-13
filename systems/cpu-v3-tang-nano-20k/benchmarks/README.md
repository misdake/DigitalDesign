# CPU V3 benchmark suite

`suite/` is the frozen benchmark set for CPU V3. Every program is rcc source (no hand-written
hex), self-contained, deterministic, and checked exactly.

## Running

```powershell
benchmarks/run-suite.ps1 -Stage <N>   # frozen suite, CSV to target/stage<N>-results.csv
```

`run-suite.ps1 -ProgramsDirectory <dir>` overrides the program directory (used for ad-hoc or
historical experiments). The harness itself is
`tests/bench_emu.rs::benchmark_suite::run_benchmark_directory` (ignored, release mode).

## Program contract

- Metadata header (parsed by the harness):
  - `// bench-tier: short|medium|long|frame|stress` — reporting group; `stress` programs are
    microarchitecture pressure tests kept out of tier-level comparisons.
  - `// bench-max-cycles: <n>` — hard cycle bound; a program exceeding it fails.
  - `// bench-expected-halt: <n>` — the exact halt signal. Programs compute a deterministic
    checksum of their results and halt with it; self-checking programs instead verify
    internally and halt(1).
- No delay loops; every cycle does real work.
- Inputs and scale are fixed once frozen. Changing a program, its inputs, or the compiler
  options is a suite revision: cross-Stage comparison is only valid within one revision.

## Tiers

| tier | scale target | programs |
|---|---|---|
| short | ~200-1000 retired instructions | fizzbuzz, insertion sort, substring match, vec+heap |
| medium | tens of thousands | dijkstra-96, sieve-2000, binary-search-2048, matrix-16x16 |
| long | ~million | quicksort-2048, streaming-mix |
| frame | 10 heavy / 30 light frames | sprite-batch (10), particles (10, fix16 physics), tile-world (30) |
| fpu | short-to-medium | horner, sincos, splat (short); mandelbrot, normalize-batch, bezier, transform4x4 (medium) |
| stress | short | interleave (FPU/integer barrier), spill (FPU spill pressure) |

## Metric set (frozen)

`tests/system_emu/mod.rs` writes a fixed `summary.txt` per program;
`benchmarks/export-results.ps1` emits a fixed CSV column order. The metric set is:

- Program characteristics: `program_words`, `retired_instructions`, `retired_words`
  (plus the per-opcode retirement counts in `summary.txt`).
- Runtime: `cycles`, CPI/CPW, fetch-wait and data-path cycle shares, the I-cache/D-cache
  request/refill/write-back counters, load/store accept-to-response latencies, redirect count
  and wait cycles, prefetch issued/useful/useless/dropped, post-halt flush cycles and
  write-backs, and the SDRAM refresh count.

Counter semantics are the actual transaction semantics of the composed system: e.g.
`dcache_refills` counts accepted D-cache line-read requests at the memory arbiter,
`prefetch_useful` counts prefetched lines later demanded, and latencies measure
request-accept to response-valid cycles. Adding or renaming a metric is a metric-set revision
and must be called out in any comparison that mixes revisions.

The I-cache next-line prefetch mechanism was removed after the stage-12 audit showed ~1.5%
Logic / ~4.6% register cost for +0.0003% suite benefit. The four prefetch columns remain in
the CSV schema so old and new result files stay column-compatible, but they are pinned to
zero in every run produced after the removal.

## Comparing runs

**Never add values across programs** — not `cycles`, not `retired_*`, not the cache
counters, not the wait cycles. A suite sum is dominated by the largest programs
(long/frame) and hides what changed everywhere else. Every suite-level number is a
**geometric mean over per-program values**, equal weight per program:

- Comparisons: the headline is the geomean of per-program ratios (each program's
  new/old value), with the per-program table alongside.
- Single-run aggregates (the performance ledger, tier tables): the geomean of the
  per-program metric itself; ratios like CPI or fetch-wait % are computed per program
  first, then geomean-averaged. Counters that may be zero (e.g. D-cache write-backs)
  use the geomean of (value + 1), noted wherever they are reported.

## BTC comparison diagnostics

Set `CPU_V3_BTC_ENTRIES` to `0`, `4` (the default), or `8` before building/running
Cargo to compare the same workload with each fetch configuration. Use a separate
absolute `CPU_V3_BENCH_OUTPUT` directory per configuration when invoking the harness
directly; `run-suite.ps1` supplies its own absolute output directory. Label exported
rows with the capacity and dirty-tree status. Changing this hardware configuration
or a timing regression assertion does not revise the frozen workload or CSV schema.

Each profiled program also writes a separate `btc.txt`, sampled from the actual
fetch emulator at halt. This diagnostic file is outside the frozen metric set:

- `btc_entries`: build capacity.
- `lookups`: eligible stream restarts, including initial fetch (not identical to
  retired branch redirects); zero when BTC is disabled.
- `complete_hits`: restarts that start a complete-entry replay, counted once even
  if the core initially backpressures the target.
- `installed` / `cancelled_fills`: complete pair installations / incomplete fills
  cancelled by restart, flush or an accepted instruction error.
- `accepted_words` / `aborted_replays`: BTC words accepted by the core / unfinished
  replays cancelled by restart or flush.
- `continuation_wait_cycles`: request cycles without a response after both BTC
  words, ending on the first ordinary word accepted, a restart or a flush.

Always compare per-program cycles (geomean-aggregated per "Comparing runs") and exact
final-state checks as well as target wait: a zero target wait can otherwise conceal a
new continuation bubble. Retired instruction/word counts and the suite/compiler inputs
must agree across capacities.

## Adding or changing a program

1. Write or edit the `.rs` file with the metadata header.
2. For a new checksum, run the calibration helper first:
   `cargo test -p cpu-v3-tang-nano-20k --test bench_emu calibrate -- --ignored --nocapture`
   prints each program's halt signal from the instruction-level oracle; bake the value into
   `bench-expected-halt`.
3. Run the frozen suite and confirm every program passes its exact check.
## Historical reruns (cross-Stage comparison)

Stage milestones carry `stageN-bench` tags: benchmark-ready backport commits whose emulators
speak the current ISA (the revised FPU encodings, and for Stages 8-12 the corrected D-cache
timing). The old trees predate the current compiler, so historical runs execute prebuilt word
images instead of compiling source:

```powershell
# one-off: build the images with the current compiler
cargo run -p cpu-v3-tang-nano-20k --bin cpu-v3-bench-images

# single ref (HEAD runs from source; a tag runs its own emulator on the images)
benchmarks/run-commit.ps1 -Ref stage12-bench -Label stage12

# everything: images + stage0..12 + current, merged CSV
benchmarks/run-history.ps1
```

`run-history.ps1` writes `target/bench-history/combined.csv`. Rows from `stageN-bench` tags
carry `config=reconstructed` (the revised encodings and corrected D-cache timing never existed
on those stages' RTL) and must not be presented as hardware-measured. Image `bench-max-cycles`
budgets are scaled by 4 (recorded in each image header) because the budgets are tuned on
current hardware; the bound is a hang guard, never a metric.
