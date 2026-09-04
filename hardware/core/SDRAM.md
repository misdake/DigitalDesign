# Fitted SDRAM integration

Tang Nano 20K contains a 64-Mibit, 32-bit SDR SDRAM die connected through the
GW2AR-18C dedicated memory interface. It is one fitted device, not inferred
BSRAM and not a divisible bit balance.

## Clock contract

The initial project profile is entirely synchronous at 54 MHz:

```text
27 MHz board clock -> rPLL -> 54 MHz 0 degrees -> implicit project clock
                              54 MHz 180 degrees -> physical SDRAM clock only
```

User logic, caches, arbitration, refresh scheduling, and Controller HS all use
the 0-degree clock. The 180-degree output is private to the target wrapper and
never appears in ordinary module IO. There is no asynchronous bridge.

If a design cannot close timing at 54 MHz, it should first pipeline the failing
path. A clock-enable only reduces the rate at which state changes; it does not
relax the single-cycle timing constraint. A future slower profile should lower
the complete project and controller together. A 1:2 logic/controller split is
deferred until a measured need justifies a related-clock bridge.

## Gowin dependency

The target configuration uses Gowin SDRAM Controller HS 1.0. Its encrypted
implementation remains in the installed IDE at
`ipcore/SDRC_HS/data/sdrc_hs_top.vp` and is not copied into the repository.
During a build, `build.tcl` locates the IDE relative to `gw_sh`, stages the
encrypted file beside the generated QN88 configuration includes, and adds that
staged file to synthesis. Exported source therefore contains no machine-local
absolute path and fails with the missing installed path when the required IP is
not available.

QN88 configuration is 32 data bits, 2 bank bits, 11 row bits, 8 column bits,
CAS latency 2, and tRP/tRCD/tWR/tMRD/tRFC of 2/2/2/2/9 controller cycles.
Refresh is deliberately explicit at the raw boundary. A reusable transaction
controller above it will own the deadline and backpressure application traffic.

Controller HS does not provide a read-valid output. Board characterization of
this exact QN88/54-MHz configuration found eight consecutive read beats at
phases 3 through 10 after the one-cycle READ command pulse; `cmd_ack` occurs at
phase 9 and therefore cannot substitute for read-valid. The Tang Nano 20K
target wrapper converts that measured window into `sdram_read_valid`. Cache and
application modules consume the explicit valid signal and do not search for a
known first word in returned data.

## Validation baseline

`hardware/vendor/gowin/examples/sdram` exercises 64 aligned 32-byte bursts distributed over
all four banks and multiple rows, holds the data while issuing refresh commands,
then reads and compares every word. It reports through the shared `DDHT` UART
status protocol with test ID `0x03`.

`systems/cpu-v3-tang-nano-20k/examples/cpu_v3_system` wires the full CPU V3
system: `CpuV3Core`, the instruction fetch queue, split two-way caches,
`CpuV3MemoryArbiter`, `BootDmaDevice` and `BootDmaEngine`, and the shared
`DisplaySdramPort`. Stage0 in a BSRAM boot ROM below word `0x400` DMAs Stage1
from SPI Flash into SDRAM, and Stage1 loads the application. The current D-cache
is write-back and write-allocate; cache lines cross the 54-MHz side as four
64-bit beats and the fixed related-clock gearbox converts them to eight 32-bit
Controller HS beats at 108 MHz. The current complete-system composition and
validation boundary are documented in
[`systems/cpu-v3-tang-nano-20k/docs/architecture.md`](../../systems/cpu-v3-tang-nano-20k/docs/architecture.md).

The former `cpu_v3_sdram` example (test ID `0x05`, three BSRAM blocks, a
reported Fmax of 57.282 MHz) was folded into `cpu_v3_system` during the
consolidation and is no longer a separate build.

The first Gowin 1.9.11.03 board run at 54 MHz completed with:

- zero setup and hold violations;
- reported Fmax 89.154 MHz for the 54 MHz project domain;
- one rPLL, no BSRAM, and no DSP use;
- repeated UART success frames after all 64 lines completed.

The encrypted controller emits two width-obscured warnings and PnR emits the
known `PR1014 clk_d` generic-clock warning. These also occur in the previously
validated controller projects. They remain visible and are not globally
suppressed.
