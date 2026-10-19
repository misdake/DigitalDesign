# CPU-only cold-boot and Display diagnostic

`rcc/system-display-test.rs` is the application selected by
`boot-applications.conf`. It uses the fixed v5 cold loader and the production
CPU, caches, seven-owner arbiter, related-clock gear, native SDRAM controller
and 800x480 HDMI Display. The GPU remains inactive. The retained
`rcc/display-demo.rs` requires the retired GPU ABI and is not this diagnostic.

## Memory and ownership

All addresses below are 16-bit word addresses. The application uses no heap.
Logical words `0x4000..0x7fff` are private scratch, outside the loader-cleared
static arena `0x0000..0x1fff` and below the stack starting at `0xc000`.
Normal legacy DSEG is zero. A scratch line at `0x4000 + offset` is filled with
16 RGB565 words and copied by `LCOPY` to the same low 14-bit offset in the
selected physical page. `DWAIT` completes each copy before scratch reuse.
LCOPY preserves source dirty state; blocking clean-plus-invalidate subsequently
completes scratch writeback and removes CPU read aliases.

Before each global maintenance command, the diagnostic issues `DCLEANL` for
scratch line `0x4000` and completes it with `DWAIT`. `DCLEANL` admission waits
for older CPU stores to drain, including the helper's stack save. This baseline's
system-control pulse requires an idle cache: `maintenance_busy` alone does not
describe an ordinary store miss or an unconsumed response. This application
supplies that precondition; it does not repair the general maintenance interface.
The final instruction audit permits no load, store, call or FPU operation
between this drain and the global command.

Color A begins at `0x200000` (physical page 128), B at `0x218000` (page 134).
Each surface initializes exactly 375 tile-linear 16x16 tiles, 96,000 words;
slot padding is not overwritten. Six independent fixed samples at the start,
an interior tile and the final pixel of both surfaces are read after cache
invalidation. A diagnostic-only read leaf temporarily writes legacy DSEG,
performs one load and restores zero. Its generated instruction interval is
checked against the exact build-generated application: no stack access,
store, call or branch may occur under the temporary mapping. This is not a
general DSEG window API.

During initial A/B filling, Display retains its existing startup behavior;
the application does not add a black-screen startup gate. Thereafter only the
back surface is updated. Each surface retains vertical RGB565 bars and one
moving white tile near the center. The previous white tile is restored before
the next update of that surface. The leftmost tile column distinguishes A from B.

## Completion and observable results

After updating the back surface, the application completes cache maintenance,
stages its base and encoded RGB565 format, and requests NEXT_SWAP. It waits for
pending/partial to clear and reads back the exact active base and format.
Frame-index changes alone do not establish a swap. Invalid-address, underflow
and memory-error status stop the test. Polling has finite counters.

Every successful actual presentation emits DDHT v1 test ID `0x0b`, status zero,
and updates the LED heartbeat. Five further frame changes space updates for
visual inspection. Failure emits two nonzero DDHT frames, sets an error LED
pattern, and halts with `0xbd00 | status`:

| Status | Meaning |
| ---: | --- |
| 1 | CPU cache maintenance failed |
| 2 | SDRAM sample readback disagreed |
| 3 | Display invalid-address, underflow or memory error |
| 4 | Frame index stopped progressing within the polling bound |
| 5 | Requested target was not applied within eight frame changes |

The functional test runs the exact generated package for four presentations
and checks both initial surfaces pixel by pixel. The separate native RTL test
uses real SPI, CPU/cache/arbiter, gear/controller, SDRAM pins, Display and UART
pin decoding for two presentations. Its existing CL2 pin model uses a
10.102 ns controller period (about 99 MHz) with the exact 2:1 logic relationship;
it does not establish 108 MHz pin timing. The emitted FPGA uses 54/108 MHz
CPU/controller and 33.3 MHz pixel clocks. Neither test is physical-board proof.
