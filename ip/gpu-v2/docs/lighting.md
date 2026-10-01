# Implemented Rust lighting models

Lighting is a pixel component. Its public ports are in `src/lighting/ports.rs`.
There is no quad allocation, GPU command ABI, final-color calculation, emulator,
RTL or web implementation in this milestone.

## Numerical contract

The raster input carries an unnormalized signed 16-bit Q14 normal and two
signed 18-bit Q16 NDC coordinates in [-1,1]. Lighting normalizes the normal and
constructs the view ray `(ndc_x * ray_scale_x, ndc_y * ray_scale_y, k)`.
The CPU supplies quantized unit light direction L, intensities Ia/Id in [0,1],
projection coefficients and a material selecting one of 17 shininess exponents.
The projection has k in [0.5,0.75] and |ray_scale| <= 0.75.

```text
pixel normal -> normalize -> N -> dot(N,L) -> d=max(nl,0) -> g
pixel NDC -> projection ray -> normalize -> V
L,V -> (L+V)/2 -> normalize -> H -> dot(N,H) -> power -> h
```

`g = min(Ia + Id*d, 511/256)`; `h = Id*max(dot(N,H),0)^s`, masked to zero
when nl <= 0. Both outputs use unsigned 9-bit Q8; 256 means one. An unlit
material returns (1,0), zero Id returns (Ia,0), and zero specular color skips
the specular path. Specular color is used only to select that path; final color
application belongs to another component.

Normal maximum magnitude below 4 raw Q14 units is degenerate. Half-vector
maximum magnitude below 64 units, after `(L+V)/2` RNE, is degenerate. Degenerate
normalizations return zero after safe internal table evaluation. General
normalization uses a shared scale, a 128-entry square interpolation table and
a 128-entry reciprocal-square-root table. Right shifts requiring RNE implement
ties to even explicitly. A positive maximum of 32767 needs a second common
shift when the first rounded shift would reach the square table's upper edge.

`spec/lighting-formats.csv` generates principal fixed types; intermediate
algorithm widths and scaling constants remain explicitly chosen in Rust.
`spec/power-segments.csv` generates 886 packed power entries and 17 contexts.
Power endpoint values use integer exponentiation and RNE in the build script.
Changing principal formats requires reviewing intermediate widths and tables.

## Three stages and evidence

The oracle uses host integer arithmetic, independent RNE and independently
computed square/rsqrt interpolation. It also supplies ideal f64 equations and
runtime precision/approximation switches. Its power path shares generated ROM
contents with counted, but addresses them independently; a full code/exponent
sweep compares those contents and interpolation with ideal power.

Counted uses closed audited values, typed stores and audited arithmetic. It
publishes intermediate stage goldens and reports logical work, operand widths,
physical product lowering and reads. Observations do not represent hardware
memory writes. Full mode still computes the specular path for backlighting and
degenerate normals. The exact power-one endpoint may skip its table arithmetic.

Timed reserves a conservative nonendpoint counted DAG for each pixel in a
batch of 1..64 with one uniform context. It schedules operand and control
dependencies on finite resources, verifies lane initiation spacing and result
latencies, waits for source data, and preserves output order. Counted evaluates
the actual numerical outputs separately; timed is an offline reservation
experiment, not a cycle-stepped datapath or runtime queue controller. It does
not yet prove continuous-stream initiation interval, backpressure, register
capacity, physical RAM allocation, DSP packing, RTL equivalence or fmax.

Default abstract hardware provides 7 small and 9 large multiply lanes (3-cycle
latency, II=1), 2 add/compare/shift/round lanes per 18/36/54-bit class, 3 select
lanes per class, 1 leading-zero lane per class, 6 pooled normalization reads,
and one power/context read each. Logic and ROM latency are one cycle. These
are experiment capacities, not fitted device usage. Small and large products
remain separate; the additional two large products construct the NDC ray.

The `resource-scheduler` adapter compares the same DAG and hardware with an
earliest-ready baseline, critical-path tie-breaking and seeded random ties.
Ordered output writes are graph nodes. At most 64 candidates are allowed; the
probe uses 32. All feasible candidates pass the generic independent timing
checker, and the selected plan passes the lighting-specific audit. Search
keeps the original plan when no candidate improves completion.

The GPU tests cover 975 representative stage vectors, 1,024 seeded vectors,
all 17 power codes at all 32,769 input values, invalid inputs, mode shortcuts,
normal/half-vector thresholds, packed signed boundaries, finite scheduling
limits and corrupted reservations. The 1,024-vector maximum absolute errors
against ideal equations on the same quantized inputs are 0.003227 for g and
0.003770 for h. These are corpus measurements, not universal error bounds.

## Candidate local data layout

This layout is internal to this experiment and does not freeze a GPU ABI.

| Row | 36-bit pixel buffer payload |
| --- | --- |
| 0 | normal X at bit 0, normal Y at bit 16 |
| 1 | normal Z at bit 0, NDC X at bit 16 |
| 2 | NDC Y at bit 0 |

Each pixel has 84 payload bits in 108 physical bits. `PixelRows` checks unused
bits and NDC range. Four uniform rows contain light XY; light Z/Ia/Id;
projection XY; projection k/mode/shininess, totaling 121 payload bits in 144
physical bits. `UniformRows` serializes these rows. Context is loaded once and
latched before the batch; counted's logical context reads consume those latches.
Rows mode models 1 or more input read lanes with configurable latency, II=1,
and a complete `4+3*N` read calendar. It conservatively fetches all rows even
for short modes. Registers mode assumes all inputs have already been captured.
Each result is one 18-bit g/h payload in a 36-bit row on a single ordered port.
No bank placement or read/write collision semantics are implied.

## Reproduce the experiment

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-v2 -CargoArgs @('-p','gpu-v2','--','--nocapture')
& scripts/run-cargo.ps1 -Subcommand clippy -Label gpu-v2-lint -CargoArgs @('-p','gpu-v2','--all-targets','--','-D','warnings')
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-v2-probe -CargoArgs @('-p','gpu-v2','--example','lighting_probe','--','target/gpu-v2-lighting')
```

The bounded probe exports counts, candidate scores, event reservations, source
reads, ordered writes and outputs. The following register-input results use
the same full-mode graph and default hardware in every column:

| Pixels | Serial cycles | Interleaved cycles | Best of 32 cycles |
| --- | ---: | ---: | ---: |
| 1 | 148 | 148 | 140 |
| 2 | 296 | 167 | 157 |
| 4 | 592 | 225 | 223 |
| 8 | 1184 | 392 | 390 |
| 16 | 2368 | 749 | 749 |

Interleaving gives the main gain. Heuristic search produces smaller additional
gains and sometimes none. A greedy schedule can get slightly worse when an
input port is added because readiness changes its choices. The restricted
one-small/one-large multiply, one-normalization-read experiment is also in the
probe. The objective is finite batch completion, not steady-state throughput.
