# Imported RGB level batches

`tools/batch-scene.py` converts the imported packed levels into fixed native
assets. `rcc/bloxorz_batch.rs` reads a small asset manifest and records the scene
through the ordinary GPU Pass API. The driver continues to own command storage,
object snapshots, address mapping and submission. This is an initial-state RGB
fixture, not the interactive game loop or a boot selection.

## Grouping and exact coordinates

Normal ground is one Mesh and one shared Instance. Its meshlets are packed in
3x3-cell spatial buckets, with at most five complete cells per meshlet. Each cell
has the three faces visible from the fixed camera: 12 vertices and 6 triangles.
Thus a meshlet has at most 60 vertices and 30 triangles, within the native 64/96
limits, and at most seven 128-byte payload sectors. Geometry on hidden sides is
not supplied by this fixed-camera fixture.

Bridge channels form independent groups. Each fragile tile, soft/heavy switch,
splitter and goal cover has its own group. Only normal tiles enter the ground
batch. The initial bridge mask controls bridge visibility, and goal covers are
initially hidden. Assets for hidden groups remain in the image. The fixture
selects initial visibility offline; it does not implement bridge/cover animation
or runtime visibility updates. Special cells currently use classified RGB floor
boxes; detailed switch glyphs and hatch/ring geometry are not exported here.

Source coordinates remain twice game coordinates, paired with half instance
scale. The retained fixture translation is -6554 Q16.16 raw units in Y; after
half-scaling the source half-height, the native top lies at -26 raw units, not
exactly Y=0. Runtime shadow receivers are not qualified against this surface. Ground cells
use exact canonical X/Z endpoints on integer source units
and Y endpoints of raw Q16.16 `-13056` and `13056`, matching the earlier tile
asset. The largest common power-of-two grid that preserves every endpoint is
512 raw units: the Y separation is 51*512. The common grid therefore has
`quant_shift=9`, phase `(0,256,0)`, and different meshlet origins on that same
grid. A 3-cell span is 768 delta units, below 1024. Larger boards add meshlets;
they do not coarsen the grid.

Every vertex must decode exactly to its canonical coordinate. Duplicate
endpoints, including those crossing meshlets, must decode bit for bit equally.
The producer checks all 35 imported levels. Metadata keeps `scale_s=5`, normal18,
RGB mode and disabled culling. Position and normal matrices remain separate.
Range checks examine unwrapped dot products before the oracle applies ABI
truncation, so wrapped normal/position values cannot hide an overflow.

## Asset image and fixture manifest

The image begins at physical byte `0x4b0000`. It contains a padded directory,
native metadata/payload records, then a padded white 1x1 texture. The existing
GPU scene oracle owns V96/T64 packing and metadata generation; this producer does
not create a parallel command format or build command buffers on the host.

`manifest.json` records group membership, native meshlets, physical allocation
and initial draw selection. `params.bin` is a fixture-only CPU table preloaded at
CPU word `0x7000`. Its eight-word header contains width, depth, start X/Z,
orientation, draw-call count, texture sector and asset allocation sectors. Each
eight-word draw entry contains directory sector/word, meshlet count, diagnostic
group ID, placement kind, cell X/Z and a reserved word. Placement kind 0 uses the
shared ground matrix, 1 the block pose and 2 the earlier per-cell comparison path.
These are static asset handles and placement data, not CB addresses or commands.

The grouped images occupy 8,448 bytes for b1 and 24,960 bytes for b33, versus
1,152 bytes for the shared local-tile comparison image. This trades fixed asset
space for less per-frame CPU recording and fewer instance snapshots; no runtime
allocator, resource relocation or repeated asset rebuilding is involved.

The RCC program builds the camera, sets Global and Render once, prepares an
instance per selected group, and calls `gpu_pass_draw_mesh`. The ground's
multiple meshlets share that single snapshot. The block remains a separate local
mesh. The output target is fixed Back B plus Depth; Front and Display are unused.

## Validation boundary

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label bloxorz-batch-cpu -CargoArgs @('-p','cpu-v3-tang-nano-20k','--example','bloxorz','batch_scene','--','--nocapture','--test-threads=1')
& scripts/run-cargo.ps1 -Subcommand test -Label bloxorz-batch-native -CargoArgs @('-p','cpu-v3-tang-nano-20k','--example','bloxorz','batched_levels_native_gpu','--','--ignored','--nocapture','--test-threads=1')
```

All 35 cameras run as compiled RCC and pass shared instance bounds checks. The
producer checks every ground vertex, including hidden groups, before native
position/normal truncation. CPU probes are bounded by 100,000 instructions;
complete recordings by 20 million. Default CPU V3 optimization is used throughout.

The first level and largest board compare per-cell and grouped CPU recordings.
Grouping must reduce executed instructions, CB bytes and object high-water usage.
Canonical geometry is equal before transformation; shared-instance endpoints
project identically. The test permits less than 0.1 pixel of endpoint difference
from the per-cell path, and requires any reference color difference to lie within
one pixel of an edge. This does not diagnose or promise to remove small seams.

Native validation replays actual CPU-recorded CB/object bytes for one frame per
representative level, with CE/backpressure, both meshlet slots and full surface
and memory guards. Color is exact; covered depth permits one D16 unit and rejects
X/Z. The read-only Display peer reports fixed Front A; the application releases
Ready using `gpu_discard_ready`, without presenting.

CPU recording and native GPU replay are separate phases, not simultaneous
CPU/cache/SystemControl/GPU validation. Board behavior, atlas materials, Flash
boot loading, Display scanning, input, runtime shadows and game scheduling are
outside these fixtures.
