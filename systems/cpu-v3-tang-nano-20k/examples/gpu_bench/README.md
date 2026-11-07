# GPU board throughput suite

One CPU V3 boot application runs static native command buffers against the
production GPU and MC. No GPU, CPU, timer or memory-controller RTL is added.
The asset producer reads the system's GPU allocation authority; the build script
packs the exact application and DRAM resources into the normal CB06 boot image.

The current catalog is version 2, suite revision `asset-banks-v2`. A complete
56-case micro suite fits in one boot image. The allocator prefers the original
high asset bank, then places an entire allocation in the low bank when it cannot
fit high; it never splits a texture, mesh, descriptor or CB across banks. Each
bank exports one resource file (`bench.bin`, optionally `bench_low.bin`). Boot
loads all resources before measurement; no measured-run upload is introduced.
The 16-bit wire token binds the ordered bank addresses, lengths and contents;
the artifact/catalog SHA identities retain the complete input identity.

This is a suite revision: physical references, resource files and token differ
from the previous high-only catalog. Case order, dimensions, options, work
counts, three warmups and 31 samples remain unchanged. Previous board results
keep their original identities and are not a performance baseline for this revision.

## Workloads and measurement boundary

The catalog in the generated `gpu-bench-assets/assets.json` records every input,
command count, repeated meshlet count, V/T count and decoded-position coverage
reference. It includes:

- Full-viewport single-triangle RGB, texturing, Lighting, depth pass/reject/write
  and alpha blending. The triangle extends beyond all four viewport corners.
- Nearest/bilinear/trilinear with coherent coordinates, fixed interior/edge/corner
  coordinates, large non-mipped working sets and explicit cache invalidation.
- Mip-ID colors and small finite gradients with high positive bias, allowing
  real two-mip filtering to be distinguished from an integer/clamped LOD. Paired
  full-screen half-texel gradients exercise ordinary tile-boundary overhead.
- V-count sweeps with one rejected triangle; T-count sweeps with backfaces;
  meshlet behind/left/right rejection, vertical out-of-screen and degenerate
  triangles; subpixel, small, thin, oblique and large raster footprints.
- Empty submission and complete-plane CLEAR controls.

Each case runs three warmups and 31 measured samples. Input upload, target CLEAR
and ordinary setup have a separate measured submission. Every measured command
buffer includes its real binding/DMA prologue: binding-valid flags reset on
submission. Both submissions end with WAIT(all), FLUSH and END. Timing stops only
after busy clears, including actual writeback, DMA and router drain.

The timed CPU path only submits and waits in a hot instruction-cache loop. It
constructs no commands, copies no resources, emits no UART and never presents.
The timer is cleared immediately before COUNT/START; observation ends after busy
clears. Timer access, CPU SEND and completion-observation overhead remains
included; the empty control
calibrates it. Do not subtract a convenient constant from reported raw intervals.
The vertex and triangle sweeps are integrated front-end measurements, not isolated
DSP instruction timing; DMA and rejection costs are part of their boundary.

On cold reset Display has `front_valid=0`. Until the first NEXT_SWAP it produces
black groups without SDRAM reads. The application finishes the whole measurement
epoch before its first present. Afterwards it continuously shows previews, with
UART case markers. Those later intervals are **not** headless samples. Reload the
FPGA before another headless epoch; do not rerun a former buffer and call it cold.
The host selects the latest **started** epoch. An incomplete new run cannot fall
back to a previous complete epoch. Markers must match the exact catalog/sample
count; duplicate samples, unknown case IDs and Display-active flags are rejected.

The existing interval timer has 1 us resolution and saturates at 65535 us. Every
sample retains a separate millisecond observation and saturation flag. Saturated
or sub-resolution results do not get an invented precise rate. Reports retain
all samples, p05/p50/p95, MAD and a deterministic 2000-resample bootstrap median CI.
The CI describes this measured sample set; it does not remove the 1 us instrument
uncertainty, which is also reported. Nearly deterministic hardware can produce
identical quantized samples. No result is an HDMI/vblank-limited frame rate.

`reference_pixels` and `reference_quads` are floating edge tests of decoded V96
positions. Shared/quantized edges can differ from real coverage. The full-screen
case has a generous geometric margin and exactly 96000 covered pixels. Tiny-case
reference counts are estimates, not hidden hardware counters. Sampler scenarios
report end-to-end work rate; no-miss sampler II cannot be inferred from an arbitrary
UV pattern or an unverified mip choice. Cold-cache setup cost is visible separately.

## Build and run

From the repository root, offline preparation only:

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_bench/run.ps1
```

For a fresh audited hardware build followed by explicit board programming and
bounded UART/HDMI capture, with the actual VCP and FFmpeg executables:

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_bench/run.ps1 `
  -OutputDirectory target/gpu-bench-run-01 -BuildHardware -Program -Port COM4 `
  -Ffmpeg ffmpeg -Ffprobe ffprobe
```

An existing `HardwareDirectory` may be reused without `BuildHardware`; the normal
source/bitstream/resource/timing audit must pass. A mismatch is not silently
accepted. Flash and SRAM are each programmed once. The suite changes the board's
application; restoring a demo is a separate normal boot-image operation.

Use `-Scene full` for the tracked classic scene's camera0: all 24 meshlets,
192 room triangles, one RGB triangle and the complete 948-face rabbit. The
six-case catalog measures each plane CLEAR, box+RGB, rabbit, the combined frame
without CLEAR, and the identical combined command stream with both CLEARs.
It reuses the demo's native record and tiled texture producers. Each sample's
untimed setup resets both planes and invalidates texture lines, so standalone
object times use cold caches and cannot be added to predict the combined frame.
The default `-Scene micro` runs the component catalog. Direct boot builds select
the full catalog with `CPU_V3_BOOT_EXAMPLE=gpu_bench` and `GPU_BENCH_SCENE=full`.

The capture profile is the measured USB Video 1600x1200 YUYV 5-fps mode. It keeps
full-range YUV codes, decodes with BT.601 and center-samples the measured 4x5 scale
to recover a 400x240 preview. This is not bit-exact RGB565 readback. UART chunk
timestamps and video wall timestamps associate settled previews only after the
matching completed measurement epoch. A gallery from before a reset is excluded.
Capture-card queues add latency, so the selected frame is near the end of the
1.2-second preview hold. The generated contact sheet is a visual diagnostic,
not a replacement for a source-pixel oracle.

Board capture requires the shared profiler `capture_bl616_uart.ps1` interface:
`-WriteTiming` and `-StopAfterIdleSeconds`, producing live
`uart.bin.timing.csv` with columns `offset,count,utc`. The runner checks these
parameters before programming. This suite does not carry a second capture-script
implementation. Until that profiler delivery is integrated, offline preparation
works and explicit board runs fail before device access. CSV byte ranges must
cover the exact raw capture, contiguously and in timestamp order. A packet is
associated with the arrival of its last byte. Archived `.timing.json` remains
readable only when CSV is absent; an invalid CSV never falls back to JSON.

## Wire format and outputs

Every UART packet is 20 bytes: ASCII GBEN; version1; kind; six little-endian u16
fields (`case`, `sample`, `microseconds`, `milliseconds`, `flags`, `asset_token`);
then CRC16-CCITT of the preceding 18 bytes, initial0xffff. Kind0 starts a measurement
epoch, kind1 reports work, kind5 reports setup, kind3 completes the epoch, kind2
marks a preview and kind128 is a failure. Start/completion fields contain catalog
and sample counts. Flag0 means saturated; flag2 marks Display-active preview work.

The host requires all 31 unique work and setup samples per case and a completed
epoch. It excludes a prior gallery and reset-truncated runs, validates the asset
token/CRC and rejects duplicate measurements inside one epoch. Raw packets, chunk
timing, manifest/firmware/config identities, samples.csv, report.json and recovered
preview images stay in the selected `target` directory. No generated binaries,
videos, fit snapshots or historical measurement ledger is checked into Git.
Reports require a fresh output directory. Work and setup saturation are recorded
separately; a saturated work interval is never assigned a precise throughput.
The runner records source-file, manifest, firmware and configuration hashes;
`programmed` becomes true only after the explicit programming steps succeed.
The gallery requires the complete report's raw-capture and manifest hashes to
match its inputs. It saves derived events under its new gallery directory and
never rewrites the raw capture or older report.

## Small checks

```powershell
python systems/cpu-v3-tang-nano-20k/examples/gpu_bench/tools/test_bench.py
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-bench-protocol `
  -CargoArgs @('-p','cpu-v3-tang-nano-20k','--test','gpu_bench')
```

The native RCC protocol test has a 200000-step bound. Optional `tools/smoke.py`
uses a fresh exported system project and the actual production GPU with an
independent delayed memory peer, a 3000000-cycle bound and 90 seconds per case.
Its default rejected-meshlet case omits the separately timed CLEAR setup and
initializes the unused color plane in the peer. It verifies command and rejection
behavior, not physical MC timing. Full-screen RTL runs are intentionally not a
routine gate; use the board and capture card for this suite.
Both `smoke.py` and `probe_sampler.py` accept `--compile-only` for offline HDL
compatibility checks. This writes a source-hashed `compile.json` and does not
invoke VVP or create an execution/throughput summary. The smoke target address
comes from the asset manifest's `targets`, derived from the system layout.

`tools/probe_sampler.py --project <exported-system-directory> --out <fresh-directory>`
isolates the unmodified exported texture cache and Color modules. It supplies
continuous four-tap groups from one resident sector, or alternating fine/coarse
groups from two resident sectors, and counts read-issue intervals after warmup.
It requires exactly one/two cold fills, no later misses, and all pixel results.
The bound is 10000 clocks and 20 seconds per mode. This probe excludes upstream
membership, rasterization and physical SDRAM costs.
`--expected-interval 2` is the default assertion for the current II2 handoff;
`--expected-interval 3` is an explicit legacy comparison, and `0` observes without
qualifying a cadence. Drain completion uses public result/valid ports, rather
than retired internal Color register names. A compiled probe alone does not
prove the interval: the source-hashed execution result is required.

The current cache-to-Color handoff first reads weights and registers the paid
address, then reads texels and exposes the result, then Color consumes the two
tap pairs on successive edges. The existing paid-address slot can prepare the
next group during the first Color edge. On the second Color edge, RAM replaces
DO only after the consumer has sampled its old value. Continuous resident input
therefore admits one four-tap group every two clocks; two nonzero mip groups
need four clocks per pixel. Startup latency does not change this steady cadence.
An integer LOD can suppress one group, making a full-GPU four-clock result
insufficient evidence for eight-tap trilinear throughput. Use the mip-ID pictures
and the controlled probe together when interpreting the board results.

## Qualification boundary

This software suite is separate from the default boot application and does not
modify GPU/MC RTL or the formal driver. Build selection uses
`CPU_V3_BOOT_EXAMPLE=gpu_bench`; generated constants, asset copies and the package
come through the existing OUT_DIR and `cpu-v3-boot-assets` authority. The
application-owned recipe is exposed by `cpu-v3-app build gpu-bench`; `--scene full`
selects the six-case complete-scene catalog.

`python -B systems/cpu-v3-tang-nano-20k/examples/gpu_bench/tools/test_assets.py`
checks whole-bank allocation, failed-request preservation and protected ranges.
The `gpu_bench` Rust target retains its protocol check and has two explicit,
bounded bank qualifications. With `GPU_BENCH_QUALIFICATION_ROOT` bound to frozen
inputs, `cargo test --release -p cpu-v3-tang-nano-20k --test gpu_bench banks:: -- --ignored`
checks all 56 raw workload outputs, a six-case low/high full-scene pair, and a
complete CB06 CPU/functional-GPU epoch including every real START and CRC packet.
Its controlled host clock skips the initial UART attachment delay; these tests
make no timing claim. The original overflowing raw micro reference is a GPU
workload oracle only and is never accepted or installed as a boot package.

Run default-image tests with the default selection. In particular,
`application_instruction_boundaries_are_safe` and
`cold_board_application_initializes_surfaces_and_presents_continuously` assert
the default application: selecting the benchmark is not a valid configuration
for those two tests. Their previous failures remain in the original evidence;
they are not silently converted into passes or skipped by this suite.

Archived 56-case microbenchmark and six-case complete-scene reports retain their
original firmware/source identity. Some software sources changed afterwards;
those board results do not qualify this delivery. Pre-II2 three-clock probe
reports also remain historical. Host protocol/statistics checks, offline firmware
packaging and HDL compilation do not establish new board throughput, physical
SDRAM timing or image correctness. No performance numbers are reconstructed from
new host tests.
