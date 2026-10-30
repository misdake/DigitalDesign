# GPU frozen-input board diagnostic

This separate software application uses the formal renderer and its
[published-input capture bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md).
It adds no GPU/MC RTL, device register, BSRAM or hardware UART engine.
The example gate is a small mode wrapper around the same shared
`rcc/profiler_runtime.rs` used by ordinary renderer builds. Those builds currently
enable the manual hook by default; `GPU_PROFILER_HOOK=0` omits it.

The default scene is the canonical minimal triangle. The optional complete scene
uses the existing box, RGB triangle and full948-face rabbit assets:24meshlets,
1141triangles, textured unlit box, vertex RGB and lit rabbit. Its fixed camera is
at `(0,0.22,2.8)`, pitch about `-0.0855` radians, focal200, far199. The public
world-light input is `(-0.45,0.8,1.1)`; the renderer normalizes and transforms it
once. A precomputed view-space light must not be supplied as a world direction.
The public
renderer owns matrices, material state, DMA and command construction. There is
no parallel hand-maintained command stream or Flash/code byte array.

## Buttons and lifetime

Button2 toggles pause/resume at a prepared submission boundary. Pausing alone
does not dump. Button1 requests a dump only while paused; successful TX leaves
the application paused. A fresh Button2 edge after transfer resumes the same CB
once. Buttons use normalized high levels and20ms software debounce; initial or
held levels do not trigger a new action. Hold a press until the next gate while
GPU work is running.

The formal driver has drained the previous GPU, sealed and published the
upcoming CB/objects/actual UI and pinned its references. The gate never submits,
presents, mutates those inputs or changes cache policy. Display keeps old Front.
Resume returns1 to the driver, which rechecks health, republishes and starts once;
debug pause is excluded from usable frame statistics. Fatal GPU/memory/Display or
local UART timeout preserves Prepared resources and sends no COUNT/START.
A PC-detected damaged/missing transfer can be retried with another Button1 press
while the board remains healthy and paused.

LED values2 and4 identify paused and dumping;0 marks resume. They are diagnostic
software writes, not proof of CPU/GPU progress.

Manual mode additionally emits short console notices outside GPS1 records:
`GPUPROF:R` at the first ready gate or explicit resume, `GPUPROF:P` after pausing
and after a completed dump, and `GPUPROF:D` before dumping. These notices leave
the frozen framebuffer and prepared command input untouched. The decoder ignores
this console text; automatic stimulus keeps its existing binary stream. Screen
contents do not change merely to acknowledge a button. Wait for readiness,
release any held button, then press Button2 to pause; only after P press Button1
to dump. A completed dump returns to P, not R.

The full scene has a fixed camera, so running and paused images can look alike.
`OBS xx.x` in the upper-left corner is the previous successful submit-to-observed-
completion interval in milliseconds, including publication and CPU observation
delay; it is not FPS or a pause/dump indicator. No acknowledgement is drawn into
the frozen framebuffer. Use the UART notices to distinguish those states.

## Preparation and identity

Run from this worktree. Export the current complete system and production
GPU+streaming MC into new directories, then bind the selected qualified hardware:

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --example cpu_v3_system -- target/profiler-system-export
cargo run --release -p cpu-v3-tang-nano-20k --example gpu_profiler_export -- target/profiler-replay-export --mc
python systems/cpu-v3-tang-nano-20k/examples/gpu_profiler_board/tools/prepare.py --out target/profiler-prepared --hardware path/to/qualified-hardware --system-export target/profiler-system-export --replay-export target/profiler-replay-export --primitive path/to/gw2a/prim_sim.v --full-scene
$env:CPU_V3_BOOT_EXAMPLE = 'gpu_profiler_board'
$env:GPU_PROFILER_MANIFEST = (Resolve-Path target/profiler-prepared/build.json).Path
cargo run --release -p cpu-v3-tang-nano-20k --bin cpu-v3-boot-assets -- target/profiler-prepared/boot
```

`prepare.py` requires all32exported hardware/constraint files to match the selected
qualified artifact before permitting reuse. It binds source bytes, the replay
export, primitive model, canonical assets and mode options to a manifest ID.
The sole OUT_DIR/build-script/boot-assets route still generates firmware. Assets
use existing native producers; the full wrapper also provides the OBS font glyphs.
Generated files remain under `target/`.

Programming is an explicit operation through the audited hardware CLI. Save a
`digitaldesign.gpu-programming-receipt` version1 JSON containing manifest ID,
`board_id`, `programmed_at`, successful `verified:true`, and actual `firmware` and
`bitstream` `{path,bytes,sha256}` entries. Do not assert verification merely because
files built. Firmware identity is external to the manifest ID, avoiding a self-hash
cycle.

## Representative short programs

Use `--short-case rtt` or `--short-case memcpy` instead of `--full-scene` during
preparation; the same `CPU_V3_BOOT_EXAMPLE=gpu_profiler_board` and manifest-bound
boot-assets route select the application. Modes are mutually exclusive; short
cases use their canonical assets, without `--column-catalog`. The manifest and
asset catalog both bind the case, and capture ABI2 / command ABI4 remain unchanged.

- **RTT:** repeat the existing checked two-stage caller. A producer clears/draws
  the borrowed color/depth allocations at `0x600000` / `0x640000`, then waits for
  actual GPU completion. The consumer samples the borrowed texture into Display's
  back buffer. Only consumer submissions enter the profiler runtime; producer
  submissions continue normally. A consumer pause freezes the complete 256-KiB
  source allocation and its generation; resume submits that same consumer once.
  The submission pin survives closing the CPU texture view. GPU/Display waits
  finish before the next producer can rewrite the source. The original finite
  nearest/bilinear callers retain their limits.
- **MEMCPY:** repeat the minimal renderer plus the existing native-encoder
  diagnostic tail. It copies 1024 B `0x700000 -> 0x710000`, then
  `0x710000 -> 0x720000`, and 1024 B `0x700e00 -> 0x720e00` across a 4-KiB
  boundary. The CPU initializes only the source; the GPU performs the copies.
  Explicit FLUSH / descriptor DMA / WAIT precede three native COPY commands.
  Destination preimages and dependent reads are captured by the existing bridge.
  This is a diagnostic producer specialization, not a new public renderer API.

The MEMCPY host generator uses the already tested
`tests/fixtures/capture_native_tail.rcc`, modifying only a private generated
`gpu_capture.rs` before capture header/range construction and before the live
publication lease. It cannot alter frozen gate inputs. Normal renderer sources,
encoders, scheduler, cache handoff and retirement remain canonical. Boot exports
include `profiler-source-specialization.json` (original/generated module and
extension hashes) and `profiler-specialized-gpu-capture.rs` for inspection.

Manual pause/dump/resume is identical in all modes. With `--automatic-test`, gate
numbers count RTT consumers only. The duplicate second-gate lease and later
third-gate Front/depth support the existing adjacent-output comparison; MEMCPY
also requires checking copied destination ranges, not just framebuffer PNGs.
The next capture observes GPU results only after normal retirement. An offline
CPU control peer proves recording, source closure and gating; it performs no
raster or native COPY writes. Actual UART, pixels, copied bytes and physical
buttons remain board acceptance work for the device owner. Cold replay does not
qualify warm CPU/Display contention.

A retained configuration can be reused only after all 32 fresh source/constraint
fingerprints match its preserved qualification. If original `.fs` bytes are
unavailable, record that fact and the retained qualification separately; do not
invent bitstream bytes or mark a new FPGA write/verification. A software-only
boot artifact is not a programming receipt.

## Automatic diagnostic stimulus

Add `--automatic-test` during preparation to bind a software test mode into
`manifest.options.automatic_test`; default is disabled. Gate1 continues. Gate2
waits20seconds for receiver setup, exports the same lease twice and returns
Resumed once. Gate3 exports again and remains paused. The later Front/depth can
be compared with a cold replay of the earlier CB. The normal GPU/driver datapath
is unchanged. This is **not physical-button qualification**, and implies no host
ACK or automatic retransmission.

For automated runs, open the receiver before reloading SRAM. The BL616 capture
script accepts optional `-WriteTiming` (live raw file and arrival-offset CSV) and
`-StopAfterIdleSeconds`; existing callers retain their behavior. Choose an idle
bound longer than the20second startup wait, such as30seconds, plus a separate
positive overall duration bound.

Use the [profiler tools](/systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/README.md)
to decode complete attempts, check identities, replay with explicit cycle/wall
limits and compare raw planes. DRAM snapshots do not contain internal warm caches
or CPU/Display contention. HDMI capture remains a separate visual check.
