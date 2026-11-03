# Indexed Triangle and T16 cycle extension

`vertex::Core::with_triangles()` and `vertex::Meshlet::with_triangles()` extend
the production [input/bounds/V subset](/ip/gpu-v2/docs/references/cycle-vertex.md)
with index decoding, bounded-F16 T execution, the default sixteen-slot result
queue and complete T16 export. Default constructors retain their previous
explicit unsupported-T boundary. No complete DRAW/Device4 GPU is exposed.

Both constructors use the existing pure `outputs` and one-old-edge `clock`
interface. Input adds `packet_ready`/`word_ready`; the core input additionally
accepts a public triangle descriptor and cull flag. Core output exposes actual
packet/word offers, done_t/triangle_ready, coefficient writes and ownership
observations. `Triangle` and `Export` expose the production register state for
tracing. They do not own physical clocks or invoke a functional renderer.
Mesh RGBA remains an explicit input; fixtures preload its matching uniform word22.
Production DRAW/command binding of that input is outside this extension.

## One arithmetic and storage path

T starts at PC128 in the sole production 512x72 ROM, using its actual compact
mask. The existing generator is not rerun or duplicated; its T schedule lies
within 128..255, while 320..511 are cleared. T and V share the same asynchronous
RF, old accumulator, registered low/high MAC products and source/private-work
storage. The top-level production connections determine which child outputs
are consumed; unused T child outputs do not become additional state.

T reads the existing synchronous position Q into the common unpacker/RF. Its
normalization state includes any_front, sticky, cross_groups, qmax, det_exp,
alignment sign and determinant sign. Each encoded 4/1-bit normalization step,
self-loop, DSP alignment, toward-zero correction, output write and final publish
commits at its real enabled edge. Geometry domain, formulas and T16 field
encoding are defined by [production geometry](/ip/gpu-v2/docs/geometry.md).
Positions must fit signed26 F16. All-backward, zero-determinant and enabled
negative-determinant rejection complete without publishing a packet. There is
no near-plane clipping fan.

The meshlet controller reads low54 bits of each 64-bit index qword after its
source synchronous read edge. Three 18-bit triangles share each group. Only T
completion advances group_position, shifts 18 bits and wraps the seven-bit
sequence; another source read follows the third group member. Header, ref,
position-domain and cursor violations stop at their production monitor boundary.
Previously committed V/T work remains part of the observable prefix; recovery
on the rejected edge is unspecified.

## Publication and lifetimes

| Resource | Storage and release |
| --- | --- |
| Input page | Two source-bank payload/index pages; remains owned until controller meshlet_done is observed by the parent binding |
| Position | Three existing result planes; reusable after that meshlet's final T computation, while queued T exports continue |
| T coefficients | Ten low36/high18 field-major planes indexed by slot4; final flow6 publishes after actual coefficient writes |
| Descriptor | Independent 16x33 asynchronous cells: public32 plus private attribute epoch; immutable head through item15 acceptance |
| N36/attribute/RGBA | Two existing attribute/color epochs; reusable only after the old epoch's last indexed export retires |
| T slot | Released only by `ce && word_valid && word_ready && word_index == 15` |

Extended produced/consumed cursors provide full/empty. No second occupancy or
per-epoch reference counter exists. A full queue blocks a new T launch using
old state, including when item15 is accepted on that edge. An admitted T owns
its slot and continues computing while word output stalls; export backpressure
does not stall its independent position read port.

Packet acceptance requests coefficient0 on the same enabled edge. Low/high
synchronous Q returns are consumed on a later enabled edge into the word
buffer; response_index carries their identity. A pending Q cannot be overwritten
until the word buffer is empty or its old word is accepted. Word data/index hold
through CE0 and downstream stalls. Six final indexed attribute fields read
references directly from the descriptor head; the command profile inserts
RGBA into the high bits of items10/11. There is no copied or rotated refs cache.

A meshlet may complete computation before its queued outputs drain. `drained`
requires core idle, empty T queue and inactive import. For consecutive A/B/C,
B can reuse positions and the alternate attribute epoch while A exports; C
waits until the FIFO head reaches B's epoch or the queue empties. A's item15
stall therefore prevents C admission. Bounds rejection does not flip the epoch.
Input-page identity is independent of attribute epoch.

Reset clears control, queue cursors/valids, pending Q owners and synchronous Q.
RF, descriptor cells, result cells, private-work cells and colors retain their
contents; source/private-work ownership is reset as in the original primitives.
No retained descriptor or result becomes valid solely because its cells survived.
Known-bit tracking and forbidden same-address cross-port access remain explicit.

## Bounded qualification

`vliw_cycle_triangle` supplies real DMA rows and an independent i128 scalar
oracle for transported cross/exponents, determinant K/ed, viewport coefficients,
refs, descriptor and all sixteen fields. The oracle does not interpret ROM or
feed results back into the cycle model. Downstream coverage reduction and
orientation canonicalization from the functional renderer are not T16 inputs.

Fixtures include 1/3/4 triangles, rejection/sign/domain/normal modes, fractional
negative discarded bits, default sixteen-slot 17/33T fill/wrap, CE/word/header
backpressure, A/B/C epoch reuse, and resets with a full queue, pending Q or held
word. Short production RTL replays compare ports and ROM/RF/MAC/work/Q/index/
queue/epoch observations with explicit known masks. Compiled actual DUT
mutations must visibly diverge; compile failures and timeouts are not passes.
Illegal peers must hit their exact production monitor before any tape mismatch.

The native fixture reuses one existing arbiter/endpoint/gear/MC service. Explicit
pre-memory drives command DMA/router writes, including real index words for two
meshlets, with other-page/cache prefetch. The Vertex/T and DMA/router RTL cones
separately replay the same recorded edge tape. CPU/Display requesters are idle;
board request slots, live whole-system RTL, loader-final equivalence, complete
frame throughput and board operation are not qualified.

Direct fixtures stop at 100000 logic edges, native fixtures at 200000; every RTL
process has a 30-second wall bound and an independent testbench watchdog.
