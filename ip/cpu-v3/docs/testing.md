# CPU V3 test ownership

Tests follow the implementation contract. A short assertion over private local
state can stay in its source file. Larger suites and process/RTL harnesses use
adjacent private `#[cfg(test)]` modules, so implementation details do not need
to become public merely to test them. CPU V3 does not need an external `tests/`
directory for these contracts.

| Owner | Test location and boundary |
| --- | --- |
| Architectural encoding and decoding | Adjacent `architecture/*_tests.rs`; fixed words, invalid encodings and round trips |
| Instruction simulator | `architecture/sim_tests/`; arithmetic, addressing, control, devices and FPU semantics |
| RCC backend | `rcc_backend/tests/`; allocation, control, debug, FPU, integer, machine, validation and value lowering |
| Hardware core | `hardware/tests/`; emulator execution, emulator/RTL co-simulation, geometry lowering and hardware leaves |
| Fetch, cache and arithmetic leaves | Adjacent private test modules beside their implementation owners |

Each grouped suite has one `mod.rs` for shared imports and registration. Core
`harness.rs` owns bounded drivers and trace comparison; case files own their
programs and assertions. Helpers are visible only inside the private test
suite. Independent architectural, numerical and RTL expectations remain
separate from the implementation they qualify.

Run ordinary cases with `cargo test --release -p cpu-v3`. Run explicit RTL
qualifications with:

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label "cpu-v3 emu/rtl co-sim" -CargoArgs @("-p", "cpu-v3", "--lib", "--", "--ignored", "--nocapture", "--test-threads=1")
```

Set `IVERILOG_EXE` and `VVP_EXE` when Icarus is not on `PATH`. The ignored RCC
production-source check also requires `RCC_GRAPHICS_API_RCC` and
`RCC_GRAPHICS_SCHEMA_RCC` to point to
`systems/cpu-v3-tang-nano-20k/rcc`; set `RCC_RUSTC_TEST_DIR` under the main
`target/` for its rustc artifacts. Every simulator
driver retains a finite step/cycle bound. Core replay retention is described
in this directory's [README](/ip/cpu-v3/docs/README.md#co-simulation-replay-files).
Board boot, native SDRAM, arbitration and application tests belong to the
[system suite](/systems/cpu-v3-tang-nano-20k/tests/README.md).
