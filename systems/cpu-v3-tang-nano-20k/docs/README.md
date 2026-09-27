# CPU V3 Tang Nano 20K system documentation

These documents describe the fitted CPU V3 system. Reusable ISA and processor-IP contracts live in
the [`ip/cpu-v3` documentation](../../../ip/cpu-v3/docs/README.md).

## Documents

- [`architecture.md`](architecture.md): current complete-system composition, clock and memory paths,
  boot chain, device ownership, display integration, and validation boundary.
- [`cpu-v3-optimization.md`](cpu-v3-optimization.md): concise one-sentence optimization index.
- [`cpu-v3-optimization-record.md`](cpu-v3-optimization-record.md): append-only implementation,
  measurement, rejected-alternative, and validation detail normally consulted only for archaeology.
- [`gpu-raster-comparison.md`](gpu-raster-comparison.md): measured tile/scanline raster rates,
  framebuffer-control savings, rejected alternatives, and full-GPU preservation boundaries.
- [`framebuffer-cache.md`](framebuffer-cache.md): accepted eight-DPB storage and fused quad-owner
  contract, independent resource/throughput results, and the pending system-integration boundary.
- [`boot-image-format.md`](boot-image-format.md): version 3 boot package and manifest format.
- [`flash-layout.md`](flash-layout.md): fitted external-Flash placement, programming workflow, and
  boot-progress reporting.

Add one sentence to `cpu-v3-optimization.md` in the same milestone change as every CPU V3
optimization and append its evidence to `cpu-v3-optimization-record.md`. Update `architecture.md`
whenever the current system contract, composition, clocks, memory path, or device ownership changes.
