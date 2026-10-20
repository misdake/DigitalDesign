# VLIW integration replay tools

This directory contains independent integer references, scene inputs and bounded
replay drivers for the standalone GPU. The contracts are maintained in
[geometry input](/ip/gpu-v2/docs/formats.md), [frontend](/ip/gpu-v2/docs/geometry.md),
[pixel backend](/ip/gpu-v2/docs/execution.md), [framebuffer](/ip/gpu-v2/docs/framebuffer.md)
and [memory transport](/ip/gpu-v2/docs/submission.md). Historical phase receipts do
not replace those implemented contracts or define a CPU submission ABI.

## Entry points

| Tool | Scope |
| --- | --- |
| verify_frontend_stream.py | V96/transform/T-record transport with source and output stalls |
| verify_stage2.py | Independent V/T/R/I, Lighting, Sampling, Final and framebuffer comparison |
| tests/vliw_sdram.py | Actual main arbiter/bridge/controller and independent SDRAM pin model |
| full_bunny.py | Meshlet partition, two-page loading and complete supplied PLY traversal |
| replay_export.py | Matched box/full-bunny audit of an explicitly supplied export with a prebuilt current MC peer |
| test_replay_export.py | Small source-identity, peer-isolation and bounded-log regression; includes a tiny Icarus simulation |
| solid_scene.py | Complete 948-face bunny inside a six-face subdivided room; constant RGB albedos, directional light and selected H9 specular |
| tests/vliw_prepare_chain.py | Small native/portable preparation regression; optional exhaustive operand-table check |
| tests/vliw_normal_quantization.py | Exact normal16/18 truncation, RNE and saturation |

The mathematical reference is independent of the current V5/T16 transport.
`transport.py` packs its results without changing numerical reference arithmetic.
Programmable UV fixtures are not a supplied model's original texture mapping.

## Bounded replay

Use a fresh tag and an ignored target output directory. Configure IVERILOG_EXE,
VVP_EXE and GOWIN_HOME as required. Every HDL replay has a finite wall-edge
bound and a host process timeout. Portable and native storage are separate modes;
scene identity and source/configuration must agree before comparing their output.

```powershell
python ip/gpu-v2/examples/vliw_integration/verify_stage2.py --tag full-box-new --name box_x_minus --mode both
python ip/gpu-v2/examples/vliw_integration/verify_stage2.py --tag full-rabbit-new --name bunny_patch_1 --mode both --stress --rgb-repeat
python ip/gpu-v2/tests/vliw_backend_complete.py --tag current-small-new --mode both
python ip/gpu-v2/tests/vliw_backend_failstop.py --tag current-fault-new --mode both --fault final
python -B ip/gpu-v2/tests/vliw_raster_scan.py --output target/raster-scan-small-new --mode both --small --limit 16
```

The focused raster replay compares every emitted h value and quad mask against
independent integer edge equations, including row ends, CE pauses and admission
backpressure. The small set bounds vendor-primitive runtime; omit `--small` and
use `--mode portable` to include the full-screen traversal cases.
The tile-prefetch checks exercise both full and partial destinations and the
ordinary LOAD fallback.
They also exercise first-word metadata publication with h0 reads overlapping
h2 writes at different addresses, and check publication/completion conservation.
The same test asserts that CHECK only advances a stored rejected predicate and
that each partial-quad admission has a stored possible predicate. Classification
uses tile-load, prefetch and next-origin writeback values; the test covers their
CE holds and admission backpressure together with all emitted pixel h values.

The preparation regression checks raw-to-reduced coefficients, packed attributes,
continuous geometry-DSP retirement during the next raw collection, CE holds and
in-flight reset cuts. It includes zero/short shifts, coarse-window boundaries
and large admitted integer rays. Independent full-width division defines each
expected coefficient; issue/write/cancel counts check the geometry pipeline.
The independent bounds replay enumerates integer tile inequalities, and checks
four consecutive D/C launches and final retirement for each single-axis pair.

Origin checks sample old C and packed Nx/Ny when the DSP consumes RAM Q, then
check the fixed two-enabled-edge retirement against an independent root equation.
An explicit reset cuts an in-flight product; issue/write/cancel conservation and
DSP-output/tag stability during CE holds remain part of both RAM-mode runs.

Its Cargo entry also checks every operand-table constant in portable and native modes:

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-prepare-chain -CargoArgs @('-p','gpu-v2','--test','vliw_prepare_chain','--','--ignored','--nocapture','--test-threads=1')
```

For a supplied Stanford PLY, keep source bounds and runtime limits explicit:

```powershell
python ip/gpu-v2/examples/vliw_integration/full_bunny.py --ply PATH/bun_zipper_res4.ply --tag complete-front-new --camera front --max-cycles 2000000 --sim-timeout 600
```

`--camera side|back` selects another view. Geometry appears once per supplied
face; smaller and larger source meshes are different workloads. The driver
preserves cross-meshlet vertex consistency, loads actual SDRAM data, drains the
backend and flushes. It does not establish CPU/Display contention or board behavior.

For a controlled RTL comparison, build the exporter and `vliw_integration`
example once per source version using `scripts/run-cargo.ps1`. Export the
`sdram` composition, and run the `vliw_sdram` test's
`export_real_shared_sdram_peer` function to materialize the current MC. Then:

```powershell
python ip/gpu-v2/examples/vliw_integration/replay_export.py --export PATH/dut-portable.v --output target/explicit-box --scene box --gold PATH/vliw_integration.exe --peer target/gpu-vliw-integration-20261018/stage3/mc-peer
```

Use `--scene bunny --ply PATH/bun_zipper_res4.ply` for all 948 faces, or
`--native` with a native export to include vendor primitives. Both simulation
limits are explicit options (`--max-cycles`, `--timeout`). The supplied export
is frozen; the driver does not silently rebuild it or select another version.
Peer `dut.v` and `tb.v` files are excluded regardless of case. Only the selected
export, copied peer sources and generated bench are passed to the compiler.
The selected export and staged DUT hashes must match before compilation and
again before starting the simulator; the final identity check remains enabled.
Full compiler/simulator output stays in `compile.log`/`run.log`. The terminal
prints a compact result and receipt path; detailed counters remain in the receipt.
Tool failures include only the final 4096 log bytes and the full log path.

Check these replay protections without rendering a full scene:

```powershell
python -B ip/gpu-v2/examples/vliw_integration/test_replay_export.py
```

For routine performance comparisons, `--scene quick --ply
PATH/bun_zipper_res4.ply --max-cycles 400000 --timeout 240` submits the existing
single box face and the first two complete bunny meshlets. All three jobs use
the real DMA/memory path, share one framebuffer, and include first load, drain
and flush. The receipt records the selected face/triangle counts; this is a
bounded subset, not a full-model throughput claim. Keep this scene, source PLY,
camera, material and memory settings identical between candidates. Full bunny
and material-room runs are separate final checks.

Bounds can be checked without running pixels or external memory:

```powershell
python -B ip/gpu-v2/tests/vliw_bounds.py --export PATH/dut-portable.v --output target/bounds-small --count 128
```

This bounded test checks the existing directed preparation fixtures plus seeded
random signed18 positions, both orientations and RGB/UV attributes under CE
pauses. Its independent integer reference enumerates allowable tile coordinates
from each constraint, rather than reproducing the RTL search schedule. A native
export additionally requires `--native` and the Gowin simulation primitives.

For the combined solid-material scene, select `--scene solid-room --camera
front|side|oblique --ply PATH/bun_zipper_res4.ply` and allow at least 3,000,000
wall cycles. All 192 inward-facing room triangles and 948 bunny triangles are
submitted through the same meshlet DMA path and share one depth surface.
Lighting, specular, ROP and the memory controller remain real RTL. Constant RGB
attributes bypass Sampling. The lighting reference accepts an optional context
file and checks its selected H9 result against counted arithmetic samples.

Successful solid-room runs save `framebuffer-raw.png` (RGB565 bit expansion)
and `framebuffer-srgb.png` (the existing Display linear-RGB565 lookup), plus
nearest-neighbor 2x versions. Images come from final SDRAM readback after flush;
the lookup preview does not claim CPU/Display integration. Receipts retain
timing, context, source and image hashes, and nonzero specular counts.

`--scene materials --camera front|side|oblique` uses a prepared, muted tile
texture on the unlit room, an unlit RGB vertex-color triangle in the bottom
corner, and the solid lit/specular bunny. All objects use actual V96/index DMA,
Sampling/Lighting/Final/ROP and one shared depth surface. The box texture is
`assets/room-tiles.png`, RGB565 with a complete box-filtered mip chain; seed and
integer sampler reference consume the same asset. Changing from the unlit group
to the lit bunny waits for `pipeline_idle` and pulses `draw_rebind`; the barrier
and memory work are included in the measured frame cycles.

The supplied zipper PLY contains opposite duplicate faces. A few vertex normals
have an area-weighted sum within 1e-12 of their incident total area. The fixture
generator repairs only those undefined directions using an incident face oriented
toward reliable neighboring normals, instead of normalizing floating-point
cancellation residue. It preserves all 948 source faces, positions and meshlet
partitioning. Repaired source vertex IDs are recorded in the scene input.

Run the small asset/reference checks with:

```powershell
python -B ip/gpu-v2/examples/vliw_integration/test_material_scene.py
```

After a completed solid-room or materials replay, independently render and audit:

```powershell
python -B ip/gpu-v2/examples/vliw_integration/float_reference.py target/explicit-materials
```

This second reference directly decodes V96 inputs and uses float64 matrix
transforms, ray/triangle intersection, perspective interpolation, normalization
and Blinn-Phong powers. It does not call the fixed-point geometry/interpolation
oracle or use RTL lighting values to produce its image. Its floating texture
filter models the public coordinate convention but omits integer tap rounding.
`float-reference.png` and `float-reference.json` complement the exact integer
audit: interior statistics require the same winning source triangle and a
distance of at least one pixel from every exact edge. Normal, diffuse, specular
and color errors are reported separately, including lit-only color errors;
edge/ownership differences are not silently counted as lighting errors.

Audit tools consume their explicitly supplied source/replay artifacts; they do
not generate missing runs or inherit a different source version's fault results.
Resource-fit scripts are optional verification tools, not a historical ledger
maintenance requirement. Generated seeds, netlists, traces and images stay under
target rather than in docs.

For independent fill-rate and small-triangle measurements, replay `--scene box`
and `--scene bunny --meshlet-limit 2` into separate fresh output directories.
Each run starts from reset, seeds its own memory, and includes first load and
final flush in its total. The bunny limit selects complete meshlets and records
both the selected counts and the original 948-face model counts. Omit the limit
for the complete model. Keep the same export, PLY, golden executable, memory
peer and cycle/time limits on both sides of a comparison; combined-scene job
intervals alone do not establish isolated throughput.

The current standalone GPU qualification is bound to source
and report hashes in [the selected receipt](/ip/gpu-v2/examples/vliw_integration/operand-table-evidence.json).
It contains only the selected implementation's acceptance evidence; generated
exports, project files, traces and rejected mapping experiments are not inputs
to the implementation.
