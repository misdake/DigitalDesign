# GPU regression tests

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
