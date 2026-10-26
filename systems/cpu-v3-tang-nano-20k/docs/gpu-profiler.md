---
id: system-gpu-profiler
status: active
last-verified: 2026-10-26
---

# GPU profiling sessions

The system-owned profiler is a passive simulation observer, correlator and offline
viewer. Its [tool README](/systems/cpu-v3-tang-nano-20k/tools/gpu_profiler/README.md)
owns usage and verification commands. It changes no synthesized logic or Device4
ABI. A render frame, a CPU recording interval and a GPU submission are distinct.

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

## Observer records

The event stream starts `FORMAT 2`. Subsequent numeric records retain decimal
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
