# CPU V3 Tang Nano 20K system documentation

These documents describe the fitted CPU V3 system. Reusable ISA and processor-IP contracts live in
the [`ip/cpu-v3` documentation](../../../ip/cpu-v3/docs/README.md).

## Documents

- [`architecture.md`](architecture.md): current complete-system composition, clock and memory paths,
  boot chain, device ownership, display integration, and validation boundary.
- [`cpu-v3-optimization.md`](cpu-v3-optimization.md): living optimization history, benchmark evidence,
  current Stage status, and risk-scaled validation record.
- [`boot-image-format.md`](boot-image-format.md): version 3 boot package and manifest format.
- [`flash-layout.md`](flash-layout.md): fitted external-Flash placement, programming workflow, and
  boot-progress reporting.

Update `cpu-v3-optimization.md` in the same milestone change as every CPU V3 optimization. Update
`architecture.md` whenever the current system contract, composition, clocks, memory path, or device
ownership changes.
