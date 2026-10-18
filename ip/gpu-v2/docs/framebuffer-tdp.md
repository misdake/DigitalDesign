# VLIW framebuffer dual-port implementation

The selected VLIW GPU replaces its framebuffer sink, cache and ROP leaf
together. The compact adoption receipt is
[framebuffer-tdp-evidence.json](../examples/vliw_integration/framebuffer-tdp-evidence.json).
Historical standalone factories remain available, but are not
reachable in the selected GPU export. Vertex and frontend numerical formats
are unchanged. Physical resource reports end at the GPU's three transaction
ports and exclude the memory controller, CPU and display. Scene timing uses
the actual memory controller and SDRAM pin model with portable GPU storage.
Native/portable equivalence is qualified separately for the cache/ROP and pool
components. There is no board qualification or CPU/display contention test.

## Physical ports and enabled-edge calendar

Eight 16-by-16 cache lines occupy four native 1024-by-16 DPB banks. Port A
belongs to ROP; port B independently performs refill and writeback on wall
clock edges. Compute CE never freezes an accepted memory transaction. An
active maintenance line cannot become a ROP owner, and active ROP owners
cannot become maintenance victims. There are at most two distinct ROP-owned
lines, so demand replacement needs only three consecutive round-robin
candidates after its invalid-line check.

The lane LSB selects the bank-group LSB. This makes the read and write groups
opposite at each phase; no arbitration or retry is required on port A.

| Enabled edge relative to a pixel's C acceptance | Action |
| --- | --- |
| 0 | Accept source C; issue old-C read when blending needs it |
| 1 | Consume old C; accept source D; issue old-D read when the depth function needs it |
| 2 | Consume old D and compare; accept the next pixel's C |
| 3 | Issue the existing shared ROP DSP phase |
| 4 | Pack the existing selected blend/quantization result |
| 5 | Capture the result in its fixed return slot |
| 6 | Write D if enabled and passing; set its dirty bit on this actual write |
| 7 | Write C if passing; set its dirty bit on this actual write |

The last C write retires a quad and releases its cache owner. Source ownership
ends earlier, after the fourth D acceptance. This permits one pixel per two
enabled edges, including successive quads at identical coordinates. CE
freezes the whole A calendar and shared compute phase. Maintenance uses B's
native held Q as its one-beat output buffer; it reads the next beat only after
the previous beat is consumed. There is no four-pixel old-C/old-D snapshot,
same-quad batch bypass, or global ROP-versus-maintenance exclusion.

## Lossy prefetch

The existing raster G14 event provides the current tile when its geometric
quad mask is nonzero. This precedes numerical depth rejection and is only a
hint. It requires no additional result-pool read port and has no ready path
to raster. Two nine-bit entries absorb hints on wall-clock edges, including
compute CE=0. Capacity uses old state: a full queue drops a new hint even if
one entry leaves on the same edge.

Hints deduplicate against queued, resident and active targets. Service
rechecks residence. Demand has priority. Speculation reuses the demand
candidate rather than an independent eight-line priority search. A current
offered row, old ROP owner or recently unused prefetched line is protected;
an unsafe hint is dropped rather than retried. Successful speculative
replacement and an unsafe nonresident drop each advance the round-robin
pointer once. This is deliberately not equivalent to searching every possible
speculative victim.

The selected compile-time policy permits dirty replacement using the existing
single writeback/refill state machine. It adds no transaction context. Hints
are cleared on abort, fault and flush. An already presented descriptor remains
stable through acceptance, and accepted transport drains to its terminal
response. A partially refilled tile never becomes resident. Flush finishes
after all ROP owners retire and dirty planes are written back.

Prefetch can waste bandwidth for sparse geometry or a poor traversal order.
Its queue occupancy, useful fills, extra transactions and total scene time
must be measured together. A high local overlap count alone is not a speedup.

## Tile identity through the whole chain

Pool head and next-head views retain only the existing metadata. Both derive
the nine-bit `{Ytile[3:0], Xtile[4:0]}` identity from their held coordinates.
The backend header derives it the same way, rather than holding another tile
register. The hint queue and cache use this raw identity consistently.

Only B maintenance converts the selected held victim/new tag to the external
linear tile number: `Ytile * (WIDTH / 16) + Xtile`. WIDTH is static and the
implementation uses constant shifts/adds, avoiding automatic DSP inference.
The materialized surface contract remains width 16..512 and height 16..256,
in multiples of 16. External color/depth layouts and the 128-byte transport
are unchanged. Raw hint validation checks X and Y separately; a raw tag cannot
be validated against the number of linear tiles.

## Bounded verification

`tests/framebuffer_tdp.py` uses an independent integer golden model, full
framebuffer bytes, paid 128-byte transfers, all depth functions, mixed blend,
CE pauses, repeated coordinates and tile-row boundaries. The selected Q6
alpha arithmetic and RGB565 quantization are preserved; this is not a change
to an ideal alpha/255 formula. `tests/framebuffer_tdp_prefetch.py` checks the
lossy queue, service recheck, held-request abort, reset, flush and early terminal
fault. Both run portable storage and native Gowin primitives and compare event
traces. Each test has a cycle bound and process timeout.

Factory tests in `tests/vliw_backend.rs` check that old active modules are
absent and that full-memory binding rewrites only the enclosing GPU ports,
leaving private cache ports intact. Whole-GPU scene and physical-fit receipts
are kept with the isolated experiment, separate from these local checks.

After setting `IVERILOG_EXE`, `VVP_EXE` and `GOWIN_HOME`, run the factory test
first to materialize the shared DSP source, then use fresh output tags:

```text
cargo test --release -p gpu-v2 --test vliw_backend
python ip/gpu-v2/tests/framebuffer_tdp.py --tag verify-tdp --mode both --prefetch dirty
python ip/gpu-v2/tests/framebuffer_tdp_prefetch.py --tag verify-prefetch
```

The lifecycle test includes two complete real refills whose second final ACK
coincides with accepting the previously protected row. Publishing a new
prefetch protection wins over consuming the previous protection on that edge.
