# Tank Racer static-map playback

Build with a static scene archive and a camera sequence staged within the
repository. The application uses the regular SDK and Display path.

The same catalog registers `bench-tankracer-physics`, a separate frozen-contact
single-vehicle benchmark. Its kernels, resource inputs and execution boundary
are described in [physical update](/systems/cpu-v3-tang-nano-20k/applications/tankracer/physics.md).

`bench-tankracer-terrain` loads two adjacent collision pages and checks the
production terrain kernels against reference queries; see
[cell-local terrain queries](/systems/cpu-v3-tang-nano-20k/applications/tankracer/terrain.md).

`bench-tankracer-driving` connects real primary contacts, surface noise/effects,
lazy auxiliary queries, track poses and immediate car/ground collisions to
ordered persistent vehicle updates sharing one RNG and effect pool;
see [real-map driving](/systems/cpu-v3-tang-nano-20k/applications/tankracer/driving.md).
Its `--scope complete --steps 3` cases compare the complete original prepare and physics
method for continuous two-car driving, car collisions and an authored object hit,
including all part poses, the complete immediate effects sequence, boost,
temporary-effect expiration and smoke's synchronous impact generation.
`--scope startup` constructs the cars from model/spawn metadata before driving;
the reusable `vehicle-startup` qualifier checks the full fresh Player state.
`vehicle-inactive` qualifies the complete non-AI middle-distance Tank branch
over the constructor row, real terrain and shared effects/RNG. The kernel also
binds the full driving owner's fields directly; `--scope visibility --steps 4`
checks both branch transitions in continuous two-car updates.
`--scope effect` exercises both game-owned surface-effect slots through complete
vehicle updates; `contact-effect` checks synchronous publication across query errors.
Its reusable complete part-pose and route-restoration kernels also have independent
`vehicle-pose` and `vehicle-transfer` qualification entries.
`player-control` qualifies sampled input/countdown, turret, auxiliary brake
restoration and collision indication on shared live fields before Tank dispatch.
`--scope controls --steps 6` connects those sampled fields to continuous Tank
updates; `--scope fast-controls --steps 2` covers fast braking and restoration.
`player-route` covers actual ENGLAND checkpoint/progress queries, lap records,
wrong-way state and the raw-time race clock.
`scene-player` connects input, lap events, turret/fire, brake and route progress
to the same scene/vehicle owners before parent Tank visibility and preparation.
`--scope player --steps 2` runs Shell/Homing/Warp input through this complete
Player stage and the continuous Tank physics owner, comparing route, inventory,
shared flight, projectile payloads, scenery, vehicle effects and RNG each frame.
`--scope clock` couples the raw race clock and countdown/release gate to that
continuous path, with independent physics-dt clamping and per-frame clock checks.
`track-effects` qualifies track spray, ground-mark FIFO and brake/reverse lights
on one shared quad/line phase bank.
`vehicle-effects` qualifies damage/traction smoke with synchronous impact emission,
shared debris ownership and shield initialization/update using the same game RNG.
`car-effects` executes the complete ordered vehicle-effects stage on those owners,
including pickup fade, impact animation, track effects and temporary electric lines.
`car-motion` qualifies complete post-collision movement and all part poses, with
explicit successful-tick hull/rate publication after the intervening effects stage.

`bench-tankracer-scene` loads the complete initial ENGLAND collision metadata,
checks candidates/class slots and runs connected Fall/Bounce contacts and motion; see
[scene collision assets](/systems/cpu-v3-tang-nano-20k/applications/tankracer/scene.md).
The reusable `scene-laps` qualifier covers real lap memberships, camera-delayed
queues and off-camera Fall restoration. `scene-train` qualifies both authored
ENGLAND paths against live class calls and shared RNG. `scene-emitter` covers
the five authored emitters and their real ownerless projectile factories.
`scene-flock` connects all three bird groups, live wing models and render mesh revisions.
`scene-ambient` qualifies chimney particles and ambient sound-region selection.

[Collision and body arithmetic](/systems/cpu-v3-tang-nano-20k/applications/tankracer/collision.md)
describes reflection, pair impulses, body springs, contact hulls and their module checks.

[Weapon and explosion state](weapons.md) describes player fire, weapon construction, projectile launch,
banked flight, targeting, reflection, trails, terrain marks, complete Concussion/Debris
impact updates, complete Shell/Homing/Warp hits with live mesh damage and temporary
effects, UFO projectile attachment/follow, UFO construction, complete transfer updates
and shared retirement, mine placement/arming/lifetime and complete
mine contacts with vehicle impulses and electric effects,
explosion animation, smoke, contact fragments and shared blast slots, with their reference snapshot checks
and scene integration boundary.

`bench-tankracer-fragments` qualifies authored auxiliary mesh activation,
real-terrain contact, vertex/center updates and live class completion through normal asset loading; see
[mutable mesh fragments](collision.md#mutable-mesh-fragments).

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map target/map.zip --camera target/cameras.json --out target/tankracer-map
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
| `far`, `lod_distance` | Render-unit depth and LOD threshold. Defaults to `110` and `20`. |
| `display_distance` | Conservative selection depth in render units, including each group's radius. Defaults to `far`. ENGLAND exports `25`, corresponding to the source's 200 world units; projection remains `far=110`. |
| `position_shift` | Shared position lattice, defaults to `10` or 1/64 render unit. Each meshlet retains its own origin. |
| `billboard_position_shift` | Optional billboard position lattice, defaults to `position_shift`; integer `0..31`. |
| `billboard_exact_anchor` | Optional boolean, defaults to `false`. Vertical billboards keep their common X/Z center line in full F16 origins with zero X/Z deltas; Y retains the absolute billboard lattice. |
| `materials` | Indexed entries with optional `texture`, `rgba`, `mipmaps`, `clamp`, `blend`, and RGBA byte `tint`. Textures are square powers of two. Omitting the texture uses RGB vertices. |
| `groups` | Spatial and material runs with `material`, `vertices`, `triangles`, and optional `name`, `lod`, `billboard` and `bounds_geometry`. |

Triangles are triples of zero-based vertex indices. Geometry uses the SDK's
ordinary native culling contract. Supply both windings for a double-sided
mesh. A group can carry a second geometry in `lod` with its own vertices and
triangles. Preserve shared boundary positions and topology across exported
LOD groups. The packer snaps all groups to the common lattice and rejects a
meshlet whose local deltas exceed their native width.
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
Each blended group is a sort unit. Keep intersecting foliage or blended
geometry in suitably small groups to preserve depth order.

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

Each group occupies 26 bytes: three signed Q9.7 center components, three unsigned
Q9.7 extents, an unsigned Q9.7 radius, fine and coarse first/count pairs, a material
with billboard bit 15, and the original stable group ID. Bounds round outward;
the packer rejects unrepresentable ranges. Native geometry keeps its original
order. Opaque selections sort by first meshlet and coalesce adjacent compatible
ranges. Transparent selections sort by depth and original ID. Drawing reuses a
texture/view across each consecutive run with the same texture and address mode.
Material tint and billboard selection stay per draw. Shared texture descriptors
use an indexed table independent of material tints.
Transparent sorting moves u16 indices while its six-word draw records remain in
place. A single previous-count u16 guards comparisons of old ordinal stable IDs
before each record is overwritten; every frame still writes its new depth.
An identical complete member sequence permits checking the previous order against
new depths and the stable-ID tie break. An ordered sequence reuses those indices;
a member or order change runs the original Shellsort. The first frame and ordinals
beyond the previous count never read uninitialized records. This adds no heap
buffer and does not reuse visibility, LOD or command recording decisions.
Radius-based display screening and box-based side/near checks keep viewing range
independent of the projection/depth encoding.

ENGLAND's offline producer supplies two terrain levels. It locks every group
boundary and reduces interior vertices only on gently sloping opaque surfaces
with a single XZ height layer. Edge collapse preserves winding and manifold
connectivity; every original position remains within 0.25 source world units
of the reduced surface. Fine data and materials stay intact. The far mesh reuses
source UV samples and textures, allowing a small change in distant texture detail.
The nearest conservative group depth must exceed 12 render units (96 source
world units) before selecting the coarse range. Boundaries use the same lattice
in either level. The exporter provides `--fine-only` for a full-detail reference.
The native pack stores all fine ranges contiguously and appends the coarse
ranges, retaining contiguous material runs when groups use their fine level.

| `--policy` | Selection |
| --- | --- |
| `full` | All groups at full geometry |
| `cull` | Full geometry with conservative section frustum checks |
| `lod` | Frustum checks and supplied reduced geometry beyond the threshold |

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
positions, indices, billboard anchors, LOD, sort units and material states.
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
and depth-sort bounds at the selected billboard precision. The renderer sorts
these bounds by depth and stable ID; refining their precision can change the
order of nearly equal-depth items. A collapsed outline retains its original quad. Textures, materials, anchors and render states retain the
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

## Fixed unlit bindings

The world registers the mesh directory and its unlit material bindings once.
The bank stores the fixed material fields and records through the normal GPU
World/View encoders. Each frame CB reserves one View packet plus one World
slot per unique binding, with the Target after this prefix. Replaying a fixed
ordered list updates View and Target only. Frustum selection, LOD and
transparent ordering still run before deciding whether the list can be reused.

See [Reusable unlit draw lists](../../docs/gpu-api-storage.md#reusable-unlit-draw-lists)
for the storage and lifetime contract.

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
entry (792 bytes); 48 fixed chunks of at most 26 records use a 1,536-byte bound
directory. The current renderer also keeps a 4-byte render-only position key.
The ring tables are loaded, including in the explicit clusters comparison build;
the current compiler retains their resource references. JSON sidecars are
inspection data and do not add another runtime copy.

Each record preserves the compact center/extents, radius, fine/coarse meshlet
index/count pairs, material/billboard key and stable group ID. Its last three
words are reserved. Intervals use X/Z AABBs rather than the radius field.
The segment directory stores cyclic start/count, X/Z origin and delta, and a
reciprocal-length hint. Meshlet indices remain logical geometry spans. Each
chunk encloses both group boxes and visibility spheres; it uses the existing
coarse classifier before reading metadata into the unchanged 416-word scratch.
Opaque and blended groups then follow their existing visibility/LOD tests,
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
CPU command recording, binding and matrix handling are unchanged.

```powershell
$assets = "systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england"
$out = Read-Host "New output root"
$env:CARGO_TARGET_DIR = "$out/target"
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map "$assets/scene-compact.zip" --camera "$assets/cameras-quick.json" --candidates route --out "$out/route"
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map "$assets/scene-compact.zip" --camera "$assets/cameras-quick.json" --candidates clusters --out "$out/clusters"
```

Check the packed real-scene ranges and independently rebuild the producer output
from a materialized snapshot's frozen inputs (the second output must be new):

```powershell
$assets = "systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england"
$out = Read-Host "New output root"
$env:CARGO_TARGET_DIR = "$out/target"
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --map "$assets/scene-compact.zip" --camera "$assets/cameras-motion.json" --out "$out/snapshot"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_route_ring.py --scene "$assets/scene-compact.zip" --packed "$out/snapshot/tankracer-static-assets" --output "$out/route-bounds-check.json"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_route_location_grid.py --packed "$out/snapshot/tankracer-static-assets" --output "$out/location-grid-check.json"
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_route_snapshot.py "$out/snapshot" "$out/rebuilt"
```

## Camera input and workload

Use a JSON list of 1–192 views with three-component `eye` and `target` and an
optional `up` vector and `label`. Keep quick and full sequences as separate
camera files. Include rear and off-route viewpoints for geometry coverage.

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/capture_camera.py RECORDING.trr2.gz target/cameras.json --start 1 --step 100 --count 118
cargo run --release -p cpu-v3-tang-nano-20k --bin tankracer-static-workload -- target/tankracer-map/cpu-v3-boot.bin 2 4000000
```

The camera exporter preserves recording world coordinates. Build with a
scene that uses the same basis. The probe emits one row per GPU submission
with geometry and billboard counts, opaque depth coverage, visible billboard
pixels, and RGB565 and D16 fingerprints. Measure raster overdraw and timing
separately through the [application profiling workflow](../../docs/application-profiling.md).
The CSV also reports CPU instruction advances since the preceding submission;
its first row includes boot/startup. These counts are distinct from CPU cycles
and milliseconds.
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

Each captured frame also includes a row-major 400×240 little-endian D16 plane.
Use `--images-only` after the output directory to capture up to 192 frames without
SDRAM preimages or PC histograms. These metadata files use
`tankracer.frame-output.v1`; GPU-cycle replay takes full `tankracer.gpu-input.v1`
captures. Compare complete output sets with an explicit near depth:

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
chosen distant LOD have explicit quality tradeoffs. Native byte/depth/state
regressions and visual Rust reference checks remain separate evidence.

## Physics arithmetic

The application-owned [integer module](/systems/cpu-v3-tang-nano-20k/applications/tankracer/rcc/integer.rs)
uses `fix32` as a raw signed 32-bit carrier. Addition, subtraction, negation
and comparisons operate on those bits directly. Call `integer::mul` for a
wrapping integer product. Ordinary `fix32` multiplication has Q16.16 semantics.
The helper uses the CPU's `MUL.LO32` instruction through `fmul_lo32`, keeping
operands and the wrapped result in the scalar register domain.

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
