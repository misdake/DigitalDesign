# CPU V3 Tang Nano 20K system documentation

These documents describe the fitted CPU V3 system. Reusable ISA and processor-IP contracts live in
the [`ip/cpu-v3` documentation](../../../ip/cpu-v3/docs/README.md). Gowin primitive references
live in [`hardware/vendor/gowin/doc`](../../../hardware/vendor/gowin/doc/README.md).

## Documents

- [`architecture.md`](architecture.md): current complete-system composition, clock and memory paths,
  boot chain, device ownership, display integration, and validation boundary.
- [Wall clock](/systems/cpu-v3-tang-nano-20k/docs/wall-clock.md): SystemControl millisecond timestamps, RCC access, wrap limits and functional-model time.
- [DualShock 2](/systems/cpu-v3-tang-nano-20k/docs/dualshock2.md): autonomous Device5 receiver, latest-only CPU snapshot protocol, pin ownership and standalone cold-boot test.
- [Calibrated sticks](input-sticks.md): eight-bit calibration, native Q16.16 motion/direction/strength, cached gamepad frames and optional four-way activation hysteresis.
- [`cpu-v3-optimization.md`](cpu-v3-optimization.md): concise one-sentence optimization index.
- [`cpu-v3-optimization-record.md`](cpu-v3-optimization-record.md): historical implementation,
  measurement, rejected-alternative, and validation detail normally consulted only for archaeology.
- [`gpu-raster-comparison.md`](gpu-raster-comparison.md): historical raster measurements for the
  retired GPU; its experiment tools and reproduction commands are no longer supported.
- [`framebuffer-cache.md`](framebuffer-cache.md): historical eight-DPB component measurements;
  current behavior belongs to [GPU framebuffer](/ip/gpu-v2/docs/framebuffer.md).
- [`boot-image-format.md`](boot-image-format.md): CB06 COPY/ZERO stream, host layout checks, Display startup and completion contract.
- [Resource renderer](/systems/cpu-v3-tang-nano-20k/docs/gpu-renderer.md): application entry, runnable call sequence, host-approved resources, prepared scene snapshots, persistent text, frame input and observed statistics.
- [GPU input pipeline](/systems/cpu-v3-tang-nano-20k/docs/gpu-input-pipeline.md): internal double input slots, target relocation, persistent UI dirty pages and independent GPU/Display completion.
- [GPU capture bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md): published-input lease, optional diagnostic gate, bounded dependency reads and paused timing semantics.
- [`gpu-driver.md`](gpu-driver.md): lower-level native encoding and single-CB facade, fixed high-memory ownership, scoped DSEG3 access and submission/fault semantics.
- [GPU profiler](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md): passive RTL capture, multi-submission trace identities, correlation and offline viewing.
- [`flash-layout.md`](flash-layout.md): fitted external-Flash placement, programming workflow, and
  cold-loader LED and application reporting.
- [`system-display-test.md`](system-display-test.md): CPU-only cold-boot diagnostic,
  scratch/LCOPY ownership, continuous framebuffer presentation and UART results.

Update the owning specification whenever its implemented contract changes.
Keep new verification output in the worktree's `target/`; the existing historical
optimization records are not live ledgers and receive no new measurement rows.
