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

Audit tools consume their explicitly supplied source/replay artifacts; they do
not generate missing runs or inherit a different source version's fault results.
Resource-fit scripts are optional verification tools, not a historical ledger
maintenance requirement. Generated seeds, netlists, traces and images stay under
target rather than in docs.

The current PC constants and A operand table qualification is bound to source
and report hashes in [the selected receipt](/ip/gpu-v2/examples/vliw_integration/operand-table-evidence.json).
It contains only the selected implementation's acceptance evidence; generated
exports, project files, traces and rejected mapping experiments are not inputs
to the implementation.
