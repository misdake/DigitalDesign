# Production input, bounds and Vertex cycle subset

`system::vliw_cycle::vertex` models the selected compact command-profile input
storage, meshlet metadata controller, bounds prologue and complete V-mode.
It consumes V96 and writes the production five-field result layout. It does
not use the independent PackedVertex v6 schedule or its seven serial rows.

`Meshlet::outputs(Input)` is pure; `clock(Input)` commits one logic edge from
old state. `Core` exposes the same interface below the metadata controller.
Neither object owns Memory, an arbiter, physical clocks or a functional GPU.
A caller samples all participants before advancing them. DMA writes are wall
edge events, including CE0; compute, metadata reads and source requests use CE.
An error terminates the run at its declared boundary; recovery is unspecified.

## Storage and ownership

| Storage or port | Implemented contract |
| --- | --- |
| Input payload | Two 128-qword pages at 0 and 128; V96 has three 32-bit words per vertex |
| Metadata | Four qwords at 256 or 260; controller reads words 0/1/3, V bootstrap reads word 2 |
| Uniforms | Four banks of 16 qwords, starting at 320, 336, 352 and 368 |
| Command cache | Two 16-qword banks at 448 and 464; legal prefetch remains independent of V execution |
| Source RAM | Two 512x36 banks; each DMA half is sign extended from 32 to 36 bits; enabled synchronous reads retain old Q |
| Private work | Sixteen 36-bit cells with registered read address and asynchronous cell output; logical-owner checks reject stale aliases and same-slot collisions |
| CP source | Concatenates the two source-bank Q low halves; bypasses the private-work selection on the right MAC input |
| RF | Sixteen asynchronously read 32-bit cells, initialized to zero as in the portable source and RAM16SDP4 default INIT; no runtime reset |
| Position results | Three fields addressed by `{field[1:0], vertex_id}` |
| Attribute results | N36 and raw attribute addressed by `{1, attribute, epoch, vertex_id}`; distinct storage from the numerically overlapping input addresses |

Reset clears controller state, synchronous Q, private-work logical ownership,
ROM Q and MAC/control registers. It retains source cells, private-work cells,
RF cells, result cells and epoch-color cells. `Bits` explicitly carries value
and known-bit masks. Unwritten cells remain unknown; consuming unknown control
or published result data returns `Error::Unknown`, rather than creating zeros.
Same-address cross-port source accesses are contract errors.

The parent command binding's `input_active`/`active_page` registers are also
modeled: they acquire ownership on accepted start and release it on the wall
edge observing meshlet done. CP reads while owned and writes to the active
payload page fail its passive monitors. The meshlet controller additionally
rejects writes to active metadata and uniforms. Inactive payload/metadata pages
and command-cache regions remain writable. Native return beats cannot stall;
the producer must reserve legal storage before issuing a transfer.

## Old-state computation and completion

The model decodes the sole production `control_rom.v` and `control_fields.vh`.
It selects the compact words and applies the original compact-used-bit mask;
there is no second hand-maintained instruction array. ROM reads take one
enabled edge. RF writeback takes the old accumulator bits 47:16. The MAC keeps
registered low/high signed products and combinational C feedback with low-54-bit
arithmetic. A new product, RAM Q or RF write cannot bypass its sampling edge.

Normal16 discards the low two source bits before RNE and signed-12 saturation;
normal18 follows its separate original path. UV and RGB attributes preserve the
raw attribute word. Each retained vertex writes XYZ, N36 and attribute, with
publication only after the fifth accepted write. `emit_address` is a sequential
offer address, not a packed-array replacement for the physical bank addresses.
`emit_valid` can stay asserted during CE0 or output backpressure. ROM, RF, MAC
and pack state hold while that offer stalls; DMA continues.

The bounds prologue starts at PC256 and includes its real work/RF temporaries,
behind/left/right rejection and eligibility checks. Ineligible bounds retain
the meshlet. Epoch flips follow the original successful bounds path; a rejected
meshlet produces no Vertex writes. V=0 completes through the production empty
path. With T=0, all V writes precede actual meshlet completion and input release.

A nonzero metadata T count returns `UnsupportedTriangles { count, slot }` at
META0_Q, before meshlet reset or any new V result. Its committed DMA prefix
remains observable and it produces no synthetic completion. An explicit
accepted core T launch returns `UnsupportedStartT`. Index/T execution, clipping,
T16 export, queued-T epoch lifetime, raster/pixel/ROP, complete DRAW submission
and root composition are outside this subset. Existing functional simulation,
profiling command layouts and production RTL are unchanged.

## Qualification boundary

`vliw_cycle_vertex` checks independent scalar output values and physical bank
writes for V=0/1/3/64, both normal and attribute modes, both pages, all four
uniform banks, negative coefficients, MAC wrap and the three rejection classes
plus ineligible bounds. Completion is followed by runtime reset and a retained
source-cell read, while RF and result cells remain present.

The native fixture preloads explicit backing memory before cold initialization,
then drives the existing command DMA/router from actual arbiter/endpoint/gear/MC
feedback. Two uniforms, two meshlets and one cache line produce 88 RAM qwords
and 30 V writes; other-page and cache writes overlap active V execution. Both
result epochs and a drained CP cache read are checked. CPU/Display requesters
remain idle; board request slots and loader-final equivalence are not modeled.

The explicit ignored RTL qualification exports the unchanged production
frontend cone and parent ownership monitors, comparing every sampled port and
ROM/RF/MAC/Q/control probe, including known bits. A second original DMA/router
cone replays the same native feedback tape and must reproduce the connecting
DMA beats. This is two connected edge-tape qualifications, not a live whole
system RTL/Memory co-simulation. Compiling an actual DUT mutation is required;
a compiler failure does not count as detecting changed behavior. Illegal-peer
runs must fail a production monitor before a differential mismatch. The T
unsupported case qualifies only the legal prefix through the stopping edge.

Each direct run is bounded at 50000 logic edges, native runs at 200000, and each
RTL process at 30 seconds with an independent cycle watchdog. Host samples
include trace collection and distinguish fixture initialization, stepping and
process wall time. They do not measure a complete frame, physical timing or
board performance.
