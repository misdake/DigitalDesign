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

### Documented lookup methods

The implementation already uses the lighting design's section 14.3 RSQRT
and section 16 shininess methods. These are runtime table/interpolation paths;
there is no repeated squaring, runtime floating power or integer divider in counted.

* Normalization needs `1/sqrt(q)`, not the general reciprocal `1/q`.
  Write `q=2^e*t`, `1<=t<2`. Exponent parity selects two pages of 64 segments.
  Each 24-bit entry packs Q15 base in 16 bits and Q15 delta in 8 bits.
  Eight segment-fraction bits drive `r0=base-RNE(delta*f/256)`; the exponent
  restores `r=r0*2^(-floor(e/2))`. Component products retain full precision
  before RNE to Q14. General RCP uses LUT+Newton elsewhere in the GPU;
  it is not an operation needed by this pixel lighting subsystem.
* Shininess codes 0..16 select exponents
  `{4,5,6,7,8,10,12,14,16,20,24,28,32,40,48,56,64}`; code 8 means 16.
  A 43-bit material context supplies the boundary, two shifts and two modular
  address bases. The power ROM has 886 entries, each containing 16-bit left
  endpoint and 12-bit delta. One product and linear interpolation produce Q15
  power. Exact `x=32768` bypasses the lookup; invalid codes are rejected.
  All 557,056 nonendpoint addresses are checked against independent piecewise
  indexing and their material's segment range; maximum address is 885.

The default retains the documented RNE interpolation. The explicit optimized
profile's power-floor option is the separately measured truncation experiment
described below; it uses the same ROM, segments and addressing. Counted still
charges a `POWER_CONTEXT` ROM read per nonendpoint pixel. Moving that lookup
to material-context preparation is a remaining integration task; the current
model does not implement a `SET_MATERIAL` command or context-bank lifetime.

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
not yet prove runtime streaming, backpressure, register capacity, physical RAM
allocation, DSP packing, RTL equivalence or fmax. The periodic variant below
additionally proves a repeating arithmetic resource calendar at II=2.

Default abstract hardware provides 7 small and 9 large multiply lanes (3-cycle
latency, II=1), 2 add/compare/shift/round lanes per 18/36/54-bit class, 3 select
lanes per class, 1 leading-zero lane per class, 6 pooled normalization reads,
and one power/context read each. Logic and ROM latency are one cycle. These
are experiment capacities, not fitted device usage. Small and large products
remain separate; the additional two large products construct the NDC ray.
The lane budget is 12.5 equivalent 18x18 multipliers when a 9x9 lane is
weighted as half an 18x18 lane. Physical macro packing is a separate binding.

The current full template has 107 add/sub events: 88 mapped to the shared
18-bit class and 19 to the 36-bit class. The 88 comprise 34 RNE increment
adds, 18 absolute-value negations, 13 exponent/shift-control subtractions,
9 square-slope `2a+1` adds, 3 rsqrt page/segment adds, 3 rsqrt corrections,
3 half-vector sums and 5 remaining power/intensity operations. N/V/H
normalization contributes 24/21/24 of those 88 events. The 19 wide events
are 15 square/interpolation sums and 4 dot-product sums.

This is an unfused operation ledger, not a minimal hardware-adder count.
The 9 slope adds and 3 rsqrt addresses can be expressed as nonoverlapping
bit concatenations. RNE incrementers and sign/exponent handling can have
dedicated hardware bindings instead of competing with general sums on two
shared lanes. In the current mapping, 88/2 gives a 44-cycle-per-pixel
resource lower bound; this bound does not apply to a revised datapath.
The `lighting_add_audit` example exports each contributing event and its
operand producers to `additions.tsv`, plus a category/stage summary.

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

## Checked DSP and small-logic binding

`Hardware::lighting_dsp()` is an explicit alternative to the unchanged generic
default. `sim/binding.rs` lowers the numerical ledger into a target reservation
graph. The ledger retains its original arithmetic and stage goldens. Binding
and timing audits independently check the lowered dependencies and reservations.

Both `dot(N,L)` and `dot(N,H)` admit the same exact fusion:

```text
third component -> standalone signed multiply -> C
first two components -> A0*B0 + A1*B1 + C -> dot result -> RNE
```

The [Gowin DSP manual, MULTADDALU18X18 section](https://cdn.gowinsemi.com.cn/UG287E.pdf)
provides this two-product-plus-C mode. The two 16-bit signed products and C
share 28 fractional bits, with no rounding inside the fusion. Together the
two dots absorb four general wide-add operations per pixel. An intermediate
product or partial sum must not escape to another operation or observation;
the binding rejects such graphs. Internal ledger events are bookkeeping only
and cannot be scheduled before the fused result completes.

The profile reserves two dedicated pair+ALU macros, five standalone 18x18 lanes
and seven 9x9 lanes. This preserves the generic profile's 12.5-equivalent-18x18
budget. Kind-separated packing estimates seven macros in either case:
`ceil(7/4) + ceil(5/2) + 2`, versus `ceil(7/4) + ceil(9/2)`.
The fused lanes have assumed latency four and II one; ordinary multipliers
retain latency three. These are planning parameters, not a fitted pipeline.
The ALUs belong to the fused macros; standalone ALU sharing is not assumed.

The binding also proves nine square-slope and three rsqrt-address additions
are nonoverlapping concatenations. It assigns the 34 RNE additions to dedicated
conditional incrementers. Full-path work is then 42 narrow general add/sub
operations, 34 narrow increments, 15 wide additions and two fused dot operations
per pixel. The numerical ledger still has 107 add/sub events.

The profile provides eight narrow add/sub lanes, eight increment lanes and
eight rounding-control lanes, while keeping two lanes per wide-adder class.
Other capacities retain their generic defaults. A separate probe reduces the
wide classes to one lane each. No LUT, register, routing or fmax cost has been
measured for these capacities.

RNE uses `increment = guard && (sticky || retained_lsb)`, followed by
`retained + increment`. This has only a one-bit second operand and permits a
specialized conditional incrementer. It still needs carry propagation, sticky
reduction and control. FPGA carry-chain mapping can make a general adder quite
efficient too, so an area-saving percentage requires synthesis. The reservation
gain here comes from separating incrementers from general adders.

The following register-input measurements compare the same full-path batch
and 32-candidate search. Average cycles are finite-batch completion divided
by pixel count, not continuous-stream II:

| Binding | One pixel cycles | 16-pixel first result | 16-pixel completion | Average cycles/pixel | 32-pixel average |
| --- | ---: | ---: | ---: | ---: | ---: |
| Generic | 140 | 698 | 749 | 46.8125 | 46.250 |
| Wiring + increments + eight narrow lanes; no fusion | 133 | 395 | 486 | 30.375 | 28.156 |
| Pair+ALU; two wide lanes per class | 135 | 400 | 485 | 30.3125 | 28.094 |
| Pair+ALU; one wide lane per class | 142 | 473 | 581 | 36.3125 | 34.094 |

Most improvement comes from the small-logic binding. Fusion moves wide work
into the DSPs but makes little further throughput difference at these
capacities; waiting for C and the assumed four-cycle latency slightly increase
single-pixel latency. Even the one-wide-lane profile beats the original batch
completion. These results support trying fewer fabric wide adders, rather than
establishing a physical area optimum. The remaining narrow compare workload
is 54 operations on two lanes, giving a 27-cycle-per-pixel resource lower bound.

GPU-only validation has 58 passing tests, including all shininess codes,
signed/degenerate boundaries, short modes, numerical fusion checks, forbidden
intermediate escape, corrupted reservations and cross-iteration phase conflicts.
GPU clippy and formatting pass.
No emulator, RTL or place-and-route validation is included.

### Capacity sweep after supplying enough adders

`lighting_capacity_probe` keeps the pair+ALU binding, DSP/ROM budgets, input
storage and latency assumptions fixed. It searches 32 candidates for each
batch of 1, 16, 32 and 64 pixels. Extra capacities are sensitivity experiments;
they do not change the default hardware or claim affordable physical area.

| Capacity profile | 64-pixel completion | Average cycles/pixel | Resource-only lower bound | Limiting resource in that bound |
| --- | ---: | ---: | ---: | --- |
| Pair+ALU profile above | 1763 | 27.5469 | 27 | 54 narrow compares / 2 lanes |
| Add/sub, increments and round control each 8 lanes | 1783 | 27.8594 | 27 | Narrow compares |
| Same three classes each 16 or 32 lanes | 1783 | 27.8594 | 27 | Narrow compares |
| Eight arithmetic lanes; compares 8, selects still 3 | 1543 | 24.1094 | 23.3333 | 70 narrow selects / 3 lanes |
| Arithmetic, compare and select each 8; shifts still 2 | 746 | 11.6562 | 8.75 | Narrow selects |
| All configurable logic classes each 8 lanes | 617 | 9.6406 | 8.75 | Narrow selects |
| All configurable logic classes each 16 lanes | 362 | 5.6562 | 4.375 | Narrow selects |
| All configurable logic classes each 32 lanes | 291 | 4.5469 | 3 | Three wide leading-zero operations / 1 lane |

Each width class gets the stated lane count independently. Leading-zero
capacities remain one per width class throughout; multiply and memory resources
remain unchanged. In the final profile, standalone large multiplication has
the next resource lower bound, `14/5 = 2.8` cycles/pixel. The three-cycle maximum
resource bound is necessary but does not prove an achievable stream II.

Supplying more adders alone has already saturated. Compare, select and shift
capacities explain the next gains. Finite search is not monotonic under capacity
changes: additional lanes alter greedy choices and may yield a slightly worse
candidate, even though the old calendar is physically still feasible.
For the all-logic profiles, isolated single-pixel completion stays 135 cycles;
the 64-pixel figures include fill, drain and ordered output. Static reservations
still do not model bounded live-register storage or streaming backpressure.

## II=2 is the throughput target

The target is one input and one ordered result every two advancing cycles in
steady state. Batch completion divided by batch size includes pipeline fill
and drain and is not the acceptance metric for that target.

`Hardware::lighting_ii2()` reserves one pair+ALU macro and seven standalone
large multiplier lanes, instead of two macros and five standalone lanes.
Both dots together need two fused issues per pixel, so one macro at II one
has exactly sufficient capacity across the two phases. The remaining 14 large
products need seven standalone lanes. With the unchanged seven small lanes,
the budget remains 12.5 equivalent 18x18 multipliers and seven estimated macros.
The earlier profile's five standalone lanes could not support II=2 regardless
of logic capacity. Two leading-zero lanes per width class also remove its
three-wide-LZD-per-pixel capacity obstruction.

Other capacities are derived conservatively from the bound logical ledger:

| Resource | Work/pixel in full mode | Capacity at II=2 |
| --- | ---: | ---: |
| Small multiplication | 14 | 7 |
| Standalone large multiplication | 14 | 7 |
| Pair+ALU | 2 | 1 macro |
| Narrow add/sub | 42 | 21 |
| Wide add | 15 | 8 per wide class |
| Narrow conditional increment | 34 | 17 |
| Compare | 54 narrow / 5 wide | 27 per class |
| Select | 70 narrow / 4 wide | 35 per class |
| Shift | 8 narrow / 15 wide | 8 per class |
| Round control | 9 narrow / 25 wide | 13 per class |
| Leading zeros | 2 narrow / 3 wide | 2 per class |
| Normalization reads | 12 | 6 pooled lanes |

Power/context reads remain one lane each. These logic pools are capacity
inputs, not a final placement or area recommendation. Width classes reserve
separate units; some wider classes are overprovisioned by the shared field.
Further abs/clamp/shift-RNE lowering may reduce this conservative logic budget.

`sim/periodic.rs` performs a bounded phase-aware list search on the checked
bound DAG. Each resource operation occupies a unique `(kind, lane, issue % II)`
slot. Scheduling delays an operation to the next available phase while keeping
all operand/control dependencies. An independent audit checks slot identity,
latency, dependencies, lane/phase uniqueness and result readiness. Uniqueness
proves resource noncollision across arbitrarily many repeated pixels, rather
than relying only on one finite expansion. Input and output times repeat with
the same period. No loop-carried mutable state is modeled.

With 32 candidates, the baseline full-mode register-input calendar has **II=2 and fixed
147-cycle output latency**. Pixel p is released at `2*p`, and its result is
reserved at `147 + 2*p`. The 64-pixel expansion is checked by the existing
finite-plan auditor and matches oracle outputs:

| Pixels | First result | Last result | Consecutive output interval |
| --- | ---: | ---: | ---: |
| 1 | 147 | 147 | — |
| 4 | 147 | 153 | 2 |
| 16 | 147 | 177 | 2 |
| 32 | 147 | 209 | 2 |
| 64 | 147 | 273 | 2 |

The last row averages 4.2656 cycles/pixel because it includes fill; its recurring
calendar still produces one result every two cycles. The 147-cycle latency is
a result of the explicit current counted-operation graph and configurable
pipeline assumptions; it does not validate the separate target-spec C37
candidate or establish optimal latency.

Periodic tests include all 17 codes, signed/degenerate boundaries, backlight,
uniform short modes, 64-pixel expansion and deliberately distinct local times
that collide across iterations. The current API requires precaptured register
inputs and one uniform context; row input scheduling is explicitly rejected.
Physical bank placement, fixed input/result mux pairs, live-register capacity,
CE freeze, context changes, numeric cycle execution and RTL remain future work.

## Avoiding repeated abs and choosing where to truncate

The current external normal is S(16,14), not Q16.16. A sign-magnitude format
with the same total 16 bits has one sign and 15 magnitude bits, symmetric
extrema +/-32767 raw and two zero encodings. It does not gain precision over
the current two's-complement format, and loses the -32768 endpoint. Keeping
all 16 magnitude bits plus an external sign increases the total to 17 bits;
the extra magnitude capacity can be traded for range or precision. For these
pixel rows, two 17-bit normals still fit row 0 and normal Z plus 18-bit NDC X
still fits row 1. This establishes packing feasibility, not a new GPU ABI.

Sign-magnitude would make abs a magnitude extraction, but signed vector sums
and conversion into two's-complement dot/DSP operands still have costs. In
particular H=L+V may cancel and change sign. Flooring magnitude and restoring
sign also means truncation toward zero, whereas signed arithmetic right shift
means floor; they are different contracts. Zero signs should be canonicalized
before comparison, addressing or external observation.

The implemented optimization preserves the external types and instead removes
abs from square interpolation. For a scaled signed component x, write
`x = 128*a + b`, with signed a in [-128,127] and unsigned b in [0,127]. Then:

```text
square_chord(x) = (a*a << 14) + ((2*a+1)*b << 7)
```

This is exactly the same quadratic chord as indexing by abs(x). The generated
signed SQ table stores a*a in 256 15-bit entries. The signed slope `2*a+1`
is 9-bit concatenation `{a,1}`, proven as wiring by the target binding. Its
signed 9-bit by signed 8-bit nonnegative-tail multiply remains a small lane.
The wide correction can be negative; the checked unsigned sum is nonnegative.
The complete 32,768-input domain is exhaustively compared with the original
magnitude chord, and representative counted stage outputs agree bit for bit.

This avoids nine secondary abs operations per full pixel, including their
negation, comparison and selection. Nine initial abs operations for the N/V/H
maximum and degeneracy checks remain. The SQ payload grows from 128x14=1,792
bits to 256x15=3,840 bits per table copy; it still makes nine SQ reads per pixel.
No physical ROM bank allocation or fitted logic/BRAM tradeoff is claimed.

The oracle has stage-isolated `RoundingPolicy` controls. The bounded rounding
probe evaluates 32,768 random inputs at full intensity, another 32,768 at varied
intensity, 8,192 near-highlight inputs and 963 half-threshold cases. It reports
maximum, mean and RMS errors against both the unchanged RNE model and ideal
equations, along with changes in H degeneracy. Generated ROM endpoints and
external-input conversion remain RNE in every experiment.

| Changed stage | Observed effect relative to RNE | Decision in optimized profile |
| --- | --- | --- |
| All stages to floor | Up to 3 h output codes on highlights; a threshold case changes h by 256 codes | Retain stage-specific rounding |
| Normalize magnitude to floor, sign restored | Highlight h decreases by up to 3 codes; mean -0.412 code | Retain RNE |
| Signed normalization to floor | Highlight h changes by up to 3 codes; breaks signed rounding symmetry | Retain RNE |
| rsqrt correction to floor | Up to 2 h codes; changes reciprocal scale | Retain RNE |
| Half-vector division to floor | At L=(127,0,-16384), N=(16384,0,0), NDC=(0,0), h changes 256 to 0 by moving the threshold | Retain RNE |
| Dot quantization to floor | Highlight g has mean -0.500 output code | Retain RNE |
| Output conversions to floor | Highlight h has mean -0.400 code; varied-intensity g mean -0.247 code | Retain RNE |
| Nonnegative power interpolation to floor | At most one Q15 unit internally; sampled h changes by at most one output code | Truncate |

An output code here is 1/256, not 1/255. The signed/magnitude rounding variants
are diagnostic alternatives, not counted implementations. The half-threshold
corpus also has large RNE-versus-ideal differences because the existing contract
applies degeneracy after quantized half rounding, while the ideal model uses
an unquantized half. Its discontinuity must not be interpreted as table error.

For power, all 17 codes and all 32,769 inputs prove that floor remains monotone
and endpoints stay exactly 0/1, with `0 <= power_rne-power_floor <= 1` in Q15
raw units. The subsequent monotone RNE conversions and intensity <=1 constrain
the final change to at most one h output code; g and degeneracy are unchanged.
On the full-intensity random corpus, mean h change is -0.000671 output code and
RMS change is 0.025911 code; none of the 8,192 highlight outputs changes.
Counted implements the floor as static slicing of the padded product, followed
by checked narrowing, rather than hiding native arithmetic or deleting events.

`counted::Config::optimized()` selects signed SQ and power-floor together;
the unchanged default is the comparison baseline. `Hardware::lighting_optimized_ii2()`
uses that numerical configuration. Its full-path general narrow add/sub work
falls 42 to 33, increments 34 to 33, compares 54 to 45 and selects 70 to 61;
wide additions remain 15 after dot fusion. Conservative II=2 capacities fall
from 21 to 17 narrow adders, 27 to 23 compares per class and 35 to 31 selects
per class. Kernel configuration is part of the plan's audited certificate.

| II=2 profile, 32 candidates | 18x18-equivalent budget | Estimated macros | Output latency |
| --- | ---: | ---: | ---: |
| Baseline | 12.5 | 7 | 147 |
| Signed SQ + power floor | 12.5 | 7 | 139 |
| Optimized + one standalone large lane | 13.5 | 7 | 139 |
| Optimized + two standalone large lanes | 14.5 | 8 | 137 |

The first extra large lane occupies the unused slot in kind-separated packing;
this is still an unfitted estimate. Every row passes periodic phase/dependency
audit, finite expansion and its matching numerical oracle. The optimized
64-pixel calendar writes at 139,141,...,265. The current selected optimized
profile keeps the original DSP budget because extra lanes do little for latency
and do not improve the already-achieved static II=2.

## Intermediate precision experiments

Stored pixel/uniform formats and g/h remain unchanged. Oracle's
`reciprocal_work_extra` retains 0..8 additional bits between the existing
Q15 RSQRT endpoints and component multiplication. For k extra bits it computes
`r0_work=(base<<k)-RNE(delta*f/2^(8-k))`; no additional ROM precision is
invented. This is distinct from `reciprocal_fraction`, which changes the ROM
endpoint precision in the oracle. These experiments do not change counted or
its timed certificate.

The precision probe isolates each stage on the same 32,768 random inputs,
32,768 variable-intensity inputs, 8,192 near-highlight inputs and 963
half-vector threshold inputs as the rounding probe. Numbers below are h RMS
error relative to ideal equations, in output codes of 1/256:

| Oracle experiment | Random | Variable intensity | Highlight |
| --- | ---: | ---: | ---: |
| Current contract | 0.141207 | 0.143533 | 0.232239 |
| RSQRT work +1 bit, same ROM | 0.141219 | 0.144350 | 0.213336 |
| RSQRT work +3 bits, same ROM | 0.141024 | 0.144369 | 0.198203 |
| Direction work Q15, stored inputs Q14 | 0.139543 | 0.144177 | 0.187832 |
| Exact square only | 0.147318 | 0.146312 | 0.090160 |

Retaining one interpolation bit helps highlights but does not improve all
datasets. Its restored reciprocal needs U(18,16), versus current U(17,15),
and component products become signed 34-bit Q30. DSP sign handling, guards
and routing would need a reviewed binding before adopting it. Retaining
three bits needs a 20-bit reciprocal and can leave the native 18-bit envelope.

Increasing direction work to Q15 changes six threshold classifications in
the dedicated edge corpus and changes h by up to 256 codes there. This follows
from the current contract rounding H to Q14 before its degeneracy test; wider
arithmetic changes that boundary. It must not silently replace the contract.
Conversely, the RSQRT work-only variants made no threshold changes in this
corpus. These are measurements, not universal numerical bounds.

Removing only square interpolation produces a much larger highlight gain,
but slightly worsens the random corpus. Square, RSQRT and rounding errors can
partially cancel; improving one local operation does not guarantee a better
whole lighting result. Keep the documented counted configuration while using
the oracle switches to evaluate precision bottlenecks. A future symmetric
sign/magnitude representation may spend an extra bit only on selected internal
values; its conversion and exact +/-1 endpoints still require explicit costs.

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
physical bits. `UniformRows` serializes these rows. Uniform rows are loaded once
and latched before the batch; counted's logical input reads consume those latches.
The derived 43-bit power context still has a separately charged ROM lookup,
as described above; it is not yet latched by a material preparation model.
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
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-v2-binding-probe -CargoArgs @('-p','gpu-v2','--example','lighting_binding_probe','--','target/gpu-v2-lighting/binding')
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-v2-capacity-probe -CargoArgs @('-p','gpu-v2','--example','lighting_capacity_probe','--','target/gpu-v2-lighting/capacity')
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-v2-ii2-probe -CargoArgs @('-p','gpu-v2','--example','lighting_ii2_probe','--','target/gpu-v2-lighting/ii2')
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-v2-rounding-probe -CargoArgs @('-p','gpu-v2','--example','lighting_rounding_probe','--','target/gpu-v2-lighting/rounding')
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-v2-precision-probe -CargoArgs @('-p','gpu-v2','--example','lighting_rounding_probe','--','target/gpu-v2-lighting/precision','precision')
& scripts/run-cargo.ps1 -Subcommand run -Label gpu-v2-optimized-ii2-probe -CargoArgs @('-p','gpu-v2','--example','lighting_ii2_probe','--','target/gpu-v2-lighting/ii2-optimized','optimized')
```

The binding probe exports `summary.csv` plus per-profile work counts, fused
groups and ordered write calendars. `lighting_add_audit` exports the original
addition provenance so physical lowering can be compared with the logical work.
The II=2 probe exports its per-event lane/phase calendar, hardware capacities
and finite expansion summary in `target/gpu-v2-lighting/ii2`.
The rounding probe exports `summary.csv` and worst-case input examples. The
II=2 example also accepts `optimized-extra1` and `optimized-extra2` as its
second argument after the report directory to reproduce the DSP sensitivity rows.

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
