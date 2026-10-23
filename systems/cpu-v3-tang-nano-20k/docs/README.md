# CPU V3 Tang Nano 20K system documentation

These documents describe the fitted CPU V3 system. Reusable ISA and processor-IP contracts live in
the [`ip/cpu-v3` documentation](../../../ip/cpu-v3/docs/README.md). Gowin primitive references
live in [`hardware/vendor/gowin/doc`](../../../hardware/vendor/gowin/doc/README.md).

## Documents

- [`architecture.md`](architecture.md): current complete-system composition, clock and memory paths,
  boot chain, device ownership, display integration, and validation boundary.
- [Wall clock](/systems/cpu-v3-tang-nano-20k/docs/wall-clock.md): SystemControl millisecond timestamps, RCC access, wrap limits and functional-model time.
- [`cpu-v3-optimization.md`](cpu-v3-optimization.md): concise one-sentence optimization index.
- [`cpu-v3-optimization-record.md`](cpu-v3-optimization-record.md): append-only implementation,
  measurement, rejected-alternative, and validation detail normally consulted only for archaeology.
- [`gpu-raster-comparison.md`](gpu-raster-comparison.md): measured tile/scanline raster rates,
  framebuffer-control savings, rejected alternatives, and full-GPU preservation boundaries.
- [`framebuffer-cache.md`](framebuffer-cache.md): accepted eight-DPB storage and fused quad-owner
  contract, independent resource/throughput results, and the pending system-integration boundary.
- [`boot-image-format.md`](boot-image-format.md): CB06 COPY/ZERO stream, host layout checks, Display startup and completion contract.
- [`gpu-driver.md`](gpu-driver.md): formal RCC native encoding, one opaque CB, fixed high-memory ownership, scoped DSEG3 access and submission/fault semantics.
- [`flash-layout.md`](flash-layout.md): fitted external-Flash placement, programming workflow, and
  boot-progress reporting.
- [`system-display-test.md`](system-display-test.md): CPU-only cold-boot diagnostic,
  scratch/LCOPY ownership, continuous framebuffer presentation and UART results.

Add one sentence to `cpu-v3-optimization.md` in the same milestone change as every CPU V3
optimization and append its evidence to `cpu-v3-optimization-record.md`. Update `architecture.md`
whenever the current system contract, composition, clocks, memory path, or device ownership changes.
