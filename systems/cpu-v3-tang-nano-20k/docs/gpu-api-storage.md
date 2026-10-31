---
id: gpu-api-storage
status: active
last-verified: 2026-10-31
---

# Explicit GPU storage core

The private modules provide the serialized interfaces used by scene preparation
and command recording. They operate on caller-provided Array spans and contain
no Renderer, Frame, scene-object cache or automatic presentation scheduler.
Lengths and offsets are 16-bit CPU words; physical extents use 128-byte sectors.

## Prepared and record storage

The schema reserves an 8192-byte low-memory arena with at most30bytes alignment
slack. Small16-word slots grow upward and large48-word slots downward from fixed
endpoints. Each class reuses its own free list before increasing its high-water
mark. A handle is word_offset+1. Freeing a descriptor does not free external data.
Release removes the debug-only 256-word live/type registry.

Camera32 and State16 words form each CB's desired snapshot. Model uses42words
in a large slot; Material and external descriptions use small slots. Producers
write every defined field and padding; the schema defines the single layout.
Two independent208-word records hold control32, Camera32, State16, last Global32,
last Render16, last UI16 and last Instance64. A shared256-word sequential scratch
is not a persistent binding cache. Each CB retains64 allocation facts of4words
(first_sector,allocation_sectors,kind,access); these facts describe provenance,
not ownership or a second resource scheduler.

## CPU-owned external transfers

The read/write helpers accept0..64words. Zero succeeds without inspecting an
address or changing DSEG. Nonzero low-memory spans must remain below0xc000;
external spans must remain inside8MiB. The helper saves DSEG3, waits before
remapping, splits at32KiB pages, waits for completion and restores the mapping.
No window pointer escapes. Complete16-word spans with both pointers32-byte
aligned use dual-address LCOPY: newest source cache data reaches destination
DRAM, and a cached destination alias is discarded. Partial or differently
aligned spans use scalar accesses and preserve adjacent words. Submission still
publishes all remaining CPU stores; these helpers do not retire GPU work.

## Native recording and completion

Each CB owns its command/push cursors and canonical binding values. Writers
preflight command capacity, aligned push storage, Pass terminal space and END
fetch padding before committing. Pass closure records WAIT/FLUSH. Seal adds END
and128-byte padding once; replay preserves bytes, lengths and target addresses.
Reset clears CPU recording metadata without clearing external memory.

Global/Render changes share one render drain; pure Instance changes do not add
one. Trusted mesh directories expand through bounded scratch into native
DMA/WAIT/DRAW. UI records only descriptors and ranges; external grids remain
caller-owned. Complete allocation facts survive execution until reset.

Execution preparation retains one active slot and finishes cache publication
before COUNT/START. Start requires the same healthy sealed identity and restored
mapping. A requested resume republishes exactly once. Poll returns that slot once
only after actual GPU completion; faults retain ownership. Source12/facts4 and
read-only health exports let an external capture consumer observe the prepared
identity without changing cursors, publishing or issuing SEND.

Display is independent: GPU retirement precedes a swap request, then the caller
waits for the request to apply and old reads to retire. Frame-number change alone
is not completion. Reconfiguration and external allocation lifetimes remain the
application's responsibility.

## Verification

The host gpu_api core cases execute real RCC/CPU V3 programs with finite bounds.
They check both profiles, arena reuse and capacity, exact native bytes, cross-page
transfers, executed dual-address LCOPY, neighboring words, mapping restoration,
publication ordering, replay stability, retained faults and delayed Display
retirement. Completion peers establish software protocol behavior; they do not
establish GPU pixels, physical CPU-cache timing or scanout behavior.
