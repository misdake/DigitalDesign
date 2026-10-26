# System regression tests

The `host` integration target groups boot packaging, module resolution, driver,
UI, display-control and timer tests. Filter by module when checking one area:

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

The `gamepad` target executes production RCC input code against an independent
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

gpu_pipeline::resource_usage compiles a test-only Renderer/Frame/Pass caller
model over the native engine. It checks lexical early-return cancellation,
post-Pass UI edits before seal, transfer/disarm, explicit close drain and implicit
Renderer freeze without hidden submission or waiting. Concrete misuse examples
must fail before lowering. The adapter in fixtures/gpu_renderer_scope_model.rs
is not the production facade: helper-domain isolation still needs the accepted
source-aware compiler capability. Camera, Mesh, frame statistics and the full
first usage round are not supplied by this scope/UI model. GPU/Display progress
is a controlled sequence of status reads, not elapsed hardware time.

Core/cache/arbiter emulator-vs-RTL comparison remains the independent
`system_cosim` target. Its functional machine and checkpoint support lives in
`system_emu`; it is not a benchmark harness. The former benchmark directory,
image exporter, calibration and folder-driven suite are removed.

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

Routine `iverilog`/`all` validation excludes these expensive extended examples;
bounded signature/range rejection tests still run. Changes to boot, Display
startup or the native memory path require selecting the affected extended
case. This split changes validation cost, not the qualification supplied by a
short protocol or functional test. A full-frame profile is not a default test,
and no offline test substitutes for board verification.

The `gpu_demo_first_person` target runs the production RCC camera/control flow
with bounded CPU steps and a separate floating basis/light reference. Its frame
control peers verify that render timing excludes present/vblank, retains linear
RGB565, and rejects failed/saturated measurements. Pixel and serial timing stay
in the dedicated native GPU/DS2 RTL tests.
