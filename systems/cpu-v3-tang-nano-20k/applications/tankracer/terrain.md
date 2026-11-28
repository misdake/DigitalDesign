# Cell-local terrain queries

The ENGLAND collision slice packs the original `ENGLAND.BSM` from FASTF101
through the reference assets decoder. Primary and fallback graphs retain their
root-cell, node, bucket and candidate order. Rendering LOD is independent.

## Resident pages

Each selected cell occupies one 32-KiB-aligned immutable `AssetBytes` allocation.
All internal addresses are u16 word offsets in that page. Only referenced
meshes are copied; primary and fallback nodes share their mesh records.
Candidates retain byte indices, byte offsets and the 255 terminator.

| Record | Words | Contents |
| --- | ---: | --- |
| Cell header | 16 | Magic `0x5443`, version 1, cell ID, used words; signed i32 X/Z origins; packed shifts; primary count/directory and fallback count/directory |
| Primary node | 16 | Mesh, bins, candidates, candidate byte length, shifts, signed half bounds, surface, four extras, owner flag and source node offset |
| Fallback node | 16 | Mesh, candidates, byte length, four basis and four bound values, source node offset |
| Mesh | 16 | Three i32 positions, u32 flags, radius, vertex offset/count, face offset/count, centers offset, original first-face offset |
| Vertex / center | 3 | Original signed i16 XYZ |
| Face | 6 | Original i16 normal, three byte indices packed into two words, flags |

The original face tag is `first_face_offset + index * 24`; the producer checks
this relationship before omitting repeated source-offset fields. Padding fills
the remainder of a page. The producer checks the size of all 64 ENGLAND cells
and fails if any exceeds 32 KiB.

## Query phase

`Terrain::begin(first)` saves DSEG3 and binds a cell. `cell_at(x,z)` returns the
original 8-by-8 root index or 65535 outside the grid. The caller selects that
cell's page and calls `bind(asset)`; an unchanged page needs no mapping write.
All queries use the selected page directly. `finish()` restores the incoming
mapping. Window pointers stay inside the module. Immutable reads use the
physical D-cache tags and the existing DSEG mapping instructions.

`sampling::Auxiliary::sample` consumes a column-major Q12 basis followed by
its raw integer origin, and a local XYZ point, both stored as u16 word pairs.
Each dot product wraps before its arithmetic shift. The sampler publishes world
coordinates before querying the selected page; on an error it retains kind,
surface and cache. Hits classify gaps below -75 as 0, [-75,75) as 1 and the
remainder as 2. Misses publish kind 3 and surface 0. A forced surface becomes 1
after either a hit or miss, and completed queries set cache to 1. This is the
auxiliary sampler used by suspension and material selection; four primary
contacts, sampling effects and continuous driving remain separate integration.

Statuses are 0 miss, 1 hit, 2 arithmetic error, 3 invalid bucket, and 4 a valid
cell whose page is not currently resident. The latter lets the caller fetch or
bind another cell rather than turning missing residency into a terrain miss.

`query` publishes named `Hit` fields only after a valid height calculation.
A miss changes height to `i32::MIN + 1`; upper output flag bits and conditional
extras retain the reference behavior. Node and face tags identify source data.
`height` and `normal_height` return `Plane { status, height }`; the height is
defined for statuses 0 and 1. On errors, callers retain their old height value.
`normal_height` publishes a selected normal before its plane calculation, as
the reference does. Its zero-denominator branch deliberately uses vertex X.

`fallback` preserves the source mode/flag filters, transformed bounds and
largest-component-plus-quarters metric. Strictly smaller distance wins, so
candidate order resolves ties. Misses preserve the incoming normal. Integer
products, shifts, comparisons and divisions preserve wrapped/truncated source
semantics, including `INT_MIN / -1` as an arithmetic error at the plane boundary.
Wrapped products use the existing `fmul_lo32` intrinsic: the operands remain
in F registers and `MUL.LO32` returns their exact integer low 32 bits. The
vehicle arithmetic helper uses the same instruction and integer contract.

## Build and qualify

The standard application `bench-tankracer-terrain` selects adjacent cells 24 and
32, following the start straight. The asset producer builds a small host tool
against the caller-supplied reference checkout, reads its original archive, and
obtains expected results by calling the unmodified reference game. The checkout
is read-only. Runtime data and the query corpus are source-declared assets loaded
by the ordinary CB06 installer.

```powershell
$reference = Read-Host 'TankRacerCUP Cargo.toml'
$archive = Read-Host 'Original ENGLAND FASTFILE.DAT'
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build bench-tankracer-terrain --reference $reference --archive $archive --out target/terrain-app
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/terrain_check.py target/terrain-app target/terrain-check --backend sim --runtime target/release/cpu-v3-display-sim.exe
cargo test --release -p cpu-v3-tang-nano-20k --test host tankracer_terrain
```

The corpus includes real face centroids, both sides of the shared boundary,
the exact boundary and raw-coordinate neighbors, grid misses and fallback modes.
Each record checks 59 u16 outputs across full-hit, height, normal/height,
fallback and transformed auxiliary queries. Source records have 66 words;
the reader advances their sector and intra-sector offsets explicitly, so the
corpus can cross a 64-Kword boundary. The producer bounds the total checks to
the diagnostic u16 counter. The target checks page restoration before a successful halt.
Synthetic target-kernel tests separately cover overlapping candidate priority,
equal-distance fallback priority, conditional outputs, zero normals, quotient
overflow, invalid bins, nonresident cells and auxiliary classification boundaries.

For a bounded native sample, build with `--samples 16`, build the
`tankracer-terrain-native` binary, and run `terrain_check.py --backend native`
with that runtime. The initial records alternate real cells and exercise hits.
The runtime is frozen in the output before execution. The passive observer
reports phase2-to-phase3 CPU/cache/SDRAM cycles and traffic per record, including
cell selection/binding, output initialization, all four query variants,
transformed auxiliary sampling and result packing. Corpus reads and expected-word comparisons are outside those
intervals. Entry is preloaded-post-boot; SPI and concurrent GPU/Display traffic
are separate qualifications. This slice supplies queries for the subsequent
four-point contact integration.
