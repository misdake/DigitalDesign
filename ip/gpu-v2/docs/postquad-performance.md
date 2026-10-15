# Complete post-quad composition

The actual `system::pixel::postquad` RTL connects Quad queues, Lighting, Runtime
Sampling, texture cache, Color, ordered join, Final color, ROP, framebuffer cache
and one shared 128-byte memory arbiter. Result production and retirement use
actual handshakes. The ROM driver never marks an unfinished branch complete.

The upstream contract is the [quad output protocol draft](quad-output-protocol-draft.md).
The [editable theory and performance diagram](https://www.tldraw.com/f/ApLq8dDsVItIaoqoFm1ro)
shows the component graph and the distinction between arithmetic and whole-chain calendars.

## Selected four-cycle post-interpolator chain

`postquad::verilog_selected` and exporter modes `selected` and `ii4` emit
`postquad_ii4`: Lighting, preparation, texture cache, Color, ordered Final, ROP,
framebuffer cache and the internal memory arbiter. The interpolator is external.
The eight-row quad protocol and immutable-draw drain boundary remain unchanged.
II4 describes admission and steady hot commits, not pixel latency. Cache misses,
same-pixel dependencies and external pauses are paid separately.

Lighting retains its numerical operations and 34 enabled-edge latency, with a
four-phase calendar, nine ordinary 18-bit multipliers and one shared dot MAC.
Normalization reads share four scheduled channels over two true-dual-port ROM
banks. The shared MAC uses its existing hard output register instead of a fabric
tail register. The closed Lighting core advances on private compute CE;
its actual parent guarantees output readiness on that CE. Public context/pixel
acknowledgments and result writes still require the common commit CE, and the
generic core retains its independent output-backpressure guard.
Color uses six small multipliers for two RGB taps per edge and a
guarded packed DSP accumulator; exact four-tap groups and trilinear results keep
their existing rounding. Final/ROP retain the shared two-phase product bank.

There are sixteen global quad owners. This capacity reduction from the Group48
reference is explicit; the public sixteen-slot owner space, Group48/Pool64 and
their old-state credits remain unchanged. Exact coefficients are written once
to the existing authoritative Color weight bank. Private coefficient/member
transport carries nonzero masks, not another full weight payload. Its complete
private composition is required; generic coefficient exports still carry full
weights. Current ordered preparation and Context checks are retained.

For a non-power-of-two surface stride, the tile number is computed by shifts and
adds and captured on exactly the canonical header's two load paths. Cache
lookup borrows that held tile number. This avoids an inferred address DSP and
keeps stride arithmetic out of the cache-readiness/global-enable return path.
It adds no issue edge and changes no enabled RAM operation. The cadence lowering
also accepts the configured surface height rather than matching a literal 256.
The II4/selected exporter defaults to the current 400x240 contract;
`surface512` explicitly reproduces the historical 512x256 benchmark boundary.

Source identities, archive review, bank/primitive counts, matched wrapper fits,
bounded replays and rejected variants have one home in
[the source-matched II4 acceptance report](postquad-ii4-qualification.txt).
Whole-GPU/CPU placement and board validation remain separate. Explicit Group48,
older factories and the legacy `CompositionProfile::selected()` template remain
available for comparisons; they do not select the new complete factory.

## Previous Group48 post-quad successor

The explicit `postquad::verilog_combined_group48_pool64_candidate` factory,
exported as `combined-group48-pool64`, builds on the retained
`combined-closed-basic-carry`,
`combined-stream-onehot` and Group34 successors. It follows continuous preparation,
canonical quad authority, smooth direct h9, integer filter and two-phase
Final/ROP composition. It keeps texels in physical bank order and applies the
inverse permutation to four nine-bit weights. The physical CacheEmu has the
matching opt-in `new_bank_order` construction. The successor stores four
unmasked weights once under the existing Public lifetime, reconstructs tap
partitions from geometry, and restores the texture slot from immutable draw
configuration. Work carries metadata only; Packet and Pool carry Group34.
The physical Store model independently represents the weight bank, paid read
and captured result. Original constructions, `combined-weight-order`, and
reference factories remain available. The framebuffer hot path now has one
work/read context and one return/write context, each containing line, origin XY
and coverage mask. Accepted row0 installs work; the first ordered Color return
transfers it to return, with a first-Color address bypass. Later Depth writes,
dirty publication and commit use return. Existing head/tail flags retain paid
credit and fault checks, but do not select descriptor payload arrays. A full
origin comparison retains same-pixel RAW waiting. The top aligns origins to
the 2x2 grid, so different legal origins do not overlap.

The closed framebuffer lookup compares all eight valid tags in parallel and
encodes the unique hit with OR reductions. A single maintenance publisher
installs only absent targets; reset-led legal states therefore have unique
valid tags. Lowest-invalid replacement remains priority encoded. The generic
cache keeps its original arbitrary-internal-state priority semantics.

The combined caller reads Basic only from published quad owners. Its issue
owner is retirement head or the sole next quad; the ingress writer owns the
unpublished tail under old-state capacity checks. The private Basic profile
uses this disjointness to simplify both ingress preflight and paid-return
control. Generic ingress and row-store exports retain their collision checks.
The texture cache derives only the four physical bank-row address bits by
XOR/carry equations, including local7-to0 wrap, with unchanged enabled reads,
paid DO capture, cache owners and result ages. The physical old-state RAM
interpreter verifies actual enabled operations and pending returns.

The Group48/Pool64 factory now selects Lighting B through the actual SmoothH9
Program/Emu/RTL calendar: a global seven18 pool and one fused MAC, with unchanged
34-edge latency, II2, dedicated H9 tail and per-pixel V/H. Both production
complete exports are byte-identical to the qualified B sources; only Lighting's
module body differs from the frozen reference. The binding and reference
constructors are specified in [Lighting](lighting.md#direct-h9-closed-draw-calendar).
Matched fit numbers remain solely in
[historical exp-010](experiment-review.md#reproduction-and-historical-boundaries);
actual adoption identity and bounded production replay are in
[historical exp-140](experiment-review.md#reproduction-and-historical-boundaries).

The Group48 capacity qualification, balanced-capacity comparison, matching
complete-chain fits and working-tree promotion are recorded in
[historical exp-022](experiment-review.md#reproduction-and-historical-boundaries).
Only Group credit32->48 and the row safety guard48->63 change in the actual
Pool leaf; Producer16, ring64, two owned heads, FF/BSRAM/DSP formats and physical
read/write ports remain. Cold waiting can accept more Packets; tested final
drain times remain unchanged. The capacity experiment does not claim a higher
steady pixel rate. Prior source identity, exact-error qualification, steady II2
pixel throughput, matched complete-chain fits and worktree promotion are recorded in
[historical exp-045](experiment-review.md#reproduction-and-historical-boundaries)
and its source freeze, matched fit receipts and `promotion.json`. The preceding
stream/parallel-hit evidence remains in
[historical exp-042](experiment-review.md#reproduction-and-historical-boundaries). The preceding
Group34 weight-storage evidence remains in
[historical exp-044](experiment-review.md#reproduction-and-historical-boundaries). The upstream geometry and physical
memory controller remain outside that fit. This successor is working-tree
source integrated into `code-v0.3-dev`, not a complete fitted GPU. The historical
CLI `selected` profile remains a reference; use `combined-group48-pool64` for
this successor. Existing fitted receipts describe their frozen exported RTL,
not a new post-integration fit or board measurement.

Texture slot may change only after the old draw is fully drained, as specified
by the existing immutable-draw contract. The two-draw qualification exercises
that rebind. The top does not compare every immutable input bit against an
accepted snapshot to detect illegal mid-draw mutations. Work16, Public16,
Producer16, Group48, Pool64, Context8 and C6 retain the existing pointer widths
and prefetch lifetime. The former Group32 construction remains available. Color arithmetic and all qualified RGB/DRAM results are unchanged.
Same-tile hot and eight alternating resident framebuffer tiles both sustain
eight advancing clocks per quad, or two per pixel. The measured central
window contains 127 intervals, all eight; this is not a fill/drain average.
Repeated identical origins retain the necessary twelve-edge RAW cadence.
Cold replacement, sparse/depth-fail, CE0, raw faults and paid transport drain
are qualified separately. Maintenance still waits for all active pixel owners
to drain. Earlier indexed two-line candidates remain retired experiments.

The remaining measurements and calendars below describe the frozen earlier
throughput checkpoint. Its nineteen-edge quad calendar is historical; it does
not describe the working-tree successor's eight-edge hot quad cadence.

## Measurement boundary

Each measured draw contains 256 prebuilt quads (1024 covered pixels) supplied
from a Verilog ROM as eight accepted rows per quad. A ROM row is held through
backpressure. The surface is 512x256, RGB565 plus unsigned D16; the measured
positions cover one 16x16 tile four times with decreasing source depth.
Framebuffer values are checked against independent pixel arithmetic and the
complete 65536-byte test memory image, including texture and untouched bytes.
Lighting, Sample and Final numeric returns are checked separately.

The memory model accepts one outstanding request, waits six edges, supplies or
consumes sixteen 64-bit beats and delays terminal ACK by two edges. It models
transport stalls, not a physical SDRAM controller. RTL cycle results do not
constitute board throughput or a fitted whole GPU result. ROM and memory model
are outside the fitted DUT; the identical registered timing wrapper is included.

Texture and framebuffer cold/hot states are independent. Real primer ROM draws
run to idle before measurement; tags and dirty data remain live. A texture-hot,
framebuffer-cold primer touches a different framebuffer tile, so its later dirty
eviction/writeback is included. Hot means the measured working set is resident,
and is confirmed by zero measured refill requests; it does not mean flush is free.

The feature matrix includes both branches bypassed, unlit textured, lit
untextured and full Lighting/texture/specular/depth/blend. Texture modes are
nearest one mip, bilinear one mip, trilinear two mips and trilinear seam/wrap.
Full mode uses alpha128, SRC_OVER, LESS_EQUAL and depth writes. Other modes use
alpha255, REPLACE and ALWAYS with depth writes. Lighting uses actual g224/h192.

## Historical reference chain performance

Input is eight accepted edges per quad. Lighting has arithmetic II2 and latency38
per participating pixel. Derivative and LOD are II8 per quad (latencies3 and4);
coordinate preparation is II2. Groups vary with footprint and coverage: one
pixel can require multiple tile groups, so Group II alone is not pixel II.
Runtime cache/Color process actual groups and share refill transport with ROP.

The reference join issues Final at activation+3,+8,+13,+18. Nine-stage Final
publishes the last RGB at +27; the sink accepts its color and depth at +28/+29,
and the next quad activates at +30. A shorter Final exposes the existing
resident ROP calendar: startup1, row reads9, color/depth reads2, capture1,
issue4, wait8, two write phases2 and done1, totaling28 edges per quad.
Arithmetic ROP II1/latency8 therefore does not imply one-pixel chain throughput.

Measurements separate prime time, first publication/retirement/commit, source
completion, drain and flush. Steady throughput uses commit indices96..223:
127 intervals, avoiding fill/drain averages. A seam/wrap workload may be
preparation-bound even when all texture tiles are hot.

## Frozen throughput checkpoint and resources

`postquad::verilog_selected` now enables `cache_pipeline`, `join_overlap` and
`rop_overlap` in the closed single-draw profile. These are static lowerings.
Public16, Context8, C6, Work16, P16, G32 and Pool64 capacities remain fixed.
D retains fabric arithmetic; Color retains independent 9x9 products and fabric
sums. Final retains three MULT9X9 and ROP six. No new DSP or BSRAM is added.

The checkpoint source/fit/performance receipt is
`D:/github/DigitalDesign-code/target/postquad-throughput-20261010/review.json`.
Its `report.md` owns that checkpoint's fitted totals and comparisons;
`component-breakdown.md/json` owns the disjoint synthesis counts. Per-component
physical Logic is unavailable; synthesis LUT/ALU counts must not be presented
as placed Logic. Both matched placements meet54 MHz with zero setup/hold
violations, with the identical registered wrapper included and ROM excluded.
That checkpoint exceeded the7000 Logic post-quad target inside the10000
whole-GPU budget. Physical memory-controller integration requires a fresh fit.

The earlier two-round area baseline remains frozen at
`D:/github/DigitalDesign-code/target/postquad-composition-20261010/review.json`.
Its reference9148/selected8030 Logic comparison and complete36-scenario matrix
below are historical measurements. They are not measurements of the current
throughput source. The independent checked/reference emitters remain available.

## Frozen throughput checkpoint calendars

The Runtime cache uses the existing ordered Pool's second head to pay the next
tag lookup on the current data-read edge. Separate pending-read and captured
result ownership permits simultaneous read/capture/consume. The new72-bit
pending payload is not a deeper work queue. Paid BSRAM returns remain owned and
can be captured under CE0; foreign denial and downstream backpressure do not
overwrite an occupied result. Miss descriptors retain their ordered Pool lease.

Actual cache/Color acceptance is Group II1. The complete native seam trace has
2548 reads and2548 Color inputs, with all2547 successive intervals equal to one.
Both stages remain continuously ready over the displayed24-edge trace. The
independent complete framebuffer image and all1024 Final returns still pass.
Raw trace and source identity are linked by the checkpoint receipt.

Final shares three multipliers across tint*texture, specular*h and base*g.
Join launches directly from the paid Basic return and co-issues its paired
depth read. RGB storage releases only on the real sink color-row ACK;
`color_paid` retains the witness while depth waits for its own ACK. Global
ownership still retires on actual row7. Borrowed g/h remains live through its
real final arithmetic use, including CE0. Final currently accepts one covered
lane every four enabled edges; the three product phases impose an arithmetic
lower bound of three, so this implementation cannot achieve pixel II2.
Four full lanes plus the current quad boundary cost19 edges/quad.

ROP prepares the second existing immutable Output2 slot during the prior quad's
tail, using dead old operands and existing synchronous source reads. Overlap
requires the same resident tile and distinct aligned quad origins. All prior
results and bank writes precede owner handover; same-pixel dependencies,
cross-tile accesses and misses retain ordered fallback. ROP-only resident
commits alternate12/19 edges for independent quads:31 edges per pair. This is
a separate leaf calendar; the complete chain remains join-bound at19.

The seam pressure ROM produces4.75 Groups/pixel. The uniform spatial ROM
produces2592 Groups/1024 pixels =2.53125 Groups/pixel, with no texture or
framebuffer refill. Its complete-chain commit window is also19 edges/quad,
or4.75 edges/pixel. Ordinary two-mip sampling still targets2 edges/pixel and
spatial cross-tile sampling approximately2.53; these sampling demand limits
do not establish whole-chain throughput. Next priority is Final/join's four-edge
lane period and quad boundary, followed by the serialized ROP fallback.

## Frozen throughput checkpoint performance and qualification

The checkpoint receipt contains bare, unlit bilinear, lit untextured, full two-mip,
full seam and cold seam runs plus the uniform spatial draw. All full-coverage
steady windows measure19 edges/quad across127 actual commit intervals. Ordinary
hot throughput improves21.05% and seam pressure hot100.50% at the same clock.
At54 MHz the simulated resident rate is11.368 Mpixel/s. Prime, first dirty
commit, source completion, drain and flush remain separately reported.

Native and portable mixed Bare/Texture/Lighting/Full sparse seam draws pass
complete memory gold,545 real Final outputs and CE0 paid-return checks. Cold
and hot seam also pass with transport/CE stalls. Cache-only qualification covers
192 Groups,129 refills, backpressure, foreign denial, CE0 and malformed/refill
fault paths in both backends. ROP qualification covers80 quads, same-pixel
dependencies, eviction, depth/blend modes, paid fault drains and a late owned
fault. No capacity is enlarged. Finite watchdogs remain in every HDL fixture.
This targeted qualification does not rerun or re-label the old36-case matrix.

## Historical area-baseline 36-scenario matrix

This section describes the earlier source frozen in
`target/postquad-composition-20261010`, before the throughput changes. Its
timestamps and resource comparisons remain historical; current measurements
are in `target/postquad-throughput-20261010`.

Every row passed the independent complete 65536-byte image and 1024 Final RGB
checks. Textured nearest and bilinear one-tile cases have 1024 Group handoffs; two mip
cases 2048, seam cases 4864. Hot texture means zero measured texture refills;
hot framebuffer means zero measured framebuffer reads. Request counts below
include final dirty flush. A texture-hot/framebuffer-cold primer can leave an
additional dirty off-target tile, explaining write counts beyond eight.

Times are wall-edge offsets from the first measured source offer, excluding
the real primer. First commit is a dirty-cache commit, not external visibility.
Visible end requires actual flush and transport terminal success. Steady II
uses 127 commit intervals; it is not total time divided by 256.

| Case | Mode | Footprint | Tex/FB | First commit | Steady edges/quad | Visible end | Requests Tex/read/write |
| --- | --- | --- | --- | ---: | ---: | ---: | --- |
| 00 | unlit + untextured | nearest | -/cold | 279 | 23.000 | 6372 | 0/8/8 |
| 01 | unlit + untextured | nearest | -/hot | 54 | 23.000 | 6147 | 0/0/8 |
| 02 | unlit + texture | nearest | cold/cold | 358 | 23.000 | 6451 | 1/8/8 |
| 03 | unlit + texture | nearest | cold/hot | 133 | 23.000 | 6226 | 1/0/8 |
| 04 | unlit + texture | nearest | hot/cold | 331 | 23.000 | 6649 | 0/8/16 |
| 05 | unlit + texture | nearest | hot/hot | 105 | 23.000 | 6198 | 0/0/8 |
| 06 | unlit + texture | bilinear | cold/cold | 358 | 23.000 | 6451 | 1/8/8 |
| 07 | unlit + texture | bilinear | cold/hot | 133 | 23.000 | 6226 | 1/0/8 |
| 08 | unlit + texture | bilinear | hot/cold | 331 | 23.000 | 6649 | 0/8/16 |
| 09 | unlit + texture | bilinear | hot/hot | 105 | 23.000 | 6198 | 0/0/8 |
| 10 | unlit + texture | 2 mip | cold/cold | 394 | 23.000 | 6487 | 2/8/8 |
| 11 | unlit + texture | 2 mip | cold/hot | 169 | 23.000 | 6262 | 2/0/8 |
| 12 | unlit + texture | 2 mip | hot/cold | 335 | 23.000 | 6653 | 0/8/16 |
| 13 | unlit + texture | 2 mip | hot/hot | 113 | 23.000 | 6206 | 0/0/8 |
| 14 | unlit + texture | seam/wrap | cold/cold | 618 | 38.094 | 10480 | 11/8/8 |
| 15 | unlit + texture | seam/wrap | cold/hot | 263 | 38.094 | 10361 | 11/0/8 |
| 16 | unlit + texture | seam/wrap | hot/cold | 344 | 38.094 | 10274 | 0/8/16 |
| 17 | unlit + texture | seam/wrap | hot/hot | 126 | 38.094 | 10056 | 0/0/8 |
| 18 | lit + untextured | nearest | -/cold | 326 | 23.000 | 6419 | 0/8/8 |
| 19 | lit + untextured | nearest | -/hot | 101 | 23.000 | 6194 | 0/0/8 |
| 20 | full | nearest | cold/cold | 358 | 23.000 | 6451 | 1/8/8 |
| 21 | full | nearest | cold/hot | 133 | 23.000 | 6226 | 1/0/8 |
| 22 | full | nearest | hot/cold | 331 | 23.000 | 6649 | 0/8/16 |
| 23 | full | nearest | hot/hot | 105 | 23.000 | 6198 | 0/0/8 |
| 24 | full | bilinear | cold/cold | 358 | 23.000 | 6451 | 1/8/8 |
| 25 | full | bilinear | cold/hot | 133 | 23.000 | 6226 | 1/0/8 |
| 26 | full | bilinear | hot/cold | 331 | 23.000 | 6649 | 0/8/16 |
| 27 | full | bilinear | hot/hot | 105 | 23.000 | 6198 | 0/0/8 |
| 28 | full | 2 mip | cold/cold | 394 | 23.000 | 6487 | 2/8/8 |
| 29 | full | 2 mip | cold/hot | 169 | 23.000 | 6262 | 2/0/8 |
| 30 | full | 2 mip | hot/cold | 335 | 23.000 | 6653 | 0/8/16 |
| 31 | full | 2 mip | hot/hot | 113 | 23.000 | 6206 | 0/0/8 |
| 32 | full | seam/wrap | cold/cold | 618 | 38.094 | 10480 | 11/8/8 |
| 33 | full | seam/wrap | cold/hot | 263 | 38.094 | 10361 | 11/0/8 |
| 34 | full | seam/wrap | hot/cold | 344 | 38.094 | 10274 | 0/8/16 |
| 35 | full | seam/wrap | hot/hot | 126 | 38.094 | 10056 | 0/0/8 |

All raw timestamps, prime/source/drain/flush intervals, refill/write beat counts,
peak credits and coordinate/Group counts are in `performance.csv/json` beside
review.json. Each clean ordinary scenario has23 steady edges/quad (5.75/pixel).
Changing Lighting/texture modes changes first-result latency but does not change
that downstream resident limit. The baseline was 30 ordinary edges/quad;
the area-baseline 23 is 30.43% more steady pixel throughput at the same clock.
The seam reference was38.063; area-baseline38.094 is essentially unchanged (about
0.08% slower in this window), and has no claimed seam speedup.

Native mixed Bare/Texture/Lighting/Full sparse seam qualification also passes:
545 Final outputs, branch bypass/frontier shadow agreement, source stalls,
CE0 paid reads and terminal transport. A portable full-cold CE/stall run passes
the same independent byte/color checks. These are adversarial qualification
cases, separate from the clean throughput matrix. Each HDL run is bounded by
200000 wall edges and a 2ms simulation watchdog. The scoped GPU library clippy
and touched-source formatting pass; no unrelated CPU/system or board tests run.

## Historical two list-first area rounds

The two predeclared lists and source-labelled complete fits are in
`round1-plan.json`, `round2-plan.json` and `experiments.json`. Estimates were
selection hypotheses; the following decisions use complete matched results.

| Round | Experiment | Complete-top finding and decision |
| --- | --- | --- |
| 1 | Four-stage exact Final | Standalone Logic increased54 despite451 fewer registers; ordinary calendar30→28. Kept the short arithmetic idea, superseded by shared/single Final. |
| 1 | Closed source-depth loan | Matched reference9148→9088; folded+loan8974/4803. This combination saves174 Logic/463 registers but fails 54 MHz. Paired source later replaces the loan. |
| 1 | Co-issue join depth | No additional28-edge throughput, timing fails; retained only as an experiment. |
| 2 | Aligned closed bank geometry | Lane XOR bank mapping and common beat address remove variable permutation muxes. Shared/bank/frontier/raw combination8226 Logic passes54. |
| 2 | Ordered completion frontiers | Two completed-prefix records replace two32-bit indexed done maps; actual mixed bypass, wrap and independent shadow checks pass. Selected. |
| 2 | Tail domain / raw proposals | Separating proposals from actual commits breaks preflight-to-enable feedback; first54 MHz full-chain pass. Selected with atomic actual row7 ACK. |
| 2 | Threshold rounding | Worse than folded-depth, not selected. |
| 2 | Three shared Final products | Alone Logic worsens; with closed bank/frontier/raw gives8226. Selected architecture, DSP falls to3 MULT9X9. |
| 2 | Reuse old/results arrays | Saves 128 FF but matched Logic increases 229. Rejected. |
| 2 | Single result and borrowed g/h | Captured8183→borrowed8076; removes 42 input captures and deep result queue. Selected, later direct publish. |
| 2 | Remove repeated sink ingress guard | Logic8168 and52.936 MHz; worse than checked8076. Rejected; ingress checks retained. |
| 2 | Output row BSRAM | Adds 1 BSRAM, removes 8 RAM16/32 FF, but increases 31 Logic. Rejected. |
| 2 | Narrow RGB565 quantization | Exact512-value exhaustive scalar proof and actual all-mode ROP qualification; matched8076→8054. Selected. |
| 2 | Late old-color transport alone |8124 Logic/51.133 MHz. Rejected alone; stream uses the qualified late-value lowering as part of its source architecture. |
| 2 | Hard descriptor head |8024 Logic but48.699 MHz. Rejected. |
| 2 | Paired output source reads | Removes eight-row snapshot. With borrowed Final7894 Logic but40.547 MHz, so that variant is rejected. |
| 2 | Final direct publish + paired reads |8030 Logic/4820 FF, both layouts54 MHz; actual steady23 edges/quad. Selected. |
| 2 | Paid6-bit descriptor control capture |7988 Logic, but51.586 MHz and25 edges/quad. Rejected. |

The historical area-baseline whole-profile reduction is12.22% Logic. Gains are non-additive:
cross-module synthesis and timing packing change when controls or RAM ports
change. Fewer stored bits are not a sufficient area criterion.

The historical area-optimization window starts04:57 UTC on2026-10-10;
its elapsed time remains recorded in the old review.json. The subsequent
throughput work has a separate receipt and source, rather than overwriting
that completed measurement window.

## Reproduction and integration boundary

Export current native RTL with
`gpu-v2-postquad-export <output-path> selected`; append `portable` for portable
RTL. Current native SHA256 is
`fb828c7a51cc20a06be51043427f1a141982b7b990d58f0cf43a928d0b47dd02`.
The fresh selected export, qualified native source and both fitted snapshots
match. The old baseline SHA256 `863ae2e01df4fdc0dc291e2ab497cf5f5cfb2c5a1f8becc5a531c6241472d8d5`
belongs to the archived area baseline, not today's selected export.

The exporter fixes one texture slot at0x1000/maxN6/full mip and a512x256 surface
at color0x100000/depth0x140000. Other surfaces/assets need their own qualification.
Set `GOWIN_HOME`, `IVERILOG_EXE` and `VVP_EXE`. To replay the qualified frozen
source, set `POSTQUAD_RTL_SOURCE` to its native/portable path and run ignored
`actual_postquad_rom_performance` through `scripts/run-cargo.ps1`.
`POSTQUAD_CASE` selects the workload; `POSTQUAD_STALL`, `POSTQUAD_SPARSE`,
`POSTQUAD_MIXED` and `POSTQUAD_PORTABLE` select directed qualifications.
`POSTQUAD_SPATIAL` supplies the measured uniform spatial footprint and its real
hot primer. The current receipt hashes raw logs, source snapshots and fit reports.

The selected checkpoint is committed on 60c7
`dev/pixel-composition-20261010@ee400a8`; the subsequent explicit candidates,
including `cadence`, form the following worktree checkpoint.
Main `code-v0.3-dev@9ae1f27` remains clean and unchanged; f207's five original
Final WIP file hashes remain preserved. No commit, merge or board programming
was part of the qualification runs. No live rasterizer/interpolator, command
processor or render/display ownership is included. The upstream protocol
remains a [draft](quad-output-protocol-draft.md) for the GPU front half.

## Fixed Final/ROP cadence candidate

Export the newer closed-draw specialization with
`gpu-v2-postquad-export <output-path> cadence`; append `portable` for portable
RTL. `postquad::verilog_cadence_candidate` starts from the independent SH9
reference and applies one common two-phase issue/consume calendar. It admits
a new pixel every two enabled edges. Final tint/base multipliers and the shared
Final-specular/ROP-delta bank each retain one enabled-edge arithmetic latency.
Global CE freezes their results and phase together; paid memory traffic continues
on wall-clock edges.

Final retains eight integer bits per specular term and a ten-bit saturating sum.
Sampling keeps its existing Q8.1 pair rounding, conserved /511 projection and
RGB8 output; texture cache remains RAW565. RGB565 quantization, alpha6 endpoints,
coverage, depth, addressing and transport are unchanged. Final RGB8 can differ
from SH9 by at most one level; the representative smooth, textured and dark
transparent picture study is a scalar study, separate from actual RTL proof.

ROP uses four enabled stages, ordered return identity and phase-held target color,
depth and flags. The arithmetic bank has fixed phase ownership, with no arbiter
or per-client saved-result bank. Final's local three-state arithmetic phase is
replaced by one live bit plus the common phase. Its single output buffer remains:
removing it by stopping the whole chain creates a cache/depth dependency cycle.
Raw proposal validation remains independent of commit gating.

Actual native hot, cold/stalled and sparse/stalled chains and portable hot/cold
chains match independent reduced-precision Final arithmetic, all 65536 bytes of
reconstructed DRAM and ordered commits. Raw faults, CE0, reset, abort and paid
memory draining are qualified. Both matched native placements pass54 MHz. The
same complete-chain steady hot window remains8 edges/quad; cold service retains
its actual memory stalls. Resource totals, source identities, original receipts,
rejected alternatives and pictures have one home in the source-owned follow-up
report at
[historical exp-190](experiment-review.md#reproduction-and-historical-boundaries).
The default `selected` and explicit `sh9` factories remain independent reference
exports. This qualification covers the post-quad chain, not the whole GPU or a
physical board image.

## Combined h9 and storage candidate

`gpu-v2-postquad-export <output-path> combined-prehead-exact` exports the explicit
complete-chain integration candidate; append `portable` for its portable backend.
It composes [direct h9](lighting.md#direct-h9-closed-draw-calendar),
[continuous preparation and integer filtering](texture.md#explicit-continuous-preparation-and-integer-filter-candidate),
and an ingress-owned canonical quad description. The old `selected`, `sh9` and
`cadence` exports remain reference choices.

The canonical32x23-bit description is written once on the accepted private row0
and becomes visible only with row7 publication. Separate branch control records
and the active execution header remain necessary lifetime views. The old row7
payload copy and one redundant read selection disappear. The next head RAM
address is prepared from exact raw sink readiness before the global commit gate;
actual header capture, publication and retirement still use terminal commit.
This changes no RAM read-port count, capacity, ownership or external transaction.

Proposal validation and terminal commit are separated. A sibling sticky fault
no longer feeds local abort-input feedback; top fault still blocks every new
commit immediately. Already presented or paid transport keeps its existing
wall-clock owner and drains through CE0 and terminal faults. No fault is delayed
and no diagnostic ownership check is removed.

The complete same-source native candidate passes both54 MHz layouts with zero
setup and hold violations. Timing margin is narrow. Native and portable hot,
cold/stalled and sparse/stalled scenarios independently check Color, Final,
65536-byte DRAM, canonical header identity and ordered commits. The steady hot
window remains8 enabled/wall edges per quad, or2 per pixel; miss/paused windows
retain their measured stalls. Actual numbers, source identities, rejected
individual alternatives, exact read-address proof and visual studies have one
home in [the combined report](../../../.agent/projects/cpu-v3-tang-nano-20k/records/experiment-consolidation-20261015/catalog.md#exp-055).
This is complete post-quad evidence including texture/framebuffer caches and
shared memory ownership, excluding the GPU front half and physical board.
