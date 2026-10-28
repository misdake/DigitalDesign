# GPU frozen-input board diagnostic

This separate software application uses the formal renderer and its
[published-input capture bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md).
It adds no GPU/MC RTL, device register, BSRAM or hardware UART engine.

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
