# SDRAM traffic and latency board probe

The `sdram_traffic_probe` Gowin example fits only the memory service, a traffic
generator, counters and UART. No CPU core, Flash, HDMI, DSP or BSRAM is included.
The target owns one PLL: all seven client interfaces and timestamps run at 54 MHz;
every request passes through the production arbiter, SharedSdramPort, pair gearbox
and 108 MHz native controller. The SDRAM clock uses the fitted 292.5-degree phase.
The dedicated wide binding retains the system's I/O-register option and SDRAM
input/output constraints. The production system configuration is unchanged.

## Current qualification

The independent cycle combination and connected pin fixture pass. Nine shortened
workload windows pass pin-model data/guard checks, real UART bit decoding and CRC32.
The full board image passes Gowin 1.9.8.11 Education PnR and setup/hold audit for
GW2AR-LV18QN88C8/I7. No board has been programmed or measured in this qualification.

| Measurement | Result | Scope |
| --- | --- | --- |
| Native controller synthesis | 234 LUT + 29 ALU = 263 Logic; 181 registers | Exclusive hierarchy under this traffic stimulus |
| Gearbox synthesis | 226 Logic; 322 registers | Excludes its controller child |
| Arbiter synthesis | 90 Logic; 6 registers | Seven live client paths |
| SharedSdramPort synthesis | 400 Logic; 153 registers | Scalar and line traffic |
| MC combination synthesis | 979 Logic; 662 registers | Sum of four exclusive scopes, excludes PLL/test instrumentation |
| Full probe post-route | 7535 Logic; 3923 registers; 5082 CLS; 1 PLL | Includes generator, counters, UART and target |
| Controller / logic fmax | 135.078 / 57.487 MHz | Required 108 / 54 MHz |
| Worst setup / hold slack | 0.493 / 0.425 ns | Full fitted project, including SDRAM I/O constraints |

Logic here is the tool's LUT+ALU accounting. Synthesis hierarchy does not provide
a per-module post-route allocation; the full-project PnR total is listed separately.
Workload-specific constant propagation can affect module area, so these numbers
are not a universal MC budget. Address-dependent xorshift patterns exercise all DQ
lanes rather than presenting fixed high-word constants.
`resources.py` collects these reports and records source/constraint/bitstream SHA256
in `measurement-manifest.json` inside the selected project output.

## Workloads

The generator initializes and independently reads back an 8 KiB pattern region.
Every subsequent read is checked and writes reproduce the address-dependent pattern.
Each mode offers traffic for 2^20 logic clocks, drains all admitted work and finishes
any partial four-sector GPU group, freezes counters, then serializes a report.
UART reporting and initialization are excluded from measured elapsed time.

| Mode | GPU traffic | Additional traffic |
| --- | --- | --- |
| 0 | Four 128 B reads, fixed four-bank row-hit group | None |
| 1 | Four 128 B writes, fixed four-bank row-hit group | None |
| 2 | Four 128 B reads, sequential 512 B groups across 8 KiB | None |
| 3 | Four 128 B reads, alternating groups 4 KiB apart | None |
| 4 | Fixed read groups | Display 32 B every 150 clocks |
| 5 | Alternating read/write groups | CPU/command traffic below |
| 6 | Alternating read/write groups | CPU/command + periodic Display |
| 7 | Alternating read/write groups | CPU/command + fifty Display reads every 7500 clocks |
| 8 | Alternating read/write groups | Saturated instruction/data/command clients + periodic Display and DMA |

Normal CPU/command traffic is instruction 32 B every 1728 clocks, data 32 B every
288 clocks with every third request writing, DMA scalar every 4096 clocks with
alternating read/write, and GPU read-only 128 B every 512 clocks. Mode 8 offers
instruction/data/read-only work every clock. CPU clients have one pending/active
transaction each. Busy clients count skipped release opportunities as missed load
deadlines instead of silently claiming the offered bandwidth was achieved.
Display batches retain one common release timestamp for all fifty members; its
entire backlog drains before reporting. Loads are explicit estimates, not a replay
of a captured CPU/display trace. Rates and window/divisor are in the source.

Four-sector groups currently traverse the existing unchained service, including
real sector gaps and possible CPU/Display interference. No oracle-predicted
continuation is substituted for physical service. Future chained hardware must
be a separate image/configuration with the same reporting boundary.

## Accounting and UART record

Latency starts at actual generator release, before arbitration. Every client
reports requests/completions, mean/min/max first response and completion latency,
and mean/max arbitration wait. Write first response means its completion ack.
Subtract mean wait from mean first/completion to obtain service after acceptance.
Display members use their batch release; GPU group time includes all four sectors
and intervening arbitration. Window and elapsed are distinct: elapsed includes
drain and is the denominator for bytes actually completed.

Useful bus utilization is `(read_bytes + write_bytes) / (elapsed * 8)`; 100% is
432 MB/s at the 54 MHz, 64-bit boundary. Owner occupancy counts logic clocks from
accepted request through accepted last response. These are separate metrics:
occupancy includes row/refresh/arbitration transport and does not imply useful DQ.
Scalar DMA counts two useful bytes, not a fictional full 64-bit payload. This probe
does not separately count raw SDRAM command/DQ electrical activity.

The UART runs at 115200 baud (divisor 469). A record contains 104 little-endian
u32 words followed by CRC32: magic `SDMC`, version/mode/failure flag, nominal clock,
window/elapsed/occupancy/read bytes/write bytes, group count/sum/max, missed deadlines,
then thirteen words per client in Display/I/Data/DMA/RO/FBR/FBW order. Client words
are counts, three u64 sums (wait/first/complete), three maxima and two minima.
The implemented sums use bounded u32 counters with zero high words: one outstanding
job bounds cumulative intervals by elapsed, or at most fifty times elapsed for
Display batches. Window is bounded to 2^24 plus a 65536-clock drain watchdog.
Pattern/protocol/timeout failure is sticky, marks the record and stops subsequent
modes. Captures with that flag or invalid CRC/accounting fail decoding.

## Build, validation and board handoff

```powershell
& scripts/run-cargo.ps1 -Subcommand run -Label sdram-board-build -CargoArgs @('-p','digital-design-hardware-gowin','--example','sdram_traffic_probe','--','target/sdram_traffic_probe_gowin','--build')
python hardware/vendor/gowin/examples/sdram_traffic_probe/resources.py target/sdram_traffic_probe_gowin
& scripts/run-cargo.ps1 -Subcommand test -Label sdram-pin -CargoArgs @('-p','gpu-v2','--test','sdram_emu_rtl','--','--ignored','--nocapture')
```

Board enablement comes from the user. The built SRAM image is
`target/sdram_traffic_probe_gowin/impl/pnr/sdram_traffic_probe.fs`; preserve and verify
its manifest SHA before programming. No Flash image replacement is required.
After the board is enabled, use the project's existing-image programmer and capture
the chosen UART port explicitly:

```powershell
python hardware/vendor/gowin/examples/sdram_traffic_probe/decode.py --port COM_PORT --seconds 30 --output target/gpu-v2-sdram/board-capture
```

`COM_PORT` is a placeholder for the verified board port. Offline input uses
`--input CAPTURE.bin`, or `--input uart.hex --hex` for pin-fixture records.
The capture has a time/byte limit and saves original bytes, JSON records and a
per-client CSV. Record image hash, board/tool identity, capture interval and repeat
agreement before treating results as physical evidence. The pin fixture runs its
two related clocks at a model-safe scaled frequency; its nominal-rate bandwidth
conversion is not a board measurement.
