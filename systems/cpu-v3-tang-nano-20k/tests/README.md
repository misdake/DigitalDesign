# System regression tests

## Bloxorz prepared command parts

`gpu_prepared_group` runs the real RCC core in both GPU debug modes. It checks fixed-part snapshots, independent replacement, complete
entry-state prefix omission, command capacity, reference/rebuild exclusion,
resource facts, reset and dirty-line same-page LCOPY behavior.

`host gpu_pipeline::bloxorz::buttons::` compares16 controlled session frames
against forced full re-recording. It checks complete physical color/depth and
actual Display, plus exactly which button parts are rebuilt during press,
animation, settlement, release, level33's13-button layout and restart. The
fixture controls state/delta; it does not claim physical-key or cycle timing.
`static_cache::prepared_bridge_capture_reads_fixed_push_after_reset_and_normal_retirement`
checks actual published capture reads and retained push data over three frames.
The separate warm and physical-input cycle fixtures supply performance evidence.
`BLOXORZ_MOVING_CAPTURE=0` disables only the passive common GPU journal and host
scanout reconstruction in the physical-input fixture. The System clock/model,
input protocol, CPU observation, stopping condition and raw-plane outputs remain
identical. The default retains the complete journal; timing-only runs do not
claim a GPU report. A same-image comparison qualifies identical event ticks,
CPU call intervals and complete color/depth with either setting.

## Bloxorz static pixel cache

`host gpu_pipeline::bloxorz::static_cache::warm_second_frame_system_timing`
is an explicitly invoked, bounded production full-system cycle measurement.
`BLOXORZ_SIM_OUTPUT_ROOT` selects this checkout's output root and
`BLOXORZ_CACHE_ARM` labels a new run. It compiles a test-only level29 PLAY entry,
neutral input, zero animation delta and HUD off; the second application frame
follows a completed first-frame warm-up. The existing signal4 instruction's
unique linked PC identifies first unheld Execute arrival. Actual Device4 START
and GPU busy-to-idle edges provide both CB issue/completion times. These five
points use the model's exact integer timebase, not host wall time. The run stops
at the second dynamic GPU completion, before subsequent presentation.

`BLOXORZ_CACHE_IMAGE` optionally reuses a previously saved `cpu-v3-boot.bin` and
its adjacent listing, without recompiling the old application or restoring an
old source tree. The output preserves the actual image/listing and raw physical
color/depth. Bounds are12M logic edges and1800 host seconds; incomplete results
are explicit failures. The synthetic cold-preloaded startup remains distinct
from real SPI-loader or board qualification. CPU issue intervals can include
producer waits and must not be described as pure active CPU execution.

`retained_static_scene_matches_fresh_recording_and_reuses_immutable_commands`
compares warm restoration with forced full static rebuilds, including complete
color/depth/Display, immutable cache allocations/tails and3/2/2 versus3/3/3
submission counts. Typed depth-copy tests preserve distinct patterns, physical
padding and allocation tails, and reject overlap. The existing52-scene gallery
covers all33 stages and representative animation states. `cache_compare.py`
exports full RGB, raw and8x difference images, contact sheets and timing JSON;
pixel agreement does not establish GPU cycle timing or physical FPS.

`system_runner` is a bounded actual-System input/watch/ch11/CPU-cache-inspection
fixture. Observer-on/off systems match every scheduler output and final backing;
real right/release reads and a4KiB GPU MEMCPY are checked. It supplies short host
and event-rate evidence, not ordinary game FPS. Small runner/recorder unit tests
check selection, consumption guards and byte truncation; Python tests cover
later-group history, frame/sidecar binding, exact timer epochs and total-wall
watchdog behavior. No browser or ordinary application result is inferred.
The return-site variant includes a real PFX+LOAD miss: each first Execute
arrival is observed once although neither retires there, and completion occurs
with no Execute PC. Retained-r10 final/intermediate discrimination and explicit
return/MMIO offsets have separate small checks.

## Full-system cycle composition

`system_cycle_components` checks the exact integer clock calendar, production
GPU descriptor registers and a single real native memory service under competing
CPU-style line write, GPU read and Display fill. It checks all data, one terminal,
no same-edge slot credit and physical write visibility on native falling edges.
Its ignored slot test compiles the actual board RTL and a credit-borrowing
mutation; timeout or compilation failure is not accepted as mutation detection.

`system_cycle_root` runs the real request-side CPU, full command GPU and Display
against that service. The cold-prefix case checks real MC initialization/cache
sweep/black scanout/ack and CSEG7 release. The RCC application checks an initial
complete CB/wait/present, a second submission with CPU/GPU/Display contention,
two complete independent-color 800x480 registered rasters, full color/depth
physical spans and padding/guards, CPU dirty publication, SP and segments.
Descriptor/terminal and payload counts are separate from simultaneous-pending
counts. The bounds are one million logic edges for cold startup, 3.8 million for
the two-present application, and 600 host seconds. Artifacts are in this tree's
`target/c4/`. This entry is synthetic cold preload, not real SPI/loader-state or
board-timing qualification. See the [composition contract](/systems/cpu-v3-tang-nano-20k/docs/system-cycle-emu.md).

## Fixed D16 depth

`host fixed_depth::` compiles the real public RCC application with both GPU debug
profiles, executes a triangle on CPU instruction sim plus functional GPU, compares
complete color/depth planes and preserves allocation tails. GPU geometry tests
retain all eight unsigned comparisons, write-enable behavior and inclusive
quantized far admission. The Global far is full positive Q16.16 below200; retired
selection tags are rejected, never truncated. Normal CP/Prepare/interpolator RTL
fixtures retain their stall, CE and completion checks.

Capture producers declare native7/RTT8, range1 and flags31. ABI4 remains fixed-depth
replay-compatible. ABI6 is historical import-only: exact former ABI definitions,
range flags, metadata and source hashes remain readable without enabling runtime
precision selection. The profiler historical-capture tests check import identity
and rejection before sim/RTL dispatch; these are not new board evidence.

## Persistent split controller

The ordinary `host public_handheld::` cases retain input, deadlines, retirement,
reporting and framebuffer alternation checks. Their delayed control peer verifies
that dynamic recording occurs while static work is outstanding; its synthetic
clock is not performance evidence. Static commands are recorded once and patched
per frame; startup maintenance is counted separately from scene submissions.

Three additional `public_handheld::stage2_` cases are ignored by default. The
native-state comparison takes `STAGE2_UNSPLIT_SOURCE`, whose frozen original
caller blob is checked, and compiles it with the current shared library. It
produces controlled candidate/reference sources. The functional raw and A1 cases
take `STAGE2_PREPARED_SOURCE` pointing to that candidate beside its reference.
Both profiles use the same Camera movement/turn, independent rabbit Y rotation
and complete fixed UI input. No image region is masked. Actual functional GPU
runs compare each completed frame's color/depth and all allocation guards,
as well as final A/B/depth allocations, within an eight-million-step bound.

A1 uses the first real bound gate after an explicit test request following F1:
submission5 is slot1 dynamic, including the initial maintenance START. It checks
unissued START, actual completed static preimages against an independent static
prefix, exact visible planes and read-only allocation tails. Front is observed
in a sidecar because the existing wire protocol omits it. The application is
compiled with the existing fixture build/catalog identity producer. The actual
raw dump passes shared GPS1 decoding/materialization and four tail/alias semantic
mutations must fail. This is not board/build-receipt verification. Outputs use
fresh per-run directories; `STAGE2_QUALIFICATION_RUN` can name a run and rejects
an existing output directory. No whole-frame RTL, board timing or speed claim is
implied by these tests.

## Explicit RCC inline helper measurement

The `rcc_inline` target copies the current production `gpu_scene_fix_read/write`
function bodies with their production inline hints into a test loader. It runs
32 dynamic iterations with output guards, the same inputs/options and a fixed
stack top, comparing inline off/on on both the architectural interpreter and the
clocked core's one-cycle instruction/data peer. It reports steps separately from
core cycles, plus words, retired words, minimum SP and actual frame/spill costs.
One shared frontend snapshot also supplies the whole-backend host wall-time A/B;
inline-on includes analysis, cloning and both target lowerings. Timing observations
are reported, not used as a host speed guarantee or acceptance threshold.
Both models have finite execution bounds. This does not measure caches, MC, a
complete renderer frame or a board.

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --test rcc_inline -- --nocapture --test-threads=1
```

The same target's matrix tests freeze the accepted pre-optimization function in
`fixtures/scene_matrix_before.rs`. They compare all 42 output words for 96
fixed-seed signed/fractional/overflow cases with inline on and off, preserve both
inputs and check an unaligned output span's guards. A six-call experiment compares the original,
inline-only and reordered production kernel on the same bounded clocked peer.

The ignored `bloxorz_one_play_frame` experiment compiles the actual Bloxorz main
and shared library, replacing only the matrix/helper implementation for its
baseline. Both variants select the same first stage PLAY pose, solid background,
neutral input and one-frame stop. It compares the complete color/depth spans and
Display pixels without masks, and records matrix instruction residency and calls.
CPU/cache/native-memory logic cycles are measured with synchronous functional GPU
execution and immediate host vblank. GPU/Display timing, their contention, FPS,
steady-state/fade behavior and hardware are outside that experiment's scope.
Set `MATRIX_EVIDENCE_DIR` to retain listings, inline reports and measurements.

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --test rcc_inline bloxorz_one_play_frame -- --ignored --nocapture
```

## Host time and input

`host::host_io` contains bounded real-RCC timer/watch and snapshot capture cases,
including publication during payload reads, slow/latest replacement, disconnect,
button edges and digital neutral axes. Pure tables check Device5 channels and
XInput axes/buttons/trigger boundaries; an injected source checks focus and
disconnected-slot polling without a real controller. Run the shared `host`
target with filter `host_io::` through `scripts/run-cargo.ps1`. The existing
independent `gamepad` and `dualshock2_ui` peers retain their own protocol and
diagnostic coverage. No serial or GPU timing claim follows from these tests.

The ignored Windows `host_io::windows_xinput_get_state_smoke` calls the official
API and accepts a connected or disconnected result; it does not prove physical
button mapping. `node systems/cpu-v3-tang-nano-20k/tests/debugger-ui.cjs` also
checks the independent heartbeat/focus events. Normal `system_sim`/`debugger`
library filters and debugger HTTP tests preserve the existing functional GPU,
reset, breakpoint and explicit vblank boundaries.

Retired facade/scheduler sources live only in `tests/fixtures/legacy_renderer`.
The test-only loader composes their byte-preserved contracts with current common
modules. Existing ownership, private-domain, capture-v1 and native assertions stay
active; production boot projects and explicit API callers cannot select this
overlay.

## Present and UI integration entry

The four Present/UI success and fault cases share the `gpu_present` integration
executable. Each remains independently filterable and ignored by default:

| Former target | Module filter in `gpu_present` |
| --- | --- |
| `gpu_rcc_present` | `gpu_rcc_present::` |
| `gpu_rcc_present_fault` | `gpu_rcc_present_fault::` |
| `gpu_rcc_ui_present` | `gpu_rcc_ui_present::` |
| `gpu_rcc_ui_fault` | `gpu_rcc_ui_fault::` |

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --test gpu_present -- --list --ignored
cargo test --release -p cpu-v3-tang-nano-20k --test gpu_present gpu_rcc_present:: -- --ignored --nocapture --test-threads=1
```

Run tool-backed cases serially to retain the former invocation order. Original
DUT/oracle code, assertions, evidence directories, ignore reasons and watchdogs
remain in their case modules and existing fixtures. Combining the executable
reduces linking and process overhead; it does not reduce simulation coverage or
case execution work. The Display simulator, GPU dump and pack CLI targets have
no unit cases and omit empty test harnesses. Their ordinary binaries remain
available; boot-assets and debugger unit tests remain enabled. Adding unit cases
to those CLI sources requires re-enabling their Cargo test target.

## Control peers and maintenance wiring

The builder and UI host suites share only the control peers in
`support/gpu_control_peer.rs`: Display status, CPU publication and GPU START
counts. Their compilers, assets, fixtures and independent command/memory checks
remain separate. `cache_maintenance_wiring` reads the production board template's
named instance endpoints, including the private SystemControl-to-CPU hold net
and the independent GPU/cache/MC/Display clocks. Wrong producer, CPU, GPU, MC,
Display and alias connections are negative controls. This structural check does
not replace the fourteen bounded RTL scenarios in `cache_maintenance_protocol`
or qualify new device execution or board timing.

## Graphics API helpers

`host::gpu_api_scene_patch` uses the checked public API and bounded CPU execution
in both profiles. It compares the full patched push span and retargeted facts
with a fresh ordinary recording, preserves the sealed command stream through
actual control-peer retirement, and alternates targets without reset. Negative
cases cover foreign/stale/context/reset bindings, projection/light rejection,
generation carry/exhaustion, mixed equal-valued Models, invalid spans and driver
ownership. Its storage-failure fixture preserves the production patch source
and injects failure into the storage callee after one real copy; this qualifies
poison/shadow/START boundaries, not a physical transport failure or GPU pixels.

`host::gpu_api_submit_modes` checks the shared public driver in both profiles:
ordinary/unarmed submissions skip the hook, pending requests survive ordinary
work, and a stale header cannot overwrite real-START counts. Its counter cases
cover low-word carry and failed START. A controlled hook observes prospective
submission numbers; a separate case runs the real profiler runtime and requires
held-button samples, a release and a fresh debounced resume before START.
All cases stop within one million CPU instructions; their peers do not render.

`graphics_api_b_modules` compiles actual B camera/model/material/state/view/UI helpers with
checked RCC and executes CPU V3 in both `gpu_debug` profiles, with a 250,000-step
limit per program. It checks final native encodings and independently compares
the complete external UI allocation across a DSEG3 boundary. Two cameras share
a nonuniformly scaled Model; independent oracles cover the entire 256-word
native/scratch span, source matrices, light quantization and frozen input arrays.
The oblique case independently multiplies host rotation matrices for full
three-axis Model/Euler camera rotations, negative/nonuniform scaling and a
non-axis look-at. It compares source world/normal/view and final native
position/normal/light, with fixed Q16/Q14 tolerances, exact padding/scratch gaps
and byte-identical healthy output across profiles.
The binding case feeds the actual B preparation into A's actual arena, record,
bind, mesh and UI packet modules. Its existing fixture also checks six-word
vector I/O with extreme bit patterns, odd/line-crossing and wrapping offsets,
unchanged input and complete neighboring-word guards.
Independent complete command/push oracles
check two CBs, six live Models, equal-value reuse, RGBA-only changes, 3D/UI/3D
and freeing/reusing a Model after recording. It includes actual A execution,
Display and fault storage. These helper fixtures carry no hypothetical C capture
allocation and do not qualify the final A/B/C budget, public owners, capture or
GPU pixels. The public fixture below measures actual composed globals instead.
Both profiles must emit the same
healthy command/push bytes. A sources are frozen per case across both profiles.
During parallel
G0 development, `GRAPHICS_API_A_RCC` selects A's actual dependency directory;
after integration it defaults to this system's `rcc/`. Captured source inputs
and reports stay in `target/graphics-api-b-modules`. Set `GRAPHICS_API_B_OUTPUT`
to a fresh directory in this tree target to retain earlier qualification evidence;
helper cases use its `modules/` and the value gate its `values/` subdirectory. These tests do not qualify
the public resource factories, combined driver budget or GPU execution.

`graphics_api_b_value_gate` is the positive frozen-API gate for the production
`Shininess::nearest` / `exponent` methods. It requires real checked RCC lowering
and CPU execution bounded to 250,000 steps in both `gpu_debug` profiles. All 17
codes/exponents, lower-on-tie selection and the 0/65535 endpoints are checked.
Both profiles use the same frozen production input bytes. This qualifies the
value methods; A's resource adapters and approved-root registration still need
their actual combined gates.

`scripts/validate-graphics-api-b-public.ps1 -InputDirectory <fixed-A-package>
-OutputDirectory target/<fresh-name>` qualifies the actual public scene root
using its frozen System loader, B helpers and C implementation. The input must
contain `manifest.json` and `sources.zip`; ZIP and every source hash are checked
before compilation, and existing output directories are preserved. No source
from an active A tree is loaded during execution. The script generates only a
temporary Cargo harness, using this tree's accepted RCC/CPU backend.

The checker and its source fixture are also copied into the output and hashed
in `run-identity.json`, with the accepted dev, source commit and any WIP identity.
The public fixture runs both `gpu_debug` profiles under a 5,000,000-step limit.
It keeps six Models and two Cameras live, checks oblique Euler/look-at native
matrices and light with independent matrix products, then mutates resources,
Drops and reuses a Model slot and appends to one of two independent CBs.
Full old command/push/fact snapshots must remain unchanged. Public texture and
RTT views survive their source descriptor's Drop. A small atlas and odd/short
UI write cross a DSEG3 boundary, preserving neighbor bytes and allocation tail.
3D/UI/3D, DMA/full WAIT ordering, equal-value reuse and RGBA-only changes are
checked in the emitted native stream. Healthy outputs match across profiles.
Projection private-field access, a moved Model and private helper calls are
negative checked cases. Static plus observed stack must fit 16 KiB, including
the actual C globals; no hypothetical capture reservation is added. This is
public recording qualification, without GPU pixels or captured DRAW/UI G3.

The ignored `raster_phase_replica::export_raster_phase_inputs` test materializes
the portable/native frontend, final production GPU leaf and inner command RTL
under `target/raster-phase-native/native/input` when
`CPU_V3_GPU_PROFILE=production`. `raster_phase_replica.py --export <rtl.v>
--output <fresh-directory>` checks their common clock/reset/CE/phase bindings
without an external baseline snapshot. Its actual modules check 256
post-edge phase values across all four active/paused phases, abort frontend
freeze and reset/restart. A deliberately delayed-Q variant must fail. Each
process is bounded to 45 seconds and the testbench to 256 edges plus a watchdog.
Active rendering uses the existing bounded column command replay separately;
phase equality alone is not a pixel or physical timing qualification.

`graphics_member` checks actual approved graphics-library bodies with rustc,
lowers their ownership/Drop chain through RCC and executes CPU V3. It covers
repeated mutable Pass draws, consuming finish, explicit submit, separate GPU and
Display waits, unauthorized successor retention, cancellation and live-reference
retention on timeout/fault. Function-entry journals count actual target cleanup
and release calls; captured CBs contain actual recorded draws. Negative callers
must fail rustc alias/move/parent-escape checks. Its real member demo runs three
frames. All cases have explicit instruction bounds and controlled retirement
peers; no host-only graphics runtime, pixel oracle or hardware timing is implied.

The reusable [GPU profiler](/systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/README.md)
has bounded format, viewer and production-GPU observer checks in its own tool
directory. It does not require a full-frame run or change synthesized logic.

The [GPU board throughput suite](/systems/cpu-v3-tang-nano-20k/examples/gpu_bench/README.md)
has small asset/protocol checks and an opt-in board run. Its hardware timing comes
from UART intervals, not the functional simulator or its optional memory peer.

The `host` integration target groups boot packaging, module resolution, driver,
UI, display-control, timer, input, camera and fetch/checkpoint tests. Filter by
module when checking one area:

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --test host boot::
cargo test --release -p cpu-v3-tang-nano-20k --test host gpu_rcc_driver::
cargo test --release -p cpu-v3-tang-nano-20k --test host gpu_pipeline::
```

The pipeline module includes persistent UI tests under gpu_pipeline::persistent.
They journal scalar stores from actual compiled CPU execution, checking every
UI write against the live Recording reference and recorder identity. Seven
alternating submissions verify native command/descriptor bytes, page convergence
and zero stores for unchanged contents. Separate cases cover field validation,
capacity rejection, cancellation, version wrap and retained fault references.
GPU/Display completion peers do not establish actual rendering, scanout, cache
publication timing or concurrent hardware qualification.

The `gpu_api_*` modules exercise the new explicit API work in progress, separately
from the legacy pipeline. They compile real checked RCC for CPU V3 in both
`gpu_debug` profiles and use bounded CPU/device peers. The implemented storage and
recording scope is described in [API storage](/systems/cpu-v3-tang-nano-20k/docs/gpu-api-storage.md).
`gpu_api_resources` uses the actual approved root for factories, Pass/descriptor
lifetimes, capacity rejection, reinitialization and Display retirement. Private
execute tests do not establish public submission/capture or GPU pixel rendering.
`gpu_api_model_cache` checks independent Model dirty data, Camera revision
snapshots across interleaved CBs, descriptor reuse, two-allocation capacity
failure, and revision carry/exhaustion. It compares complete recorded commands
and push data with a forced-recompute run; historical-source and native-cycle
qualification remain separate from this bounded functional regression.
The required private-memory-field registration regression currently fails against
the accepted frontend; positive-only development runs explicitly exclude that
single test and are not full API acceptance.

The `gamepad` module executes production RCC input code against an independent
Device5 snapshot peer. It checks explicit edge accumulation/consumption,
unchanged and torn snapshots, disconnect/reconnect, digital mode, configuration
invalidation and held motion across ticks without a new sample. Cached motion,
direction and strength must match the original stick kernel bit for bit across
63 cases and three calibration/dead-zone profiles. A passive compiled-function
entry counter proves repeated captures do not convert intermediate positions,
first consumption resolves only pending results, and queries/configuration/
disconnect cannot return stale results. Every run has an explicit
CPU step bound. Serial timing remains in `dualshock2`'s actual-CPU RTL test;
the snapshot peer does not qualify physical controller behavior.

The gpu_pipeline::resource_usage cases compile the actual approved production
root, without the former scope adapter. They check raw/constructor/consumed-owner
misuse, early cancellation, transfer failure, duplicate open, explicit close and
implicit freeze, plus owned text tail/visibility/cleanup semantics. The scene
cases use an independent float oracle and exact native bytes; the minimal caller
executes examples/gpu_renderer/rcc/main.rs with its actual packed assets. Timing
cases use a clock advanced only by host events, covering observer delay, clear
epochs, saturation and wrap. Frame queries cannot reread live input; held input
integrates on subsequent ticks without new packets. GPU/Display completion peers
qualify software ownership/bytes, not actual GPU/cache/Display concurrency.

The `gpu_pipeline::changes` cases execute the real changing-frame application,
interpret every native draw snapshot/state and UI range, and hold recorded/inflight
UI images immutable. A production CPU/FPU core run uses fixed instruction/data/
control peers and checks every UI store handshake, including FPU stores. It writes
per-frame recording/seal costs and UI store counts under `target`; these exclude
cache/SDRAM/GPU/Display latency. Sequential completed Passes and a later abandoned
Pass are checked through the public resource facade.

The `gpu_pipeline::bloxorz` cases execute the checked RTT application with bounded
GPU/Display peers. Restart, repeated transitions, disconnected input and
success-to-next-level ordering check actual command/object snapshots, no-blend
state, source epochs/pins, screen matrices and independent block geometry. A
session-only caller checks black-presentation gating and ignored inputs. The
actual final-fade Display wait receives coherent START/TRIANGLE press/release
and held reports; entry must survive these edges and accept a later fresh press.
unattended capture callers check both128 and255 retirement stops without input. Four
original host geometry cases remain. These peers prove CPU state and bytes;
GPU pixel replays and physical board qualification are separate.

The opt-in [RAM collision consumption checks](/systems/cpu-v3-tang-nano-20k/tests/ram_collision/README.md)
export current CPU/FPU, D-cache and Display modules and poison only native RAM
collision read returns. Bounded independent oracles and live-Q negative controls
check effective consumption; they do not qualify board timing or explain flicker.

Core/cache/arbiter emulator-vs-RTL comparison remains the independent
`system_cosim` target. Its functional machine and checkpoint support lives in
`system_emu`; it is not a benchmark harness. The former benchmark directory,
image exporter, calibration and folder-driven suite are removed.

Input, first-person camera, retained-control, board-application
and fetch/pause regressions also run in `host`; their module names remain filters.
The classic 512-step camera exporter stays separate: it shares the demo asset
producer, whose private resolver would duplicate the host resolver in one crate.
The historical template identity check that required a dated maintainer target
directory is removed; bounded retained-input and fault controls remain. The full
65,536-pair stick oracle remains a routine regression because its measured release
runtime is short. GPU model characterization and full-board extended workloads
retain the existing explicit validation tiers.

GPU rendering, fault and profiling tests retain explicit ignored entry points.
Preparation and full-frame profiling use the same `gpu_demo_profile` executable;
the former duplicate wrapper is removed. Run these serially when rebuilding
their test executable on Windows.

Whole-board real-SPI and native-pin success/large-resource simulations have an
`extended_` prefix. They retain full timing, completion assertions and bounded
watchdogs, and remain available explicitly:

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --example cpu_v3_system extended_ -- --ignored --test-threads=1
& scripts/validate-hardware.ps1 -Mode iverilog -ExtendedSimulations
```

Routine `iverilog`/`all` validation selects the production `cpu_v3_system`
example and excludes its expensive extended cases; unrelated game/demo examples
are selected explicitly, even when `-ExtendedSimulations` is enabled. This avoids
implicitly running Bloxorz native scene replays. For example:

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --example bloxorz -- --ignored --test-threads=1
```

For the production example, bounded signature/range rejection tests still run.
Changes to boot, Display
startup or the native memory path require selecting the affected extended
case. This split changes validation cost, not the qualification supplied by a
short protocol or functional test. A full-frame profile is not a default test,
and no offline test substitutes for board verification.

The `gpu_demo_first_person` module runs the production RCC camera/control flow
with bounded CPU steps and a separate floating basis/light reference. Its frame
control peers verify that render timing excludes present/vblank, retains linear
RGB565, and rejects failed/saturated measurements. Pixel and serial timing stay
in the dedicated native GPU/DS2 RTL tests.

The bounded `capture_layout` target executes the actual formal renderer producer
with the production column capture layout. It emits portable
manifest/source identities and real CPU header/range/public-read fixtures under
`target/capture-layout`, checks exact physical spans, resource mismatches,
end-bound rejection and lease expiry, and rejects legacy row metadata. Complete
payloads are host copies during the frozen gate. It provides no UART, GPU pixel
or board qualification; see [capture bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md).

The separate bounded `capture_draw` target records one native triangle across
three real RCC/CPU Prepared gates for the column profile. It checks actual
DRAW bindings, positive Z, material/transform changes and A/B/A command-slot
reuse with distinct tokens. Set `GPU_CAPTURE_DRAW_DIR` to archive its manifest,
program, origins and each gate's complete frozen inputs/preimages outside the
build cache. GPU/Display peers only retire control work; independent GPU cold
replay and pixel/depth verification remain consumer responsibilities.

The same target's `real_full_scene` case executes the pinned formal profiler
frame body, with complete box/RGB/rabbit assets and OBS UI, across three real
gates for the column profile. The middle gate resumes after a modeled long pause;
production input/time baselines and statistics invalidation/recovery are checked
without changing the scene body. `GPU_CAPTURE_FULL_SCENE_DIR` selects its output;
`GPU_CAPTURE_LAYOUT_DIR` overrides the older layout target's default archive
path. Set these and `CARGO_TARGET_DIR` outside the source tree when preserving
evidence. Full-scene bytes remain CPU producer inputs, with no GPU pixel or board
qualification implied.

`host gpu_render_buffer::` compiles the actual RTT member API and checks external
capacity/alias validation, read/write pins, cancellation after sealing, terminal
epochs, capture dependency closure and Rust ownership negatives. Its two public
demo callers export native command/object/asset fixtures under `target/rtt/demos`
for the independent GPU replay harness. All CPU runs have explicit step limits;
completion peers qualify software state and bytes, not GPU rendering or HDMI.

## Native resource concurrency fixture

The ignored `gpu_renderer_native` target compiles the approved production resource
root and a small caller with the actual asset producer. It exports the production
CPU/FPU, D-cache, system control, GPU, Display, arbiter and three descriptor slots.
Instruction fetch uses a separate ROM; an ordered, stalled 64-bit memory service
terminates the arbiter's external port. The default pre-submit consumer is the
ordinary no-op gate. There is no alternate scheduler or status-completion peer.
The arbiter's instruction/DMA clients and memory lookahead are inactive; native
MC/bridge continuation is outside this fixture.

The successful run cancels one complete recording, then submits A/B/A and
explicitly drains. Accepted CPU scalar/FPU stores journal ownership separately
from write-back DRAM. Both complete 64-KiB command and object arenas are protected;
only the recorded 16-word Target relocation has the normal pre-publication
exception. The fixture checks END/fetch padding, UI cells/descriptor/padding,
publication of old scalar/FST stores, and unchanged in-flight input snapshots.
GPU idle must follow actual RO/FB-read/FB-write terminals and empty request slots.
UI release is observed while Display remains Pending; color writes cannot target
Front or a surface with an old outstanding Display read. Swaps require old-read
retirement. GPU output and visible Display RGB are checked at independent samples.
The projection oracle uses floating source-world triangle coordinates, not driver
matrices, GPU output or the old cmodel; it does not qualify full-frame equality.

`GPU_RENDERER_NATIVE_CASE` selects one bounded scenario:

| Value | Additional boundary |
| --- | --- |
| `success` (default) | Three submissions, real recording/execution overlap, CE pauses, backpressure and independent input/color retirement |
| `cross-window` | Two submissions; source Camera/Model/Material mutations preserve the first recorded draw and affect the second; CPU writes and GPU reads cross the second object window, then restore DSEG0..3 |
| `capacity` | Full object arena rejects the successor while the first GPU input remains live |
| `gpu-error` | A real second-CB read error on a valid LAST freezes its in-flight reference, with no later submit/present |
| `display-error` | A real old-Front read error freezes the recorded successor before a later submit |
| `publication-error` | A real CPU writeback error leaves Recorded ownership and zero SEND/present; full-maintenance `ST_FAULT` rejects new cache work for 1,024 observed cycles |

Cross-window/capacity cases substitute one accepted D-cache cursor store to
represent an unused allocated prefix. They still execute the normal checked
builder, CPU mappings and actual GPU reads; this is diagnostic stimulus, not a
public application API or evidence that a normal game fills that prefix.
Publication failure is a hardware fail-stop until whole-system reset. It does
not require a software return through potentially dirty stack data or a readable
software fault record.

The production GPU treats transport status as a passive sticky observation, not
an abort/recovery command. Its normal simulation monitors terminate on peer
errors. Only `gpu-error` admits its one deliberate RO error at that one monitor;
all other conditions still call the original fatal assertion. The original
export, narrowly adjusted compilation source and override manifest are retained.
No datapath statement changes and no global `SYNTHESIS` switch is used. The error
retains the valid payload and burst LAST, checks the real sticky Device4 status,
and need not clear GPU busy or manufacture a successful terminal.

Run each case in a separate process with a fresh evidence directory:

```powershell
$env:CPU_V3_GPU_PROFILE = 'production'
$env:GPU_RENDERER_NATIVE_CASE = 'success'
$env:GPU_RENDERER_NATIVE_EVIDENCE = (Join-Path $PWD 'target/renderer-native-success')
& scripts/run-cargo.ps1 -Subcommand test -Label 'renderer native success' -CargoArgs @('-p','cpu-v3-tang-nano-20k','--test','gpu_renderer_native','--','--ignored','--nocapture')
```

Python, Icarus (`IVERILOG_EXE`/`VVP_EXE`) and Gowin primitive models (`GOWIN_HOME`)
are required. The HDL watchdog is four million CPU clocks; asset production,
Verilog compilation and VVP have 60/120/1,800-second wall bounds. Evidence includes
the actual compiled RCC/listing, assets, generated sources and testbench, independent
pixel expectations and raw tool logs. The pixel clock accelerates cold blank startup
and uses a 2x protocol-stress scanout profile (about 1.92x in the cross-window
case, to avoid waiting an extra blank frame); GPU CE is a test-only override.
These checks exclude fetch-queue/I-cache integration, physical MC/bridge timing,
SDRAM pins, HDMI serialization, fitted timing, board behavior, RTT and checked
member-call borrow rules. The retained flat caller does not qualify profiler pause
behavior. Long native runs remain explicit; ordinary host tests do not launch
them, and host peers do not substitute for them.

## GPU profiling input parity

`host public_handheld::` executes the production public classic caller and its
motion helper through real checked RCC compilation in both check profiles.
Finite control-only peers observe the complete 24-draw/1141-triangle inputs,
material/clear codes, B/A/B presentation and old-read retirement, input and UART
ordering, rendering-interval promotion, visibility/saturation and wait deadlines.
Separate CPU probes check the initial motion codes, dynamic limits and a public
Camera view grid against independent trigonometry. They do not render pixels or
qualify physical timing; native and static-profiler references stay separate.

`profiling_capture::tests` checks recorded real v1/v2 pre-gate envelopes and
range rows, plus malformed length/version/publication/binding/flags and physical
extent boundaries. `fixtures/prepared_capture_descriptors.json` records the
compiled-image identities and actual capture points; positive descriptors/counts
are producer output, while negative tests mutate copies. The system frontend
and GPS1 decoder remain separate gates; these tests do not render GPU work.

`capture_draw::compiled_program_system_frontend` compiles a real RCC/CB06 scene
and freezes the first Prepared gate in `CpuV3SystemSim`. The ordinary case stays
small. `GPU_PROFILER_FULL_SCENE=1` selects the pinned canonical box/RGB/948-face
rabbit source and its default camera; output uses `GPU_CAPTURE_FULL_SCENE_DIR`.
Explicit `GPU_PROFILER_RTT=1`, `GPU_PROFILER_NATIVE_COPY=1` and
`GPU_PROFILER_UART=1` add an owned offscreen target, a trusted fixture-only native
encoder tail and the shared manual UART runtime. The tail uses existing native
encoders and the required FLUSH; it adds no public graphics API. Copies cover a
page crossing and a later read of a completed earlier write. Ignored staging
padding is deliberately nonzero.
`GPU_PROFILER_NATIVE_COPY=reserved` injects one reserved descriptor bit and checks
sticky dependency error9 before the consumer or any GPU SEND.

UART mode primes a settled debounce edge at that same first gate, then uses a
functional GPIO/time/TX peer to pause, dump twice, remain paused and explicitly
resume the same CB once. It checks that frozen resources remain unchanged and
retains `uart-offline.bin` plus the peer assumptions. This is real RCC protocol
execution, not physical button, serial timing or board evidence. The ordinary
system frontend has a five-million-step bound; UART mode has explicit two-billion
step and180-second wall bounds. The shared decoder checks both UART attempts
against the system snapshot before the common cold backend is used.

The `host gpu_pipeline::profiler_short` case runs the actual repeated RTT board
application and its consumer-only gate through three consumer boundaries, with
five submitted stages and two Display retirements. It checks mutable source
closure and proves the control peer leaves RTT pixels untouched. The controlled
peer advances wall time and supplies fresh debounced resume edges after explicit
snapshot requests; this test does not serialize framebuffer dumps. Canonical short
asset/mode binding and deterministic native MEMCPY specialization are checked by
the profiler Python suite. `capture_draw`'s explicit `GPU_PROFILER_NATIVE_COPY=1`
and `GPU_PROFILER_UART=1` modes reuse the same source generator for bounded actual
UART runtime and system-front-end fixtures; cold GPU+MC replay remains separate.

The `gpu_api_capture_source` tests qualify actual A fact/source/health exports,
including optional Display observation and fixed publication counts. They do
not install a C shim or establish public submit/capture combination acceptance.


The `gpu_api_integration` tests use actual A/C modules and the approved public
root for unique retirement, interleaved replay, the real diagnostic gate and
fatal ownership retention. `gpu_api_scene` qualifies the public B adapters and
records exact facts/native input; a separate case proves malformed capture
dependencies issue no SEND and preserve the prepared owner.
`gpu_api_pixels` is explicitly ignored by default and requires the native Gowin
simulation library plus Icarus. It executes the public API submission on production
GPU RTL, imports actual color pixels and permits retirement only after completion;
all CPU, GPU and child-process runs are bounded. No board is used.

## Dual-address LCOPY and maintenance

`system_cosim` covers independent source/destination DSEG translation, remapping
both DSEGs while an accepted copy is active, same-physical-line logical aliases,
ALU overlap, and HALT/DWAIT completion through the full fetch/cache/arbiter path.
The `cpu-v3` cache tests replay the cycle model against portable and native DPB
implementations. Their direct goldens include equal-tag/different-set target
invalidation (`0x120 -> 0x240`), dirty target discard, source dirty retention,
cold-source victim/refill, the complete last physical-row address and payload,
alignment failures, stalls and terminal errors.
The reserved opcode2/function7 and removed RCC intrinsic are rejected.

`cache_maintenance_protocol` checks fourteen bounded CPU/cache/SystemControl
scenarios. Its injected full-clean write error is fail-stop: software receives
error status, while cache ownership/admission remains locked until reset. The
fixture observes the CPU's halted state and verifies that architectural HALT
and command ready remain suppressed; it does not wait for a successful drain
from a deliberately faulted cache. Existing reset/error ownership and DWAIT
checks retain their behavior.

The public recording gate is valid with the frozen A stage and C047 capture
implementation on accepted ISA0.10. It measures actual C globals in each profile;
it never uses the recording stack as capture/dump stack evidence. The final A
root/loader/caller composition is a separate required input and qualification.


## Native-edge CPU smoke

`native_system_emu.rs` checks the public native-edge CPU composition using the
existing core/FPU, fetch, read-only I-cache, D-cache, arbiter and hardware
SystemControl. Its two bounded runs distinguish dirty cache visibility from
explicit Device0 clean after FST, retain fitted initialization and check the
2:1 native/logic calendar. It does not reuse this directory's abstract
`system_emu` memory peer. The Gowin vendor `native_port` ignored test independently
checks the full stream/gear/controller/pin chain against production RTL.
These are not GPU/Display application, Flash-boot or routed timing qualifications.

The native self-modifying-code case warms a BTC target, updates its opcode via
D-cache, explicitly cleans and invalidates instructions, then must execute the
new value. `system_emu::flush_tests` separately exercises the production frontend
flush wiring for SystemControl invalidate, halted and faulted, with a warm BTC,
genuinely pending captured old responses and fresh data afterward. It performs
no core/cache reset after setup and keeps late-response ownership intact.


## Native functional-device composition

`native_devices` uses finite CPU bounds and one backing per instance. It covers
actual CPU cache publication, deliberate missing clean/invalidate, a functional
GPU partial error, CSEG7 CB06 startup, hardware timer/watch release, Device5 input,
Device7 observation and Display publication only at explicit vblank. Two prefix
cases execute CLEAR then memcpy before pausing a third dependent memcpy: both
CPU modes match the complete defined8MiB preimage, repeated pause changes no
memory, PC/retirement or native clocks, and resume executes the target once.
The fast case also retains an unused prefix until the paused write retires.

The ignored CPU library tests
`instruction_wrapper_matches_rtl_for_held_error_refill_and_invalidation` and
`fitted_initial_segment_survives_reset_and_executes_from_cseg7` compare the actual
unchanged RTL boundaries. They retain bounded source/TB/output and reject real
DUT address/parameter mutations. The wrapper checks all public outputs, including
invalid payload, for held errors, misses/hits and invalidation during refill.
The CSEG replay covers configured cold and post-halt reset with hold; it does not
qualify every warm-reset phase. Existing CPU/system mandatory cosimulations and
Prepared-gate capture regressions remain required for affected paths.

The display runner can test an exact generated package without regenerating a
second boot image:

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin cpu-v3-display-sim -- --cpu-mode native --max-cpu-cycles 3000000 --frames 120 --stop-after-present 1 --ppm --package target/application.cb06
```

Use `instruction` with `--max-cpu-steps` for the fast backend, or replace the stop
option with `--freeze-submit 2` for raw physical pre-submit capture. A CLI exit0
alone does not prove rendering: check accepted GPU execution, error/busy, a valid
front, an actual applied swap and nontrivial pixel output. Compare raw physical
allocations separately from Display1-converted PPMs. Different timer schedules can
legitimately change statistical UI commands and CPU-private dirty/dead stack data;
record exact differences rather than hiding them or claiming full-image equality.
These host runs do not qualify GPU/Display bus competition, full-board FPS,
physical SDRAM timing, a fitted RTL frame or arbitrary warm reset.

`vblank_fast_forward.rs` uses the real RCC Display wait and Device0 watches.
Its bounded instruction comparison checks the full physical image, registers,
retirement, explicit timer/input events, UART and Display state against empty
watch stepping. Cases cover read-to-arm races for both CONTROL and FrameIndex,
frame wrap, already-completed swaps, error/changed values and unrelated watches.
The actual display CLI is checked with and without `--no-vblank-fast-forward`,
including step/sample bounds. A native fixture verifies that both real clock
counts keep advancing during a watch while CPU retirement remains held. This is
host wait-overhead qualification, not physical vblank latency or whole-system FPS.


## Stage2 native Prepared capture

`public_handheld::native_cpu::controlled_camera_rabbit_and_first_eligible_native_capture`
is an opt-in bounded actual application test. It compiles the current canonical
Stage2 caller with explicit test-only changes: two frames, fixed UI, a controlled
frame2 Camera change, an independent rabbit Y rotation and a snapshot request
after frame1. Production application and normal timer semantics stay unchanged.
Each control/capture package is shared byte-for-byte between the two CPU modes.
The unchanged firmware gate is selected by real eligibility, not by skipping to
the fifth raw START. All preceding CBs execute on the existing functional GPU.

The test compares complete consumed mesh/global/render/instance/UI traces, both
retired frame allocations, final color A/B/depth slots and full unused tails. It
checks ordinary static submission retains the request, dynamic slot1 is the
first reached Prepared gate, every metadata/payload range matches, active Front
is the completed first frame, and Back already contains the second static prefix.
Replaying the unconsumed dynamic target must match the complete second-frame
control output, preserve Front and all unrelated physical words/guards. This
uses functional GPU/Display without memory competition, not fitted-frame timing.
Artifacts go under `target/public-handheld/test-assets/native-eligible-*`;
`STAGE2_QUALIFICATION_RUN` supplies a unique output suffix. Run just this test:

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --test host controlled_camera_rabbit_and_first_eligible_native_capture -- --ignored --nocapture --test-threads=1
```

`system_cycle_profile` compares capture on/off at every scheduler instant through
a bounded cold-start CPU application: core output, Display, native activity,
final registers, counters, full backing and scanout bytes remain identical.
Its CPU submits an actual NOP/WAIT/FLUSH/END CB and checks the formal Recorder's
submit/latch/Exec/issue/release/drain events. The limit is 1000000 logic edges
and a 300-second host watchdog. The capture keeps one initial backing preimage.
`hardware/vendor/gowin/tests/native_activity.rs` separately checks actual native32
read/write slots, all DQM masks and no opposite-half-edge double counting.
Profiler Python/Node tests cover zero-submit/partial sources, transaction/data
ownership, separate memory boundaries, bound scanout PNGs, shared time bins and
compatibility with the older source types. These are model/report checks, not
electrical or board qualification.
The profiler suite also checks presentation-owned F1/F2 exports, exact window
bytes/carry, missing legacy facts, source identities and archived replacement of
generated frame bundles. `check_system_capture_import.py <actual-gpu-fixture>
<new-output>` exercises official build/render/native-asset replacement using the
bounded real GPU/native-MC journal in an explicitly synthetic importer envelope;
it makes no CPU/Display or application performance claim. The actual system
Recorder test remains a separate source. Native-history checks compare full
400x240 RGB565/D16 planes, cache overlays, same-address overwrites and raw hashes.
Public render tests reject closed payload-size/timing mismatches and preserve
legal same-edge payload/terminal, error-terminal and open partial prefixes.

`system_emu::metadata_tests` separately checks pending scalar metadata, dirty
resident metadata, cache-miss fallback and a real functional GPU memcpy behind a
stale CPU line. Repeated capture reads must retain the unpublished raw Header,
return the new GPU physical payload rather than stale cache data, preserve all
memory/clocks/registers, and allow normal explicit clean/invalidate afterward.
This does not qualify arbitrary FPU producer stores, a serialized CPU checkpoint,
all interrupted resets, real input, GPU bus competition or board performance.


Bloxorz's `gpu_renderer_bloxorz.rs` also checks the load-time heap builder against
independently decoded legacy native vertices for33 stages, current-address
capture after level1->2->1 rebuilds, and vblank-to-final-GPU-wait HUD boundaries
including wrap/cold startup. The sibling `gpu_renderer_bloxorz_batch_cost.rs`
contains the explicit ignored stage8 legacy/batch CPU+functional-GPU comparison;
its logic cycles are not whole-system GPU/display performance.


`gpu_api_target_patch.rs` compares multi-group target-only replay, with/without
background MEMCPY, against fresh native payload/facts after Display retirement.
Its bounded peers check stale generation, payload spans, active owners, source/
asset/plane aliases, Front/pending presentation and partial-I/O poisoning; these
are software protocol tests. Existing single-scene patch tests remain separate.
`gpu_renderer_bloxorz_frame_schedule.rs` freezes both accepted and pre-batch
candidate Git source blobs for its explicitly historical exact multi-frame
entrance/goal/fallback and native shadow-cost controls. These controls do not
qualify the current receiver-cell batch raster. The ordinary batch lifecycle
test instead uses the actual caller, checks two live split slots and public
storage facts over three frames, observes stack/heap separation, and verifies
complete heap coalescing at finite exit. The explicit historical native cost
test uses stationary PLAY and actual rolling ticks with per-arm instruction
publication and full pixel oracles.
Signal wrappers time total shadow preparation and nested projection/hull/clipping/
triangle recording. CPU/native-memory cycles exclude functional GPU cost and
scanout contention; wrapper overhead remains included. Cached-veil tests check
one snapshot per boundary, immutable source reuse, alpha arithmetic, real HUD
command counts and presentation/input endpoints.


`gpu_renderer_bloxorz_fragile_batch.rs` independently decodes all33 heap-built
fragile layouts and all153 cell hide/restore operations, preserving neighbor
indices, padding and complete restored payloads. Bounded peers reject inflight
patch/rebuild/free, replay one retained CB against changed same-address mesh data
and check zero degenerate coverage, and exercise ordinary restart/next-stage
presentation resets, shadow identity and finite heap release. The actual native
Instance normal matrix for compact floors matches the original half-scale axial
basis and stays inside normal18 range. Representative stage8/15/17/30/1 color and
depth outputs retain every accepted-source difference. The explicit ignored
stage4 PLAY CPU-cost test uses default capture hooks, neutral input, visible
shadow, actual CRACK/FALL/undo and separate per-arm instruction/native publication
and full-image oracles. It measures CPU/native-memory cycles with synchronous
functional GPU, excluding GPU timing, physical contention and FPS. A previously
qualified accepted arm may be read-only reused via `BLOXORZ_REUSE_ACCEPTED_COST`;
its original source/data labels remain recorded rather than relabeled as a run.

`gpu_renderer_bloxorz_atlas.rs` contains explicit ignored, bounded seven-frame
atlas comparisons (stationary level4 twice,30/15/17/8, then a real110ms roll),
freezes actual before-START physical inputs and requires all full depth planes
to equal the frozen8235 source. Select `BLOXORZ_ATLAS_ARM=before|candidate` and an
explicit `BLOXORZ_SIM_OUTPUT_ROOT`; the before tree is `<root>/source-8235`.
Candidate dimensions use `BLOXORZ_ATLAS_SIZES=6x7|7x7|6x8` (main/marker exponents).
The short replay test compares bilinear code1 and trilinear code2 at base-only
LOD0 on two real warm submissions, restoring only their explicit descriptor
input differences before requiring every physical output word to match.
An explicit `control-identity.json` identifies controls that retain the8235
application while using an accepted newer driver; these do not relabel the old
native6 evidence. The native two-frame cost test reuses the saved exact programs,
checks full color/depth against their instruction outputs, and stops after
frames0/1 with observed SP recorded. It excludes physical GPU/scanout timing;
the separate finite lifecycle test checks release after normal execution.
For a current fixed-depth control, extract the accepted driver tree and overlay
only the8235 application directory, recording both identities in that file.
`check_atlas_borders.py` additionally checks all67 packed rings/UV guards and33
camera geometries; the observation-only `atlas_trace.py` reports actual versus
diagnostic unclamped LOD and weighted taps without editing production GPU code.
