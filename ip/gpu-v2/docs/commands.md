# Explicit 32-bit GPU commands

The `command` factory executes a complete DRAM command stream with the existing
GPU input scratchpad. It contains one decoder, one command/data DMA engine and
the existing texture/framebuffer router. There is no separate control RAM,
Mesh directory walker, pending draw queue or second geometry DMA engine.
The viewport is fixed400x240. SET_TARGET installs independently allocated color
and depth plane addresses at runtime. A single runtime texture descriptor supplies the
aligned base, square size, mip layout and filter to both LOD and Sampling.
There is no texture slot registry or duplicate LOD size configuration in this
profile. A serial CLEAR command fills one complete installed plane through
the existing framebuffer-write master. Vertex and fragment
formats are unchanged.

## Submission and commands

The production control port is the synchronous CPU Device4 bus (16-bit data).
Only one command buffer is active; the driver owns its lifetime and all indirect
resources. There is no pending submission, hardware token, held result or ACK.

| Operation | Data | Meaning |
| --- | --- | --- |
| SEND device4 channel0 | bits13:0 instruction count minus1; bits15:14 zero | Stage length while idle |
| SEND device4 channel1 | byte base/128, u16 | Accept base and launch on this wall-clock edge |
| RECV device4 channel2 | bit0 busy, bit1 sticky transport error; others zero | Pure status query, no read side effects |

Base is128 B aligned, valid length is4 B aligned, nonzero and at most64 KiB.
The allocation includes readable padding through the final128 B sector and
lies below8 MiB. `DeviceSubmission::new` validates these driver contracts.
The driver serializes both SENDs, sends only while idle, and keeps the CB plus
all referenced input/target allocations alive while busy. An idle staged length
is not an accepted CB. A different selected device or channel reads zero.

SEND is independent of GPU CE: a one-cycle CPU write during CE0 is accepted,
and busy is immediately visible on the following RECV. GPU execution may wait
for CE and normal memory readiness. The staged page count occupies the existing
10-bit `fetch_left` and is consumed in place; the launch address directly loads
16-bit `fetch_line`. The hardware instruction cursor is six bits (page, qword,
half). Exact length and full byte PC exist only in passive simulation monitors.

Busy clears only after END and actual render/DMA/router drain. Valid streams
end with WAIT(all), FLUSH, END; flush completion fences dirty framebuffer data.
Fetching END, finishing V/T, or supplying the last write beat is not completion.
The driver may immediately submit the next CB after busy clears, without ACK.
For example, a372 B stream at byte0x40000 submits0x005c on channel0 followed by
0x0800 on channel1. CPU16-bit word addresses must first be converted to bytes.

Each instruction is one little-endian u32. Unlisted/reserved bits are zero.

| Opcode31:28 | Command | Payload |
| --- | --- | --- |
| 0 | NOP | Zero |
| 1 | DMA_LOCAL | 27:10 source byte address/32; 9:3 destination byte address/32; 2:1 size code; bit0 zero |
| 2 | DMA_MESHLET | 27:12 record byte address/128; 11:4 zero; 3:1 payload sectors minus1; bit0 input slot |
| 3 | DRAW_MESHLET | Bit0 input slot; 27:1 zero |
| 4 | WAIT | Bit0 data DMA; bit1 complete rendering; 27:2 zero |
| 5 | SET_GLOBAL | 6:0 source local byte address/32; 27:7 zero |
| 6 | SET_RENDER | Same slot encoding |
| 7 | FLUSH | Zero |
| 8 | CLEAR | 16 selects depth (1) or color (0); 15:0 repeated value; 27:17 zero |
| 9 | TEX_INVALIDATE | Zero; explicit WAIT(all) required |
| 10 | SET_TARGET | 6:0 source local byte address/32; 27:7 zero |
| 11 | UI_GRID | Same slot encoding; serial 50x15 char grid, described in [UI](/ip/gpu-v2/docs/ui.md) |
| 15 | END | Zero; must be the last valid instruction |

DMA_LOCAL size codes0/1/2 mean32/64/128 B. Code3 is invalid. Local destinations
are staging qwords264..319, or one whole128 B uniform block at320/336/352/368.
The complete copy must stay in its selected region. Each source is aligned to
its selected32/64/128 B length, so a local copy is exactly one physical burst.
Local destinations retain32 B alignment; only meshlet payloads advance between
bursts. Addresses never wrap. DMA_MESHLET uses the
[prefixed record](/ip/gpu-v2/docs/formats.md#command-meshlet-record), reads32 B
metadata and then1..8 complete128 B payload sectors.

Both copies are asynchronous after their descriptor is installed. A second
data copy waits at its command until the single descriptor becomes free.
WAIT bit0 waits for completion, independently of command prefetch. Only a
final write publishes a meshlet page; abort cannot publish partial data.
Uniform completion marks its selected resident block valid. Transport errors
and malformed input are simulation contract violations, not hardware recovery paths.

## Execution and lifecycle

DRAW requires complete global/render bindings, a loaded uniform selected by
the page metadata, and a completely loaded input page. It may start while the
other page is loading. The decoder waits for the existing meshlet_done before
advancing past DRAW: all V/T computation has completed and surviving T records
have entered the result queue. Indexed export, preparation, R/I and the backend
can continue. A later DRAW can still wait for an attribute epoch or queue room.

An input page is reusable after its V/T completion. Retained N36/attributes and
T slots keep their separate epoch/item15 release rules. Uniform copies between
DRAW commands reach their final storage directly; there is no instance staging
copy or SET_INSTANCE command.

WAIT bit1 observes compute/backend drain, including their transactions, but
excludes the CP submission owner and its command/data DMA. It never substitutes
for bit0. SET_GLOBAL/SET_RENDER require a preceding successful render wait,
current rendering idle and completed data DMA. They copy native
objects into the sole live CP FF binding. Consumers borrow those stable fields
directly; there is no separate Lighting context-load handshake or rebind state.
A state instruction retires after its final object row is consumed.
DRAW clears that permission. Different material/global state therefore has an
explicit full-render boundary; compatible meshlets do not.

DRAW reads Instance128 words22/23 through the existing synchronous CP port before
starting V/T. It pays three enabled edges (read, consume, start), without changing
Vertex arithmetic or the pixel initiation interval. Mesh uniform RGB modulates
the selected vertex/texture color; mesh uniform alpha is the sole runtime alpha.
The driver encodes no modulation as white/opaque RGBA; there is no enable bit
or hardware fallback selector. Render has no alpha field.
The color follows the retained attribute epoch and T export into the active I
context, then is copied on quad allocation. A later mesh may replace its uniform
while old quads still shade. Final reuses its existing tint-times-texture product;
no additional per-pixel multiply or DSP is introduced. Uniform RGB is always one
Final operand; only the vertex/texture source is selected for the other operand.
A24-bit distributed-RAM issue view holds uniform RGB. Alpha shares the existing
depth BSRAM with D16 and reaches ROP on its paired D edge, without another RAM,
port, alpha FIFO or C-row alpha register.

Texture fields are driver-validated data, not a hardware validation protocol.
Their [native encoding](/ip/gpu-v2/docs/formats.md#native-command-objects) has one
size exponent and mip flag. Both LOD axes derive from them; a full chain allows
LOD distance0..n, and a base-only texture uses0. Bias is applied once upstream.

TEX_INVALIDATE requires the same successful render wait, current rendering idle
and completed data DMA as SET_RENDER. Its execution edge clears the existing
texture cache set-valid masks and tag-read valid. It has no new queue, RAM or
maintenance state and does not clear data RAM. It never
flushes the framebuffer. SET_RENDER performs no implicit texture invalidation.
The driver explicitly invalidates after completing an in-place texture update
and external coherence. Immutable resources remain cached across descriptor,
filter/bias and Lighting changes. A descriptor replacement still requires the
explicit full-render wait; it does not invalidate cache contents.

Packet production forms the final physical128-B sector before queue storage.
Its first enabled edge preadds base/prefix while capturing member fields. The
second forms the selected tile offset, adds it to that base, and publishes the
final group and metadata. The initiation interval remains one.
The ordered group is35 bits in the same36-bit BSRAM word: sector15:0,
local-X18:16, local-Y21:19, owner29:22 (lane2, quad4, last1, first1),
weight-bank parity30 and texture-tap mask34:31. Parity belongs to the mip level, not to
the physical address; it selects the existing fine/coarse weight bank.

The cache uses physical sector low4 for its16 sets and high12 as the tag.
Lookahead exposes only sector16. Tag comparison and queue retirement contain
no texture row shift or base/prefix addition. The tap mask is consumed directly;
the old level/XY mask restoration and redundant Color level check are absent.
The four-way tag storage retains its existing two native banks. Explicit
invalidation is still needed when memory contents at a cached address change.

FLUSH requires the explicit drain and completed data DMA, then writes back and
invalidates framebuffer tags. END requires that completed flush, stops new
prefetch and waits for every accepted DMA/router transaction before publishing
success. Dirty target residency alone does not require flushing between meshes.

### Runtime target

SET_TARGET reads one32-byte staging object. Qword0 bits13:0 are color base/512,
bits27:14 depth base/512, and bits63:28 are zero; the other24 bytes are padding.
Both full192,000-byte planes must be disjoint,64-KiB aligned and below8 MiB.
The low7 bits of each base/512 field are therefore zero. Three64-KiB blocks hold
one plane, with4,608 bytes of trailing allocation padding; these bytes are not
part of the framebuffer and are not transferred by normal rendering.
The fixed400x240 dimensions and tile layout are unchanged.

The driver explicitly orders WAIT(data+render), FLUSH, then SET_TARGET. The
command performs no implicit flush or clear. Coordinate cache tags require the
old cache to be invalidated before replacing either base. SET_TARGET clears the
completed-flush permission for END, and DRAW requires a target in this submission.
Refill, prefetch, eviction, flush and CLEAR all derive their addresses from
the installed plane bases. Each target register has
only7 variable bits. The existing read/write preparation edge captures the final
sector before request publication, adding no transaction cycle. There is no
target-ID table or hardwired A/B allocation.

### Serial CLEAR

CLEAR carries its repeated16-bit value and one plane bit in the32-bit
instruction. `Command::clear(depth, value)` selects the installed target's
color or depth base. It always writes192000 bytes (400x240 RGB565 or D16),
exactly1500 consecutive128-byte sectors. It has no descriptor, arbitrary start
address or programmable length. SET_TARGET's64 KiB alignment remains unchanged.

CLEAR requires an installed target, explicit render wait and completed data DMA.
It contains no flush, invalidate or dirty-discard operation. Before clearing a
plane that might be resident, the driver explicitly orders WAIT and FLUSH so
dirty cached data cannot later overwrite the clear. Only FLUSH grants the
stored permission required by END.

The decoder enters CLEAR_REQUEST directly from EXEC. Its instruction register
retains the plane/value and reuses bits27:17 as the sole11-bit sector index,
0..1499. The low nine sector bits concatenate with the installed base; only
sector bits10:9 are added to the selected seven-bit64 KiB page. No separate
remaining-length register, full16-bit address increment or staging read remains.
CLEAR bypasses the cache router's single-write lifecycle and drives the existing
framebuffer-write master while rendering is drained. Acceptance advances the
sector cursor; a two-bit count retains up to two accepted writes until their
real terminal responses. The plane and repeated value remain unchanged until
both are retired. The shared memory endpoint counts data beats. Each sector
still has its own arbitration and completion boundary, allowing Display and
command prefetch to interleave. No payload buffer, BSRAM or DSP is added.

Physical request retention, constant writes and terminal handling run at wall
speed during CE0. The final successful terminal is held in CLEAR_DONE until CE
permits the next command. Abort preserves an already presented request and its
value through retirement. Abort stops building new offers, drains the held offer
and all accepted writes, then cancels.
Recovery uses the existing coordinated-reset contract.

```text
WAIT(data + render)
FLUSH                         // explicit driver cache-coherence operation
CLEAR(color, rgb565_value)
CLEAR(depth, d16_value)
...
WAIT(data + render)
FLUSH
END
```

## Storage, ports and schedule

| Local qwords | Owner |
| --- | --- |
| 0..127 / 128..255 | Two existing1 KiB input payload pages |
| 256..259 / 260..263 | Corresponding metadata |
| 264..319 | Driver global/render staging |
| 320..383 | Four existing128 B uniform blocks |
| 384..447 | Not exposed to CP; logical T workspace selection |
| 448..463 / 464..479 | Two128 B command pages |
| 480..511 | Reserved |

DMA owns both input-RAM write lanes. The decoder owns their read ports between
DRAWs, and yields them to the meshlet controller/V/T through meshlet_done. CP
source reads use the raw input Q, bypassing the unrelated private T-work mux.
Normal V/T operand reads retain their existing path. Commands use a32-bit
instruction register after the synchronous Q return; ordinary commands pay
read, latch and execute edges. State
objects use separate read/consume edges. These short fixed delays avoid a
RAM-Q-to-command-decode-to-V/T-control path. CE0 holds decoder work while DMA
and accepted memory returns continue at wall speed.

The held instruction supplies WAIT's mask and SET's object address/type until
that instruction retires. SET_GLOBAL consumes three64-bit qwords, SET_RENDER
two, and SET_TARGET one. The allocated Global64/Render32/Target32 object formats
are unchanged; their remaining padding is not read by the decoder. The qword
address concatenates the instruction's32-byte block address and a two-bit row
index, with no saved object-base register or address adder.

Two ordered logical data jobs may be accepted. Their source/destination progress
is distinct from retirement: issuing a final request does not release the job.
A command refill can be inserted at an unoffered burst boundary and has priority
there. Refill and data share two physical return contexts. Requests already
presented remain immutable through acceptance, including cancellation.

The two return contexts keep command/data and end-of-job flags. The active
qword destination already lives in the write cursor; only one successor base is
stored, as7 bits in32-byte units. Its two low qword bits are always zero by the
producer's destination-alignment contract. A successful LAST loads that pending
base, or a simultaneously accepted new request's base. Command-page bank identity
comes from the active cursor before its final increment. No active destination
is copied back into a return table and no payload FIFO or burst shift buffer is
present. Return data writes directly to RAM. The RO router keeps its own two
ordered owner/length tags, independently of its held request; texture and DMA
responses do not interleave within a burst.

Source progress and saved work destinations use32-byte blocks. A meshlet header
advances source by the full128-byte prefix even though only32 metadata bytes
are read. The payload address does not depend on header return, so it can be
issued early. Work, uniform, command-page and meshlet validity is published only
at the appropriate successful final return. Meshlet begin and done carry
separate slot identities because different jobs may generate them concurrently.
DRAW retains conservative exclusion while a local DMA is pending; it can overlap
pending meshlet writes only when all such writes target the other input page.

The shared memory router owns return counting. DMA does not duplicate the beat
counter or recompute a base-plus-beat write address. Simulation assertions check
alignment, beat count, LAST and peer errors. Those checks do not gate a write,
ready, CE or publication in either simulation or synthesis. Valid peers supply
the exact declared burst; malformed transport has no hardware recovery contract.

```text
DMA_MESHLET(M0, slot0)
WAIT(data)
DMA_MESHLET(M1, slot1)
DRAW_MESHLET(slot0)
WAIT(data)
DMA_MESHLET(M2, slot0)
DRAW_MESHLET(slot1)
...
WAIT(data)
DRAW_MESHLET(last_slot)
WAIT(render)
FLUSH
END
```

The host expands this sequence; hardware has no Mesh count/loop. Native state
and uniform copies can occur while old R/I/backend tails run, with the required
explicit wait before replacing live global/render state. The command writer
and replay tools emit both the binary stream and its readable disassembly.

## Diagnostics, cancellation and scope

The command factory emits one production datapath. Independent simulation-only
monitors check submission and instruction encoding, source/destination ranges,
bindings, explicit waits, state objects, V/T/R/I scheduling, queue ownership,
static port calendars and memory framing. A violation calls `$fatal`; it does
not first stop a stage, suppress a write, change ready or repair state. Defining
`SYNTHESIS` removes these monitors without selecting a different datapath.
Malformed driver data and internal protocol violations have no hardware
recovery guarantee. The host command/data encoders retain their software checks.

Normal capacity backpressure, pipeline ownership, explicit WAIT, request
retention and transaction completion remain hardware functionality. External
abort remains a debug operation: it stops future work and drains presented/
accepted memory transactions. It does not produce a completion result or clear
busy, and requires coordinated reset. The system Device4 binding ties abort low.

RECV bit1 is a reset-only sticky OR of the three existing valid memory-error
inputs. It observes transport failures without controlling writes, advance,
ready or retirement. It is not command validation, ECC, a checksum, or proof of
pixel correctness. The driver checks it while polling and before presenting a
completed frame. On error it stops new submissions and retains the current CB
and resources; a timeout cannot manufacture completion or return ownership.
The stream MC likewise never replaces a missing physical terminal with a timed
error/LAST. Its timeout is a passive simulation assertion.

For offline diagnosis retain a pre-submission DRAM image, the exact CB and all
indirect assets, non-DRAM initialization and configuration, source/export hashes
and memory-client stimulus. An error-time memory dump alone cannot reconstruct
registers, dirty caches, BSRAM or in-flight transactions. Replay from the known
initial boundary with the same release datapath and passive assertions; CPU/
Display contention requires its recorded stimulus. GPU completion and Display
presentation are separate ownership boundaries. CPU cache clean/pinning,
Display handoff and physical board qualification are not supplied by this port.

Bounded interface checks use `tests/vliw_command.py --export <command.v>
--output <new-dir> --native`; `--system-leaf` also inserts the actual tracked
`CpuV3Gpu` pin binding. They cover two consecutive CBs without ACK, command page
wrap, meshlet overlap, CE0 submission, passive error detection and reset-only
RECV error retention in the monitor-free production datapath.
The shared fitter's `--system-binding` selects the real system constants
(CE=1, abort=0, output_ready=1) and excludes debug observations in both trials.
Its CE input gates the measurement boundary only. The default fit preserves
the independently pausable, fully observed component boundary. Neither scope
includes CPU/Display engines or qualifies a complete board image.
