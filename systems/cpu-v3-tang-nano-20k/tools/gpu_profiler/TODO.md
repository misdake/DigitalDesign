# GPU profiler remaining qualification

Implemented interfaces belong in [README.md](README.md), the
[session specification](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md).
Detailed pending architecture remains in the local project design area.

- [ ] Extend physical-button qualification to initially held and simultaneous
  buttons. These cases must remain separate from automatic software stimulus;
  a completed transfer must never serve as a resume request.
- [ ] Preserve the isolated RTL replay's explicit cold-only/no-CPU-or-Display
  scope. Its warm checkpoint reconstruction remains unqualified. The separate
  full-system cycle source observes live CPU/GPU/Display contention directly;
  it does not extend the isolated replay's coverage.
- [ ] Separate memory-read, codec/CRC and UART-line cost before increasing baud.
  Hardware stays at the qualified115200-baud configuration; faster dividers and
  sustained CPU/host feeding remain unqualified.
- [ ] Omitting preimages/static bytes later requires proof of initialization or
  exact recoverable content. Missing dependencies must still fail; catalog
  references alone are not captured bytes.
