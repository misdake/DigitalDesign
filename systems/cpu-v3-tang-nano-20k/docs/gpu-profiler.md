---
id: system-gpu-profiler
status: active
last-verified: 2026-11-05
---

# GPU profiling sessions

## Common collector protocol

New RTL and cycle-emu collectors persist `capture.json` with schema
`digitaldesign.profiling-capture`, version2, and one `events.jsonl` stream.
The owning point catalog is
`ip/gpu-v2/src/system/vliw_cycle/profiling_contract.json`; both collectors read it.
The common parser, CP/slot-generation correlation, validation, memory formulas,
native history builder and `write_report`/`write_viewer`/`viewer.html` path are
shared. Backend transport signals are converted in the collectors, not the viewer.

Each line is `[time,kind,value]`. Logic time is an integer in a quantum clock
`148 * clock_hz`; a logic period is148. `native` and `physical_write` use the exact
pair `[numerator,denominator]` in that same quantum. The full profile retains
these source pairs alongside their numeric plotting projection. RTL uses the
first actual logic posedge as origin and its declared physical period in fs;
no measured event is moved to another edge. Its related-clock replay retains
the157.5-degree pin phase and accelerated MC initialization. The emu retains
its actual native half-edge calendar. Different startup/CE/pin configurations
do not establish timing equivalence merely because their write bytes agree.

| Point | Committed fact and common payload |
| --- | --- |
| `cb_submit` | Accepted Device4 SEND, including CE0; submission/base bytes/valid bytes |
| `cb_latch` | Enabled CP LATCH consumes actual RAM Q; submission/PC bytes/original word |
| `cb_execute` | First enabled EXEC, independently of retries; same original PC/word |
| `cb_issue` | EXEC actually leaves/advances, including its ready condition |
| `cb_release` / `cb_drain` | Actual PC advance or END-to-ENDING; separate END busy-to-idle drain |
| `gpu_control` / `gpu_load` | Old-state predicates and39 defined inventories/rates, including CE/reset |
| `dma_job/request/return/done` | Accepted job/return slots, page/source/count and real LAST; not busy estimates |
| `vt_step` / `vt_rf_write` | Shared V/T PC/mode/72-bit ROM split32/32/8; RF address/value/known mask |
| `t_publish/prepare/t_word/t_release` | Physical16-slot T FIFO and accepted indexed54-bit output with known masks |
| `r_start/r_end/i_context` | Actual raster and interpolation context handshakes |
| `q_alloc/q_publish/q_retire` | Physical32-slot pool, generation, coverage and empty retirement |
| `light_accept/light_result` | Accepted slot/lane/mask; returned key/last/G/H and known masks |
| `sample_publish/sample_accept/sample_result/sample_last` | Actual public-slot ownership, row acceptance and returned RGB/known mask |
| `final/rop_begin/cache_write/q_commit` | Final slot/lane, independent ROP context, bank/address/data/known bits and last write |
| `master_request/master_payload/master_terminal` | Three GPU64 interfaces; address/count, low/high32/mask/error and real LAST/error |
| `target/target_unbind/gpu_cache` | Committed target binding and old valid/tag/base state |
| `physical_write` | Actual backing mutation branch; physical byte address, raw32 data and4-bit unmasked-byte mask |

RTL records old state at posedge and observes the actual NBA commit for CP
issue/release/target changes using an observation-only delay. Emu publishes
`CommandEdge` only after a successful clock. Raw event ticks identify the
acceptance/commit edge; the common model alone makes exclusive endpoints at
`tick+148`. An unchanged cache slot in a full state snapshot preserves its dirty
word versions. Unknown enabled write bits, unexplained writeback, ownership
overwrite and excess payload are rejected.
Invalid cache tags are encoded as zero together with `valid=false`; they do not
assert a tile binding. The four owner columns retained in the raw load array
are reserved zero: occurrence IDs exist only in the common model and are
assigned from accepted DRAW/T/pool/ROP handovers, never from a busy estimate.

`domains` and `supported_points` explicitly declare availability. CPU, Display,
shared-service64, native-model32 and physical writes are separate optional
domains. GPU-only RTL captures do not acquire CPU/Display rows or zero-valued
measurements. New GPU-master payload accounting uses its own observed strobes;
the board's descriptor-slot service remains separately bound by FIFO fields.
Historical `FORMAT 2`/`load.csv`, `system-events.jsonl` and common version1
captures retain compatibility import and their original capabilities. Missing
points are not synthesized or relabelled as newly collected.

Canonical JSON/CSV retain the source facts and partial/open owners. HTML omits
validation-only duplicate journals; frame HTML projects its real submission
owners and time window, preserving true endpoints and carry baselines. Identical
PNG payloads are deduplicated by hash without merging color/depth/raw identities.
The canonical session remains a separate linked report.

The optional ordinary-runner sidecar binds the exact plan and actual CPU device,
committed source-PC, input, phase and raster events into report input hashes.
Ch11 intervals pair the actual clear and RECV sampling edges, including the
read-before-update prescaler rule. Cold, saturated and changed/unknown resume
epochs are invalid. Source-mapped vblank-return/final-wait-return sites must
match the same next presentation and epoch before a900000-cycle verdict is
reported. `runner-frame-timings.json` keeps this interval separate from the
applied-to-applied frame window, and exports only the selected presentation's
rows on a scoped page. Concurrent CPU-runnable/GPU-busy and three-client pending
cycles, Client64 payload and native32 bytes come from the saved interval; CPU
runnable does not assert the absence of pipeline/memory stalls.
Plans declaring `site_observation=first-unheld-execute` bind logical return
arrival independently of retirement, retaining the true prefix/LOAD return PC
and the later ch11 clear PC separately. Intermediate waits do not enable the
final site: saved finish r10 must be nonzero. At read, saved started r8 must be
nonzero and saved startEpoch r9 must match the current epoch. Source-mapped ch11
PCs are checked. Reports include both return-to-return cycles and actual ch11
clear-to-read cycles, plus return-to-clear/read-after-wait offsets and separate
900000-cycle verdicts. No return is relabelled as a neighboring MMIO instruction.

Explicit runner native selection names up to four actual presentation ordinals
(F1/F2/A/B), independently of the twelve selected registered-raster slots.
Historical native400x240 color/D16 reconstruction retains the entire original
preimage and every preceding physical/cache write, including unselected groups.
Only proven completed command owners in the selected drained groups get images;
unavailable/open owners remain explicit. Legacy captures retain the first-two
native-group policy. A later group never gets a fabricated late preimage.
The runner also declares a maximum2048 command-image attempts, bounding distinct
color/D16 raw planes to800MiB before PNG/HTML. If that declared count is exceeded,
export fails explicitly and retains the capture; it does not omit command images
to make a page appear complete.

## Full-system cycle source

`cpu-v3-system-emu --profile-out` supplies a third profiling origin, alongside
the existing RTL observer and result-only functional simulator. It runs the
complete CPU/GPU/Display/shared-memory composition; it does not use the
`system_dump.py` freeze-and-isolated-replay execution path. The source is always
`system-cycle-emu`, with synthetic preload and fixed input/limited device scope
declared by the [system cycle contract](/systems/cpu-v3-tang-nano-20k/docs/system-cycle-emu.md#passive-full-system-profiling).

Legacy `capture.json` schema `digitaldesign.system-emu-capture` version1 describes a
bounded `system-events.jsonl`. New capture uses the common protocol above.
Each ordered legacy line is `[absolute_tick,kind,data]`;
equal-time lines retain file order. The integer timebase is 7,992,000,000Hz,
with logic period148, native half-period37 and pixel period240. Sparse state
changes describe held values until the next change or coverage boundary.
Events include CPU counters, CP phase, actual descriptor acceptance, payload,
terminal, submit/drain and present request/application. A descriptor's optional
requested-beat count is distinct from measured payload; absent counts remain
unavailable. Same-port write payload is assigned to the earliest accepted write
that still needs data, independently of the oldest pending terminal response.

Build and render share transaction validation against the saved payload events.
Successful closed transactions with known requested sizes must transfer exactly
that size in the declared direction. Error terminals may retain a short payload;
open transactions require partial coverage and no terminal error. Neither case
permits excess payload, conflicting ownership or a fabricated `last_data_tick`.
That timestamp must equal the last payload event assigned to the transaction.
`end_tick` is exclusive: a terminal sampled at tick `t` stores `t+148`.
Payload and terminal on the same logic edge are legal (`last_data_tick+148 ==
end_tick`); closing at the payload timestamp itself is not. This validation
replays metadata ownership only and never executes the simulated application.

The existing `digitaldesign.gpu-profile` v2 format retains its original GPU-only
semantics. Full-system reports add a strictly source-dispatched `system`
version1 object containing exact-tick state/retirement/phase tracks, separate
client64 and native-model32 data events, accepted transactions, submissions,
present events, scanouts, coverage and capabilities. The integer logic-cycle
axis encloses the trace; pixel/native source timestamps are never rounded to
logic cycles. Zero-submission intervals are legal only for this source.

New captures declare `command_control_lineage`, `gpu_signal_journal` and
`instruction_owner_lineage`. `cb_submit` records the accepted byte base/count;
`cb_latch` records the actual RAM word and relative byte PC. `cb_execute` is the
first enabled Exec observation, `cb_issue` is actual acceptance, and `cb_release`
is CP residency release. These boundaries can differ under CE, DMA or renderer
stalls. END completes at actual drain; abort/reset retain open owners instead of
inventing a successful drain. The original instruction is immutable: CLEAR and
MEMCPY progress may change the live control word, recorded separately in
`control_word_updates`. Neither a CP phase nor the startup CB06 image supplies
missing command facts. Old captures without these capabilities retain explicit
unavailable command, owner and native history fields.

Sparse `gpu_event` entries correlate DMA jobs/requests/returns, DRAW/VT,
generation-tagged T and quad slots, preparation, R/I stages, sample publication,
Final and ROP retirement. DMA attribution follows actual jobs and returns;
asynchronous work never inherits whichever PC happens to be current. CP release
can precede VT, T or quad completion. Open owner ends remain null, including
after abort/reset. `gpu_load`, `gpu_control` and `gpu_cache` preserve actual
connected old-state values; load impulses and ownership transition edges survive
run-length encoding. The common command, meshlet, stage, wait, load and three-port
GPU memory views consume this graph, with exact shared-memory transaction
accounting. CPU/Display/client64 and native-model32 remain distinct boundaries.
`gpu_master_requests` records acceptance at each of the three GPU interfaces,
before the board descriptor slots. The seven-client service records acceptance
by the shared arbiter later. Per-port FIFO order, address and beat count bind
these two events in `system.gpu.service_bindings`; equal timestamps are not
required. GPU outstanding/wait statistics start at master acceptance, including
time held in the slot. Payload/terminal bypass those slots and keep their actual
times. Physical framebuffer writes follow the matched master path owner. The
system overview continues to measure the separate arbiter-service boundary.

```mermaid
flowchart LR
    CP[Committed CB latch and issue] --> DMA[Accepted DMA job and page]
    CP --> VT[DRAW and VT owner]
    DMA --> VT
    VT --> T[Physical T slot and generation]
    T --> RI[Prepare and R/I context]
    RI --> Q[Quad slot and generation]
    Q --> SF[Sampling and Final]
    SF --> ROP[ROP context and retirement]
```

Arrows represent recorded ownership transfers, not extra execution or clocks.
Completion requires the associated asynchronous owners to close.

The common viewer uses shared time bins, zoom and hover for CPU/GPU activity,
three-way pending requests, per-client bytes/request backpressure, accepted
counts/requested bytes, mean outstanding and write-terminal waiting. Data events
belong wholly to half-open bins; state durations integrate exact overlap.
Client64 and native-model32 are separate rows and CSV boundaries and must not
be summed as independent traffic. Both explicitly disclaim physical board pin
bandwidth. Masked native write slots count4B; unmasked bytes are shown separately.
Missing tail coverage is shaded and cannot become zero traffic.

`system-memory.csv`, `system-state.csv` and `system-transactions.csv` retain these
boundaries and raw ticks. The existing report generator binds capture/program/
executable/trace/raster SHA-256 identities in `report-inputs.json`; `--binding`
checks those exact dependencies on rebuild. The same `render` entry validates
the optional extension, copies bound PNG bytes and embeds the same viewer.
Registered images are complete live scanouts; their RGB receives no second gamma
conversion. New native history uses one read-only 8MiB backing preimage plus
actual physical write address/data/DQM masks and valid cache/tag/bank writes.
It reuses the historical image engine without extra MC reads, cache cleaning,
per-DRAW backing copies or application replay. Native color RGB565 and D16 use
the 400x240 tiled layout, with all 204800 bytes per plane including padding;
registered scanout is separately 800x480. Hash-bound `plane-<sha256>.bin` assets
provide raw downloads; PNGs use the declared preview policy.

History is bounded to the first two presentation target groups (or two GPU
submission scopes when no present is observed). A bounded open group may expose
only proven completed command prefixes. An open command or a cut final event
has no fabricated image. Each snapshot identifies its actual target epoch and
binding command. Before a target is bound, a later actually observed SET_TARGET
in the same submission may select an explicitly labelled output preimage;
`target_epoch` is null and this does not claim an active binding. Submit/reset
target invalidation prevents stale bindings from leaking into another CB.

`build --frames 1,2 --source-identity identity.json` writes a canonical session
and `frames/frame-N` reports. Identity requires source/tree Git SHA plus program
SHA-256; executable SHA-256 is checked when supplied. Application frame N means
presentation request/application N, with all completed GPU submissions between
the preceding and selected requests. It requires a full effective matching
registered raster, excluding boot black frames. Its observed CPU/memory window
ends just after that applied-present edge and begins just after the previous
one (capture origin for F1). This is not an unrecorded semantic CPU frame or HUD
wait-return boundary. GPU-owned lifetimes, concurrent window bytes and registered
scanout intervals are labelled separately. Carry-in/out transactions retain
their original endpoints; `frame-summary.json` gives exact window totals.

All profiling captures open the same full GPU viewer as RTL/native replay,
including legacy and zero-command captures. `?view=system` explicitly selects
CPU/shared-memory/registered-scanout context in the same HTML; `?view=gpu` returns.
There is no automatic fallback to a reduced GPU timeline. A legacy source maps
actual submission lifetimes and saved complete registered scanouts into the
common layout. Its missing instruction/meshlet counts and VT/backend occupancy
are labelled unavailable, never measured zero. Native command history remains
preferred when present; registered scanout has a separate caption and PNG link,
without a fabricated command completion or D16 plane.

Legacy GPU arbiter-service descriptors with recorded sizes, payload handshakes,
terminal events and valid/not-ready states reuse the common memory correlator,
histograms, wait/outstanding statistics and CSV exports. Their boundary stays
`arbiter-service64` after the board descriptor slots. Master acceptance and
DMA/texture attribution remain unavailable; sources stay `unknown`. Missing or
unsupported descriptor sizes disable request accounting without inventing counts.
Build/render derive this optional extension from intact saved facts, and validation
recomputes it before accepting its totals. CPU and Display traffic remain in the
explicit system view and are excluded from these GPU service rows.
Frame list, Fit, zoom, statistics, CSVs and native image lists use the selected
scope. Canonical raw context is retained for validation, with a separate link to
the full session. Build and render rederive owner, frame, target and image
bindings from the saved journals; they do not execute commands.

## Identity and time

`profile.json` has `schema: "digitaldesign.gpu-profile"` and `version: 2`.
All timestamps are nonnegative integer cycles relative to `origin`; `clock_hz`
converts them to display units. Integers must fit JavaScript's exact integer
range. Intervals are half-open `[start,end)`. Session `start/end` enclose all
recorded work, including CPU recording for later submissions.

| Collection | Identity and binding |
| --- | --- |
| `submissions` | Increasing `id`, accepted `start`, drained `end`, byte `base/bytes`, optional logical `frame_id/name` |
| `commands` | Unique occurrence `id`, owning `submission`, display-only byte `pc`, `word/opcode`, fetch `start`, `execute`, release `end` |
| `jobs` | Session-wide DMA `id`, owning `command/submission`, source/slot and true completion `end` |
| `dma_transactions` | Session-wide `id`, `submission`, logical `job` or zero for CB prefetch, request-to-real-LAST interval |
| `meshlets` | Session-wide `id`, owning `command/submission`, associated DMA job, VT and R/I/P intervals |
| `cpu` | Unique span `id`, logical `frame_id`, label `name`, independent `start/end` and optional cropped `partial` |

CB address or PC reuse does not reuse identity. A/B/A buffers may all contain PC
zero; selecting CB3/PC0 cannot select CB1/PC0. Physical triangle/pool/context slots
carry independent observer generation ownership; their reuse is not a meshlet ID.
No frame is inferred from a CB address. The current hardware runs one CB at a time;
CPU preparation may overlap its execution.

## Archived snapshots

The offline decoder and cold replay retain support for archived snapshot formats.
Their constants live in `tools/gpu_profiler/snapshot_abi.rs`; the SDK no longer
produces board pause/UART snapshots. Current application profiling uses sim/emu.

## Legacy observer records

The legacy event stream starts `FORMAT 2`. Subsequent numeric records retain decimal
cycles and IDs; instruction words in `CMD_EXEC/CMD_END` are hexadecimal.

| Record | Fields after the tag |
| --- | --- |
| `SUBMIT` | cycle, submission ID, CB byte base, valid byte length |
| `DONE` | cycle, submission ID, cumulative allocated/published/retired/cache-committed/empty-skipped quad counts |
| `CMD_EXEC` | cycle, PC bytes, instruction word |
| `CMD_END` | release cycle, PC bytes, fetch cycle, execute cycle, state, word |
| `DMA_JOB / DMA_DONE` | accepted cycle/job/PC/mesh/slot/source/size; completion cycle/job |
| `DMA_REQUEST / DMA_RETURN` | cycle/transaction/job/address/count; completion cycle/transaction/job |
| `DRAW / VT_END` | cycle/meshlet/PC/input slot/DMA job; completion cycle/meshlet |
| `Q_ALLOC / Q_PUBLISH / Q_RETIRE` | cycle/quad/meshlet/slot; cycle/quad/meshlet/mask; completion cycle/quad/meshlet/empty |
| `ROP_BEGIN / Q_COMMIT / FINAL` | cycle/quad/meshlet; completion cycle/quad/meshlet; cycle/quad/meshlet/lane |
| `STATE` | cycle, PC, CP state, render idle, work busy, DMA idle, memory busy, flush complete, waits done, CE |
| `CPU_BEGIN / CPU_END` | cycle/span/logical frame/remaining-line label; cycle/span |
| `CAPTURE_END / BOUNDED_STOP` | cycle at explicit successful scope end or incomplete watchdog stop |

Additional prepare/raster/interpolator transition records accompany the per-cycle
`load.csv`. Samples describe `[cycle,cycle+1)`. End events for an accepted edge
use the following cycle. SEND acceptance is a wall-clock event even during CE0.
Old completion is recorded before accepting a successor on the same edge.
The observer remains open across DONE, until the requested scope is drained.

The correlator binds commands by `(submission,PC)`, then assigns unique occurrence
IDs. DMA/meshlet IDs follow their accepted submission; quad ownership follows
T slots, R/I context, pool generation and independent ROP context. Pool retirement
precedes final cache commit. P is the union of published-quad lifetimes through
commit or ordered empty skip, including dependency waits. It is not ALU occupancy.
DONE requires empty T/pool/ROP state and cumulative quad/token conservation.

## Completion, evidence and compatibility

Normal builds require CAPTURE_END, completed submissions/jobs/transactions,
ordered retirement and quad conservation. Watchdog or truncated captures need
explicit `--allow-partial` and retain a visible incomplete status. An open CPU
annotation at a successful GPU scope boundary is cropped and marked individually;
it does not fabricate completion of CPU work.

`provenance` retains capture configuration and hashes of sources, injected
observer/testbench and initialization inputs. Correlation rechecks the copied
source/input hashes. This binds simulation evidence, without claiming rendered
image correctness, board correctness or equivalence to another RTL revision.

The v1 importer maps its sole submission and PC references into occurrence IDs,
preserves source timestamps and verification metadata, and uses a null CB base
when it was not recorded. If its CPU endpoint extends beyond the old container,
the session extent expands to enclose it; the original extent is retained in
provenance. Scene-specific names survive import; new captures use metadata labels
or generic meshlet names. Unsupported format versions are rejected.

## Derived display

The cold replay exports `images/final.png`, one `command-<id>.png` for each command except DRAWs with zero own published quads, and `images/index.json`. Each entry binds the command occurrence,
`complete_cycle`, byte target bases, target epoch, color interpretation and plane/
PNG digests. The report embeds these PNGs, so the offline page remains portable.
A DRAW with zero own published quads has no completion screenshot. Its early
front-end completion can precede older in-flight pixel work, so the viewer hides
its preview even when reading an older report that contains such an image. No
hardware wait or drain is inserted. Nonempty DRAW images retain their own final
quad/cache-commit completion and historical owner filtering; they cannot include
younger writes or be replaced by a later empty DRAW's partial picture.
It starts with the final output thumbnail; hover enlarges it and selecting a
command switches the same preview to that command's completion image. Right-click
inside instruction flow clears selection, highlights and time selection and
restores the final output. Other regions retain native context menus; middle or
Shift-left drag pans the charts.

The observer journals actual BSRAM A writes, valid/tag transitions, SET_TARGET
generations and accepted framebuffer write payloads. Reconstruction combines
physical word history with the latest eligible versions in valid resident cache
slots. Slot replacement/invalidation discards resident deltas. Writeback archives
each word's older cache owners before committing its latest physical value;
otherwise a younger overwrite followed by eviction could erase an older DRAW's
completion image. Snapshots exclude younger command owners even when they have
already written overlapping pixels. Direct CLEAR, serial UI and MEMCPY payloads
retain their own command identity rather than stealing the current DRAW owner.

Same-cycle cache writes precede accepted writeback, directory lifecycle updates
and the completion projection. A writeback victim remains owned through its real
terminal response, so the valid-cache layer covers payload still in flight to
memory. Stale target epochs and invalid lines cannot overlay a different target.
Unexplained writeback or missing preimage fails reconstruction. Commands before
the first target binding explicitly show the capture-declared output preimage.
The pixel decoder is shared with the existing final RGB565/column preview. No
extra flush, wait or GPU serialization is introduced.

Borrowed render-target allocation tails are READ/PREIMAGE ranges. Only the
204800-byte physical plane is writable/FULL_CLEAR; allocation capacity does not
grant permission to overwrite guards.

The passive observer samples RO, framebuffer-read and framebuffer-write master
interfaces in the GPU logic clock domain. An accepted request counts as issued;
each read response and write-data-ready strobe counts one actual 64-bit payload
slot; LAST/error counts a completed transaction. The request count encodes
32-byte lines minus one (legal values 0, 1 and 3). Request volume is not reported
as completed payload. Current links have no byte-enable field: each slot carries
eight bytes. Failed read payload contributes no effective bytes.

`profile.json.memory` and `memory-transactions.csv` preserve transaction identity,
source, halfword address, expected/actual beats and real terminal time. Reports
include issued/completed/error counts, wire/effective bytes, total/read/write
average rates and a 256-cycle sliding-window peak. Shorter selections have no
full-window peak. Link utilization uses eight bytes per logic cycle; aggregate
utilization sums the capacity of three independent links. These are RTL interface
measurements, with no physical SDRAM pin-utilization or board-timing claim, and
no downstream transfer is counted again.

Sources follow the actual RO arbiter owner and DMA job kind, or the direct CLEAR
mux. Shared framebuffer-cache traffic retains its component identity; it is not
guessed to belong to whichever DRAW/UI/COPY command currently executes. Unknown
sources remain unknown. The right inspector has four native, initially open
collapsible groups: Inspect Instruction plus detail, Legend plus its two notes,
Bin cycles, and GPU memory summary. The frame preview is above these groups and
explains missing saved images or zero-own-quad DRAWs. Redraw updates group bodies,
retaining native keyboard operation and each group's collapsed state.

Bin cycles and the memory summary share one active time range. A selected DRAW
uses `bounds(c)`: command start or earlier associated DMA through proven command/
job completion and last-quad completion. Viewport focus padding and subsequent
pan/zoom do not expand DRAW statistics. An explicit overview range takes
precedence; clearing restores the full frame. Non-DRAW selection uses the time
range. View range start/duration round to integer cycles; DRAW bounds round
outward to preserve the complete lifetime. Hover tooltips keep the original chart bin
semantics and do not replace inspector range statistics. Read/write totals query
raw saved event timestamps with half-open boundaries. Average read/write/combined
rates use that interval's duration and clock. Peak combined throughput uses a
complete 256-cycle sliding window within the selected range; shorter windows
have no peak. Partial coverage labels known counts and unknown full-range rates;
empty ranges have zero bytes and no rate. Old captures without exact memory events
cannot supply selected-range estimates. Full-capture totals remain a separately
labelled reference.

Each accepted payload beat contributes eight bytes once; requests and terminal
acknowledgements contribute no bytes. CPU/Display traffic is excluded. Masked/error
beats still occupy wire bytes, not useful bytes or physical SDRAM utilization.
Backpressure, outstanding-without-data and write-terminal waits count link-cycles
and may overlap; unavailable sample coverage remains explicit. Histograms and wait
rows stay in the existing GPU pane on its shared axis, with no separate Memory
panel. On narrow screens the inspector stacks below the plots.

Saved display previews default to the same 64-entry `LINEAR6_TO_SRGB8` table as
`display.rs` and Display1 RTL. Red/blue 5-bit values first replicate to 6 bits;
green 6-bit values index directly. Gamma is applied once before PNG compression
and an sRGB tag identifies the encoded result; the viewer directly embeds it.
Image metadata keeps the captured `format`, raw `plane_sha256`, and separately
records preview encoding, display-format override and LUT/source identities.
Presentation policy is separate from captured format. The CLI defaults to
`--preview display1 --capture-format unknown`; an unspecified stored zero remains
unknown. Explicit diagnostic `raw` bypasses the LUT, and `capture` uses it only
when the caller declares the field known and its value is 1. Raw color/depth
buffers remain authoritative and unchanged.

The shared reporting backend serves `profiler.py build/capture` and RTL replay.
Replay saves its frozen snapshot and automatically produces command PNGs and a
self-contained HTML report. Offline `build --snapshot GPS1_DIRECTORY` regenerates
history from original trace bytes: it verifies snapshot identity, reproduces and
compares all four seed files with the production generator, checks the command
stream and binds separate raw outputs to the identical trace/source capture.
Historical final color and depth hashes must match the same-run raw outputs.
`report-inputs.json` records dependency hashes, preview policy and LUT identity;
`--binding` checks that record on repeat builds.
The record names external dependencies; rebuilding requires those files.
Explicit capture, `--snapshot` and `--raw` paths can relocate them while the
original `--binding` continues checking their content hashes.
`render` validates and exports
the embedded PNG bytes directly, with no second conversion. Timing-only captures
remain supported with explicit images-unavailable reasons; `--require-images`
rejects incomplete image reports. SIM remains final-result-only.

New/empty report outputs are the default. `build` and profiling `render` accept
`--replace-report` for a stable generated report entry. A verified sibling stage
is published only after the prior directory has been archived and its hashes
checked. The new `report-replacement.json` maps original paths to the archive and
records previous file hashes. Unknown content, reparse points and mismatched
generated HTML/profile data refuse replacement. Failed staging preserves the
published entry.
The preview wrapper has an explicit author hidden rule, and no-image/empty-DRAW
transitions remove both image sources while keeping their explanations visible.

`commands[].complete_cycle` includes every associated meshlet stage and ROP
commit, plus owned DMA completion. `end` retains control release. Partial
captures have no proven command completion time. CSV asynchronous ends and the
inspector use this distinction rather than stopping DRAW at instruction release.

RLE tracks retain exact sampled state; event-rate tracks retain accepted events.
Time-bin occupancy and queues are time-weighted. Event bins count accepted events
and display their rate per 1024 cycles. Backend overview activity is the per-cycle
OR of `r_busy`, `i_busy`, `quad_total > 0` and `rop_contexts > 0`, then aggregated.
It covers the pool-release/cache-commit tail and includes waits; later FLUSH
maintenance remains separate. Types filter command rows/picker/command marks only;
physical load, occupancy overview and all-alive inspection are not filtered.

The viewer embeds validated JSON safely, has no CDN or server dependency and
formats all time using `clock_hz`. CSVs preserve submission IDs and command IDs
alongside PC. Raw capture, generated HTML/CSV and large replay data belong in
`target`; the small synthetic regression fixture lives beside the tool tests.

### Optional exact transfer timeline

Profile v2 may contain `memory.timeline` version1. `available:true` includes its
half-open `start/end`, sparse `data` rows `[cycle,read_wire_bytes,write_wire_bytes]`,
and RLE `states.outstanding`, `states.waiting`, `states.terminal_wait`. Rows merge
concurrent master slots: reads0/8/16B and writes0/8B, with each64-bit
GPU-master payload slot counted once. Timeline bytes must equal existing summary totals;
state integrals must equal captured request/link-cycle totals. No byte-enable
field or physical SDRAM pin measurement is introduced.

`backpressure` has independent availability, start/end and an RLE `track`, because
valid/not-ready is observed by load samples rather than request acceptance events.
Missing samples remain unknown, including uncovered beginning/end fragments.
Outstanding means accepted/uncompleted request inventory; waiting means the
number of live links without a data slot during a sample. Terminal write wait
covers complete data delivery until acknowledgement. None is inferred from zero
throughput, and these waits may overlap.

The same activity canvas adds actual read/write bytes/bin and request/wait rows.
All use the shared time coordinate and adaptive bins. Hover distinguishes nominal
accepted-request bytes from transferred bytes; a binary cycle detail still shows
the common memory bin. Selection/zoom sums exact timestamped bytes in `[a,b)`;
RLE states integrate overlap of `[cycle,cycle+1)` and may have fractional covered
cycles. Partial bins display known bytes/state means and mark unknown coverage,
rather than discarding a bucket or filling missing samples with zero. Legacy
profiles without the optional timeline still load with temporal rows unavailable;
their valid full-capture totals are retained. Saved MEM events can reconstruct
this optional data without a new hardware run. Provenance and cold/warm labels
are preserved, and no second Memory panel or frontend is created.

## One GPU frame

`cpu-v3-system-emu ... --profile-out CAPTURE --profile-frame N` selects a
positive application presentation ordinal, starting at1. It defaults the stop
presentation to N; an explicit `--presents` must reach N. Existing cycle, host,
event and image budgets still apply. Warmup executes unchanged and its raw
physical/cache journal is retained from the original preimage. No cache flush,
cold replay or synthetic checkpoint changes the measured workload.

The ordinary report build automatically honors capture `profile_frame`; existing
captures can use `profiler.py build CAPTURE --out REPORT --only-frame N`.
`render PROFILE --out REPORT --only-frame N` also supports an existing profile
whose native history covers the selected group. The selection cannot be combined
with `--frames`; a conflicting capture selection or unreached frame is rejected.
One frame includes every CB between its previous and current presentation
requests, retaining original IDs. The group must complete before the request.

The GPU window spans its first START through its last completion. It does not
claim CPU frame ownership or include the subsequent present/scanout wait. This
mode needs no applied present or complete HDMI raster; native command images
remain available. The previous `--frames` applied-to-applied reports retain their
strict matching-raster requirement. Browser data, CSV and image reconstruction
select only the requested group; canonical JSON/raw evidence retain warmup facts
for validation and faithful cache history. Opening `profiler.html` does not load
those raw journals. This setting is passive and never changes emulated timing. The CLI runs the
model on one8MiB host worker, avoiding the smaller Windows main-thread stack;
modeled stack memory and all emulated clocks remain unchanged.
