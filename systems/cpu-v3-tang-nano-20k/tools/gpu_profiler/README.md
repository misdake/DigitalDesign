# GPU profiler

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
[capture bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md#explicit-record-integration-entry).

`tests/test_recorded_replay.py` is an explicit native GPU/stream-MC tier. Set
`GPU_API_CAPTURE_FIXTURE_DIR`, `GPU_API_CAPTURE_RTL_DIR` and
`GPU_API_NATIVE_REPLAY_DIR` to retained C producer fixtures, production exports
and a fresh output root. It checks raw MEMCPY, Load preimages, visible UI and
all outputs of multiple targets. Each replay has a finite cycle/process limit;
these controlled-input tests do not qualify A ownership, board timing or
CPU/Display contention.

Read-only RTL capture, frozen pre-submit memory snapshots, bounded cold replay
and a self-contained offline viewer. Python uses the standard library. Node is needed for viewer
regressions; Icarus and the Gowin simulation library are needed only for RTL capture.
Production GPU/CPU datapaths and Device4 ABI are unchanged. The formal renderer
owns its optional software capture lease and read bridges.

The [capture bridge](/systems/cpu-v3-tang-nano-20k/docs/gpu-capture.md) owns
resource freezing, publication and the sole later submission. The separate
[board diagnostic](/systems/cpu-v3-tang-nano-20k/examples/gpu_profiler_board/README.md)
owns the button gate and TX-only transport. Remaining qualification and future
extensions are listed in [TODO.md](TODO.md).

## Compiled system-simulation input

Build `cpu-v3-gpu-dump` with `scripts/run-cargo.ps1`. Compile a capture-enabled
program and preserve `CapturePoint::from_program(&program)` alongside its CB06
package; addresses are physical u16 words except the gate's CSEG word offset.
The recipe has this shape (addresses and digest must come from that compilation):

```json
{"schema":1,"format":"cb06","program":{"file":"app-flash.bin","sha256":"<SHA-256>"},"capture_point":{"gate_pc":0,"header_word":0,"ranges_word":0},"memory_seed":0,"max_steps":5000000,"timeout_seconds":30}
```

```powershell
python -B systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/system_dump.py recipe.json --frontend target/release/cpu-v3-gpu-dump.exe --out target/system-dump
python -B systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/replay.py target/system-dump/snapshot --rtl target/profiler-rtl --build build.json --simulation-source target/system-dump/snapshot/source.json --out target/system-replay --max-cycles 1000000 --timeout 180
```

To select native CPU/cache/MC execution, add `"cpu_mode":"native"` and replace
`max_steps` with positive `max_cycles`. The ordinary command above is unchanged;
for example a native recipe contains:

```json
{"schema":1,"format":"cb06","cpu_mode":"native","prefix_gpu":"functional","program":{"file":"app-flash.bin","sha256":"<SHA-256>"},"capture_point":{"gate_pc":0,"header_word":0,"ranges_word":0},"memory_seed":0,"max_cycles":4000000,"timeout_seconds":40}
```

The direct Rust CLI equivalents are `--cpu-mode native --max-cycles 4000000`;
omitted/instruction mode keeps `--max-steps`. Do not mix their budget flags.
Use actual compilation metadata in the recipe, not the placeholder zero addresses.
Native captures use coherent producer metadata but physical GPU payloads and
preserve raw `physical.bin`. Their origin reports modeled clocks, not fake CPU
steps; neither these clocks nor result-only replay are board frame timing. Both
modes pass through the same materializer, lease validation and replay pipeline.
For a short result-only run, use the existing replay command with `--backend sim`
and its `--simulator` executable instead of selecting the RTL backend.

The optional real-backend regression in `tests/test_system_dump.py` uses
`GPU_DUMP_FRONTEND` and `CPU_NATIVE_PREPARED_RECIPE` (a valid compiled recipe).
It runs that same package through both CPU modes and compares input identities.
`GPU_SIM_EXE` enables the bounded simulator success/error/access regressions in
`tests/test_simulator.py`, including valid dense bytes with absent read access.

The optional recipe field `"prefix_gpu":"functional"` enables real synchronous
functional execution of CBs before the bound gate; the CLI equivalent is
`--prefix-gpu functional`. Omitted or `"reject"` preserves the original behavior.
Prior writes enter the target preimage; a prefix error prevents a complete dump.
The target itself is still unconsumed. The source records this mode separately
from `gpu_consumed:false`, which describes that target. No GPU bus cycles are
modeled. This stops at the first reached supplied gate PC; it neither skips an
earlier UART gate nor selects a later invocation. Bind it to the consumer's
actual compiled gate; the default `CapturePoint` lookup is unchanged.

Output directories must be fresh. Export GPU+MC with the existing
`gpu_profiler_export` example's `--mc` flag and bind its files in the build
declaration before compiling the application. The existing `capture_draw`
integration fixture accepts `GPU_PROFILER_RTL` to include this export in its
declaration; its `compiled_program_system_frontend` case uses real compiled RCC
and the system's devices, stops at the gate, and checks a CPU step-limit failure.
The capture frontend stops at the first reached bound Prepared gate, which may
follow real functional prefix submissions. Firmware still owns ordinary versus
eligible selection; raw START ordinal pause is not that gate. Seed and functional
vblank are explicit. Native counters exclude GPU/Display memory competition and
are not board frame timing. Failed GPU prefixes or missing captured reads remain
errors, without an uncaptured-memory/zero fallback.
The [implemented contract](/systems/cpu-v3-tang-nano-20k/docs/gpu-profiler.md#compiled-program-system-capture)
defines provenance and failure/completion boundaries.

Ordinary renderer builds currently enable the shared Button2 pause/resume and
Button1 dump hook. Set `GPU_PROFILER_HOOK=0` when building to omit it. An optional
`GPU_PROFILER_MANIFEST` binds a formal build; otherwise the descriptor ID is zero
and cold replay explicitly labels firmware/bitstream identity unverified.

Start recording before pressing dump. The command below prints **Recorder ready**
only after route selection and live raw recording, then decodes and replays each
complete attempt through the same backend:

```powershell
python -B systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/snapshot_cli.py receive --port COM5 --seconds 300 --out target/uart-capture --rtl target/profiler-rtl --max-cycles 1000000 --timeout 300
```

Omit `--rtl` to record/decode only. Supply `--build` and `--receipt` together for
the strict qualified board route; omission never claims board qualification.
Dump completion keeps the producer paused. A fresh Button2 edge resumes the
same prepared CB once; receiver completion never sends a resume.

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

## Frozen inputs and cold replay

`snapshot_cli.py build-manifest spec.json --out build.json` creates a content
declaration from an explicit source/export/options/catalog list. Header/range
indices are read from the single driver-owned `rcc/gpu_capture_abi.rs`; they are
not duplicated in Python. Build IDs bind logical file names, lengths and hashes;
file locations can be relocated without changing content identity.

```powershell
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/snapshot_cli.py receive --port COM4 --seconds 180 --out target/snapshot-receive
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/snapshot_cli.py decode path/to/uart.bin --out target/snapshot-decoded
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/snapshot_cli.py preflight path/to/complete-attempt --build build.json --receipt receipt.json --out target/preflight.json
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/replay.py path/to/complete-attempt --rtl path/to/fresh-export --build build.json --receipt receipt.json --out target/replay --max-cycles 2000000 --timeout 1800
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/compare.py previous-attempt next-attempt --replay target/replay --out target/comparison.json
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/qualify.py target/bound-board-run --out target/bound-board-run/qualification.json
```

Every attempt is independent. The decoder preserves an error report for a
truncated/corrupt attempt and can accept a subsequent complete retry; it never
combines their chunks. A complete attempt materializes `snapshot.json` and flat
`range-NNN.bin` files with lengths, CRC32 and SHA256. Replay reloads and checks
those files rather than trusting cached JSON. Command padding, objects, actual
UI, every catalog allocation, color/depth preimages and old Front must be present.
Version1 captures preimages even when the CB declares full CLEAR.

The header ID is a declaration, not a hardware self-attestation. A separate
verified programming receipt binds actual firmware and bitstream bytes. Missing
files, changed identities, unsafe filenames, inconsistent aliases or incompatible
ABIs refuse replay. The physical initialized-read guard checks every accepted
read offer, through both ordinary `take` and `read_chain_accept`, against captured
bytes or an earlier replay write. Both paths use the same address/byte-mask check.
The bounded native-MC regression exercises two128-byte GPU RO bursts, verifies
actual READ continuation and both payloads, and rejects an uncaptured successor:

```powershell
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/tests/read_chain_guard.py --peer path/to/fresh-export/peer --out path/to/new-test-output
```

Production uses column layout ABI2 and RTT command ABI4, including the native
ABI3 MEMCPY encoding. GPS1 wire/header/range versions remain1. Every new build
v2 binds the exact `production` layout, memory map and typed resources. Old row,
missing-layout and old command captures are rejected without repacking.
Before either replay preparation or execution, the declared command ABI is
checked against every opcode: ABI1/2 cannot enable MEMCPY; ABI3/4 include it;
reserved13/14 remain unsupported. Production snapshots themselves declare4.

Column addressing is `base+512*((x>>4)*16+(y>>4))+32*(y&15)+2*(x&15)`.
A plane has 96000 logical pixels and 6400 distributed padding words, totaling
204800 bytes. Its reserved allocation is 262144 bytes; CLEAR does not own the
57344-byte tail. Physical preimages are mandatory; allocation-tail capture is
explicit. Reports separate pixels, padding and captured guards. A FULL_CLEAR
range extending into a guard is rejected. Native writes must stay within the
captured writable byte masks, and every dynamic read must be initialized.

POT textures use 16x16 column macros, complete 512-byte mip tails and 16x8 cache
lines. Fixed 400x240 views decode the same storage while retaining signed 18-bit
pre-clamp, true dimensions, base-only and nearest/bilinear semantics. Matrix and
UI logical row order and the atlas odd-row encoding are unchanged.

Every RTL export has `layout.json` with `schema:1,layout_abi:2,gpu_profile:production`.
The exporter checks generated structure as well as metadata. Replay rejects
missing, unknown or mixed declarations; source hashes bind the actual hardware.
Fixed A/B/depth and explicitly borrowed, disjoint64-KiB-aligned RTT planes use
the same column decoder. The old Front remains a fixed Display-owned plane.

Preparation may consume an explicit hash-bound `--column-catalog` containing
`assets.json`, `renderer_assets.rs` and resources. Catalogs declare physical
ranges, kinds and texture packing. Normal minimal/full-scene packers now produce
this sole layout directly. Firmware rebuilds restore their bound bytes through
the boot-assets route. CPU producer support is not board timing qualification.

The MC export preserves a `peer/stream-contract.json` and its original source
names. Replay validates the fixed stream profile and every peer source against
that manifest; removed bridge-parameter text is not an identity check. Old
exports without that contract are rejected rather than silently substituted.

Cold replay uses the production GPU, arbiter, streaming endpoint, registered
CL3 native return and SDRAM controller. It has explicit cycle/wall limits and
isolated, hash-checked exports. It includes no CPU/Display requests and restores
no internal warm cache, FF or BSRAM contents. The pin model has explicit functional
timing assumptions; it is not board/PVT qualification. `color.bin`/`depth.bin`
are authoritative native output; `color.png` is a viewing aid.
`*-allocation.bin` contains exactly the declared comparison extent;
`replay.json.captured_plane_bytes` distinguishes physical-only and guarded capture.
Guard writes fail rather than being hidden as off-screen data.

Offline `tests/layout_replay.py --rtl CURRENT --out FRESH` consumes the canonical
GPU+MC export directly. NOP,`--clear`,`--physical-only`,`--missing-read` and
`--forbidden-write` keep explicit cycle/time limits and permission checks. There
is no row peer replacement, frozen column substitution or layout conversion.

`tests/producer_handoff.py` consumes an archived committed producer fixture with
`--fixture`, `--producer-receipt`, `--producer-commit`, `--rtl` and fresh `--out`.
It checks archived file and build hashes, the real header/ranges, catalog byte
closure and actual column origin. The generated wire is labeled host-encoded,
not UART. Its default independent oracle handles the supplied CLEAR plus fixed-grid UI
caller, including odd-row glyph packing, untouched depth and physical padding;
this default rejects DRAW. `--expect-fault
missing-read|forbidden-write` poisons a simulation mask without changing producer
bytes. Limits remain180000 cycles/180seconds. No verified programming receipt or
combined CPU/GPU source build is implied by this offline handoff check.

Both the earlier single-fixture receipt and the CPU-owned
`digitaldesign.cpu-capture-draw-delivery` v1 archive are accepted. The latter
retains one build/program per profile and `frame-0..2` occurrences; it is not
repacked into a fabricated legacy receipt. The consumer verifies every sealed
evidence file, program/build commit and digest, per-occurrence command/object
views, token/publication/slot/address, resource bytes and actual layout origin.
The producer's `observed` projection/sanity fields are not oracle inputs.

With `--oracle draw`, the same producer/input identity checks precede an independent
color/depth reference. It decodes the actual native commands, matrix, packed
meshlet and uniform color. Exact integer cross products and rational perspective
weights replace the hardware coefficient reduction/reciprocal algorithm. Depth
is camera Z in Q8.8, not far-normalized depth. ROP color uses nearest RGB565.
This deliberately restricted small-fixture path accepts at most eight triangles,
positive-Z vertices, opaque unlit shading, no blending, LESS/ALWAYS depth test
with writes, RGB attributes or a no-mip constant texture. Filtered UV is restricted
to white1x1 at zero UV. UI, lighting, clipping and unsupported sampling reject.
Both plane CLEARs must precede DRAW; overlapping depths and far-plane distances
within two D16 codes reject as ambiguous. At least256 stable covered pixels are
required, so an empty output cannot pass the DRAW handoff check.

One-pixel L-infinity bands about ideal triangle edges are excluded and counted
explicitly. All other background pixels compare exactly. Stable drawn pixels
use a declared default depth budget of two Q8.8 codes and exact RGB565; options
can tighten depth to zero or allow one RGB565 channel code for varying RGB.
Reports retain mismatch coordinates and maximum errors. These budgets describe
an independent mathematical comparison, not bit-exact RTL equivalence.
`--include-edges` compares all96000 logical pixels with no edge exclusion.
Reports distinguish the original ideal edge band from the number actually
excluded and compared. With an explicit depth CLEAR of0xffff, full-image mode
also checks exact coverage: every supported live depth is below far<200 and
therefore below0xffff. Background remains exact; the declared depth budget
only applies to mathematically covered pixels. The sequence runner accepts the
same option. A stable-region pass never silently becomes a full-image pass.

`tests/producer_sequence.py` takes three `--fixture` arguments and the same
producer/export options. It requires natural CB address A/B/A reuse, distinct
tokens/submissions and at least256 changed stable DRAW pixels on the reused
address, beyond differences attributable only to CLEAR. It independently
cold-replays each exact frozen occurrence. It does
not infer warm queue behavior or CPU/Display concurrency from isolated resets.
`tests/draw_replay.py` provides a bounded synthetic DRAW check of this plumbing,
with explicit `--rtl` and fresh `--out` paths.
Synthetic input success never establishes real CPU producer or physical-button
qualification. Put outputs under this worktree's `target/`.

### Complete frozen scene

`tests/full_scene_replay.py` consumes the CPU-owned
`digitaldesign.cpu-capture-full-scene-delivery` v1 envelope, whose profiles live
under`full-scene/column16/frame-0..2`. The consumer verifies its sealed
program/build, commands, objects, UI and all range bytes without rewriting the
producer archive or using its `observed` sanity/projection fields as answers.
The same explicit export options as the single-DRAW runner apply. Cycle and
wall limits default to2000000 and1800seconds; both are explicit manifest fields
and may be overridden within replay's existing bounds.

The independent NumPy reference decodes every native DMA/state/DRAW/UI command
in order. It performs the documented Vertex output-window/normal quantization,
then direct float64 ray/triangle intersections and perspective attributes. It
does not read RTL, control ROM, T's reduced coefficients, R's traversal answers,
I's reciprocal table or CPU-provided projections. Textures come only from
captured RGB565 bytes: column16x16 macros and their actual mip padding.
Sampling uses the documented integer tap/Color quantizers; Lighting uses ideal
normalization and Blinn-Phong with each current global context. Final, ROP
packing and UI's binary-alpha glyph writes are independently evaluated.

This runner requires24 DRAWs:192 box triangles in six textured/unlit meshlets,
one RGB triangle,948 rabbit triangles in17 lit meshlets, and nonempty frame-time
UI. It independently checks the actual GPU trace for24 DRAW/start and VT/end
events, all command executions, UI, substantial T/quad work and completed drain.
Thus a parsed manifest, single DRAW or empty run cannot satisfy this contract.

Reports separately count full-image coverage, exact UI and nonboundary color
and depth. One-pixel ideal edge bands and two-code depth ties are marked using
the independent geometry, never DUT primitive ownership. Coverage/depth/color
deviations in excluded regions are still counted and sampled. Stable depth's
budget is two Q8.8 codes; unlit RGB permits one RGB565 channel code (whose bit
expansion can differ by nine RGB8 units). Texture and ideal-light comparison
budgets are24 and16 RGB8 units maximum, with mean at most two; these are
approximation checks, not bit-exact Lighting or arbitrary-scene guarantees.
Background and UI color remain exact. Minimum checked texture/RGB/lit pixel
counts reject vacuous comparisons. The original/actual/reference images and
counts remain available even when a budget fails.

NumPy is required only for this complete-scene path and its separate opt-in
`tests/scene_oracle_checks.py`; the basic profiler format/viewer checks remain
stdlib-only. Native GPU/MC runs still require Icarus and the existing Gowin
primitive library. Frozen CPU producer input, cold GPU execution, warm queue
concurrency, physical buttons and board qualification remain separate evidence.

The next frozen lease can expose the previous submission's actual output in its
confirmed old Front and depth preimage. `compare.py` checks adjacency, addresses,
formats, input identity and both complete output planes. An equal memory image
does not certify the HDMI link or infer whole-board GPU timing.

`qualify.py` summarizes the diagnostic run's two repeated frozen dumps, adjacent
output comparison and exact independent wire-encoder check. It reports the
accepted-START-to-drained interval separately from observed CLEAR command time.
Subtracting those intervals is accounting on that run, not a separate no-clear
render. Arrival-offset CSV records support per-attempt transfer measurements;
board dump milliseconds end when END is assembled, before its final bytes finish.

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
