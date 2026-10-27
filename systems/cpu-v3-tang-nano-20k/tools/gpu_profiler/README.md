# GPU profiler

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

## Commands

Run from the repository root:

```powershell
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py check --node node
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py build systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/tests/fixtures/pingpong --out target/profiler-example
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py render path/to/profile.json --out target/profiler-view
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py capture path/to/manifest.json --out target/profiler-capture
python systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/profiler.py build target/profiler-capture
```

Open the output directory's `profiler.html`. It embeds the data and works without
a server. The JSON and submission/command/meshlet/CPU CSVs retain integer cycles.
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
ABIs refuse replay. Physical initialized-read guards additionally reject any
native MC read not covered by captured bytes or an earlier replay write.

Cold replay uses the production GPU, arbiter, streaming endpoint, registered
CL3 native return and SDRAM controller. It has explicit cycle/wall limits and
isolated, hash-checked exports. It includes no CPU/Display requests and restores
no internal warm cache, FF or BSRAM contents. The pin model has explicit functional
timing assumptions; it is not board/PVT qualification. `color.bin`/`depth.bin`
are authoritative native output; `color.png` is a viewing aid.

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
