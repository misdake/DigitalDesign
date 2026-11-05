# GPU regression tests

The ignored `common_profiling_collectors_observe_the_same_draw_edges` and
`common_profiling_reset_preserves_the_same_stop_boundary` in
`vliw_cycle_gpu` and `common_profiling_uv_and_abort_collectors_use_identical_points`
in `vliw_cycle_draw_uv` feed the same finite real GPU inputs to the emu and RTL
collectors. They retain every-edge DUT output checks and observation-on/off state
parity. The profiling bench uses the same nominal54MHz clock as its manifests;
the generic functional replay bench remains unchanged. The system profiler's
`check_common_events.py` verifies that bench clock and compares timestamps,
handshake fields, generations and shared model ownership, including CE/retries,
Sampling wrap and stopped/open owners. `check_native_points.py` separately
compares actual backing mutations and raw completion planes; different MC init
or pin-clock phases are not silently shifted into timing equivalence.

Fixed D16 geometry checks retain all unsigned comparisons, depth write gating,
inclusive quantized far and rejection of retired Global selection tags. CP and
Prepare use the original full far and fixed bias; their existing bounded RTL
checks retain completion, CE and stall coverage.

## Complete command GPU cycle composition

`vliw_cycle_aux_control`, `vliw_cycle_aux_ui`, `vliw_cycle_aux_tdp` and
`vliw_cycle_aux_native` qualify CLEAR, UI_GRID/range and MEMCPY against actual
production modules and one native backing. Explicit ignored tests compile real
DUT mutations and whole GPU roots; set Icarus/Gowin paths. `vliw_cycle_draw_uv`
adds full-command mixed RGB/UV/lit/UI/copy runs. See the
[implemented composition and bounds](/ip/gpu-v2/docs/references/cycle-auxiliary.md).

## Production command transport cycle model

`cargo test --release -p gpu-v2 --test vliw_cycle_transport -- --include-ignored --nocapture --test-threads=1`
runs the bounded two-job DMA/router checks, native arbiter/endpoint/gear/MC
closure, short production RTL replay, actual DUT mutations and finite host
fixture samples. Set `IVERILOG_EXE` and `VVP_EXE` for the explicit ignored checks.
No full GPU command/compute/Display implementation is claimed; CPU and Display
masters are idle and board GPU request-slot stages are outside the native fixture.
See the [implemented contract](/ip/gpu-v2/docs/references/cycle-transport.md).

## Production command-control subset

`cargo test --release -p gpu-v2 --test vliw_cycle_command -- --include-ignored --nocapture --test-threads=1`
checks CE0 Device4 submission, two CBs without ACK, native DMA/cache-bank reuse,
synchronous RAM, WAIT/FLUSH/END, abort/reset and malformed input contracts.
The explicit ignored qualification compares the original small production
control cone and detects actual DUT mutations, with isolated Device4 sticky
checks. Render/flush/source-ready peers are controlled; no DRAW/full GPU,
CPU submission path, Display or board timing is qualified.
See the [subset contract](/ip/gpu-v2/docs/references/cycle-command.md).

## Production input and Vertex cycle subset

`cargo test --release -p gpu-v2 --test vliw_cycle_vertex -- --include-ignored --nocapture --test-threads=1`
checks compact V96 storage and metadata, old ROM/RF/MAC/Q, all-vertex result
writes, bounds, ownership, warm reset, explicit unsupported T, and native
Memory-fed double-page/cache overlap. The ignored short qualification requires
`IVERILOG_EXE` and `VVP_EXE`; it replays production Vertex and DMA/router cones,
compares known bits, and detects compiled DUT mutations and illegal peers.
No complete DRAW/T pipeline, CPU/Display contention or board timing is claimed.
See the [subset contract](/ip/gpu-v2/docs/references/cycle-vertex.md).

## Production indexed T16 cycle extension

`cargo test --release -p gpu-v2 --test vliw_cycle_triangle -- --include-ignored --nocapture --test-threads=1`
checks real index reads, shared production T32 arithmetic, all sixteen exported
fields, default16-slot fill/wrap and old-full credit, three-meshlet epoch lifetime,
reset, illegal peers, actual DUT mutations and native Memory-fed inputs.
Set `IVERILOG_EXE`/`VVP_EXE` for explicit ignored RTL qualification. The sink only
provides ready and checks accepted offers; no functional rendering fallback,
complete GPU/CPU/Display composition or board timing is claimed.
See the [extension contract](/ip/gpu-v2/docs/references/cycle-triangle.md).

## Production DRAW preparation and interpolation

`vliw_cycle_draw` qualifies explicitly enabled installed-state/DRAW command
control using controlled render/flush peers; see its [contract](/ip/gpu-v2/docs/references/cycle-draw-control.md).

`cargo test --release -p gpu-v2 --test vliw_cycle_prepare --test vliw_cycle_context --test vliw_cycle_compute --test vliw_cycle_interpolator -- --include-ignored --nocapture --test-threads=1`
checks private PA/PB/PC and DSP arithmetic, empty bounds, geometry replay,
inactive-page writes, old-I drain and actual four-phase interpolation/analytic
LOD. Explicit ignored RTL entries require `IVERILOG_EXE`/`VVP_EXE` and include
actual DUT mutations and the original sample-before-switch monitor. Raster and
pool remain controlled peers in this fixture. No complete DRAW, native-memory
rendering, pixel backend or board timing is claimed. See the
[implemented contract](/ip/gpu-v2/docs/references/cycle-draw-interpolation.md).

`vliw_cycle_raster` and `vliw_cycle_pool` provide the explicit ignored portable
RTL qualification for the actual walker/scanner and command pool. Run with
`--include-ignored --nocapture --test-threads=1`; they require `IVERILOG_EXE`/`VVP_EXE`.
`vliw_cycle_frontend` and `vliw_cycle_producer` check the live indexed G3-to-pool
Rust wiring with finite downstream peers. Their scope is described in the
[producer contract](/ip/gpu-v2/docs/references/cycle-draw-producer.md); none of
these entries executes a complete pixel backend or physical framebuffer write.

`vliw_cycle_pixel_math` and `vliw_cycle_tdp` compare the selected shared
Final/ROP arithmetic, four-bank framebuffer storage and wall maintenance with
production RTL. Their explicit ignored cases include actual DUT mutations and
original contract monitors. `vliw_cycle_render` connects real G3/R/I/pool,
join, Final and TDP for a small RGB/unlit triangle, with independent constant
color/coverage and whole-backing checks. Its ignored case replays the actual
current GPU compute root, including the vendor model for the fixed native
Group35 bank. Set `IVERILOG_EXE`, `VVP_EXE` and `GOWIN_HOME`; all entries have
finite edge/process bounds. CP, DMA loading and framebuffer sector transport
remain peers here. See the [pixel contract](/ip/gpu-v2/docs/references/cycle-draw-pixel.md).

`vliw_cycle_gpu` drives two real CP/DMA RGB command buffers through one native
arbiter/controller/pins model and checks partial physical writes and the full
backing after each END. Its ignored positive case compares the actual command
GPU RTL root with that native feedback tape; the MC stays outside the RTL DUT.
Its ignored negative case checks target wiring, second-submission validity and
the original busy COUNT monitor. Its RGB/lit case connects actual pool tuples
and Lighting with installed ambient mode and compares the whole command root;
Sampling remains a peer. `vliw_cycle_lighting` separately checks full H9 against
independent stage integers and portable/vendor-DSP RTL, including live globals,
stalls, masked lanes, warm reset and three compiled mutations. See its
[contract](/ip/gpu-v2/docs/references/cycle-draw-lighting.md).

The unignored `passive_` cases in `vliw_cycle_gpu` and `vliw_cycle_draw_uv`
repeat finite captured inputs through ordinary and observed clocks, comparing
every original output and final state with a 200000-edge limit. They export one
preimage, exact native writes, sparse GPU facts and independent completed-plane
oracles under `target/frame-reports`. RGB, UV publication wrap, abort, mixed
UI/copy/clear and reset target invalidation are covered. The system profiler's
`check_actual_gpu_journal.py` checks owners/conservation and lossless RLE;
`check_actual_native_images.py` checks full native color/D16 and re-rendered assets.
These are finite component facts, not whole-system frame timings.
`vliw_cycle_command_trace` separately checks actual command latch/Exec retries,
accepted issue/CP release/drain, physical PC wrap and abort/reset boundaries.

`vliw_cycle_sampling_direct` checks POT/RTT source-Q coordinates and six-row
credit/lifetime against independent integer layout and both actual factories.
`vliw_cycle_sampling_coefficient` checks the selected three-product arithmetic,
sole-bank weight write ports and mask/metadata publication. Both include finite
compiled mutations and original monitor rejections. See the
[Sampling leaf boundary](/ip/gpu-v2/docs/references/cycle-draw-sampling.md).
`vliw_cycle_sampling_packet` covers actual Member/Work16/two-edge Packet and
registered native Group35/H15 transport. Geometric references cover POT/coarse
and RTT, full old credits, ring wrap and warm reset; finite mutations and
original monitors exercise its last-consumer and physical-address boundaries.
`vliw_cycle_sampling_color` checks independent per-tap half-up RAW565 sums,
the two-edge cache DO loan, Public16 credit/reuse and direct result-bank writes.
Portable and vendor six-DSP/ALU54D RTL expose all retained known bits; finite
mutations and original completion/ownership monitors cover the boundaries.
`vliw_cycle_sampling_cache` checks physical32-line/two-sector fills, the sole
128-row weight bank, paid pins, wall returns and borrowed texel DO. Independent
memory geometry, both RAM profiles, five abort phases and invalidation with a
held paid pin have bounded RTL checks; five mutations and three original
monitors exercise these resource boundaries.
`vliw_cycle_sampling` connects all selected sampler owners and compares
independent integer pixels for both RAM profiles, all three filters/full mip
fractions, periodic small mip padding, RTT and Work16 pressure.
`vliw_cycle_draw_uv` executes two Device4 CBs with actual RGB then UV/lit,
nonidentity H9, crossing texture fills, depth and Q6; full native backing checks
include both planes and guards. Repeated draws exercise the fifth source bit
and public31-to0 wrap. Complete production CP-to-router DUTs use native feedback;
their separate60-second simulator budget complements leaf30-second bounds.
The CE0 abort case observes physical drain while logical pool tokens remain.

## Native functional backend

`cargo test -p gpu-v2 --test functional` checks bounded current-CB memory/UI/error
execution. The preserved `host/component_oracles.rs` tests compare independent
Lighting, Triangle and framebuffer references; they do not execute the retired
whole-renderer composition.

Build `--bin gpu-v2-sim` and export the actual GPU+MC with the system
`gpu_profiler_export <directory> --mc` example. Then run
`python ip/gpu-v2/tests/functional_replay.py --rtl <exports> --executable <exe>
--out <fresh-directory> --cases <names>`. Use the repository cargo wrapper for
build/test commands. Run RTL serially to keep finite wall limits meaningful.

Each vector freezes an independent native CB and full physical pre-memory,
executes both backends, and compares409600 raw color/depth bytes including
padding, declared copy outputs and other-memory guards. Negative/half normal16
and normal18, vertex MAC wrap, Q6 alpha endpoints, all eight depth functions,
POT/full-mip/fixed-framebuffer Sampling and native UI ordering have separate
vectors. `classic-floor-two-bunny` uses the current production asset packer,
scene.json camera0, one floor and the first two bunny meshlets. These are explicit
fixtures, not captured CPU submissions or historical/board frame acceptance.

The GPU watchdog is2000000 clocks; tool wall limits are180 seconds for short
vectors and600 for the dense classic input and32-draw depth matrix. Native work
is bounded at64000000 with a120-second process limit. `--reuse-rtl <previous>`
rechecks saved actual outputs only after the original input and exported-source
SHA checks; it does not claim a new DUT run. Public RCC submit/wait/CB replay,
three UI frames and host-vblank/Display interpretation are tested in the system
crate's `host/gpu_functional.rs`.

`functional_parallel.py --baseline <frozen-accepted-sim> --candidate <new-sim>
--out <fresh-directory>` reuses these20 independent inputs without running RTL.
It compares all8MiB, access flags, completion effects and diagnostics at1/2/4/8
threads. Both binaries and each input are hashed; successful physical results
are compressed, while failures keep their loose diagnostic files. Each process
has a30-second wall bound and64000000 work units.

The native geometry unit tests separately compare optimized execution with the
original240-row traversal, including overlapping blended triangles, framebuffer
texture feedback, unknown reads, denied writes and work exhaustion. They compare
full memory/access, errors, partial effects and successful work counts in local
one/four-thread pools.4096 deterministic quantized half-plane sets check that
row bounds never exclude an original covered span, including horizontal,
vertical, empty and top-left boundary cases.

The opt-in `functional-batch` feature adds a true multi-draw tile prototype.
Build both a separately frozen accepted CLI and a feature-enabled candidate.
`functional_batch.py --baseline <accepted> --candidate <candidate> --input
<full-scene-directory> --out <fresh-directory>` mutates the supplied105-command
input into bounded budget/unknown-read/denied-write/state/feedback/CB-alias and
DMA-target cases. It compares exit/diagnostics and all memory/access/completion
bytes, and checks telemetry so a successful ordinary fallback is not mistaken
for a normal batch success. Every process has a60-second wall limit.

The feature's `empty_draw_events_and_later_pixel_free_state_are_both_preserved`
unit checks two no-pixel DRAW bindings plus later Render/Global updates without
another DRAW, including final frontend state and exact successful work count.
Two explicit ignored diagnostics accept `GPU_SIM_SHARED_INPUT` (memory-before,
access-before and expected memory-after), `GPU_SIM_SHARED_COUNT`, and a fresh
output path. `shared_prepared_rebin_comparison` uses
`GPU_SIM_SHARED_REBIN_OUT`; it reuses the same in-memory StateChange/Pixels input
for A/B/C/D at1/2/4/8 threads and compares every tile message. The separate
`isolated_stage_diagnostic` uses `GPU_SIM_STAGE_DIAGNOSTIC_OUT` and verifies its
materialized geometry/shade output against fused samples. Both use64000000 work
units; run only the named ignored test, not the entire ignored suite.

Pure host regressions share the `host` integration executable. Their component
modules remain independently filterable, for example:

```powershell
cargo test --release -p gpu-v2 --test host lighting_scalar::
cargo test --release -p gpu-v2 --test host texture_coordinate::
```

RTL/qualification tests keep their independent integration targets and explicit
ignored-test entry points. Tests whose private fixture depends on its original
crate-root context also remain separate. Shared host fixtures are loaded once
by `host.rs`, rather than compiled independently in every component module.

`live_global_gold::export_live_global_profiles` writes ten actual native/portable
factory exports to `GPU_LIVE_GLOBAL_EXPORT_DIR`. Run it through
`scripts/run-cargo.ps1` with `-p gpu-v2 --test live_global_gold
export_live_global_profiles -- --ignored --nocapture`. Export a fixed accepted
revision separately when making an equivalence comparison; do not use current
source as its own baseline.

`live_global_profiles.py --exports <current> --base-exports <accepted> --output
<fresh-directory>` compares every ordered module token in simulation and
`SYNTHESIS` modes, checks private-profile scope including a subsequent generic
export, and checks generic installed-context/CE0/held-result behavior on both
backends. It explicitly restores the older caller-visible command-port/DSP
defines before selecting the standalone generic profile, while leaving the
private live-global/trusted-frontend scope under test. Each context simulation
has an 800-clock watchdog and each tool process a 60-second limit.

`direct_global.py --base <accepted-command.v> --candidate <current-command.v>
--backend native|portable --output <fresh-directory>` checks four successive
contexts through the real SEND/CP/frontend/Lighting/Final/memory path. It compares
every captured wall-cycle event, accepted Lighting input/result and framebuffer
byte, with CE, output and memory stalls. `--interrupt reset|abort-reset` checks
owned work and subsequent resubmission. The default original signed16 projection
inputs remain a DUT-equivalence case: CP does not reject those magnitudes.
`--projection-fixture oracle-domain` supplies a separate, narrower-domain family
for `live_global_gold::actual_live_global_samples_match_independent_stage_oracle`,
whose `GPU_LIVE_GLOBAL_GOLD_INPUT` points to its captured `base` or `candidate`
directory. That oracle uses independent stage arithmetic and mathematical H9
endpoints, without reading RTL or generated ROM values and without relaxing its
domain or numerical assertions. Program and memory hashes distinguish the
families even when their command words coincide.

`--early-global` expects the existing CP monitor to reject a missing-WAIT update
at the same edge in both exports. `--synthesis --mutation far|clock` instead
changes one actual candidate data/clock owner and requires result-conservation
or watchdog rejection with production monitors disabled. These are test-only
mutations; the ordinary candidate must pass. `--reuse-base <previous/base>` is
optional and requires byte-identical RTL, bench, memory and compile mode plus a
previous PASS. All four-context simulations have a 20,000-clock watchdog and
60-second compile/run limits; short fixtures do not qualify a full scene or board.

The lighting corpus uses pairwise normal/position/direction combinations and
shininess endpoints. Dedicated scalar/system checks retain every shininess code,
signed and prescale boundaries, degenerate half vectors, normal/light
cancellation and fixed-seed random/highlight samples. All stage equality and
error limits remain assertions. Dense historical characterization sweeps are
not part of routine regression, and the resulting sample reports are not new
exhaustive numerical qualifications.

The framebuffer-cache protocol tests retain all six independent cases, complete
byte-image checks, CE pauses, beat backpressure and transport/compute failures.
They compile once per geometry within each test process and replay independent
input files through that executable. Geometry, buffer limits and the finite
watchdog are checked before each replay; no compiled artifact survives as a
trusted cache across runs.

`command_dma_stream.py` checks scratchpad writes, completion events, CE pauses,
request holding, malformed responses and stop/drain behavior. Its maximum-size
mesh case queues a second job before retirement and inserts command-cache DMA
between payload bursts. All128 payload qwords and both independent destinations
are checked; each replay has a5,000-clock watchdog and a20-second process limit.

`command_dma_stream.py --export <command.v>` and
`framebuffer_router_stream.py --export <command.v>` bind the complete actual
command factory rather than raw generic leaves. `--no-monitors` selects the
same synthesis datapath; `--trace` saves wall-edge ports, return tags, cursors
and completion events. DMA includes stop before an offer, while presented,
before the first response, during data and on the final response. Earlier
writes survive; no stopped operation publishes success. The generic entry
retains malformed/error rejection cases.

`transport_profiles.py --export <command.v> --output <new-directory>` checks
command monitor failures and monitor-free ownership on error-only, real/early/
missing/late LAST, orphan/wrong-direction responses, invalid descriptors,
underprovided writes, premature ACKs and texture overrun. It also fills both RO
credits with DMA/texture owners, holds a third offer, and accepts it on the old
owner's terminal. `--generic --export <memory.v> --dma <command_dma.v>` retains
the checked controls, including optional CONTROL=0/1. Each case has a250-clock
watchdog and20-second process limit; compile processes have a30-second limit.

`transport_equivalence.py --before <frozen-exports> --after <exports>
--output <new-directory> --traces` compares every complete ordered preprocessed
stream in monitor-on/off modes, verifies closed private scopes and public-port
independence, and checks every qualified saved trace byte-for-byte. The input
directories include native/portable command and generic DMA/memory exports;
additional full/SDRAM/backend exports are compared too. Selected-profile
exclusions run after preprocessing rather than rejecting inactive generic
source branches. All eight exclusions have active/inactive declaration controls
in both monitor modes, including a whitespace-formatted far_z declaration.
The framebuffer continuation fixture likewise preprocesses
its actual GPU export before extracting source-profile leaves.

The ignored `texture_registered_return` Rust target exports the current command
cache and connects its native DO directly to the production two-edge Color
consumer. Eight bounded cases check native/portable RAM, one/two mip groups,
447 consecutive hit intervals at II2, per-owner weights and keyed RGB results.
Pressure cases combine CE pauses, consumer stalls, refill gaps and conflicting
ways; they also check held Q and read/refill line exclusion. Each simulation is
limited to 30,000 clocks and each process to 45 seconds.

`texture_registered_return.py` compares a current physical Group35 command export
with an approved baseline. It exercises native/portable cache banks, all64 local
origins, conflicting ways, CE and commit denial, refill stalls and held results.
The 384 returned payload/texel records must match exactly; each simulation has
a 30,000-cycle and 60-second bound.

The ignored `texture_registered_return::packed_coordinate_to_color` test connects
the production coordinate, coefficient, membership, cache and Color modules.
It checks 512 keyed, integer-reference pixels per case for bilinear, fractional
trilinear and magnification, with CE pauses. Resident one-mip traffic must sustain
II2; two-mip traffic must sustain II4. Each process is bounded to45 seconds and
each simulation to30,000 clocks. `vliw_backend_complete.py --mask-matrix` separately
checks all16 masks through the real producer pool and the complete backend:
exact UV-read conservation, no helper reads, actual first/last lane ownership,
Final/ROP results and the entire memory image, in portable and native RAM modes.

The production pipeline probe also covers a four-tile fine footprint and its
five-Group trilinear counterpart: their hit intervals remain8 and10 clocks.
The Group-Q lifecycle probe checks256 distinct35-bit words through four address
wraps, with native/portable RAM, CE pauses, all48 FIFO credits reached, held
current/registered native heads, in-flight reads, exclusive Q return/promotion,
next-key order and concurrent writes/reads. Its simulation
is bounded to10,000 clocks. `group_lookahead_overlaps_next_tag` joins the actual
Group queue and cache:96 keyed Groups check resident hits at II2 and consecutive
misses at20 clocks with a fixed16-beat memory peer, in both RAM modes. This is a
bounded cache-schedule regression, not a physical MC throughput measurement.
`direct_lod_stage1.py --production-source <gpu.v>` checks all mip sizes and
filters against integer coordinate/owner gold while honoring the producer's
U/V Q lifetime. `vliw_backend_complete.py --nearest` uses a nonuniform texture
oracle for nearest filtering; it does not substitute the trilinear reference.

`texture_registered_return::tile_crossings_preserve_nonuniform_texels_and_single_mip_quad_rate`
uses the production coordinate-through-Color path with sector-dependent RGB565
texels and an independent integer address/mip/weight/color reference. It checks
actual lambda0 versus256, fine/coarse member conservation, single-mip bilinear
and true trilinear,25%Xedge/25%XYcorner mixes, saturated rates and complete quads
offered every16clocks. Both single-mip filters must retain16clocks/quad in the
warm window; two-mip rates reflect their actual additional Groups. Output records
separate first latency, steady intervals, final drain and warm/cold queue peaks.
Each case is bounded to512pixels,30,000clocks and45seconds. The16-beat memory peer
is not a physical-MC or full-GPU performance model.

The lighting stream regression simulates the complete clean and CE-stalled
64-pixel streams independently. Numerical/date/overflow audit faults reuse the
accepted clean result, restoring the first record before each mutation; they do
not repeat identical arithmetic simulation. The input corpus, context switches,
II assertions, numerical oracle and watchdog checks are retained.

The production texture backend uses16x16 column macros and16x8 cache lines.
`test_texture16x8.py --export <command.v> --output <fresh-directory>` checks
independent mip/packet addresses, all128 local origins, asymmetric weights,
native/portable bank data, stalls, the48-credit FIFO, actual Group/cache
lookahead, child failures and abort drain. Each subprocess is bounded to60
seconds and each bench has a finite cycle watchdog. The generic numerical
sampler fixtures keep their own local format and are not native texture loaders.

`test_texture16x8.py --terminal-only --export <command.v> --output <fresh-directory>`
runs the bounded success and first/second child failure checks with `SYNTHESIS`
set, for portable and native cache RAMs. Each case has a 300-cycle watchdog.
Refills run with proposal CE disabled. The oracle checks parent FILLING/READY
or invalidation, descriptor drain, publication/consumption counts, independent
texel values and held DO. These cases also run in the normal script path;
the existing monitor-on failure and three abort cases remain separate checks.

The fixed-column RTT harnesses consume the same production`command` export.
`test_rtt400.py` checks independent coordinate/packet references and native
sampler pixels under stalls; `--exhaustive-axis` checks every 16-bit axis input.
`test_rtt400_command.py` checks producer/consumer commands, modulation, supported
Q6 blending, invalidate visibility and malformed descriptors.
`test_rtt400_replay.py --backend native|portable` consumes the actual RCC demo
fixtures: axis-x/axis-y modes check all 96000 nearest pixels, and chain mode
checks real raster UVs and source/consumer results. Each subprocess has a wall
limit and each RTL bench a finite cycle watchdog. Portable frame replay,
native submodule checks and physical board qualification have distinct coverage.

`test_rtt400_replay.py --tint R G B` checks the specified opaque uniform
multiplication in axis modes and nearest producer/consumer comparisons. Defaults
remain255. `test_rtt400_command.py` also accepts black modulation: framebuffer
coverage checks compare against zero without treating black pixels as missing.

`frontend_diagnostics.py --exports <six-variant-directory> --output <fresh-directory>`
checks each Raster/LOD/Compute diagnostic write site (3/1/4), both arithmetic
backends, command monitors on/off and generic checked bits. Directed predicates
are injected at the leaf; each bench has a 40-edge/500ns watchdog and each
process a 30-second limit. Command checks distinguish a synchronous every-edge
zero register from a constant signal and from a CE-gated clear. Generic bits
must remain sticky during CE0. `--flow-only` reuses the bounded three-draw
`current_texture_command.py` fixture and observes every output of all three
leaves plus CE before/after each edge, with monitors on/off. This qualifies
local diagnostics and legal data/cycle preservation, not new protocol recovery
or physical timing.


`frontend_control_profiles` is an ignored, bounded RTL gate exported through the
actual frontend, generic system and command factories (native and portable).
It checks private-profile scope, real queued meshlet CE/backpressure traces,
malformed header/index/reference/capacity/compact/DMA decisions, and prepare's
sticky ready/core-CE stop. Two mutations remove the actual prepare gates and
must fail hardware observations with simulation monitors disabled. The command
cases check its original simulation fatal edges and synthesized continuation.

Run `cargo test --release -p gpu-v2 --test frontend_control_profiles -- --ignored
--nocapture --test-threads=1` with `IVERILOG_EXE`, `VVP_EXE` and `GOWIN_HOME` set.
`GPU_FRONTEND_CONTROL_OUTPUT` chooses a fresh output root;
`GPU_FRONTEND_CONTROL_BASELINE` optionally supplies the same eight accepted
factory exports for full ordered preprocessed-token and per-cycle comparisons.
Each DUT has a 12000-cycle watchdog; each compile/run and the Rust parent have
wall limits. This gate does not replace scene or physical board qualification.

`coordinate_lod_native.py --exports <directory> --output <fresh-directory>`
connects the actual command export's coordinate, DirectLOD and six-row queue.
The directory supplies `command-native.v` and `command-portable.v`. Independent
integer gold covers POT mip/filter decoding and signed fixed400x240 clamp;
CE0, commit stalls, source-Q lifetime, full/wrap and simultaneous issue/return/take
are checked under both arithmetic backends and monitor settings. Six injected
DUT violations cover phase, queue overflow/return/consume, inactive lane and
premature V-source changes. Monitor-off checks diagnostic feedback stays absent;
it does not promise recovery from invalid trusted input. Each legal bench has
a 20,000-cycle watchdog, each negative a 1,000ns bound, and each process a 45-second
limit. `--baseline <prior-output>` compares the complete sampled data/address/
control trace to a frozen run. This is a bounded leaf qualification, not a frame,
physical SDRAM or board test.

`command_packet_group.py --export <command.v> --baseline <fixed-command.v>
--output <fresh-directory>` binds the selected Membership/Work/Packet/Group
owners. Independent POT/RTT vectors check Member76, metadata/masks, Packet35
publication edges, owner conservation and H15 lookahead with48-credit saturation.
Portable/native Group RAM and monitor on/off traces must match the fixed source.
The existing RTT sampler/oracle supplies a64-pixel two-filter closed chain from
coefficient weight pins through actual cache `read_issue`, with CE/refill stalls.
Twenty-eight directed detector alternatives check the original failure edge and
monitor-off state; explicit abort stays sticky without CE until reset. Four
source mutants break Membership X seams, RTT pitch, H15 or native Q OCE and must
fail their independent oracle. Each bench has a30,000-cycle or smaller bound and
each subprocess a60-second limit. `--negative-only` and `--sampler-only` select
these focused subsets. `--portable-export` and `--portable-baseline` bind the
actual portable exports for the corresponding legal traces. This is bounded
producer/consumer evidence, not a frame,
physical-memory timing or board qualification.

`maintenance_profiles.py --exports <eight-factory-exports> --baseline
<frozen-accepted-exports> --output <fresh-directory> --diagnostics` checks complete
ordered preprocessed tokens for command/full/sdram/backend, native/portable and
simulation/synthesis profiles. Each preprocessing process has a60-second bound.
The private command-maintenance define must be closed after its factory output.
Twenty-eight directed checks cover real failed ACKs, original source write-site
errors and UI lower-readiness withdrawal: command monitors stop simulation while
monitor-off diagnostic feedback stays zero, and standalone generic components
retain sticky CE0 rejection, including the public command-color-port profile.
Diagnostic benches are bounded to25cycles; compile/run limits are45/20seconds.
The generic `framebuffer_tdp_prefetch.py`'s `+BAD_ACK` replay separately checks
premature completion before payload; it is not failed-ACK recovery evidence.

`ui_grid.py`, `ui_draw_transition.py` and `memcpy.py` accept `--trace` and
`--no-monitors`. Tracing only observes the actual CP, wrapper, TDP bank ports and
router transport every edge; it does not drive the DUT. Use repeatable frozen
inputs and `maintenance_profiles.py --trace-pair <baseline-run> <candidate-run>`
(repeatable option) for byte-exact edge comparison. The existing independent
screen-space, neighbor/depth/padding and raw-byte golden checks remain active.
Their finite cycle/process bounds are unchanged. `memcpy_counter.py` retains
its separate100000-cycle/60-second2048-block cursor check.

`maintenance_mapping.py --exports <factory-exports> --output <fresh-directory>`
runs the actual command TDP for every375logical UI tile and seven raw-copy
boundary blocks, through actual accepted requests, held descriptors,64data beats
and two half-tile terminals per operation. CE stays zero throughout. Independent
integer addresses, no depth/other-line bank writes and no temporary resident
publication are checked in all four native/portable monitor modes. Every case
has a100000-cycle watchdog and each compile/run a60-second wall bound. This
component sweep complements the real MC/pin pixel gold; it is not a frame or
physical-memory qualification. Legacy generic-only fields are checked after
actual preprocessing, because inactive alternatives remain in native sources.

`command_direct_weight.py --rtl <actual-command.v> --baseline <accepted-command.v>
--output <fresh-directory>` checks the actual coefficient producer through the
unchanged cache weight-bank write port and native/portable storage. One compiled
fixture per memory/monitor mode replays bounded POT/RTT cohorts with independent
integer values, seven-bit addresses, E0/E2/E3/E4/E5 events and exact write counts.
It covers zero-parent writes, masks-only publication, legal key reuse, metadata
wrap, CE/commit/ready stalls and old-state credit/key overlap. Real DUT key,
plane, value and extra bank-writer mutations must fail; forced metadata collision
is diagnostic-only and is checked with monitors enabled and removed. The cache
request/read/retirement lifecycle and full frames are outside this fixture.

`final_policy.py --exports <eight-factory-exports> --output <fresh-directory>
--negatives` checks the actual selected Final, mul9 and shared Final/ROP bank.
One compiled fixture per checked/command, native/fabric and monitor mode replays
96 inputs with independent integer RGB gold, old two-slot credits, result order,
CE/commit stalls, simultaneous old-Q replacement and CE0 abort/sticky reset.
Illegal H and credit cases preserve checked rejection or command monitor edges;
monitor-off command behavior is compared with the frozen baseline. Four real
value, ACK-credit, compute-clock and abort-clock mutants must fail with production
monitors removed. Benches have a3000-cycle limit; each process has a60-second bound.

Add `--baseline <accepted-exports> --baseline-results <accepted-receipt.json>`
for exact stimulus/fixture and per-edge numeric/diagnostic comparisons, private
profile leakage checks and eight-profile simulation/synthesis preprocessing.
Public and command-port-only checked streams must match completely. The command
source has only two explicitly matched legacy no-op removals; every other token
must match, without generic statement, alias or declaration filtering. This is
local Final/shared-DSP evidence, not pool, TDP, frame or board qualification.

`pool_color_exports::export_pool_color_policy_factories` is an ignored exporter
for dependency-closed native/portable command, backend, full, SDRAM and public
component RTL. Set `POOL_COLOR_EXPORT_DIR` to a fresh directory and run that
exact test with `--ignored --exact`. The pre-physical Color component entry is
separate from the final command Group35 adapter: `raw-command` explicitly selects
the private Color policy before that adapter; `command` exercises the physical
Group interface. Neither replaces the other. These exports use slot max0/no-mip
and depth0x140000, while the join exporter uses max5/full-mip and depth0x200000;
keep their inputs and output directories separate.

`pool_policy.py --exports <exports> --output <fresh-directory> --mutations`
replays the existing independent Pool scoreboard on actual selected modules.
Checked, command and public data-port/global profiles cover 24/8 saturation,
32-slot wrap, sparse/empty masks, real retirement, independent enables, RGB/UV
payloads, NDC and held Q, with native/portable RAM and monitors on/off. Bounded
bad-retirement/unsafe-skip cases distinguish sticky feedback from observation;
actual DUT capacity and retire-increment mutations must fail their specific
scoreboard checks. Every simulation has a cycle bound and a process timeout.

`pool_color_equivalence.py --baseline <frozen-exports> --candidate <exports>
--output <fresh-directory> [--trace-pair <before> <after>]` compares every ordered
preprocessed token of the complete factories in both monitor modes, checks
private profile scope, and compares the frozen actual ownership edge traces.
The comparison includes both `raw-command` exports and preserves string literals;
it has no statement-removal allowance. Scanner-adapter retirement keeps complete
raw factory output identical, including the existing line-ending normalization.
These checks do not establish a new physical fit or full-frame result.

`color_ownership.py --exports <exports> --output <fresh-directory> --mutations`
checks actual selected wrapper/direct modules and the actual Sample bank, with
the production result-fire/address wiring verified in the complete factory.
Independent integer RGB565 expansion and rounded products check sparse masks,
one/two-group pixels, sixteen live public IDs, fifth-bit bank identity, legal
ID reuse, old-state credits, final-write release, CE0/commit stalls and held Q.
Physical command Group35 and raw selected component packet contracts are
separate cases. Public data macros retain checked diagnostics. Unpublished
input, bad raw packet and refused closed-bank completion distinguish checked
feedback, command monitors and monitor-free functional behavior. Real DUT
premature-release and packet-gate mutations must hit their specific failures.
The fixture stops at 10000 cycles and uses bounded compile/simulator processes.
It complements the retained FIFO Color miter and independent public Color tests;
it does not establish global D3 retirement, a frame or device qualification.

`vliw_backend::export_join_q_profiles` is an ignored case in the existing `host`
executable. Set `GPU_JOIN_Q_EXPORT_DIR` to a fresh absolute directory to export
8 actual selected command/generic/full profiles and 4 public event-return banks.
Freeze the accepted revision separately before changing its implementation.

`join_q_policy.py --base-exports <accepted> --exports <current> --output <fresh>
--negatives` compares 16 complete preprocessed streams, exact public bank bytes,
private profile scope and default checked selection. Only the exact independent
TOP abort process relocation and two empty native-bank processes may differ.
The existing integer/whole-memory fixture supplies a 64-quad stream and all16
masks, including independent producer CE,17-edge held Q, wrap, empty heads,
backpressure and C-paid depth conservation. Both RAM backends and monitor modes
must match the frozen accepted event trace. A direct observer checks legal raw
retirement against paid D3; real masked-read, premature-Q, C-retirement and double
empty-advance mutants must fail. The actual split command TOP separately checks
wall-edge abort at CE0 and rejects a CE-gated abort-owner mutant with monitors off.
Use `--fixtures <frozen-complete-fixtures>` to replay already captured accepted
inputs rather than regenerate them. Baseline files are hashed and never used as
an execution working directory. Simulations have a30,000-wall bound and each
process a60-second limit. This is source/lifetime qualification, not a new
physical timing, whole-frame, memory-controller or board result.

The join fixture receives its hold setting before import and restores caller
environment, argv and its temporary base-module binding after success/failure.
The receipt checks actual seventeen-edge CE/advance and source-prefix trace
windows, in addition to the unchanged DUT Q-hold assertions and executed count.
`python -B ip/gpu-v2/tests/test_join_q_policy.py` covers import ordering, repeated
independent calls, failure restoration and rejection of unexercised hold coverage.
