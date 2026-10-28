# Macro-column16 texture/framebuffer experiment

The active qualification target is the **pure layout** profile, with no RTT
view descriptor, clamp/product coordinate path, or NPOT extension. Export with
`gpu-v2-vliw-export OUTPUT command-column16 [portable]`; the matched row export
is `command16x8`. Complete board profiles are `texture16x8-column16` and
`texture16x8-row`. Production remains unchanged.

Both use16x16 RGB565/D16 macrotiles with row-major pixels inside a tile. The
texture cache remains8KiB:32 resident16x8 lines,8 sets,4 ways, four1024x16 banks
and two serial128B refills per256B line. The raster grid, local bank permutation,
FIFO/lookahead and sampling schedule are unchanged.

Earlier `rtt400x240-column16` / `rtt400_column16.rs` WIP and its board reports are
preserved, but RTT development and device integration are paused. In particular,
its53.632MHz failed fit contains RTT coordinate logic and is not the pure layout
result. Git integration and eventual RTT rebase belong to the designated single
integrator; this experiment does not merge, cherry-pick or rewrite history.

## Physical bytes and allocation

For the fixed400x240 framebuffer,25 columns each reserve16 macrotiles vertically:

```text
macro_tile = ((x >> 4) << 4) | (y >> 4)
byte_address = base + 512*macro_tile + 32*(y&15) + 2*(x&15)
half_line = base + 512*macro_tile + 256*((y >> 3)&1)
refill = half_line + 128*sector, sector=0,1
```

There are375 live and25 padded macrotiles. Active logical pixels still contain
192000 bytes, spread across a204800-byte physical plane. Its last live word
ends at offset204288; its final512-byte tile is padding. The former1536-sector
allocation is too small: the last live macro column starts at byte196608.
The CPU allocation proposal reserves2048 sectors /262144 bytes per64KiB-aligned
allocation; the highest such allocation base is `0x7c0000`. CP target encoding
already requires64KiB alignment. Its physical extent threshold is `0x7ce000`,
which rounds down to the actual legal CP base `0x7c0000`. Display independently
accepts32-byte alignment, up to `0x7ce000`. These hardware bounds are not an
allocation grant. Physical bytes grow6.67%; reserved
allocation grows33.33% compared with the old196608-byte reservation.

The required CPU domain proposal is:

| Object | Byte base | Reserved bytes |
| --- | ---: | ---: |
| Color A | 0x400000 | 0x40000 |
| Color B | 0x440000 | 0x40000 |
| Depth | 0x480000 | 0x40000 |
| Commands | 0x4c0000 | 0x10000 |
| Objects | 0x4d0000 | 0x10000 |
| Assets begin | 0x4e0000 | As declared by asset manifest |

The complete input/asset ranges are:

| Object | Half-open byte range |
| --- | --- |
| Slot0 commands | [0x4c0000,0x4d0000) |
| Slot0 objects | [0x4d0000,0x4e0000) |
| Asset pool | [0x4e0000,0x7df800) |
| UI page0 | [0x7df800,0x7dfc00) |
| UI page1 | [0x7dfc00,0x7e0000) |
| Slot1 commands | [0x7e0000,0x7f0000) |
| Slot1 objects | [0x7f0000,0x800000) |

The UI/slot1 addresses retain the current CPU input-pipeline reservation.
Do not derive a slot1 by splitting the low command/object windows. Compute
exclusive ends in a type wide enough to represent0x800000 bytes /65536 sectors.
The hardware bound alone is not an allocation permission: a view near the top
of physical memory must not alias those input/UI regions.

The minimal renderer's current asset roles can move together by0x30000:
mesh `0x4e0000`, texture `0x4e1000`, atlas `0x4f0000`. The classic demo may instead
use separate64KiB atlas/mesh/texture slots. Role addresses are not hardware ABI;
the manifest and every embedded mesh-directory sector pointer must agree.
Mesh records/CBs need128B alignment, ordinary POT textures256B alignment,
UI atlases64KiB alignment and SET_RENDER/target source objects their32B DMA
alignment. Every resource's full padded extent must fit the asset pool and
exclude all borrowed render-buffer allocations.

For the paused, future two-stage RTT caller, a non-overlapping column allocation is
color `[0x600000,0x640000)` and depth `[0x640000,0x680000)`, each262144 bytes.
The old depth base0x630000 overlaps the enlarged color. Its asset packer must
exclude the entire `[0x600000,0x680000)` interval, even when padding is not drawn.
CPU owns these constants, build-time packing, validation, lifetime and firmware;
this hardware experiment does not edit CPU RCC layout/packer WIP. A matching
CPU branch is required before executing this profile on a device.

Pure layout retains the ordinary POT descriptor ABI and adds no bit23 meaning.
CP enforces400-macrotile plane separation and the tighter physical target limit.
The paused RTT-specific profile separately retains its previous bit23/n9/clamp
contract and tighter64KiB-aligned view base limit; it is not exercised here.
Each CLEAR writes1600 consecutive128B sectors, including all padded tiles,
and leaves the remainder of the2048-sector allocation untouched.

Ordinary square POT textures use column-major macrotiles with their actual
POT macro-height. Mip prefixes and minimum16x16 periodic padding are unchanged.
A64-square full chain remains100 sectors, but its macrotiles are transposed
relative to the row-major experiment. Repack assets offline/build-time; do not
convert live framebuffer pixels on CPU.

## FB, UI and Display

FB resident tags remain raw `{Yhalf,Xtile}`. Only maintenance converts the tag
to external macro order. The current focused repair keeps the original UI tile
ownership/lifetime and applies logical tile/25 conversion combinationally.
Earlier v1 reused `mt_target` for that conversion; its source, full-scene result
and pathological routing attempts remain archived. No cause is inferred merely
from build duration. The half bit and128B request boundaries are unchanged.
UI keeps its logical50x15 character-cell indexing and partial-range ABI. Its
logical macro index is converted with a bounded9-bit divide/remainder by25
before the physical address is formed; this compatibility logic has a real
cost and prevents assuming the new address layout saves total area.

Display keeps32-byte horizontal row requests, two-row groups and the existing
line buffers. Horizontal neighboring macrotiles now differ by8192 bytes;
vertical macro neighbors differ by512 bytes. The memory request formula,
tile-row base increment and maximum legal Display base change together.
SDRAM sector/bank/controller mapping is unchanged, so locality effects belong
in the measured full-scene result rather than an arithmetic-only prediction.

## Reproduction and acceptance boundary

```powershell
python -B ip/gpu-v2/tests/test_texture16x8.py --export COLUMN.v --output FRESH --column-major
python -B ip/gpu-v2/tests/test_column_clear.py --export COLUMN.v --output FRESH
python -B ip/gpu-v2/tests/ui_grid.py --export COLUMN.v --peer MC_DIRECTORY --output FRESH --case sparse --modes 0 1 2 3 --column-major
```

Earlier six-pass RTT evidence is historical and remains paused. The pure CLEAR test checks all3200 requests /409600 bytes
across both planes, exact addresses/data, delayed/same-edge terminals and final
retirement under CE0. UI adds all375 logical-to-physical mapping checks and
sparse glyphs including the last column/row. The native Display test checks
every request of complete source frames, pixel colors, range/alignment rejection
and pending swaps under the selected profile.

`macro_column_replay.py` runs the identical full24-meshlet,1141-triangle scene
for either profile with `--layout 16x8`; add `--column-major` for the candidate.
Both runs use the same frozen numerical executable and physical MC/pin peer.
Initialization packs static assets and seeds the physical target; it performs
no runtime CPU pixel copy. Readback normalizes only the final framebuffer dump
to canonical row-order logical bytes for the unchanged independent oracle.
Padded tiles must contain their intentional CLEAR values; untouched allocation
slack must retain its guards. Readback is written before guard assertions so
a diagnostic cannot discard an otherwise complete observation. The normalized
TB and counter definitions must match in `compare_macro_layout.py`; normalization
removes only inspected pure post-frame padding reads and the layout localparam.
The legacy peer currently runs about49.5/99MHz, CAS2, identically on both sides;
complete board54/108MHz CAS3 qualification is a separate boundary.

Complete offline board builds use
`systems/cpu-v3-tang-nano-20k/examples/texture_layout_board/build.ps1` with fresh
target directories. Select `-Profile texture16x8-row|texture16x8-column16`.
The bounded robustness comparison repeats production place1/route2 in fresh
directories for each layout. An attempted library timing-driven selector failed
before export/PNR on duplicate existing process options; logs remain as tooling
failures, not STA outcomes. No shared tool library is modified. Clock/SDRAM
constraints and actual generated source must match across each repeat; retain
all failures/passes and do not treat a single marginal pass as qualification. Generated bitstreams/reports remain experimental until the
normal timing/resource audit passes. A routed bitstream with timing violations
must not be treated as deployable or silently substituted for the verified
row-major board. Matching firmware and device handoff are separate requirements.


## Current pure-layout evidence

`target/texture-column16-pure/source-snapshot-v1/identity.json` binds the restored
`145f5c8e` worktree plus its full preserved WIP snapshot. Pure column GPU SHA256
is `773be528bd786611dd0c8d121ca0c6a1eb05943410b933c0de1f926175cd5702`.
The row GPU is byte-identical to the previously audited `6aa8deb5...` full-scene
DUT. `full-row-identity-audit-v1` rechecked the complete DUT, timed observer,
fixtures, command bytes and MC source identity before reusing its completed
trace, and reran the independent frozen numerical oracle. It is not a new RTL
run. Its legacy boundary string incorrectly says54/108MHz; the actual frozen
TB is approximately49.5/99MHz CL2, as documented above. Whole-board timing is
separate and uses54/108MHz CL3.

`protocol-v1`, `clear-v1`, and `ui-sparse-v1` contain the pure profile's address,
cache, protocol and physical-memory regression receipts. Full column scene
and matched board fits are separate artifacts and must be complete/pass before
claiming qualification. No physical device has been programmed by these scripts.


## Complete-scene result for the pure profile

The native full replay completed in `full-column-v1`; all independent numerical
and whole-image checks passed, with96000 texture outputs and105953 final pixels.
Physical padding/slack checks passed35072 words. Canonical color SHA256 is
`163a8981d85b280f561ef86fbac9c9be8b5db6cfd230257f5ec25fd448a44542`;
depth is `4c65ad234962919d80a4a8d6615e25a05838be49bff9144f68bc14707198e4bc`,
identical to the row reference. Both retain8KiB/32 lines/8 sets/4 ways.

| Metric | Row | Column |
| --- | ---: | ---: |
| Groups/access denominator |118447|118447|
| Logical parent misses |60|61|
|128B texture sectors |120|122|
| Actual texture read bytes |15360|15616|
| All RO+FB read bytes |860832|861088|
| All FB write bytes |1203712|1229312|
| Complete cycles |664766|667920|
| Preload including both CLEARs |60358|63642|
| Rendering interval |602189|602061|
| Final tail |2219|2217|

Column total cycles rise0.474%; the rendering interval falls only128 cycles.
Both padded-plane CLEARs add200 sectors/25600 write bytes; texture misses and
read traffic also increase by1 parent/2 sectors/256 bytes. There is no full-scene
performance win. Wait counters overlap and are not additive. These measurements
use the matched frozen approximately49.5/99MHz CL2 peer, not production board
frequency or concurrent CPU/Display traffic. `full-comparison-v1` binds all
fixtures, frozen oracle, timed observer and MC source identity.

Production-timing UI regression `ui-production-v1` separately passed normal,
backpressure, abort and CE-paused modes using54/108MHz CL3/RCD3/RP3/CAP3/DQ32
and registered return. Its clock phases match `board_application_tb.v`; MC
logical-body equivalence is in `production-ui-source-equivalence-v2.json`.
This is functional simulation, not device or final timing qualification.


## Current pure layout profile

The emitted GPU SHA256 is
`b235b488b839d3fcfe94656ccd7da522bd9d3ec862c58fa7b5967e3bb18b7e77`.
The profile retains the ordinary UI tile ownership and target lifetime; it
changes external addressing without adding UI writes to the shared target.
It exposes no RTT view descriptor or framebuffer-texture coordinate path.

Activate `texture16x8-column16` only with matching column POT assets and the
explicit serial scene firmware described in
[GPU column scene](/systems/cpu-v3-tang-nano-20k/docs/gpu-column-scene.md).
The default production export remains separate. UI grids and glyph-atlas bytes
retain their existing ABI; ordinary texture transposition does not apply to UI
atlas assets.

Whole-system qualification binds the actual generated RTL, boot stage, clock
constraints, firmware and bitstream identities. Re-export the intended firmware
configuration and audit the matching implementation before programming.
Standalone cache or full-GPU replay results do not certify CPU/Display
concurrency, physical scanout, cold power-on or PVT behavior. Prior experiment
resource and timing observations do not transfer to a changed system build.
