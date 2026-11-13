# GPU profiler

Open the generated `profiler.html` beside its `profile.json` to view a report.
The source file `viewer.html` is a template; opened directly, it displays an
empty-data explanation instead of a report.

New RTL and cycle-emu capture output is the same `capture.json` + `events.jsonl`
protocol (`digitaldesign.profiling-capture`, version2), consumed by the same
parser/model/validator/report builder. GPU logic events use148 quantum ticks per
cycle; native writes retain exact rational timestamps at their actual backing
mutation edge. Optional CPU/Display/service/native domains are declared, not
filled with zeros. Legacy raw files remain importable with their original point
coverage. See the [common point contract](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md#common-collector-protocol).

Finite point comparisons live in `tests/check_common_events.py` and
`tests/check_native_points.py`; the latter compares actual native byte writes and
completion planes without asserting identical timings for different clock/init
configurations. `tests/common_native_replay.py` exercises the actual cold
GPU+MC input/build/image path with a saved tiny input fixture.

## Full-system cycle emu

The ordinary level-29 runner is application-owned:

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/bloxorz/tools/profile.py path/to/verified-plan.json
```

The [Bloxorz profiling specification](/systems/cpu-v3-tang-nano-20k/applications/bloxorz/docs/profiling.md)
owns exact-plan fields, selection/input rules, wait-register interpretation,
budgets and qualifications. Build its `bloxorz-profile-runner` target and bind
the new executable's actual SHA; retired runner command names are not aliases.
`application_sidecar.py` explicitly resolves the published runner v1 format to
the application codec. Old raw captures and reports keep their original format
and derived semantics. `interval_activity.py` provides common model accounting,
independent of application conditions or thresholds.

Build and run in the feature checkout, using its own `target` (never the main
checkout's target). Add `--profile-out <new-directory>` to the existing
`cpu-v3-system-emu` command; retain explicit entry, logic/present and host bounds.
`--profile-max-events` defaults to2000000, maximum10000000;
`--profile-max-frames` defaults to2, maximum1024 (zero disables registered
scanout images; native history has its separate two-target budget).

Pass that capture directory to this same `profiler.py build <capture> --out
<new-report>` command, then `profiler.py render <report>/profile.json --out
<new-render>`. `--allow-partial` is required for bounded, truncated or undrained
captures; `--require-images` requires a complete saved scanout or a proven native
command image. `--frames` additionally requires a complete matching application
scanout for every selected presentation. `--binding
<report>/report-inputs.json` checks exact input identities on rebuild. This path
consumes live system events, not a Prepared snapshot or a functional GPU replay.

The common webpage displays real CPU retirement/holds, GPU phase/stage activity,
submit/drain/present, shared memory competition and registered scanouts. Client
64-bit payload and directly observed native model32 slots have separate bins,
labels and CSV boundaries; neither is physical board pin bandwidth. Unknown
timing stays unavailable. New captures include committed CB latch/issue/release,
DMA/VT/T/quad/ROP ownership, all GPU load tracks and native color/D16 history.
Every profiling source uses the same full GPU viewer as RTL replay by default;
`?view=system` explicitly selects CPU/shared-memory context in the same HTML.
Legacy captures keep their real submission rows, recorded GPU service memory
histograms and registered preview in that common layout, with missing instruction
and fine stage histories marked unavailable. Legacy memory acceptance is at the
arbiter after board descriptor slots, labelled separately from new GPU-master
timing. No unknown source or missing request size is guessed. Registered RGB PNGs
remain unchanged. See the
[source contract](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md#full-system-cycle-source)
for time, masking, synthetic startup and device coverage.

For application F1/F2 exports, use the production entry:

```powershell
python -B systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py build path/to/capture --out target/frame-report --frames 1,2 --source-identity path/to/identity.json --require-images
python -B systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py render target/frame-report/profile.json --out target/frame-render --frames 1,2 --require-images
```

The identity JSON contains `source`/`tree` (or
`application_source`/`application_tree`), `boot_sha256` (or `package_sha256`)
and optional `runtime_sha256`/`executable_sha256`. Program/executable hashes must
match the saved capture. The canonical session links to generated
`frames/frame-N/profiler.html`; frame lists, Fit, statistics and CSVs use its
presentation/window scope rather than the entire session. CPU semantic frame
ownership is not inferred. Complete scanout selection excludes boot black
rasters, so the default two-raster capture budget may be insufficient; an
explicit budget such as 12 can retain two application presentations. Select
capture bounds deliberately; event limits still cap at 10000000.

Native history takes one 8MiB preimage and real masked writes/cache events.
Its independent budget covers two target groups and proven completed prefixes.
400x240 tiled RGB565/D16 downloads are separate from 800x480 registered scanouts.
Open owners and truncated image tails remain explicit. Old raw captures cannot
gain absent PC/owner/cache facts through rebuilding.

`--replace-report` stages and validates a generated report bundle, archives its
original bytes/hashes, then replaces it. Generated frame directories and native
assets are supported; unrelated user files and links still refuse replacement.

## Result-only simulator backend

The existing dump frontends support a second backend for fast software/game
feedback. In project terminology **sim** executes a CB to its final memory result;
**emu** would reproduce module effects every RTL cycle. Sim does not create a
profiling session or estimate cycles, queues or utilization.

Build the native core's CLI once:

```powershell
& scripts/run-cargo.ps1 -Subcommand build -Label "native GPU sim" -CargoArgs @("-p", "gpu-v2", "--bin", "gpu-v2-sim")
python -B systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/replay.py path/to/snapshot --backend sim --out target/gpu-sim-result --timeout 60
```

The default executable is `target/release/gpu-v2-sim[.exe]`; `--simulator` selects
an explicit binary. `--max-work` defaults to64000000. Sim uses finite work and
process bounds rather than `--max-cycles`/`--rtl`. RTL remains the default backend
with its existing explicit exports/cycle/timeout requirements.

System-program dumps continue through `system_dump.py` into the same GPS1
snapshot. UART capture can select `snapshot_cli.py receive ... --backend sim`
and the same process/work options; decoding and pause/resume ownership are
unchanged. It validates the executable before opening a port. There is no new
main frontend or capture protocol.

`result.json` uses `digitaldesign.gpu-result` version1. `profiler.html` reuses
the existing viewer in final-result mode: image plus raw color/depth and every
writable recorded range, including no-target and multiple-target streams.
`profiler.py render result.json --out <directory>` verifies/copies the raw results
and produces that same view. No `profile.json`, trace or performance statistics
are fabricated. Raw tiled RGB565/D16 remains authoritative; PNG is a viewing aid
using the captured target's color interpretation.

Capture reads check known physical bytes, writes check declared writable bytes,
and writes make bytes known for later GPU commands. Physical aliases must agree;
partial glyph atlases and copy dependencies do not require fabricated preimages.
Failure produces diagnostics and preserves any native partial-memory/access
files for inspection, without publishing a successful result/frame/viewer.
Input provenance and executable hashes identify the software result; a qualified
input receipt does not make sim output a board timing measurement.

The CPU/system simulator uses the same native core through a thin Device4
adapter and shared memory. Its interface and completion/error effects are in
the [GPU simulator specification](/ip/gpu-v2/docs/functional.md).

Explicit `Gpu` captures use header2 and build manifest3. Their
`record-ranges-v1` policy keeps the column pixel packing while deriving addresses
and allocation bounds from the recorded range table. Legacy header1/manifest2
retain their fixed-map validation. No-target streams have no fabricated plane
preview; cold replay preserves outputs for every writable range, including
targets preceding the header's final target. See the
[archived snapshot format](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md#archived-snapshots).

`tests/test_recorded_replay.py` is an explicit native GPU/stream-MC tier. Set
`GPU_API_CAPTURE_FIXTURE_DIR`, `GPU_API_CAPTURE_RTL_DIR` and
`GPU_API_NATIVE_REPLAY_DIR` to retained C producer fixtures, production exports
and a fresh output root. It checks raw MEMCPY, Load preimages, visible UI and
all outputs of multiple targets. Each replay has a finite cycle/process limit;
these controlled-input tests do not qualify A ownership, board timing or
CPU/Display contention.

## Commands

Run from the repository root:

```powershell
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py check --node node
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py build systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/tests/fixtures/pingpong --out target/profiler-example
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py render path/to/profile.json --out target/profiler-view
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py capture path/to/manifest.json --out target/profiler-capture
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py build target/replay/capture --snapshot target/replay/snapshot --out target/profiler --require-images
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py build target/replay/capture --binding target/profiler/report-inputs.json --out target/profiler --replace-report --require-images
```

Open the output directory's `profiler.html`. It embeds the data and works without
a server. The JSON and submission/command/meshlet/CPU CSVs retain integer cycles.
Cold replay also exports per-command PNGs and `images/index.json`, with command,
actual completion cycle, target, format and image hashes. The page embeds these
images: final thumbnail by default, hover enlargement and selected-command image.
Right-click instruction flow clears selection/highlight and restores the final
view; other regions retain the browser menu. Older activity-only captures have
no image preview. Image observations never add cache flushes or waits.

`capture` automatically builds `OUT/profile/profiler.html` after a successful run;
add `--snapshot GPS1_DIRECTORY --require-images` for a complete image report.
RTL replay saves its input snapshot and automatically produces `profile/profiler.html`,
`images/index.json`, command PNGs and `color.png`. The offline `build` command above
regenerates the same images from the snapshot, four verified seed files, observer
trace and same-run raw color/depth, without executing RTL or reading old PNGs.
`--raw DIRECTORY` supplies raw outputs from a separate copy of that same capture.
`report-inputs.json` binds trace/source/seed/snapshot/raw file hashes and the actual
Display1 LUT; `--binding` verifies them on another rebuild.
This manifest declares external dependencies: keep those files available for a
rebuild. The HTML and PNG exports are self-contained for viewing. After moving
inputs, pass their new capture directory and explicit `--snapshot`/`--raw`
directories together with the original `--binding`; hashes are still checked.

Saved images default to the actual Display1 sRGB conversion before PNG compression.
`--preview raw` is an explicit diagnostic export; `--preview capture` follows the
capture field only with `--capture-format known`. The default declaration is
`unknown`: a stored zero does not prove a color format. Raw planes and their
capture metadata stay unchanged. `render` saves the validated embedded PNG bytes
directly and never converts them again. Missing snapshot, image journal or raw
evidence yields an explicit unavailable reason; `--require-images` refuses such
a report. The SIM backend remains final-result-only and refuses that strict
command-history option.

Output directories must be new or empty. `build` and profiling `render` support
`--replace-report` to update a stable report entry: they first validate a new
report in a sibling staging directory, archive the previous generated directory,
verify every archived file hash, then publish at the original path.
`report-replacement.json` records the original path, archive path and previous
hashes. Unknown files, links/reparse points or mismatched HTML/profile data refuse
replacement; a failed rebuild leaves the published report intact. Archives and
other historical image directories are retained.
Captures with master-interface events also show full/selected-command memory
traffic and export `memory-transactions.csv`. The scope is three 64-bit GPU links
at the declared logic clock. Peak uses 256 cycles; wire/effective bytes, request
acceptance, actual data strobes and terminal response remain separate. Missing
events show unavailable statistics. See the [measurement contract](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md#derived-display).
`render` accepts schema v2 or converts the earlier single-frame v1 JSON; it does
not need the old frozen RTL folder. Unsupported versions and invalid references
are rejected. `build --allow-partial` is diagnostic and visibly marks incomplete
captures; ordinary `build` requires a complete capture.

The small source fixture is **synthetic**, exercises A/B/A address reuse,
backend retirement tails and CPU overlap, and is not a performance measurement.
`check` runs bounded Python format/correlation and Node math/interaction tests.
No command creates ZIP archives, flashes hardware, commits or pushes.

## Capture a harness

Place `// GPU_PROFILER_OBSERVER` once inside the testbench module. Supply an
explicit manifest; paths are relative to the manifest or absolute:

```json
{
  "version": 1,
  "title": "My scene",
  "clock_hz": 54000000,
  "top": "tb",
  "testbench": "tb.v",
  "sources": ["gpu-export.v", "prim_sim.v"],
  "inputs": {"program.hex": "program.hex", "assets.hex": "assets.hex"},
  "bindings": {"GPU": "gpu", "CLOCK": "clk", "RESET": "reset", "ENABLE": "ce", "CYCLE": "cycle"},
  "max_cycles": 2000000,
  "timeout_seconds": 60,
  "submissions": 3,
  "submission_labels": {
    "1": {"frame_id": 1, "name": "A"},
    "2": {"frame_id": 2, "name": "B"},
    "3": {"frame_id": 3, "name": "A reused"}
  }
}
```

List all exported source files and primitives. Source entries may instead be
`{"path": "...", "sha256": "..."}` to require a frozen identity. Every source
basename and input filename must be unique. Generated capture filenames
(`profiler_tb.v`, `capture.json`, `capture.vvp`, `compile.log`, `run.log`,
`events.txt`, `load.csv`) are reserved and rejected for sources and inputs.
Use relative filenames in testbench
`$readmemh` calls. The runner copies a snapshot into a new/empty output directory,
records source/input/observer hashes, compiles and runs there, and enforces both
cycle and wall limits. It stops after the requested number of fully drained GPU
submissions, or records a bounded incomplete stop. `--compile-only` only prepares
and compiles the snapshot. `IVERILOG_EXE` and `VVP_EXE` override executable names.

The observer binds the current GPU's hierarchy and physical T/quad/ROP ownership.
It never writes DUT state. Device4 accepted SENDs are observed independently of
CE. It does not assume a scene, meshlet count, CB base or first-completion stop.
The capture contains one monotonic counter domain; a reset during capture is
rejected and requires a new session.

Optional harness-side CPU annotations use:

```verilog
profiler_cpu_begin(17, 2, "Record next frame"); // span ID, logical frame ID, label
// CPU executes while the previous GPU submission is active.
profiler_cpu_end(17);
```

Call these on a stable testbench phase, such as the falling edge, or adapt existing
software marker observations to these tasks. IDs must be unique per session.
Labels are at most 96 bytes. CPU spans need not coincide with GPU submission
boundaries. A span still open at the explicit capture end is cropped and marked;
this permits recording frame N+1 while the last captured GPU frame N completes.
Logical frame IDs are annotations: multiple CB submissions may belong to one
frame, and a prepared frame may not yet have submitted.

## Production-GPU smoke test

This creates a fresh export, so it has no archived `target` prerequisites:

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --example gpu_profiler_export -- target/profiler-rtl
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/tests/rtl_smoke.py --rtl target/profiler-rtl --out target/profiler-smoke
```

Set `GOWIN_HOME`, or pass `--primitive path/to/prim_sim.v`. The real production GPU
executes three minimal streams A/B/A with CE0 launches and a backpressured memory
peer. CPU markers demonstrate annotation overlap; the harness does not simulate a
CPU or measure CPU performance. The limits are 20,000 cycles and 30 seconds.
The synthetic fixture additionally exercises meshlet/quad ownership and final
cache commit. These checks do not replace rendering or board validation.

## Viewer

The top overview shows green VT ownership and orange pixel-backend activity.
Backend is the per-cycle OR of raster busy, interpolator live returns, allocated
quads and ROP cache contexts, formed before time binning. Both bands include waits
and are independent of type filtering. Later dirty FLUSH maintenance has its own
detailed metric.

- Overview left-drag selects a range; drag its interior to move or edges to resize.
  Double-click outside it, right-click, or Fit session to clear. Right-drag moves.
- Wheel scrolls rows; Ctrl+wheel zooms around the pointer. Middle/right drag or
  Shift+left drag pans time and the current pane's rows.
- Types opens a collapsed popup beside Fit session; WAIT/DMA start hidden.
- Command labels show `CB ID · PC`; hover labels for that occurrence's details.
  Hover the time plot for all alive occurrences, including filtered types.
- Click selects; double-click or Focus selection zooms to its DMA/backend bounds.
- CPU spans and GPU submission intervals share the same time axis. Selection,
  dependency arrows, CSVs and ownership use occurrence IDs, never PC alone.
- Load hover reports the plotted time bin, busy fraction, queue mean/peak or event
  count/rate. Mouse controls (?) lists the principal interactions.

The versioned contract is in the
[system profiler specification](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md).

The right inspector has four independently collapsible, initially open groups:
Inspect Instruction and detail; Legend and its notes; Bin cycles; GPU memory
summary. The frame preview stays visible above these groups. Missing saved images
and zero-quad DRAWs show an explanation instead of an unexplained disappearing
preview. Complete replay reports include the final image and each nonempty
command image; a timing-only JSON does not manufacture them.

Bin cycles and the main memory numbers use one active half-open cycle range.
Selecting a DRAW uses its complete existing lifetime, including associated DMA,
asynchronous jobs and last-quad completion. Focus padding, viewport pan/zoom and
hover do not enlarge that DRAW accounting range. An explicit overview range
replaces DRAW accounting; clearing the range restores the full frame. Non-DRAW
selection retains time-range accounting. View ranges round their actual start
and duration to native cycles, preserving pan width; DRAW lifetime bounds round
outward so no work is truncated. Fractional boundaries remain supported by the
internal exact-event query. Chart hover continues to inspect its rendered bin without replacing
the right inspector range. Instruction selection and redraw preserve each
section's collapsed state.

Read/write bytes use exact saved payload-event timestamps, including fractional
range boundaries. Average rates divide by this range's duration and clock; the
combined peak uses a complete 256-cycle sliding window entirely inside it.
Incomplete coverage keeps known byte counts explicitly marked, with unknown
range rates/peak. Empty ranges have zero bytes and no rate. Legacy timing absence
stays unavailable; the separately labelled full-capture reference is not selected
range accounting. CPU/Display are excluded and 64-bit payload beats count once,
including transferred masked/error beats. These are not physical SDRAM rates.
Byte histograms and wait rows remain in GPU Activity & Memory Throughput on the
shared axis. Narrow screens stack the inspector below the plots. Nonempty DRAW
images retain their own final-quad/cache-commit boundary. For display previews,
`images.preview(..., linear=True)` consumes the owning Display1 Rust LUT:
R/B5 expand to6bits by bit replication, G6 indexes directly, then the fixed64-entry
`LINEAR6_TO_SRGB8` table produces saved sRGB bytes. There is no floating gamma
substitute or browser-side second conversion. `images.build(...,
display_preview=True)` explicitly requests this mode for all images in one report;
it keeps the captured target format and raw plane hashes, while recording the
preview override/LUT identity separately. Historical raw PNGs and binary color/
depth references remain unchanged. Rebuilding from the same raw planes is
repeatable; re-rendering a report only embeds those already converted PNGs.

### Exact memory timeline in the activity pane

Read/write histogram rows use the same adaptive time bins and axis as GPU metrics,
with actual full-slot **bytes/bin** rather than burst sizes distributed over time.
The boundary is GPU RO, framebuffer-read and framebuffer-write logical64-bit
links; it is not physical SDRAM pins and excludes CPU/Display traffic. Each
observed payload slot contributes8B once, including masked/error slots. Accepted
request count and requested bytes are shown separately in the shared-bin hover.

Outstanding counts accepted requests until their exclusive completion edge.
Live-links-without-data counts links with outstanding work but no payload slot
in that clock sample; write-terminal wait begins after the last full burst beat
and ends at the terminal acknowledgement. Request backpressure is valid/not-ready,
only over the saved `load.csv` sample interval. These link-cycle quantities can
overlap and do not measure an unobserved queue depth or physical bus idle time.

Optional `memory.timeline` version1 retains sparse `[cycle,read_bytes,write_bytes]`
data events, RLE outstanding/wait states and separately bounded backpressure.
Its absence in older v2 profiles means timing unavailable, not zero traffic.
Rebuild from preserved `events.txt`/`load.csv` to obtain timing without rerunning RTL.
Missing MEM_DATA events remain unavailable. The v2 profile version is unchanged;
existing totals, commands, provenance and cold/warm metadata remain authoritative.

Bytes belong to half-open event-time bins `[a,b)` and are never fractionally
allocated. State samples cover `[cycle,cycle+1)` and integrate exact overlap;
means use the known covered duration. Partial boundary bins retain known bytes
with shaded unknown portions; header/hover spell out missing coverage. In
particular, a GPU/load sample may extend one cycle beyond the memory trace.
Full-capture overview totals remain separate from the selected/zoomed interval.
Binary exact-cycle hover also includes the common rendered memory bin.

`tests/test_memory.py` checks trace/schema/conservation boundaries;
`tests/viewer_checks.js` checks actual histogram draw calls, shared hover/zoom/
selection, old-profile unavailability and optional real-profile totals. Pass an
existing pingpong profile followed by an exact-memory profile as its two optional
positional arguments. These tests and report rendering do not execute RTL.

## One GPU frame

Capture with `cpu-v3-system-emu ... --profile-out CAPTURE --profile-frame N`.
For saved captures use `profiler.py build CAPTURE --out REPORT --only-frame N`.
N is the application presentation ordinal; all its CBs are included. Warmup
still executes and its raw state journal remains available, while the browser
and CSV select only N. See [frame scope and bounds](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md#one-gpu-frame).
