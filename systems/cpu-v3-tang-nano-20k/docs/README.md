# CPU V3 Tang Nano 20K system documentation

These documents describe the fitted CPU V3 system. Reusable ISA and processor-IP contracts live in
the [`ip/cpu-v3` documentation](../../../ip/cpu-v3/docs/README.md). Gowin primitive references
live in [`hardware/vendor/gowin/doc`](../../../hardware/vendor/gowin/doc/README.md).

## Documents

- [Application quickstart](/systems/cpu-v3-tang-nano-20k/docs/application-quickstart.md): first build/frame, target language restrictions and application/static-content integration.
- [Constructing SDK values](/systems/cpu-v3-tang-nano-20k/docs/sdk-values.md): readable numbers/colors, source-value builders and derived asset/framebuffer descriptions.
- [Static assets](/systems/cpu-v3-tang-nano-20k/docs/static-assets.md): constant descriptions, source includes, external placement and loader-owned CPU data images.

- [UI text helpers](/systems/cpu-v3-tang-nano-20k/docs/ui-text.md): stateless ASCII number formatting over the public GPU UI, separate from menu layout and driver ownership.
- [CPU canvas](/systems/cpu-v3-tang-nano-20k/docs/cpu-canvas.md): CPU-owned RGB565 spans and simple shapes, with explicit GPU/Display handoff.

- [Application profiling](/systems/cpu-v3-tang-nano-20k/docs/application-profiling.md): bounded backend/scene/frame runs, offline reports and comparable sealed measurements.

- [Application builds and existing artifacts](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md): explicit application selection, owned recipes, pure packing and fresh snapshot publication.

- [Same-build application artifacts](/systems/cpu-v3-tang-nano-20k/docs/application-build-artifact.md): final linked debug/listing, source and resource identities, relocatable exports and offline symbol lookup.

- [Full-system cycle composition](/systems/cpu-v3-tang-nano-20k/docs/system-cycle-emu.md): exact clocks, unique native memory ownership, CPU/GPU/Display wiring, explicit preloaded cold entry and passive scanout.

- [Display cycle components](/systems/cpu-v3-tang-nano-20k/docs/display-cycle.md): edge-driven line RAM ports, source/color read calendar, registered pixel/sync and digital TMDS outputs, knownness, collision diagnostics and bounded RTL qualification.

- [Host time and input](/systems/cpu-v3-tang-nano-20k/docs/host-simulation.md): explicit elapsed events, Device5 latest snapshots, Windows XInput adaptation and the preloaded native-edge CPU composition.

- [Explicit GPU API internal core](/systems/cpu-v3-tang-nano-20k/docs/gpu-api-storage.md): public resources, native recording, submission, unique retirement and Display handoff.
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
- [`gpu-driver.md`](gpu-driver.md): native single-CB diagnostic library used by gpu_demo and dualshock2_ui, encoding, DSEG3 and submission/fault semantics.
- [GPU profiler](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md): sim/emu collection, cold GPU/MC replay, passive trace identities and offline viewing.
- [`flash-layout.md`](flash-layout.md): fitted external-Flash placement, programming workflow, and
  cold-loader LED and application reporting.
- [`system-display-test.md`](system-display-test.md): CPU-only cold-boot diagnostic,
  scratch/LCOPY ownership, continuous framebuffer presentation and UART results.

Update the owning specification whenever its implemented contract changes.
Keep new verification output in the worktree's `target/`; the existing historical
optimization records are not live ledgers and receive no new measurement rows.
