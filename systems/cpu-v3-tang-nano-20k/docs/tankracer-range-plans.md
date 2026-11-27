# Tank Racer bounded visibility plans

The static-map application can bake visibility directly into marked Prepared
ranges. This is an opt-in alternative to runtime group selection. It uses the
existing GPU command-list, PreparedPool, mark_slice and link_prepared_range
interfaces; no gateware or submission ABI change is required.

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --camera systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england/cameras-loop.json --recording prepared --range-limit 8 --range-layout route --out target/tankracer-ranges
```

| Option | Meaning |
| --- | --- |
| `--prepared-selection plans` | Offline bounded plans when range-limit is nonzero; otherwise the original renderer. Default. |
| `--prepared-selection coarse-sphere` | Runtime coarse PVS and sphere selection, with both opaque and cutout Prepared ranges. Requires Prepared recording and range-limit 0. |
| `--prepared-selection coarse-sphere-shared` | Same runtime selection, with checked material sharing inside each cutout view group. |
| `--prepared-selection coarse-sphere-incremental` | Exact coarse-sphere selection with a CPU-side cache for nearby camera poses. |
| `--range-limit 0` | Default. Original dynamic selection, or the coarse-sphere mode selected above. |
| `--range-limit 4\|6\|8` | Maximum number of immutable links across opaque and cutout work combined. Requires Prepared recording, culling policy and a route. |
| `--range-layout route` | Prefer route progress for opaque and cutout grouping, while retaining intersecting surfaces' native relative order. Default. |
| `--range-layout lanes` | Prefer left/center/right route bands, then progress, with the same intersection constraints. |
| `--range-layout stable` | Route grouping for opaque content and the complete original texture/stable-ID order for cutouts. |
| `--range-cell 16\|32` | Source-world size of a visibility cell. Default 16. Headings have 32 bins. |

`MAX_GROUPS_PER_PREPARED_UNIT` in `tools/range_pvs.py` sets the maximum number
of groups per marked unit for both offline plans and runtime selection. The
default is 3. Smaller units offer finer endpoints and require more command
and push storage. The builder checks the resulting layout against page capacity.
The ENGLAND route layout at this size can require five pages in one view.
Use `--range-limit 6` or `--range-limit 8` for its offline plans.

## Offline construction

`tools/range_pvs.py` uses immutable group bounds and the authored route, never
sampled camera visibility. For each complete cell and heading bin it builds a
conservative horizontal cone with the guarded pitch domain. A projected far
envelope includes the old independent-axis box test's loose candidates. Camera
height and roll do not enter the lookup key.

All groups in a required marked unit are admitted. The producer finds runs in
the actual recorded order. It may fill holes to reduce their count, retaining
the most expensive gaps according to meshlet/triangle weights. Physical page
boundaries cannot be bridged. The weighted gap selection minimizes added unit
cost for that fixed linear order and page layout; the weight is a GPU-work
estimate, not a prediction of fragment/SDRAM timing.

The maximum is enforced offline. An infeasible page layout fails the build;
runtime never truncates a selection or records overflow draws dynamically.

Opaque commands precede cutouts. A uniform opaque atlas uses the existing
shared-state Prepared mode in its own pages; cutouts retain ordinary pages.
Spatial layouts preserve native order for intersecting quantized opaque bounds
and conservative Billboard envelopes. Cutout texture order is retained, with
spatial grouping inside each texture. Filtered alpha edges can still overlap
in screen space even when geometry is disjoint. The route/lanes layouts accept
that approximate color ordering; stable is the full-order control. No runtime
sort or order repair is performed. Fractional-source-alpha SrcOver content is
rejected by this renderer and remains supported by ordinary selection.

## Stored data and cache

The range asset contains a cell directory, shared 32-heading rows and shared
fixed-stride plans. A plan stores its range count, seed unit and interval
endpoints. Small unit directories pack both endpoints into one u16; larger
directories retain explicit u16 pairs. Plan records use a power-of-two stride
and sections are 128-byte aligned. Asset metadata records the actual encoding.

A cell change reads its directory and heading row. A changed plan ID reads one
bounded plan record and the seed draw; unchanged IDs reuse the current plan.
There is no bitmap expansion, delta chain, route search or group-metadata scan
in the frame loop. The 48-byte camera view is still read every frame.

## Recording and ownership

`rcc/range_map.rs` records all opaque and fixed-order cutout commands once.
Each unit has marked entry/end offsets and a mark after its first draw. Draws
inside a unit reuse existing material/World state. Entries use ordinary
self-contained Prepared recording for cutouts, including vertical Billboards.
Uniform opaque pages reuse their checked shared material/response state while
each marked entry still establishes World.

Each frame's ordinary prefix draws the seed to establish checked Target/View
state. The first linked interval skips that draw through its marked endpoint.
The ordinary tail closes with the existing WAIT/FLUSH/END rules. A nonempty
plan therefore submits at most K+2 list entries; an empty plan uses one dynamic
entry. No restoration segments or command copying are introduced.

The old and bounded renderers reuse one application-owned static backing
reservation. Only the selected renderer instantiates its pool. Existing
Prepared ownership and list pins retain the immutable command/push storage.
The bounded build omits the old visibility bitmap and alpha-rank payloads.

Authored camera matrices are validated against the grid and guarded pitch
before an executable is published. Runtime checks remain fail-fast for a
violated domain. Use range-limit 0 for arbitrary cameras outside this domain;
the bounded static renderer does not silently switch to an unbounded path.

## Validation

```powershell
python systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_range_pvs.py
python systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_range_pvs.py --snapshot target/tankracer-ranges --probes 4096 --output target/range-check.json
```

The checker compares the weighted cover with exhaustive small cases, traverses
every stored key and validates Q16 camera/heading/cell boundaries. Functional
and native GPU replays validate the actual submitted command lists. CPU work,
GPU work, startup, cache misses and presentation must be reported separately;
fixed list length alone does not imply a bounded board frame time.

## Runtime coarse PVS with Prepared cutouts

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-static --camera systems/cpu-v3-tang-nano-20k/applications/tankracer/assets/england/cameras-loop.json --recording prepared --prepared-selection coarse-sphere --out target/tankracer-hybrid
```

This mode reuses the route candidate search, chunk classification, metadata
reader and guarded sphere tests. Its PVS has eight heading bins and one bit
per aligned 32-group metadata chunk. Shared heading rows point directly to
8-byte masks; the CPU cache occupies 32 bytes including allocator tags.
The producer retains only chunks that the cell's route segments or their
successors can query, including complete partially queried chunks. Outside
the PVS corridor/grid/pitch domain, ordinary runtime candidate selection remains
available. This mode does not require the offline range-plan camera domain.

A resident stable-group-to-unit index maps visible groups into a unit bitmap.
There are no per-frame opaque draw arrays, alpha draw arrays or cutout sorting.
The same recorder, marked entries, prefix seed, safe links and final WAIT/FLUSH
used by offline plans submit the result. Both renderers keep the same opaque
and approximate fixed cutout order. Ordinary fractional-alpha surfaces are
still rejected by this Prepared layout.

Adjacent selected units become one interval unless a physical Prepared page
ends. A frame can link at most 30 intervals plus its prefix and tail, fitting
the existing 32-entry command list. If there are too many intervals, retain
the most expensive command gaps and merge the rest. Every page crossing is
mandatory; no selected unit is dropped and no overflow tail is re-recorded.
Gap costs come from actual marked command offsets, not asset reads or a
per-frame GPU estimate. The cost minimization is for that fixed order and
command-length metric; it does not minimize rasterization time.

Block size 1 retains individual group choices except for overflow gap fills.
Larger blocks draw every group in a selected unit and reduce list work.
These roundups can include content beyond the group's visibility/distance
cutoff, so GPU work and pixels can change even though the selection is
conservative. Compare CPU recording savings against actual GPU cycle replays.

The unit directory is read from SDRAM only while recording Prepared commands
at startup. Each unit retains five u16 values: command start/end, the mark
after its seed draw, page and first draw index. Runtime scratch reserves two
u16 endpoints per unit. The old dynamic draw/sort buffers are omitted. The
existing application-owned Prepared backing is sized for the generated pages;
there is no second command copy or SDK ABI change.

The shared variant puts each cutout material group on its own pages. If an
original visibility block crosses a group boundary, its fragments share one
visibility decision. The first selected draw in each active group goes in
the dynamic command stream and installs its material. The marked remainder
uses checked material-only links, retaining the World upload and response
synchronization. No restoration segment is needed. The link budget reserves
one entry per material group and one final entry in the 32-entry list.

### Incremental sphere selection

The route candidate span, coarse PVS and chunk classification are evaluated
for the current frame. A fully accepted chunk reuses its Prepared unit IDs.
For a partially accepted chunk, the CPU reuses group decisions whose distance
to the visibility boundaries accounts for the maximum change in view-space
projection since the reference pose. Groups near a boundary repeat the exact
sphere test for the current pose.

The asset producer supplies an integer bounding box of the packed group
centers. The CPU bounds each camera row's projection change over that box in
Q16.16. If any row exceeds 2.25 render units, the current view becomes the
reference pose and each touched boundary chunk rebuilds its lists as needed.
A change to a chunk's covered group span also rebuilds its lists. Cameras
outside the supported route and translation domain use the uncached selector.

The cache uses seven 16-bit words per route ring group, four per chunk and
48 words for the current and reference projection rows, before allocator
overhead. The bitmap passed to the range recorder is rebuilt each frame.
Prepared commands, safe links and draw order are shared with coarse-sphere.
