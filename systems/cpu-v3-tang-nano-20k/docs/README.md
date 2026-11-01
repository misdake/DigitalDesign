# CPU V3 Tang Nano 20K system documentation

These documents describe the fitted CPU V3 system. Reusable ISA and processor-IP contracts live in
the [`ip/cpu-v3` documentation](../../../ip/cpu-v3/docs/README.md). Gowin primitive references
live in [`hardware/vendor/gowin/doc`](../../../hardware/vendor/gowin/doc/README.md).

## Documents

- [Host time and input](/systems/cpu-v3-tang-nano-20k/docs/host-simulation.md): explicit elapsed events, Device5 latest snapshots and Windows XInput adaptation.

- [Explicit GPU API internal core](/systems/cpu-v3-tang-nano-20k/docs/gpu-api-storage.md): public resources, native recording, capture-aware submission, unique retirement and Display handoff.
- [Prepared scene values](/systems/cpu-v3-tang-nano-20k/docs/gpu-api-scene-values.md): source-world transforms, six-argument native conversion, material/state, independent texture views and persistent external UI updates.

- [Explicit GPU application callers](/systems/cpu-v3-tang-nano-20k/docs/gpu-explicit-examples.md): migrated examples, application-owned completion, RTT, input and bounded caller qualification.
- [Column scene firmware](/systems/cpu-v3-tang-nano-20k/docs/gpu-column-scene.md): explicit non-RTT serial scene profile, offline assets, physical spans and firmware receipt.
- [GPU board throughput suite](/systems/cpu-v3-tang-nano-20k/examples/gpu_bench/README.md): static native workloads, headless timing, UART sample statistics and HDMI validation.
- [`architecture.md`](architecture.md): current complete-system composition, clock and memory paths,
  boot chain, device ownership, display integration, and validation boundary.
- [Wall clock](/systems/cpu-v3-tang-nano-20k/docs/wall-clock.md): SystemControl millisecond timestamps, RCC access, wrap limits and functional-model time.
- [DualShock 2](/systems/cpu-v3-tang-nano-20k/docs/dualshock2.md): autonomous Device5 receiver, latest-only CPU snapshot protocol, pin ownership and standalone cold-boot test.
- [Calibrated sticks](input-sticks.md): eight-bit calibration, native Q16.16 motion/direction/strength, cached gamepad frames and optional four-way activation hysteresis.
- [`cpu-v3-optimization.md`](cpu-v3-optimization.md): concise one-sentence optimization index.
- [`cpu-v3-optimization-record.md`](cpu-v3-optimization-record.md): historical implementation,
  measurement, rejected-alternative, and validation detail normally consulted only for archaeology.
- [`boot-image-format.md`](boot-image-format.md): CB06 COPY/ZERO stream, host layout checks, Display startup and completion contract.
- [Resource renderer](/systems/cpu-v3-tang-nano-20k/docs/gpu-renderer.md): retained test-only Renderer/Frame contract; current callers use the explicit API.
- [Borrowed render targets](/systems/cpu-v3-tang-nano-20k/docs/gpu-render-targets.md): retained legacy target-registry regression contract.
- [GPU input pipeline](/systems/cpu-v3-tang-nano-20k/docs/gpu-input-pipeline.md): retained test-only double-slot scheduler and UI mirror contract.
- [GPU capture bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md): published-input lease, optional diagnostic gate, bounded dependency reads and paused timing semantics.
- [`gpu-driver.md`](gpu-driver.md): native single-CB diagnostic library used by gpu_demo and dualshock2_ui, encoding, DSEG3 and submission/fault semantics.
- [GPU profiler](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md): published-input snapshots, TX framing, cold GPU/MC replay, passive trace identities and offline viewing.
- [`flash-layout.md`](flash-layout.md): fitted external-Flash placement, programming workflow, and
  cold-loader LED and application reporting.
- [`system-display-test.md`](system-display-test.md): CPU-only cold-boot diagnostic,
  scratch/LCOPY ownership, continuous framebuffer presentation and UART results.

Update the owning specification whenever its implemented contract changes.
Keep new verification output in the worktree's `target/`; the existing historical
optimization records are not live ledgers and receive no new measurement rows.
