# System regression tests

The `host` integration target groups boot packaging, module resolution, driver,
UI, display-control and timer tests. Filter by module when checking one area:

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --test host boot::
cargo test --release -p cpu-v3-tang-nano-20k --test host gpu_rcc_driver::
```

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
