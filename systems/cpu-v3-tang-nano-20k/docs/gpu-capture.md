# Published GPU input capture bridge

## Fixed depth and historical inputs

Current producers declare native command ABI7 or framebuffer-view ABI8, range
version1 and range flag bits0..4 (mask31). Native command bytes use the original fixed D16
format: Global word0 is positive Q16.16 far below200; no k tag or depth precision
range bits exist. ABI3/4 use those same command bytes, so ABI4 captures can be
replayed after ordinary input validation. The version advance identifies removal
of selectable depth and its content-provenance rules without reusing ABI5/6.

The host snapshot reader imports sealed ABI6/range2 captures with their original
flags and exact ABI-source hash, including the original CRLF form. This is a
historical reading path only: current sim, RTL replay and scene oracle reject ABI6
before rendering or creating output. No historical snapshot/report is rewritten
or relabeled. Current live capture rejects retired tagged Global far values.

## Explicit-record integration entry

`gpu_api_capture.rs` is a separate private entry consuming the agreed source12
snapshot and A-owned facts4 allocation table. It copies the source, retains no
facts pointer, and neither publishes memory nor sends or retires GPU work.
The version1 bridge remains only in test-owned legacy regression fixtures.

The explicit entry emits **header version2**, with the existing64-word header,
8-word ranges, fixed-depth RTT command ABI8 and column layout ABI2. This header version
is separate from the existing manifest version. Addresses come from the actual
record, native descriptors and allocation facts, without a fixed slot map.
Header flags contain PUBLISHED and optional HAS_TARGET/HAS_FRONT/HAS_UI bits.
Absent fields are zero; physical address0 remains legal when its presence bit
is set. Frame0 means unspecified, and publication counters0 mean unavailable;
PUBLISHED relies on the prepared-source health boundary, not those counters.
Slot is only a diagnostic record index. The last SET_TARGET supplies the header
target; the range table preserves all parsed targets.

The native parser follows DMA_LOCAL provenance, installed Global/Render/Target,
uniform/page initialization, DRAW, UI_GRID, MEMCPY, maintenance and terminal END.
It checks valid/padded bounds, zero padding, allocation facts,64 ranges and a
16-MiB aggregate serialized-byte limit. Native opcodes13/14 remain invalid.
Each target plane spans1600
sectors; allocation tails have READ|PREIMAGE only. Clear marks the selected
physical plane FULL_CLEAR; Load preserves its input. Front is explicitly
optional and omitted. Display observations do not open or wait for Display;
an explicit Display error fails the lease. Each successful replay gets a new
token/submission, while failures preserve the first capture error and lease.

Mesh DMA captures the addressed record prefix and padded payload, not the whole
asset pool. UV draws include the selected POT base/full mip chain or1600-sector
fixed-view plane and its read-only allocation tail; RGB bypasses texture input.
Render's texture fields are copied when SET_RENDER executes in the dependency
scan, so later staging reuse cannot change the installed binding. Descriptor
DMA from an earlier declared writable region is rejected as data dependent;
ordinary uniform data and texture/payload contents may flow through GPU copies.
UI records the intersecting grid sectors and the readable kind3 allocation
beginning at the atlas base. It does not assume a64-KiB allocation: three glyphs
can occupy six sectors. If fewer than510 sectors are available, each selected
non-FF ID must have its complete256-byte glyph within that allocation. FF and
masked neighboring cells cause no glyph read. A short grid span declared writable
by earlier GPU work is rejected because its preimage cannot certify future IDs;
510 or more atlas sectors cover every non-FF ID. Failures retain the error and
lease before gate/submission. Optional manifest3 atlas catalogs retain64-KiB
base alignment and describe the actual nonempty sector allocation, at least
256 bytes; manifest2 retains its fixed64-KiB footprint. Header
UI fields summarize the last UI call's grid read span; all calls remain in the
range table, and UI_REFERENCE is zero. MEMCPY records both exact source and
destination spans, requires declared access, and rejects command self-modification.

The gate is a thin wrapper around the configured `profiler_gate`, followed by
a health recheck. Unknown actions fail. Only A publishes again after Resumed
and performs COUNT/START; `submitted` merely commits the prevalidated identity
and closes the lease after both sends. Disabled capture returns
true/CONTINUE/no-op. The shared UART runtime imports `gpu_capture_access`, whose
legacy adapter re-exports the three public read-only renderer bridges. An
explicit-root loader must select those same public bridges from `gpu_api`.

PC consumers dispatch version1 using the original fixed map, and version2 using
explicit recorded ranges. Build manifest3 declares `record-ranges-v1`, the same
column packing without a fixed memory map or allocation size; manifest2 retains
its original contract. Static catalogs may be empty for manifest3. Preflight
requires every recorded payload, all targets and exact physical plane spans.
Cold replay dumps every recorded writable range, including previous targets;
no-target submissions produce no fabricated plane preview. The initialized-read
and writable-byte guards still apply to actual GPU/MC transactions.

The bounded C contract fixtures execute the actual bridge through RCC/CPU V3
with small source/health/window peers. The actual shared UART runtime is tested
through pause/dump/resume and busy responses, then decoded on the PC. Opt-in
native GPU/stream-MC replays compare raw-copy bytes, Load preimages, dynamic UI
pixels and multiple target outputs. Contract/control peers establish neither A
ownership nor cache publication. These cold replays exclude CPU/Display
contention and do not provide board or PVT qualification.

For record-v3 builds the typed catalog binds complete source allocations while
captures contain the actual packet dependency ranges. Unreferenced catalog
allocations need not be captured. Referenced immutable fragments are compared
byte for byte with a complete asset file bound by the same length and SHA256;
without that file, the complete captured allocation must authenticate by hash.
A recorded WRITE aliasing an immutable catalog allocation rejects. All declared
capture ranges still require every byte, and native replay independently guards
every physical read and write. Legacy fixed-address captures retain their whole
catalog coverage rule.

The trusted MEMCPY diagnostic selects `native_sources.py --explicit-api` for
this root. Generated `gpu_packet` and `gpu_execute` sources append its three
copies and READ/WRITE allocation facts before seal and initial publication.
A sealed replay reuses that tail without appending again. Normal sources gain
no public copy operation or additional owner. Capture still checks descriptor
reserved bits and access extents before any COUNT/START; rejection retains the
prepared owner. Boot artifacts include the two actual specialized source files
and their generation identity rather than an unused legacy capture module.

`tools/qualify_api_callers.py` checks the frozen source manifest and records the
actual caller, driver and ordinary-module hashes. Its optional native callback
waits for production GPU/MC completion and imports only the reported writable
ranges before the CPU can retire. `--native-timeout` sets each replay's finite
wall budget (default300 seconds, allowed1..3600); `--wall-timeout` bounds the
whole command (default720 seconds with native replay,600 otherwise, allowed
1..7200). These budgets are recorded separately from the replay cycle limit and
do not change the captured inputs, completion condition or output comparisons.

## Resource-renderer boundary

The resource renderer can call one synchronous diagnostic gate after successful
cache publication, before Device4 COUNT/START. The preceding GPU is idle and
Display continues reading old Front. This is not a pipeline checkpoint, second
scheduler or recovery protocol. Normal renderer builds temporarily enable the
shared button/UART hook by default; `GPU_PROFILER_HOOK=0` disables it.

## Explicit consumer selection

`RendererCaptureConfiguration::from_manifest(gate_path, manifest_path)` binds
an explicit v2 manifest, capture ID, exact column layout/map and typed resource
catalog. Production uses the framebuffer-view command ABI named by
`GPU_CAPTURE_RTT400_COMMAND_ABI` (currently4), with layout ABI2. Missing/v1/row
layouts, old command ABI1 and mixed metadata are rejected. Wire/header/range
versions remain1. The low-level `CaptureConfiguration::from_identity` entry
only injects IDs for host tests; it is not production manifest validation.

`gpu_layout.rs` is the sole allocation source. The approved derived renderer
root copies these canonical sources and records their real origins; there is
no alternate layout-file substitution. POT and fixed framebuffer views are
available in every production build. Hardware metadata uses`gpu_profile=production`.

The selected gate is an ordinary module at its real canonical path, without
authority over private driver state. Normal builds pin the shared gate/runtime,
Device ABI, layout and capture ABI. Local replacements cannot override them or
the generated, unprivileged `gpu_capture_config`. A disabled profile selection
alone never enables capture. `with_default_hook` enables the ordinary shared
consumer without a build catalog; its manifest ID is zero. `without_capture`
and the explicit low-level `capture=None` entry retain the empty gate and omit
the UART implementation. The normal runtime and diagnostic automatic wrapper
use one `profiler_runtime.rs` implementation.

A v2 catalog supplies typed `mesh`, `texture` and `atlas` roots with physical
byte addresses/lengths inside the selected asset pool. Resources are nonempty,
128-byte aligned and nonoverlapping. Texture layout must match the profile;
column textures need 256-byte base alignment and at least 512 bytes. Atlas keeps
its aligned 64-KiB ABI. Optional `other` entries identify additional catalog
storage without authorizing a known root. Before the gate runs, actual exported
Mesh/Texture/Atlas ranges must exactly match their typed catalog roots; a
mismatch becomes sticky `GPU_CAPTURE_IDENTITY_ERROR` and prevents SEND. Catalog
hash validation and dependency closure remain the external consumer's job.

`CPU_V3_BOOT_EXAMPLE=gpu_profiler_board` registers the diagnostic integration
entry. Its separate profiler-owned example supplies `rcc/main.rs`,
`rcc/profiler_gate.rs` and the normal renderer asset-producer outputs. It uses the
existing OUT_DIR/boot-assets path and requires `GPU_PROFILER_MANIFEST`, a JSON
path with exactly 64 hex digits in `manifest_id`. Other normal renderer builds
may bind that manifest through the same environment variable, or use the
explicitly unverified zero-ID mode. Source filenames do not select a consumer.

Prepared capture scans actual command bytes and tracks DMA_LOCAL origins for
native MEMCPY descriptor slots66..79. Static descriptors add source READ/PREIMAGE
and destination WRITE/PREIMAGE BUFFER extents, with containing-range reuse.
These buffers obey the driver's existing owned GPU-domain/window restrictions.
Copy results used by later copies are read during replay, never replaced by the
initial preimage. Descriptors read from already declared mutable memory, missing
slot provenance, reserved descriptor bits, invalid/overlapping copies and writes
to command storage fail with sticky `GPU_CAPTURE_DEPENDENCY=9`. Capacity remains
64 ranges. This bounded closure adds no public graphics operation and does not
guess self-modifying descriptors; the backend still guards every physical read.

The consumer implements `profiler_gate(descriptor: Array<u16>) -> u16`:

| Result | Driver action |
| --- | --- |
| 0 Continue | No pause/transfer; immediately start, retaining one publication fence |
| 1 Resumed | Explicit resume; reset input/time baselines, republish and start the same input once |
| 2 Fatal, or unknown | Freeze Prepared CB, UI reference and target ownership; zero SEND |

The consumer owns its buttons/wait/transport loop. Pausing alone does not dump.
Successful local TX still leaves it paused: TX-only UART has no receiver ACK.
Recoverable transfer failure can wait for another explicit dump. Resume needs a
new edge after transfer, not a held button. GPU/memory/Display faults are fatal.
The consumer cannot submit, present, record, release or recursively call the
scheduler. Wire framing/checksums and PC validation are separate responsibilities.

## Descriptor and read bridges

The sole field/state/role constants are in
[`gpu_capture_abi.rs`](/systems/cpu-v3-tang-nano-20k/rcc/gpu_capture_abi.rs).
The header has 64 little-endian u16 words. Address/length pairs are physical
**bytes**, low16 first; COUNT is u32 instruction-count-minus-one. CB bytes are
nonzero and 4-byte aligned, and its allocation covers 128-byte fetch padding.

| Words | Contents |
| --- | --- |
| 0..3 | Version/size, published flag, self-contained-binding policy |
| 4..10 | u32 token/submission, u16 Frame, slot0/1, COUNT |
| 11,44 | Low/high initial publication epoch, identified by the lease token |
| 12..25 | CB base/valid/padded bytes, objects base/used bytes, actual UI base/bytes |
| 26..35 | Color/depth/Front bases, Front/target formats, confirmed Display/GPU status |
| 36..43 | Range count/stride/version, layout/command ABI, UI reference, plane bytes |
| 45..47 | Manifest words16, then two reserved zero words |
| 48..63 | Original 32 manifest-ID bytes, low byte first within each word |

The independent table has at most 64 entries, eight words each:
`flags, tag, base_lo, base_hi, length_lo, length_hi, generation_lo, generation_hi`.
Flags are READ/WRITE/IMMUTABLE/PREIMAGE/FULL_CLEAR. Tags identify commands,
objects, UI, mesh, texture, atlas, color, depth and Front. Generation 0 makes no
reusable content-version claim; input generations use the prepared token.

Private CPU storage is 64 header, 512 range, 64 read-cache and 13 control words,
plus one statistics word. The frontend reserves it even in no-op renderer builds;
it is ordinary CPU data, with no heap or new FPGA storage primitive.

The producer includes CB padding, used objects, actual UI page, complete known
Mesh/Texture/Atlas allocations, both target preimages and old Front. FULL_CLEAR
does not omit preimages. Column color, depth and old Front each cover all
204800 physical bytes, including 12800 bytes of macro padding; their header and
range length pair is 8192 / 3. Their 262144-byte allocations have a further
57344-byte tail, which is neither exported nor marked WRITE/FULL_CLEAR. The
current minimal hook rejects `options.capture_allocation_guards: true`; consumers
requiring that tail must not treat this producer as guard-capable. A replay
consumer must verify actual commands, bindings and dependency closure rather
than trusting declarations. Native CB bytes have no snapshot header. The table
supplies no external-buffer or RTT API.

| Public diagnostic bridge | Contract |
| --- | --- |
| `gpu_capture_range_word(range, field)` | Read one of eight range fields |
| `gpu_capture_read_word(range, offset_low, offset_high)` | Read u16 at a u32 **word** offset within the declared range |
| `gpu_capture_error()` | Separate lease/health result; zero payload is not an error sentinel |

Only Prepared leases permit reads. Invalid range/field/offset sets a sticky
fatal error. Outside a lease, Error returns NoLease without memory access.
Private 64-word caching amortizes reads; existing DSEG3 scopes restore the mapping.
Frozen sources and physical tags permit read-only scopes without per-fill full
maintenance; resume republishes before SEND. Architectural memory failure stays
fail-stop. Repeat dumps keep identity. Launch ends the lease and its read cache;
fatal paths retain input/UI and output ownership while HDMI continues.

## Mutable render-buffer dependencies

RTT uses the same Prepared lease and range table. A sampled RenderBuffer source
is a mutable READ/PREIMAGE range covering its complete registered capacity,
identified by the completed source epoch. It is not an IMMUTABLE catalog texture;
only immutable mesh, texture and atlas ranges are checked against catalog roots.
A released CPU TextureView leaves the submission read pin alive through the
actual successful GPU terminal, so capture cannot outlive its source contents.

Color/depth targets mark the 1600-sector physical span as FULL_CLEAR. The rest
of each registered allocation is a separate PREIMAGE tail, without FULL_CLEAR.
Thus a consumer snapshot includes its dynamic source, target preimages and
unchanged tails. The 64-word header and eight-word range format are unchanged;
command ABI 2 identifies the view extension independently of column layout ABI 2.
See [render targets](/systems/cpu-v3-tang-nano-20k/docs/gpu-render-targets.md) for
the ownership, generation and publication boundaries.

## Identity, time and validation

The driver copies a declared build/catalog ID, without measuring the FPGA or
computing SHA256. External audit/session metadata binds software/compiler/options,
ABI/layout/assets, GPU/MC/ROM and actual firmware/bitstream hashes. The final
firmware hash stays external to avoid self-reference. Declaration is not physical
image proof, and address reuse never identifies a snapshot.

Resume clears the microsecond epoch, consumes input as a baseline and resets
the next game tick's elapsed origin. Finalized preparation survives, but paused
Frames set `FrameStats.debug_paused=true`, observed_valid=false and zeroed
submit/queue/span intervals. Those zeros are invalid measurements. An uninterrupted
successor restores ordinary statistics, which still include publication and CPU
observation delay rather than pure GPU time. GPU and Display retire independently.

Bounded compiled-RCC tests cover both input/UI pages, A/B/A reuse, repeated reads,
offsets beyond65535 words, short objects/exact ends, DSEG restoration, zero SEND
or present during the consumer and once-only resume. Simulated pauses advance
40000ms including wrap. Fatal/bounds/cache/GPU/Display cases retain ownership;
origin/manifest tests separate explicit consumers from local overrides.
These peers do not qualify UART, rasterization, actual cache contention, scanout
or board capture. Consumer and matched hardware qualification are separate.
DRAM alone cannot reconstruct arbitrary warm-cache or internal pipeline state.

The `capture_layout` integration target compiles and executes the real RCC/CPU
producer for the sole column layout. Its portable `build.json`
binds copied source inputs, catalog and the selected effective RCC root. It emits
actual `header.bin`, `ranges.bin`, `producer.v3bin` and frozen range payloads under
`target/capture-layout`. First/last payload words pass through public read bridges;
complete payload files are host copies while the gate holds ownership. Controlled
GPU/Display peers establish identity, bounds and lease behavior, not UART delivery,
triangle rasterization or board capture. Firmware hashes remain external to the
manifest ID. No serial column demo capture is enabled by this test or selection.

The separate `capture_draw` target executes three submissions in each explicit
column layout and framebuffer-view command ABI. It uses the catalog's single
triangle and white texture, unlit RGB material, LESS and depth writes without
blending. Camera focal length 160 and world Z -4 produce positive native Z +4;
the three model X positions are 0, +0.5 and -0.5. Actual recorded matrices,
bindings and material words are checked before any independent GPU replay.

The natural double-slot command addresses follow A/B/A. Each gate has its own
token, publication epoch, header, ranges and complete immutable inputs and
color/depth/old-Front physical preimages. First and third referenced object
bytes differ even though their command address is reused. `GPU_CAPTURE_DRAW_DIR`
selects the output root (otherwise a process-specific temporary directory).
Each profile contains `build.json`, actual compiled program/listing, source
origins and `sequence.json`; `frame-0` through `frame-2` contain frozen payloads
and `producer.json`. The identity manifest also binds all capture fixture test
sources. Column padding is captured, while its allocation tail remains outside
the declared writable span.

GPU/Display control peers only retire protocol work; they never render or scan
out. Thus the three preimages are independent replay inputs, not asserted
consecutive GPU pixel outputs. Public bridges sample each range's first/last
words and the host copies the full frozen range. Positive-Z/interior checks and
ideal projected coordinates are producer checks, not a raster/depth oracle.
The GPU consumer must independently replay each actual input, including the
reused address, and verify coverage, color, depth and allowed writes.

The same `capture_draw` target runs the current production
`examples/gpu_profiler_board/rcc/full_scene.rs` through its bounded
`profiler_full_three_frames` entry and the checked explicit GPU loader. Every
frame records six box meshlets, one RGB meshlet, seventeen rabbit meshlets
(948 faces) and OBS UI: 24 DRAWs and 1141 triangles. It uses the production
camera/model/material preparation and an application-owned input snapshot.

`tools/capture_full_assets.py` composes the canonical scene and font packers.
It binds the current application source separately from the frozen geometry
origin; the old source snapshot remains a geometry reference and is not compiled
as the current caller. The packer preserves native asset bytes and supplies a
typed mesh/texture/atlas catalog.

`GPU_CAPTURE_FULL_SCENE_DIR` selects a fresh output root. Each gate preserves
the whole recorded command/push/UI and physical target input. Each reserved
262144-byte color/depth allocation is checked as an adjacent 204800-byte physical
WRITE/PREIMAGE/CLEAR range and 57344-byte READ/PREIMAGE tail, including both
endpoints and every payload byte. The tail is captured but is never writable.

The fixture returns Resumed at its second gate and advances modeled wall time by
40000 ms. The application-owned profiler clock excludes this interval and
changes epoch once. UI observations are unavailable at startup and after resume;
the intervening completed observation is valid. Publication is checked at the
actual Device0 writes and GPU STARTs, including exactly one extra publication
after resume. Unavailable header counters remain zero. These controlled peers
neither rasterize nor scan out; frozen frames are independent GPU replay inputs
and do not claim consecutive rendered outputs or hardware performance.
