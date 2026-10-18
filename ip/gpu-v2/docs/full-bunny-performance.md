# Complete bunny performance replay

The current independent GPU and memory-transport replay are qualified by the
[Color completion receipt](../examples/vliw_integration/color-direct-evidence.json).
The [frontend preparation receipt](../examples/vliw_integration/frontend-preparation-evidence.json)
retains the preceding preparation and older-memory measurement identity.
It uses the complete `bun_zipper_res4.ply` from the Stanford bunny dataset:
453 source vertices and 948 triangles, submitted exactly once without further
decimation. This is the complete version of the earlier small fixture, not the
69,451-triangle original scan. Vertex arithmetic and numerical precision are unchanged.

## Workload and timing boundary

The replay uses a 400x240 target, single-sided triangles, a front camera about
2.8 units from the model and a focal length of 180 pixels. The longest model
extent is scaled to 1.5. One position grid and accumulated vertex normals are
shared across the complete source mesh, so duplicated meshlet vertices remain
bit-identical. UVs and the 64x64 mip texture are generated, not scan materials.
Lighting is diffuse, specular is zero, alpha is 255, blending is disabled, and
LESS depth testing and depth writes are enabled.

The mesh is partitioned into 17 meshlets, each within 64 vertices and 96
triangles. Boundary duplication requires 670 executed vertices. Payload is
10,648 bytes, metadata 544 bytes and configuration 128 bytes. Two input pages
allow the next DMA to overlap the active meshlet without draining the backend.

Timing starts at the first loading descriptor and ends after the final backend
drain and dirty framebuffer flush. It includes memory service and SDRAM startup;
the startup interval is not a recurring per-frame DMA cost. ROM initialization
and preclear are excluded. The actual arbiter, bridge, controller and separate
SDRAM pin model are present, without CPU or display contention. Functional
clocks are approximately 49.5/99 MHz; conversion to 54 MHz is only cycle scaling,
not board performance or renewed MC timing qualification.

## Selected front-camera result

| Measurement | Result |
| --- | ---: |
| Total wall cycles | 294680 |
| Triangles accepted / backface rejected | 417 / 531 |
| Traversal nodes | 10653 |
| Quads / empty quads | 3559 / 532 |
| Covered pixel samples | 7107 |
| ROP depth pass / reject | 6753 / 354 |
| Pool-full / R credit-wait cycles | 0 / 0 |

Samples include overlap across triangles; they are not unique final pixels.
Empty quads skip the expensive backend, while their frontend cost remains in
the total. The selected box comparison submits only `box_x_minus`: eight
triangles covering 24,400 samples, not a full-screen or closed six-face box.
Its total is 134982 cycles. Complete cycle partitions and source/output hashes
are in the receipt. The old side-camera measurement predates this preparation
rewrite and is not claimed as a current-version result.

## Validation and reproduction

Replay checks all V outputs, triangle indices and records, h values,
interpolated attributes, depth and LOD, independent Lighting integer results,
sampling, Final, ROP order, and all 400,384 final memory bytes including untouched
regions. The selected numerical output and final memory match the remeasured current-base
implementation exactly, with no total-cycle regression. The baseline was rerun
after the shared memory-transport changes; that earlier improvement is not a Color
optimization result.

```powershell
python ip/gpu-v2/examples/vliw_integration/full_bunny.py --ply PATH/bun_zipper_res4.ply --tag complete-front-new --camera front --max-cycles 2000000 --sim-timeout 600
```

Each tag creates a fresh ignored target directory. `side` and `back` are also
supported, but a new camera requires a new measurement. Inputs are the official
ASCII PLY extracted from Stanford's bunny archive; generated fixtures, netlists,
frames and historical snapshots are not committed. The high-resolution original
has only a capacity study, not a measured frame rate in this qualification.
