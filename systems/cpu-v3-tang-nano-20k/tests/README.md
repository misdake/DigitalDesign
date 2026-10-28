# System regression tests

The reusable [GPU profiler](/systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/README.md)
has bounded format, viewer and production-GPU observer checks in its own tool
directory. It does not require a full-frame run or change synthesized logic.

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

The `gpu_pipeline::bloxorz` caller reuses the original pure game rules and host
rolling geometry. It executes 22 input/time ticks, two whole-Frame early-return
cancellations and 20 submissions with logical floor/bridge/block batches. Native
mesh selection, matrices, UI, observed input edges, reset/level transitions and
continuous input reuse are checked independently. The shared production CPU/FPU
fixed-port runner guards command/object stores as well as UI, with only the
explicit pre-publication Recorded Target relocation permitted. Per-window costs
compare changed/steady identical geometry work; none represents real GPU/cache/
Display timing or full gameplay. The four original host geometry checks run with
the imported reference module.

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
with legacy row, explicit row and pure column capture profiles. It emits portable
manifest/source identities and real CPU header/range/public-read fixtures under
`target/capture-layout`, checks exact physical spans, resource mismatches,
end-bound rejection and lease expiry, and preserves legacy row bytes. Complete
payloads are host copies during the frozen gate. It provides no UART, GPU pixel
or board qualification; see [capture bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md).

The separate bounded `capture_draw` target records one native triangle across
three real RCC/CPU Prepared gates for each row/column profile. It checks actual
DRAW bindings, positive Z, material/transform changes and A/B/A command-slot
reuse with distinct tokens. Set `GPU_CAPTURE_DRAW_DIR` to archive its manifest,
program, origins and each gate's complete frozen inputs/preimages outside the
build cache. GPU/Display peers only retire control work; independent GPU cold
replay and pixel/depth verification remain consumer responsibilities.

The same target's `real_full_scene` case executes the pinned formal profiler
frame body, with complete box/RGB/rabbit assets and OBS UI, across three real
gates per row/column profile. The middle gate resumes after a modeled long pause;
production input/time baselines and statistics invalidation/recovery are checked
without changing the scene body. `GPU_CAPTURE_FULL_SCENE_DIR` selects its output;
`GPU_CAPTURE_LAYOUT_DIR` overrides the older layout target's default archive
path. Set these and `CARGO_TARGET_DIR` outside the source tree when preserving
evidence. Full-scene bytes remain CPU producer inputs, with no GPU pixel or board
qualification implied.
