# Published GPU input capture bridge

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

The same `capture_draw` test target has a separate complete-scene case,
`real_full_scene_three_gates_resume_and_restore_stats`. It executes the exact
formal profiler `full_scene.rs` snapshot pinned by
`tests/fixtures/capture_full_scene/origin.json`; only the external entry wrapper
selects three frames and a final drain. Each actual frame records all six box
meshlets, one RGB meshlet, all 17 rabbit meshlets (948 faces), and OBS UI: 24 DRAWs
and 1141 triangles. The fixed camera, model and light come from that example;
camera preparation, frame input snapshots and observed-time UI use production
calls. This fixture does not model camera motion from controller input.

`tools/capture_full_assets.py` composes the existing classic scene packer and
renderer font packer for either profile. It preserves native mesh and texture
bytes, adds the OBS atlas glyphs, and supplies typed catalog roots and renderer
metadata. Repository-relative source hashes bind the complete scene, texture,
font and fixture origins into the portable identity manifest. No parallel native
asset encoder, command construction or capture ABI is introduced.

Set `GPU_CAPTURE_FULL_SCENE_DIR` for the full-scene output root. Each profile
retains the same three gate directories and payload contract, with an additional
`ui.bin` convenience copy and actual renderer input/timing/statistics words in
`producer.json`. `sequence.json` includes the final drained statistics. The
second gate advances modeled wall time by 40000 ms and returns Resumed; the
others Continue. Its paused submission has invalid observed intervals, the next
tick excludes the pause, and the final submission restores valid statistics.
Neutral analog packets travel through the real input path. The synthetic timer
advances one modeled 54-MHz cycle per CPU instruction; its numeric intervals
are control-test stimuli and supply no hardware performance measurement.

The recorder can seal a successor before observing the previous completion.
Consequently, the third frame's OBS bytes show the first frame's valid duration,
while its gate already observes the second frame's paused, invalid statistics.
The first and third UI payloads actually differ at the reused A slot. Neither
the test nor its metadata rewrites sealed UI to match later statistics. Full
color/depth/old-Front preimages remain independent controlled-peer inputs;
only independent GPU/MC replay can establish their rendered results.
