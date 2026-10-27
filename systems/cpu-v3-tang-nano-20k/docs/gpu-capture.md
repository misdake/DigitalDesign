# Published GPU input capture bridge

The resource renderer can call one synchronous diagnostic gate after successful
cache publication, before Device4 COUNT/START. The preceding GPU is idle and
Display continues reading old Front. This is not a pipeline checkpoint, second
scheduler or recovery protocol. Normal applications use the shared no-op gate.

## Explicit consumer selection

`compile_renderer_program_with_capture` takes
`CaptureConfiguration { gate_path, manifest_id }`. Root/private helpers keep their
actual shared canonical origins. The selected gate is an ordinary module at its
real canonical path, without authority over private driver state. Normal builds
pin the shared no-op gate, Device ABI and capture ABI. Local replacements cannot
override them or the generated, unprivileged `gpu_capture_config`.

`CPU_V3_BOOT_EXAMPLE=gpu_profiler_board` registers the diagnostic integration
entry. Its separate profiler-owned example supplies `rcc/main.rs`,
`rcc/profiler_gate.rs` and the normal renderer asset-producer outputs. It uses the
existing OUT_DIR/boot-assets path and requires `GPU_PROFILER_MANIFEST`, a JSON
path with exactly 64 hex digits in `manifest_id`. Other applications cannot
enable capture merely through a source filename or manifest environment variable.

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
does not omit preimages. A replay consumer must verify actual commands, bindings
and dependency closure rather than trusting declarations. Native CB bytes have
no snapshot header. The table supplies no external-buffer or RTT API.

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
