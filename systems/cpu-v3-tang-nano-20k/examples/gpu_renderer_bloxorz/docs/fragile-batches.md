---
id: bloxorz-fragile-batches
status: implemented
last-verified: 2026-11-04
---

# Fragile cells: retained geometry and reversible index visibility

Kind2 is a fragile floor cell, marked with an X. Its top and four sides have20
attribute vertices and10 triangles; the bottom is absent. Cells share one white,
lit material and the immutable256 marker atlas. Four visual variants select UVs
inside that atlas, without four material groups. Ordinary resting cells do not
move. A standing block still loses support according to the unchanged rules;
when landing enters CRACK and sets FRACTURE, that cell disappears immediately.
It no longer renders a shaking or falling tile. Player crack/fall timing and
other animations remain unchanged. Undo cancels the fracture; restart and stage
replacement restore every cell through the normal presentation-owned reset.

The application pre-records fragile geometry in its retained static CB. This
opaque Less/depth-write/Replace group precedes the dynamic bridge/button/pad,
shadow and player path. It is separate from colored bridge materials, alpha
hints, shadow blending and UI. No transparent material or depth state is merged
into this group. GPU geometry still executes on every replay; skipping CPU
recording does not skip GPU vertices or triangles.

## Exact source geometry and storage

The asset producer derives400 immutable u16 corner/normal/UV template words from
the original four native fragile meshes, appended to the existing level archive.
There is no second per-stage native mesh asset. The loader retains every face,
normal, UV, winding and variant, and constructs native records in its own heap.

Cells are grouped into3x3 spatial buckets and at most three complete cells per
meshlet:20/40/60 vertices and10/20/30 triangles. Native positions use unsigned
delta10 with quant_shift9 and signed F16 origins; a common positive half-scale
Model preserves the original world grid and bottom-13056/65536 exactly. This
does not promise identical projection rounding to individual translated Models.
The payload remains within8 sectors; its128-byte metadata prefix is additional.
One/two/three-cell records occupy4/6/8 sectors including that prefix.

An eight-u16 heap context owns a separate per-level allocation. Its size is
computed from the actual bucket counts, map size, directory and record lengths.
The imported maximum is38 cells/18 records on stage30, not a fixed33-cell limit.
An explicit8192-word geometry bound rejects excess allocation before writes.
Stage4 has33 cells/13 records and93 GPU-visible sectors (11904 bytes); private
map storage and alignment are additional. The existing ground allocation stays
16KiB, and no static global buffer or resource-handle array is added.

The allocation is64-word aligned. CPU-private pairs map cell indices to physical
record sectors and a0..2 cell slot. The GPU storage range starts at the following
directory and covers only the directory/records. The original malloc owner is
retained separately for free; no mapped-window pointer escapes. Empty stages
have zero records and no geometry allocation.

## Index patch contract

Native indices are6-bit vertex numbers. Triangle t occupies18 bits at
`index_qword[t/3]`, shifted by `18*(t%3)`; the high10 bits of every qword remain
zero padding. A cell in slot s uses triangles10s..10s+9. Each field is modified
with a masked u16 read/modify/write, including fields crossing a word boundary.
Hiding writes valid indices[0,0,0], yielding zero determinant and no coverage.
Restoring reconstructs the original five quad fans with vertex base20s.

| Cell slot | Covered bytes relative to the index area | u16 words touched |
| --- | --- | --- |
|0 |[0,32) |0..13 |
|1 |[24,56) |13..26 |
|2 |[48,80) |26..39 |

The32-byte qword spans share boundary words; each masked patch changes28 bytes
at most and preserves neighbor indices and padding. It is not a contiguous
180-bit stream. With n cells, the index area begins at record byte128+240n.
Vertices, header, directory, counts and addresses remain unchanged, so the
recorded CB remains usable. A stable hidden index causes no mesh writes.

## Retirement and visibility

At the start of scene preparation, after the preceding GPU work has retired and
before submitting the static CB, the caller applies a visibility change. A busy
or transport-error Device4 status stops patch/rebuild/free before mutation. Level
replacement resets both retired CBs before freeing/rebuilding geometry. Finite
exit drops both CBs after their last wait before freeing geometry and context.

The existing submission clean/invalidate publishes CPU stores before GPU START.
Every DMA_MESH reloads the current record into an input page; geometry is not a
persistent texture-cache entry. No TEX_INVALIDATE or per-frame full-mesh upload
is added. The retained-CB functional test observes changed published DMA inputs,
zero pixels for all ten degenerate triangles, and exact restored framebuffer
bytes at the same address with unchanged commands. This is software/cycle-model
qualification, not an additional RTL or board result.

Shadow receiver exclusion starts at the same CRACK boundary. The receiver key
includes the fractured cell identity as well as its visibility bit and level
generation, so undo, a different fracture or a reset cannot reuse its old hole.

Bounded tests independently decode all33 stages and hide/restore all153 fragile
cells, preserving complete non-target geometry/directory/padding bytes. Separate
tests exercise inflight rejection, actual ordinary restart/next-stage snapshots,
heap coalescing and mixed-material/goal scenes. CPU cost uses a fixed stage4 PLAY
workload with visible shadow and separate accepted-source instruction/native
oracles; startup geometry construction and warm frames have different costs.
Those costs and complete image differences belong to the task evidence, not to
a hardware frame-rate claim.
