# GPU profiler remaining qualification

Implemented interfaces belong in [README.md](README.md), the
[session specification](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md) and
[driver bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md).
Detailed pending architecture remains in the local project design area.

- [ ] Integrate the CPU-owned explicit column producer/profile selector, then
  bind its actual firmware, generated catalog and qualified layout bitstream.
  Current ABI2 support is consumer/fixture qualification; the formal producer at
  this base still emits row ABI1. Do not relabel its snapshots or old board runs.

- [ ] Qualify physical Button2 pause/resume and Button1 dump separately from
  automatic software stimulus, including held/simultaneous buttons and no resume
  merely because transfer finishes.
- [ ] Preserve explicit cold-only/no-CPU-or-Display scope. Warm internal state or
  actual contention needs additional reconstructable state/workload evidence.
- [ ] Separate memory-read, codec/CRC and UART-line cost before increasing baud.
  Hardware stays at the qualified115200-baud configuration; faster dividers and
  sustained CPU/host feeding remain unqualified.
- [ ] Omitting preimages/static bytes later requires proof of initialization or
  exact recoverable content. Missing dependencies must still fail; catalog
  references alone are not captured bytes.
