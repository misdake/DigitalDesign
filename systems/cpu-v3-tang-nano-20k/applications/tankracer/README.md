# Tank Racer static-map playback

The default scene is `assets/england/scene-grid-atlas.zip`. Supply a camera
sequence staged within the repository; `--map` selects an alternative archive.
The application uses the regular SDK and Display path.

This scene uses a 512-square atlas with an 8×8 grid of 64-square sampling cells,
trilinear filtering and a −0.5-level application LOD bias. All 31 64-square
sources and seven 128-square sources retain their original resolution. A large
source occupies four cells and stops at its 2×2 mip; smaller sources stop at
1×1. Every opaque draw shares the same view and command-ring state. IGATE and
SCARE occupy 128-square and 64-square RGBA4444 regions in the same bytes.
They share one RGBA4444 view with 64-square clamp cells. IGATE's smallest
sampled source mip is 2×2. SCARE's is 1×1.
The remaining 16 cutout images fill one 256-square RGBA4444 atlas.
The original bordered atlas in
`scene-opaque-atlas.zip` and its input archive remain available through `--map`.

The same catalog registers `bench-tankracer-physics`, a separate frozen-contact
single-vehicle benchmark. Its kernels, resource inputs and execution boundary
are described in [physical update](/systems/cpu-v3-tang-nano-20k/applications/tankracer/physics.md).

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --camera systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england/cameras.json --recording prepared --out target/tankracer-map
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- preview bench-tankracer-static --snapshot target/tankracer-map --presents 2
```

## Static scene archive

The ZIP contains `scene.json` and its texture payloads. All texture paths are
relative ZIP entries. Positions use a shared Y-up source-world basis.
Coordinates are transformed as `(source - origin) * scale` for both the
geometry and the replay camera positions. Camera directions keep their basis.

| Scene field | Value |
| --- | --- |
| `version` | `1` |
| `origin`, `scale` | Optional source-world translation and positive uniform render scale. Defaults to `[0,0,0]` and `0.125`. |
| `far` | Projection depth limit in render units. Defaults to `110`. |
| `display_distance` | Conservative selection depth in render units, including each group's radius. Defaults to `far`. ENGLAND exports `25`, corresponding to the source's 200 world units; projection remains `far=110`. |
| `position_shift` | Shared position lattice, defaults to `10` or 1/64 render unit. Each meshlet retains its own origin. |
| `billboard_position_shift` | Optional billboard position lattice, defaults to `position_shift`; integer `0..31`. |
| `billboard_exact_anchor` | Optional boolean, defaults to `false`. Vertical billboards keep their common X/Z center line in full F16 origins with zero X/Z deltas; Y retains the absolute billboard lattice. |
| `materials` | Indexed entries with optional `texture`, `rgba`, `mipmaps`, `clamp`, `blend`, and RGBA byte `tint`. Textures are square powers of two. Omitting the texture uses RGB vertices. `vertex_color: true` retains RGB geometry while supplying a shared texture binding. `atlas_grid_log2: 0..3` selects a fixed grid with full mips and clamp. |
| `groups` | Spatial and material runs with `material`, `vertices`, `triangles`, and optional `name`, `billboard` and `bounds_geometry`. |

Triangles are triples of zero-based vertex indices. Geometry uses the SDK's
ordinary native culling contract. Supply both windings for a double-sided
mesh. The packer snaps all groups to the common lattice and rejects a
meshlet whose local deltas exceed their native width.

An RGBA material can declare `atlas_region: [x, y, width, height]` in base-level
pixels. The packer validates that rectangle and checks its actual alpha values
when classifying a mixed-format atlas as a cutout. Subregions align with
complete clamp cells, and the generator checks that packed mesh UVs stay
inside the declared region. The region describes the stored layout.
`bounds_geometry` supplies additional conservative bounds using the same vertex
format. Its geometry is included in culling and sort bounds but is not submitted.
It preserves the original sort unit when trimming a transparent card's outline.

Materials with `encoding: "gpu-column16"` supply native little-endian texture
words and an explicit `size`. `rgba` selects RGBA4444 instead of RGB565;
`mipmaps` selects a complete smallest-first chain. Each level uses native
16×16-column storage with a minimum 512-byte extent. The producer validates the
payload through the regular SDK asset path. Matching texture descriptors share
one physical resource across material tints. Alpha atlases use per-region colour
extension and gutters; their base-only payload avoids filtering between regions
at smaller mip levels.

RGB vertices are `[x,y,z,r,g,b]`, and textured vertices are `[x,y,z,u,v]`.
A vertical billboard group sets `billboard: true`, references a blended
texture, and uses `[x,y,z,side,u,v]` vertices. The position lies on its center
line, and `side` is the signed camera-right displacement in source units.
The packer automatically classifies blended RGBA textures with binary base-level
alpha and opaque uniform tint as cutouts. This includes billboards. They use
SrcOver with depth writes: color keeps normal filtered alpha blending; only full
final Q6 alpha writes Z, so a hedge's holes cannot hide trees behind it. Cutouts
follow opaque terrain in texture/stable-ID order. Full-opacity samples resolve
visibility through Z; filtered partial-alpha edge colors follow this fixed
submission order. This scene uses that order instead of a camera-dependent sort.
The [GPU alpha/depth rule](/ip/gpu-v2/docs/framebuffer.md#alpha-qualified-depth-writes)
qualifies Z only and keeps normal color blending.
Fractional source or uniform alpha retains SrcOver without depth writes; each
such blended group remains a sort unit. Keep intersecting blended geometry in
suitably small groups to preserve depth order.

The asset manifest contains source hashes, per-group inventory, meshlet
counts, native extents and selection storage. Native assets remain in SDRAM.
The producer groups selection records into 16-render-unit XZ cells and splits
each cell into clusters of at most 32 groups. A cluster directory stays in CPU
memory. Frames reject whole clusters, read surviving records through the ordinary
immutable asset window into one fixed scratch buffer, then cull boundary groups.
Fully enclosed clusters accept all their groups. Current replay matrices also
live in an asset; the CPU reads only the current 48-byte view.
The culling kernels borrow matrix rows and group bounds; their fixed-point dot
products read the existing fields directly throughout the candidate loop.

Each group occupies 22 bytes: three signed Q9.7 center components, three unsigned
Q9.7 extents, an unsigned Q9.7 radius, one meshlet first/count pair, a material
with billboard bit 15, and the original stable group ID. Bounds round outward;
the packer rejects unrepresentable ranges. Native geometry keeps its original
order. Opaque selections sort by first meshlet and coalesce adjacent compatible
ranges. Ordinary transparent selections sort by depth and original ID. Cutouts
occupy the other end of the same six-word record scratch. After blended rank
seeding finishes, its mask and ordinal buffers build texture/stable-ID order
into the unused suffix of the existing order array, without another allocation.
Drawing reuses a texture/view across each consecutive run with the same texture
and address mode.
Material tint and billboard selection stay per draw. Shared texture descriptors
use an indexed table independent of material tints.
Transparent sorting moves u16 indices while its six-word draw records remain in
place. A previous-count word guards comparisons of old ordinal stable IDs
before each record is overwritten; every frame still writes its new depth.
An identical complete member sequence permits checking the previous order against
new depths and the stable-ID tie break. An ordered sequence reuses those indices.
Otherwise direction ranks seed an exact repair, as described below. The first
frame and ordinals beyond the previous count never read uninitialized records.
Visibility and command recording decisions are still evaluated each frame.
Radius-based display screening and box-based side/near checks keep viewing range
independent of the projection/depth encoding.

| `--policy` | Selection |
| --- | --- |
| `full` | All groups at full geometry |
| `cull` | Full geometry with conservative section frustum checks (default) |

The frame loop uses two command buffers. The CPU records the next view while
the GPU executes the previous one. Each buffer owns its push snapshots; submission
waits until the GPU and Display release the next target. Depth has one GPU owner.

`--recording reuse` is the default. Each buffer retains the ordered
`first/count/material` draw list. An unchanged list lets the recorder update
the camera View and Target while reusing the same World and draw commands.
A changed list records the pass again. The keys occupy six bytes per group per
CB. `--recording always` disables the list cache and replay for timing
comparisons. Visibility and transparent sorting run in both modes.

## Compact alpha pages

`assets/england/scene-alpha-pages.zip` is an alternative ENGLAND resource input.
Its 18 original alpha images occupy two 256-square RGBA4444 atlases and one
128-square clamped IGATE texture: 294,912 bytes of resident alpha data.
The 512-square layout in `scene.zip` occupies 524,288 bytes. Opaque texture
payloads retain their RGB565 mip chains.

`assets/england/alpha-pages.json` defines the source image membership and pins
the reference scene and original archive hashes. Each transparent sort group's
source images share a page. The page assignment follows adjacent uses in the
representative view set; the renderer retains its depth order and stable IDs.
Each atlas region has two texels of color extension and edge protection. IGATE
uses ClampToEdge over its complete original image domain.

The converter reads decoded original SUR data and builds geometry and UVs
forward from the structured source world. It first reproduces the complete
reference ZIP, then checks source-image pixels, protected edges, source UVs,
positions, indices, billboard anchors, sort units and material states.
Group names and `source_group_ordinal` preserve source identity; the generated
material mapping is exported separately. `source_alpha` on a merged material
labels its first source image; its `blend` field defines the runtime state.
Native u16 UV quantization allows small filtered-color differences when moving
regions between texture sizes. Depth, geometry and rendering states have their
own exact checks.

To rebuild from the original source checkout into a new output directory:

```powershell
$source = Read-Host "TankRacerCUP checkout directory"
$out = Read-Host "New output root"
$assets = "systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england"
$archive = "$source/assets/game/LEVELS/ENGLAND/FASTFILE.DAT"
cargo run --manifest-path "$source/Cargo.toml" --target-dir "$out/source-target" --release --locked -p tankracer-assets --bin decode-surfaces -- "$archive" "$out/decoded"
node systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/split_alpha_atlas.mjs --source-root "$source" --decoded "$out/decoded" --archive "$archive" --baseline "$assets/scene.zip" --layout "$assets/alpha-pages.json" --output "$out/split"
node systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_alpha_pages_layout.mjs
```

The output contains `candidate/scene.zip`, `group-material-map.json` and the
conversion checks. The staged compact input uses the ordinary build path:

```powershell
$env:CARGO_TARGET_DIR = "$out/target"
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map "$assets/scene-alpha-pages.zip" --camera "$assets/cameras-quick.json" --out "$out/app"
```

## Native-affine tree outlines

`assets/england/scene-compact.zip` combines the compact alpha pages with
conservative NTREE2 silhouettes. One hundred complete cards use eight vertices
and six triangles each; the four partial cards retain their source geometry.
The full scene adds 400 input triangles. Each affected native meshlet still fits
one payload sector, so the native mesh remains 506,112 bytes.

The converter starts with the original quad's packed side offsets, vertical
positions and u16 UVs. It derives their affine UV mapping, clips the nonzero
alpha support with a bilinear guard, then quantizes each new position before
calculating its UV. The compact scene stores each vertical billboard's X/Z
anchor exactly in its Q16.16 origin; vertical positions use a shared
1/32-world-unit lattice and side offsets use 1/64 world unit. The position
error is at most 1/64 world unit vertically; native record sizes stay the same.
The crown and trunk form one concave polygon, with shared vertices at every
triangle seam. The converter selects a guarded split row in source texels and
checks both portions of every bilinear support rectangle across that row.
Six nondegenerate triangles cover the polygon exactly. The final quantized polygon
must contain the required alpha support intersected with the original quad's
actual UV domain.

Original quads remain in `bounds_geometry`, providing conservative visibility
and depth-sort bounds at the selected billboard precision. Fractional-alpha
groups sort by depth and stable ID; refining their precision can change the
order of nearly equal-depth items. Binary-alpha cutouts use fixed texture/stable-ID
order instead. A collapsed outline retains its original quad. Textures, materials, anchors and render states retain the
compact page layout. Native u16 UV and fixed-point triangle interpolation allow
small color changes; depth and renderer state are checked independently.
The GPU's quantized N27 edge coefficients can also change isolated boundary
pixels when a card is retriangulated. Coverage checks distinguish these changes
from missing source alpha, duplicated blending and changed depth state.

Use the decoded SUR data and compact conversion output from the preceding
section. A fresh output directory also receives numeric checks, comparison
geometry and close tree cameras:

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/evaluate_tree_outline.py --source-root "$source" --decoded "$out/decoded" --input "$out/split/candidate/scene.zip" --precision 8 --billboard-exact-anchor --output "$out/outlines"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/build_tree_silhouette.py --source-root "$source" --decoded "$out/decoded" --input "$out/outlines/scene.zip" --output "$out/silhouette"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_tree_outline.py
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_tree_silhouette.py
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map "$assets/scene-compact.zip" --camera "$assets/cameras-quick.json" --out "$out/compact-app"
```

The rebuilt compact package is `$out/silhouette/scene-compact.zip`. The source-quad input
and comparison packages remain explicit choices in the build's `--map` option.

## Opaque atlas and retained source inputs

The default `scene-opaque-atlas.zip` derives from the retained
`scene-compact.zip`. All 38 opaque native textures share one 1024-square RGB565
atlas with full mips. Each original image occupies the middle of a power-of-two
cell twice its side; clamped edge extension is 32 pixels for a 64-square source
and 64 for a 128-square source. Geometry, group identity, tint and transparent
assets remain unchanged. RGB triangles keep their native format and bypass
sampling while sharing the installed opaque texture state.

`opaque-atlas.json` records the source/archive hashes, producer identity, layout
and content-quality contract. The original `scene.zip`, `scene-alpha-pages.zip`
and `scene-compact.zip` remain available, including every original opaque mip.
The standalone converter consumes those native mips directly; it needs no
external game checkout. Rebuild into a fresh directory:

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/opaque_atlas.py "$assets/scene-compact.zip" --output "$out/opaque-atlas"
Get-FileHash "$out/opaque-atlas/scene.zip" -Algorithm SHA256
```

ZIP entry times, ordering and attributes are fixed. The rebuilt archive hash
must match `archive_sha256` in `opaque-atlas.json`. Converter checks are in
`tools/check_opaque_atlas.py`.

Source cells remain isolated through the original image's final mip. Deeper
atlas levels merge cells, and filtered footprints can cross a cell before that
merge. The ENGLAND visual comparison accepts these distant colour differences
and UV requantization; the bordered scene uses ordinary sampling mode. Sampler probes and
pixel extraction remain available to inspect that content-specific tradeoff.

## Fixed-grid scene

The bordered archive and its producer are retained and are not overwritten.
`tools/grid_atlas.py` builds the borderless fixed grid from the same
retained source archive. Its two-bit `atlas_grid_log2` field selects1,2,4 or8
cells per axis. The matching GPU gateware limits texture mip selection and clamps both filtering
planes inside each selected cell; older gateware does not implement this field.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/grid_atlas.py "$assets/scene-compact.zip" --side 512 --grid-log2 3 --native-cells --embed-rgba textures/IGATE_ALPHA.bin --move-alpha-source SCARE --output target/tankracer-grid
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map target/tankracer-grid/scene.zip --camera systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england/cameras.json --recording prepared --out target/tankracer-grid-app
```

`--native-cells` packs larger sources into aligned groups of sampling cells.
The default 512/8 layout uses all 64 cells. Seven opaque 128-square images
and IGATE occupy four cells each. The 31 opaque 64-square images and SCARE
occupy one cell each.
No source is reduced or enlarged. Every opaque material keeps grid exponent
three, preserving the single shared-state opaque ring. The shared RGB565/RGBA4444
mip allocation is 700,928 bytes. Texture descriptions alias the same
allocation. The separate 256-square RGBA4444 atlas uses 176,640 bytes,
for a total of 877,568 bytes across the two physical payloads.

Filtering clamps at every 64-square boundary, including the internal boundaries
of a 128-square source. At the minimum eight-square atlas mip, a large source
therefore retains 2×2 texels rather than its original 1×1 tail. This deliberately
trades continuous filtering across those internal boundaries for one view and
the existing shared-state command path. Source regions remain isolated.

Each primitive must stay inside its source region. The converter moves its
normalized UV endpoints two Q16 units into that region to cover the
interpolator's one-unit truncation; no texture gutter is allocated. Complete mip
allocations are emitted even for forbidden coarse levels. `--embed-rgba` also
remaps IGATE into the atlas and builds its mip chain using the shared
premultiplied-alpha source encoder, preserving every original base word.
`--move-alpha-source SCARE` recovers the 64-square sources from the retained
two-page inventory and copies SCARE into the last free cell. IGATE and SCARE
share the RGBA4444 view with exponent three. This gives IGATE a minimum 2×2
source mip and clamps across its internal 64-square boundaries. The standalone
IGATE embedding without alpha consolidation retains its 128-square clamp
and 1×1 mip. The other 16 source images fill the new 4×4 alpha
grid with independent premultiplied-alpha mips through 1×1. Texture filtering
is trilinear, including IGATE. Compared with the archived base-only alpha
pages, the new mips change cutout sampling and depth coverage at distance.
Without `--native-cells`, the tool retains uniform-cell resampling for
explicit alternative builds.

Run `tools/check_grid_atlas.py` for native mip choice, guarded endpoints and
deterministic conversion. Use the sampler probe and `tools/check_atlas_probe.py`
on the intended camera sequence to check actual weighted-tap isolation. A grid
does not infer arbitrary UV wrapping or primitive ownership from the source.

The default archive uses this grid conversion. Cutout classification happens
when packing the application, from actual texture alpha rather than material
names. The retained bordered and native archives stay available. Regenerate into
a fresh destination:

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/grid_atlas.py "$assets/scene-compact.zip" --side 512 --grid-log2 3 --native-cells --embed-rgba textures/IGATE_ALPHA.bin --move-alpha-source SCARE --output target/tankracer-grid-source
```

## Fixed unlit bindings

The world registers the mesh directory and its unlit material bindings once.
The bank stores the fixed material fields and records through the normal GPU
World/View encoders. Each frame CB reserves one View packet plus one World
slot per unique binding, with the Target after this prefix. Replaying a fixed
ordered list updates View and Target only. Frustum selection and
transparent ordering still run before deciding whether the list can be reused.

See [Reusable unlit draw lists](../../docs/gpu-api-storage.md#reusable-unlit-draw-lists)
for the storage and lifetime contract.

### Prepared opaque command rings

`--recording prepared` selects an application-owned alternative to the default
`--recording reuse` UnlitBank path. It uses the existing `PreparedPool`,
`PreparedPart::mark_slice` and the CommandBuffer linked-list APIs.
If all opaque materials have one identical native texture/filter/raster state,
recording enables `PreparedPart::share_state()`. Native vertex-colour geometry
keeps its RGB format while sharing that installed texture state. The prefix's
real draw establishes the checked state; opaque slice entries upload World
without additional material SETs, full-render fences or restoration segments.

The producer orders disjoint opaque groups by their authored route-ring order.
Potentially intersecting bounds retain native order, with padding for position
and depth quantization. Without a route, native group order is retained.
Visibility, display distance and transparent depth sorting keep their
existing tests; the command ring does not draw rejected candidate groups.

Each command page reserves 8 KiB of commands and 32 KiB of push storage. Offline
packing bounds the stream to 1,960 instructions and 256 World packets per
page. A larger individual group is rejected for this recording option. Startup
records one start/end pair per group once. ENGLAND uses three pages, reserving
120 KiB of external command/push storage. Mesh directory views are temporary
and restricted to the group being recorded; external geometry remains allocated.

Each frame records one real opaque group in its dynamic prefix to establish the
checked Target/View state. Adjacent selected command ranges then merge into
safe slices, including the two sides of a wrapped ring selection. Visibility
holes and page boundaries can require additional slices.
Up to 30 slices leave room for dynamic prefix and tail in one 32-entry list. Excess
opaque groups and the unchanged transparent order use the original UnlitBank
writer in the tail. If that tail exceeds a list segment's 8-KiB limit, playback
switches to ordinary double-CB submission with the full 64-KiB CB limit. The
limit includes the tail's first-sector offset and the final END instruction.

One writable CB owns both dynamic intervals: `begin_list`, closed prefix pass,
`link_prepared_range`, closed UnlitBank tail pass, then `finish_list`. The tail
reserves its View/World block after the prefix's aligned push data. The resulting
list owns only one of the two configured CB slots. After `submit_list`, the CPU
can select and completely record the next frame in the other slot while the
GPU runs. Only then does it retire/present the previous frame and submit the
next list. Prepared command/push data stays immutable throughout playback.
Reused opaque commands are referenced directly, with no restoration segments.

Compare identical map/camera inputs with `--recording reuse` and
`--recording prepared` through the workload and cycle-replay commands below.
Separate startup from warm CPU work, and include consecutive or stationary
cameras: UnlitBank can patch only View/Target when its complete draw list stays
unchanged. Native CPU timing with functional GPU and cold GPU/native-SDRAM
replay are separate costs; neither establishes whole-system frame time.

## Authored route input

The optional route producer reads `assets/maps/ENGLAND/map.json` and its
`scenes/ENGB.json` from the read-only TankRacerCUP source. It exports all 36
WayPoints and the 33-point circuit beginning at `wptstart`, following the first
`:next` link and closing back at the start. Coordinates are the exact signed Q10
scene integers divided by 1024. The other links and original parameter strings
remain in the graph. `object_index` is the zero-based index in the ENGB source
object array, not a runtime entity ID. Source JSON and manifest SHA256 values,
and the manifest's original ENGB resource identity, accompany the export.

```powershell
$source = Read-Host "TankRacerCUP checkout directory"
$out = Read-Host "Output directory"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/export_route.py --map-directory "$source/assets/maps/ENGLAND" --output "$out/england-route.json"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_route.py --map-directory "$source/assets/maps/ENGLAND"
```

The official application build defaults to the checked-in
`assets/england/route.json`; `--route PATH` selects another staged route input.
The recipe declares the actual file, and the materializer snapshots it by hash.
Both generated JSON sidecars are also frozen in `tankracer-static-assets/`.
A standalone `static_assets.py` invocation keeps route input optional through
`--route PATH` or `TANKRACER_ROUTE_INPUT=PATH`.

The packer validates names, source object indices, Q10 conversion, every branch
reference, duplicate edges, and the exact branch-0 circuit. `route.json` wraps
the authored graph with positions transformed using the scene origin and scale.
`route-ring.json` records the static-group ordering and conservative segment ranges.
For the current compact ENGLAND scene, all 1,235 static groups share one ring,
including 697 opaque and 538 blended sort units. Each record has 16 u16 words:
39,520 bytes of metadata. The 33-entry segment directory uses 12 u16 words per
entry (792 bytes); 39 fixed chunks of at most 32 records use a 1,248-byte bound
directory. The renderer keeps a 4-byte render-only position key and a 2-byte
location-grid cell index.
The ring tables are loaded, including in the explicit clusters comparison build;
the current compiler retains their resource references. JSON sidecars are
inspection data and do not add another runtime copy.

Each record preserves the compact center/extents, radius, meshlet
index/count pairs, material/billboard key and stable group ID. Its last five
words are reserved. Intervals use X/Z AABBs rather than the radius field.
The segment directory stores cyclic start/count, X/Z origin and delta, and a
reciprocal-length hint. Meshlet indices remain logical geometry spans. Each
chunk encloses both group boxes and visibility spheres; it uses the existing
coarse classifier before reading metadata into a shared 512-word scratch.
Opaque and blended groups then follow their existing visibility tests,
opaque coalescing and depth/stable-ID transparent sort.

The renderer recovers camera X/Z from the rigid view. A baked X/Z grid selects
segment IDs, then the original projection, distance and tie tests compute s.
The current 32-world-unit cells are exactly four render units wide. There are
18 by 27 cells (972 bytes of u16 list offsets), sharing 77 ascending segment
lists (468 bytes). A normal query examines zero to three segments. The lists
contain every native segment within 24 world units of the complete cell AABB;
they use the quantized `a` and `a+d` endpoints and exact integer comparisons.
Thus they retain every segment that the old 23-world-unit acceptance test can
choose, including ties. Cell-edge/corner and corridor tests use the old full
33-segment scan as an independent oracle. Empty cells or positions outside the
grid use cluster fallback. Other scales choose an exact power-of-two render
step; unsafe layouts or insufficient fixed-point quantization margin retain
the complete segment scan.

The 33 real segments still define the rendering position. Its key is Q16.16 `s = segment + t`, retained as two u16 words.
The integer part selects a segment range; the fraction is progress along that
segment. Segments have different lengths, so the key has no fixed world-unit
spacing.
`33` wraps to `0`, and reverse movement naturally decreases the key. This key
is only for rendering and does not change game checkpoints, physics or RNG.
A projected point is accepted only within 23 world units in X/Z of the camera. The
baked corridor is 24 world units, leaving one world unit for fixed-point view
reconstruction and projection error. A reciprocal approximation can cause an
extra fallback, but cannot bypass this final distance check.

Positions and bounds use render coordinates (`world * 0.125` for ENGLAND).
The baked coarse radius is 200 world units plus the 24-world-unit corridor.
Removing the largest empty cyclic gap gives at most two metadata spans, with
at most 646 candidate groups in this scene. Full bounds retain adjacent
hairpin legs and large objects. The 200-world radial distance is a coverage guarantee; the candidate spans and
coarse bounds can include farther objects. The existing final visibility test uses forward depth,
so distant view-corner and bounding-sphere false positives can differ. These
far changes are checked explicitly, without relaxing near geometry or state
checks.

`--candidates route` is the application default. The normal route path scans
only the unified ring chunks; it does not make a second transparent-cluster
pass. Outside the accepted corridor, or without a route table, it uses the
original complete spatial-cluster path. `--candidates clusters` selects that
baseline explicitly. `--policy full` always bypasses route coarse filtering.
The candidate path does not alter the draw binding or matrix contract.

Opaque selection marks a meshlet-first bitset and streams its baked span table
in ascending order. Nonempty buckets reuse the existing group scratch. Adjacent
compatible spans coalesce during emission. ENGLAND uses a bitset and
a sparse immutable span table; selection needs no per-frame opaque
sort. Transparent depth order and the UnlitBank replay
path retain their existing behavior.

Coarse classification shares projected centers and extents between rejection
and full acceptance. Its checked host fixture runs the actual RCC function
against the original visibility rules, including fixed-point clip boundaries,
moving cameras, rear views and off-route positions:

Group-center acceptance checks depth first, then horizontal and vertical
containment. Each rejected component ends that fast path before the next
projection. Boundary groups retain their full conservative box test.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_classification.py --output target/classification-check
```

```powershell
$assets = "systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england"
$out = Read-Host "New output root"
$env:CARGO_TARGET_DIR = "$out/target"
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map "$assets/scene-opaque-atlas.zip" --camera "$assets/cameras-quick.json" --candidates route --out "$out/route"
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map "$assets/scene-opaque-atlas.zip" --camera "$assets/cameras-quick.json" --candidates clusters --out "$out/clusters"
```

Check the packed real-scene ranges and independently rebuild the producer output
from a materialized snapshot's frozen inputs (the second output must be new):

```powershell
$assets = "systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england"
$out = Read-Host "New output root"
$env:CARGO_TARGET_DIR = "$out/target"
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map "$assets/scene-opaque-atlas.zip" --camera "$assets/cameras-motion.json" --out "$out/snapshot"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_route_ring.py --scene "$assets/scene-opaque-atlas.zip" --packed "$out/snapshot/tankracer-static-assets" --output "$out/route-bounds-check.json"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_route_location_grid.py --packed "$out/snapshot/tankracer-static-assets" --output "$out/location-grid-check.json"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_route_snapshot.py "$out/snapshot" "$out/rebuilt"
```

## Static visibility and transparent-order priors

The renderer keeps the route ring as the coarse candidate domain and individual
groups as visibility units. Immutable opaque command pages reuse native
World packets, while the transparent tail keeps the established depth/stable-ID
order. Static and slowly changing views still benefit from the default UnlitBank
View/Target patch path; Prepared mode targets changing selections.

`tools/view_pvs.py` bakes a conservative candidate mask for each existing XZ
location cell and 32 horizontal headings. The circular cone encloses the CPU
frustum under arbitrary roll. A one-world-unit camera guard and a two-radius
group guard retain the loose candidates accepted by the existing box test.
At runtime, the table is used only with an accepted route position, an in-grid
cell and absolute pitch at most 19 degrees; other views retain the original
candidate path. A cached 156-byte mask rejects empty 32-group chunks before
their metadata DMA and rejects individual groups before their exact tests.
Only cells with a nonempty route-location list need distinct baked masks;
other cells use the universal mask. Cells share identical 32-heading directories,
and masks share native 16-byte blocks across headings and neighboring positions.
The cache retains the current heading directory and block references; a changed
view copies only changed blocks, with uniform blocks filled locally. Storage is
bounded by a 2-MiB budget. This exact representation does not perform occlusion
culling or change any candidate bit. See [PVS storage](../../docs/tankracer-pvs-storage.md)
for the layout, bounded random entry and cache validation.

`tools/alpha_order.py` bakes stable-ID ranks for 16 headings and nine pitch
representatives from compact static group centers. Translation contributes the
same offset to all center depths, so it need not enter the prior. A changed
member set gathers a candidate permutation using ranks; bounded insertion
repair then uses actual frame depths and the original stable-ID tie break.
After four moves per visible group it falls back to the original Shellsort.
Unchanged members retain the previous-order check. Approximate direction bins
affect work only, never the final order. ENGLAND uses 165,888 immutable bytes
and 2,226 extra heap bytes including tags.

The priors use static bounds and route data, independently of replay cameras.
One-sided boundary cards do not imply one global transparent order: a billboard
can interleave with a fixed card, and visible domains overlap. The prior/repair
path preserves those relationships without a special fixed-card pass.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_alpha_order.py --output target/tankracer-alpha-check
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_view_pvs.py target/tankracer-map --probes 1024 --output target/tankracer-pvs-check.json
```

The sort check executes the actual RCC repair bodies with independent total
ordering and guarded buffers. The visibility probe covers native Q16 cameras,
roll, cell/heading boundaries and variable height; it corroborates the baked
guard construction rather than proving all continuous camera poses by sampling.
`tools/map_priors.py` inspects fixed-card support planes and partial painter
constraints within an explicit route domain; its report is diagnostic and does
not change runtime order.

## Camera input and workload

For board playback, use `assets/england/cameras-loop.json`: one complete recorded
lap, linearly resampled at 30 Hz and paced by the wall clock at 60 seconds per lap.
The build and regeneration commands are in [camera playback](../../docs/tankracer-camera-playback.md).

Use a JSON list of 1–2730 views with three-component `eye` and `target` and an
optional `up` vector and `label`. Keep quick and full sequences as separate
camera files. Include rear and off-route viewpoints for geometry coverage.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/capture_camera.py RECORDING.trr2.gz target/cameras.json --start 1 --step 100 --count 118
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-static-workload -- target/tankracer-map/cpu-v3-boot.bin 2 4000000
```

The camera exporter preserves recording world coordinates. Build with a
scene that uses the same basis. The probe emits one row per GPU submission
with geometry and billboard counts, depth coverage including cutouts, visible billboard
pixels, and RGB565 and D16 fingerprints. Measure raster overdraw and timing
separately through the [application profiling workflow](../../docs/application-profiling.md).
The CSV also reports CPU instruction advances since the preceding submission;
its first row includes boot/startup. These counts are distinct from CPU cycles
and milliseconds.

For a bounded CPU timing window through the actual core, I/D caches, arbiter
and native SDRAM controller:

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-cpu-timing -- target/tankracer-map/cpu-v3-boot.bin 12 35000000 600 target/tankracer-cpu-time
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/profile_cpu.py target/tankracer-map target/tankracer-cpu-time/frame-002.cpu-execute64 --output target/tankracer-cpu-profile.json
```

This peer uses functional GPU/Display, manually released vblank and no GPU
memory competition. START-to-START samples include CPU submission work; the
first also includes initialization, but not SPI loading. The execute histogram
records scalar execute occupancy, leaving fetch and FPU-only cycles unassigned.
`profile_cpu.py` also accepts the workload's `.cpu-pc64` instruction histogram.
An optional `--gpu-cycles N` holds BUSY for a fixed synthetic service time to
compare CPU/GPU overlap and post-service submission gaps. It is a controlled
pipeline experiment, not an end-to-end GPU or board frame-rate measurement.

Both the workload inspector and GPU cycle replay accept direct command buffers
and the current ordered command-list submission. Inspection follows the
right-aligned table and each instruction-aligned slice in execution order,
preserving meshlet DMA slots across segments. END must coincide with a segment's
end, and the final segment must contain END.
The PPM probe scales RGB565 components with integer division; registered Display
output uses bit replication. Compare native scanout through that RGB565 expansion
contract; the recorded RGB565 fingerprints retain the original words.

The CSV includes live heap bytes (including allocator tags), the largest free
heap block at submission, and the stack high-water mark observed instruction by
instruction. The probe validates the board's boundary tags and resident stack
range. Heap values describe submission samples; descriptor scratch inside the
preallocated GPU arena is included in that arena's fixed allocation.

Passing a fresh capture directory as the workload command's fourth argument
records up to 16 submissions. Each capture includes the 8 MiB pre-submit SDRAM
image, command/target metadata, functional colour/depth fingerprints and a PPM
image. A `frame-NNN.cpu-pc64` file contains 65,536 little-endian u64 instruction
counts indexed by the CPU's logical PC, covering the interval since the preceding
submission. Resolve those PCs using the captured application's `application.dbg`;
these instruction counts do not include cycle weights. Replay one GPU capture
through the native GPU/SDRAM cycle models:

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-static-workload -- target/tankracer-map/cpu-v3-boot.bin 12 60000000 target/tankracer-captures
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-gpu-timing -- target/tankracer-captures/frame-003.json 4000000 180 idle target/tankracer-gpu-frame-003
```

The cycle replay takes explicit cycle and host-second limits and a fresh output
directory. It reports GPU busy cycles at 54 MHz, meshlet starts, final fragments,
texture beats and colour/depth agreement with functional execution. `idle` measures
the GPU with the production memory arbiter and native SDRAM controller. `display`,
`display_cpu` and `display_cpu_heavy` add synthetic traffic. CPU application work
and actual Display presentation remain separate measurements. A successful command
alone does not establish image agreement: inspect both `functional_*_match` fields.
The replay writes complete little-endian physical planes to `color.bin` and
`depth.bin`, including padding, for exact comparisons beyond the visible PPM.
Append `--trace` to write `timeline.json`: it records each executed command's
physical address, latch, first execution, issue and CP release, command-cache DMA
intervals, CP phase intervals, and selected DMA/render/memory events. Timestamps
are absolute 54 MHz GPU clocks and intervals are `[start,end)`. Subtract the
reported `start` to align submissions. A DRAW's CP release ends its vertex work;
downstream pixels can remain in flight. Phase totals partition GPU busy time;
pipeline load counters and DMA intervals overlap and must not be added to them.

Append `--uv-probe` to the workload command (after `--images-only` when used) to
write a `frame-NNN.uv.json` alongside each image. A separate full functional
replay records UV/LOD/tap bounds and checks both complete physical output planes
against the application. Sampling probes do not enter CPU/GPU timing comparisons.
`--uv-samples` additionally captures bounded transactions at LOD 6 and above,
including nonzero-weight tap coordinates/colours and final opaque-pixel ownership.
Partial-alpha overlays are marked separately. `tools/check_atlas_probe.py` resolves
the observed bounds against original source cells; `tools/extract_atlas_pixels.py`
extracts actual cross-cell pixels, reference crops and an isolated tap-clamping
diagnostic. Both bind the probe's package identity to the supplied snapshot.

`tools/opaque_atlas.py SOURCE.zip --output NEW_DIRECTORY` produces an explicit
atlas candidate from native opaque source mip chains. It preserves geometry,
sort identities and transparent inputs; UVs are remapped into clamped source
cells and native RGB geometry retains its format. The audit lists protected mip
levels. Atlas levels that merge source cells are not source-isolated: measure
unclamped LOD and actual taps, then compare images before adopting an input.
The existing map packages remain the baseline.

Each captured frame also includes a row-major 400×240 little-endian D16 plane.
Use `--images-only` after the output directory to capture up to 192 frames without
SDRAM preimages or PC histograms. These metadata files use
`tankracer.frame-output.v1`; GPU-cycle replay takes full `tankracer.gpu-input.v1`
captures.

New GPU captures preserve the exact Device4 COUNT SEND in `count_word` and the
START byte address in `base`. Flat captures also retain `words`, their instruction
count. List captures omit `words`: their COUNT identifies a table rather than
an instruction count. The cycle replay accepts older flat captures containing
only `base` and `words`, and rejects contradictory count/size metadata.

Compare complete output sets with an explicit near depth:

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/compare_frames.py target/before-images target/after-images --near 25 --out target/frame-comparison.json
```

The comparison reports all changed colour/depth pixels per frame and preserves
exact-equality results independently of its acceptance budgets. Near pixels use
baseline opaque depth with eight fractional render-unit bits. Use `--near-mode union`
to include pixels that are near in either the baseline or candidate; this also
catches newly appearing near geometry. Colour and depth budgets default to zero. `--near-color-budget` and `--near-depth-budget` set
explicit per-frame pixel limits for evaluated visual changes. Transparent foreground
over far background and overall distant appearance still require visual review.

## Rust/Wasm reference views

At selected rendering checkpoints, compare a few matching camera poses with the
existing Rust/Wasm static inspection renderer. The reference helper loads the
existing build, stops its loop and uses only the camera API. It records game
state before and after rendering. Source cameras use Y-up unreflected world
coordinates; the Rust boundary reflects Z once. Both render targets use 400x240
with the same focal scale.

```powershell
node systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/rust_reference.mjs --source-root "$source" --cameras "$assets/cameras-coverage.json" --output "$out/rust-reference"
```

The helper accepts 1–16 upright `eye`/`target` cameras with optional labels.
`up` and roll fields are rejected; the inspection API's pitch range is limited
to 1.5 radians. `--site` selects an existing build, defaulting to the source
checkout's `build/rust/site-extra-maps`; `--chrome` or `CHROME` selects Chromium.
Outputs are `frame-NNN.ppm` and `result.json`, including actual camera poses,
frame statistics and Wasm/resource hashes. The headless profile and local server
belong to the probe and close after it finishes; the source checkout stays read-only.

The playback clears to the static Rust scene's sky color `[0.48, 0.62, 0.72]`,
rounded to RGB565 (`0x7cf6`).

Use these views to assess composition, geometry, occlusion, billboard continuity
and transparent edges. Native RGB565/RGBA4444, fixed-point interpolation and the
native vertex quantization have explicit quality tradeoffs. Native byte/depth/state
regressions and visual Rust reference checks remain separate evidence.

## Physics arithmetic

The application-owned [integer module](/systems/cpu-v3-tang-nano-20k/applications/tankracer/rcc/integer.rs)
uses `fix32` as a raw signed 32-bit carrier. Addition, subtraction, negation
and comparisons operate on those bits directly. Call `integer::mul` for a
wrapping integer product. Ordinary `fix32` multiplication has Q16.16 semantics.
The current checkout forms the integer product from unsigned 16-bit product
windows, including the high cross terms.

| Entry | Contract |
| --- | --- |
| `from_i16(value)` | Sign-extend an integer into the raw carrier. |
| `mul(a, b)` | Low 32 product bits, wrapping for both signs. |
| `shl(value, count)`, `sar(value, count)` | Mask the count to five bits, then shift left or arithmetically right. |
| `trunc_shift(value, count)` | Divide by `2^(count & 31)`, truncating toward zero. |
| `div(numerator, denominator)` | Signed quotient truncated toward zero. `INT_MIN/-1` wraps. A zero denominator halts with diagnostic `0x7b01`. |
| `scale3_q12(values, denominator)` | Scale three in-place `i16` components by `4096/denominator` and retain the low 16 quotient bits. Zero is checked before any component write. |

`scale3_q12` uses a 12-step integer remainder loop when each component has
magnitude at most 4096 and the denominator lies in 2048–32768. The integer
quotient starts in 0–2, and each doubled remainder fits an unsigned word.
Other nonzero inputs use the general signed divider. The module allocates
no heap storage and owns no static data. Callers supply three writable words
for the scaling operation and must handle inadmissible divisors before
requesting a recoverable physics update.

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-physics-check
```

This command runs rustc and RCC checks, compares fixed-seed integer and Q12
cases with native Rust arithmetic, and verifies a 600-step arithmetic
integration sequence. A separate CPU core emulation measures dependent
multiply and repeated triple scaling with one-cycle instruction and data
responses. The report and compiler inputs go to `target/tankracer-physics`.
Core timings exclude the system caches, SDRAM arbitration and GPU. Vehicle
attitude, contact response and driving acceptance require their own inputs.
