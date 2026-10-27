# Frame statistics components

This example checks RCC frame-time formatting. Its timestamp source is the
production SystemControl millisecond channel; it does not submit a GPU command
or present a frame.

- [Implemented components](/systems/cpu-v3-tang-nano-20k/examples/frame_stats/docs/components.md)
- [System wall-clock contract](/systems/cpu-v3-tang-nano-20k/docs/wall-clock.md)
- [SystemControl](/systems/cpu-v3-tang-nano-20k/src/hardware/system_control/mod.rs): continuously running main-clock timer, device 0 / channel 10.
- [RCC time](/systems/cpu-v3-tang-nano-20k/rcc/time.rs): timestamp and modular elapsed-time helpers.
- [RCC frame statistics](/systems/cpu-v3-tang-nano-20k/rcc/frame_stats.rs): shared four-word frame state and five glyph IDs.
- `main.rs`: bounded compiled CPU V3 tests, using the default optimizer.

Run the CPU check with `cargo test -p cpu-v3-tang-nano-20k --example frame_stats`.
Run `cargo test -p cpu-v3-tang-nano-20k --test host time_device:: -- --include-ignored`
with `IVERILOG_EXE` and `VVP_EXE` set for production device RTL, actual CPU/cache
RECV, maintenance/DWAIT/watch, and functional-model checks. The timer testbench
has a fixed watchdog and tests 131080 active clock edges.
