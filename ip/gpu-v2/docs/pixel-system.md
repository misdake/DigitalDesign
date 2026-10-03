# Controlled pixel composition (J1)

`system::pixel` connects bounded result storage and exact final color to the
existing framebuffer model. Branch results are external test stimulus: lighting
uses its baseline counted model checked against the independent oracle, and
sampling uses its UNORM9 oracle. This composition does not instantiate either
branch's cycle executor. It has no command, geometry, rasterizer, RTL or board path.

## Interfaces and ownership

`Model::new(Context, max_cycles)` fixes a materialized surface, ROP state,
specular RGB and alpha until completion or fault drain. `Tick` supplies one quad,
one lighting result and one sampling result per edge. `Cycle` reports actual
acceptances, store accesses and lifecycle events. No separate output queue is
added: final feeds `OutputRow` into the framebuffer model's existing two slots.

Each nonempty admission reserves one of 16 ordered global slots and all three
payload destinations. `Ticket { quad, serial }` identifies that allocation;
`PixelKey` adds a covered lane. The serial and per-row ownership witnesses detect
stale host injection after wrap. They are simulation diagnostics, not a proposed
wide hardware tag or a change to the six-bit internal pixel key.

| State | Capacity and owner |
| --- | --- |
| Header/join | 16 slots; ingress initializes coverage/defaults; each writer updates its own done mask |
| Basic store | 64 pixels, two logical rows each: RGB24 then D16; ingress alone writes, final alone reads |
| Light store | 64 rows of g9+h9; result injection alone writes, final alone reads |
| Sample store | 64 RGB24 rows; result injection alone writes, final alone reads |
| Basic ingress | One held quad of attributes; at most one row written per enabled edge |
| Final | One lane's work state; one issued-read descriptor/data latch, one reserved return record, one held output row |
| Output/ROP/MC | Existing two output slots, bounded ROP work and one outstanding 128-byte framebuffer transaction |

Payload arrays have fixed host containers (128/64/64 `u32` rows). Effective
fields and reserved bits are enforced by writes. Control, diagnostic witnesses,
ingress and return/output registers are additional state; host layout is not a
fitted RAM or logic budget. Issued and held return records are mutually exclusive.
No dynamically growing queue holds GPU payloads. Test-side lists are finite
stimulus producers and are not counted as GPU storage.

Every store permits one read and one write per edge; same-address collisions
are rejected by assertions. Basic readiness follows its depth-row write. Branch
readiness follows its payload write. Join uses readiness from before the edge,
so a completion write cannot bypass into a read. Only the oldest ready quad joins.
Default-light selects g=256/h=0, default-sample selects RGB=255; neither accesses
its stale store. Mask zero drops before allocation, and uncovered lanes emit
deterministic zero output rows without payload reads.

## Final color and synchronous returns

For each channel, with byte-valued tint, texture and specular color:

```text
base  = nearest_integer(tint * texture / 255)
color = saturate_u8(RNE((base * g + specular * h) / 256))
RGBA  = color plus immutable context alpha
```

The divide-by-255 implementation uses the exact bounded multiply/add shortcut;
the divide-by-256 stage uses round to nearest, ties to even. Inputs outside
g=0..511 or h=0..256 are rejected. RGB565 quantization happens only in ROP.
The final numeric function executes atomically on return consumption: it is not
an audited counted/timed arithmetic calendar or a latency/resource certificate.

A color read captures basic RGB and required branch payloads together; a
separate basic read supplies depth. Reads reserve their destination before issue.
The next wall edge captures the stored row values even under CE=0 or final
backpressure. Only a previously valid return can be consumed, and the resulting
output row cannot be accepted on its creation edge. A held output may leave and
the next read issue on the same edge, using that actual transfer. The return
position is distinct and no future ready credit is predicted.

This deliberately serial final controller has no II2 claim. Workload timings
include admission policy, result injection, CE pauses, misses and flush; they do
not measure lighting/sampling arithmetic throughput.

## Release, completion and errors

```text
payload written -> done -> ordered join -> store return captured
  -> final consumed -> all eight output rows accepted -> global slot released
  -> ROP payload captured (output slot released) -> ROP committed
  -> dirty data written back and ACKed -> render complete
```

Global slot reuse is independent of output slot reuse, ROP line dependencies
and MC transaction lifetime. The output retains everything ROP needs after the
global slot is released. Each branch must send exactly one result per required
lane. Duplicate, uncovered, invalid or stale writes latch a terminal fault;
neither branch write from that edge is accepted.

`finish` under CE stops new admissions and keeps receiving already allocated
results. After final drains, the model requests flush once. `complete()` requires
framebuffer flush completion and idle; the test composition also checks adapter
idle. ROP's existing maintenance continues every wall clock, including CE=0.
Fault stops new work, drains presented/accepted transport, then discards remaining
local state. `drained()` reports this fault path, never successful rendering;
partial external writes are not rolled back. No in-place context switch or reset
is provided. The wall watchdog returns an explicit error; after that bound the
caller must drain external transport separately rather than keep calling `step`.

## Verification and remaining integration

`tests/pixel_system.rs` uses an independent integer final/depth/blend/addressing
golden and compares the complete color/depth byte image, including guards. It
covers masks/default combinations, full credits, more than three slot rotations,
out-of-order branch writes, same-address order, duplicate/stale/default rejection,
real slot reuse, synchronous return reservation, CE/backpressure, cold/dirty
maintenance, fault drain and a permanent-result-stall watchdog. Reversing the
same-address stimulus changes the independent golden, proving order matters.

`tests/pixel_sdram.rs` uses the existing direct `Combination` burst adapter with
the actual serial arbiter/gearbox/controller cycle emulator and pin memory model.
Every wall edge advances this combination exactly once. Complete images and
guards match; all accepted 128-byte requests return/consume sixteen beats and
exactly one terminal response. Reference branch cases include zero and
unnormalized normals, negative components, NDC boundaries, shininess endpoints,
and nearest/bilinear/trilinear sampling with four helper UVs. Branch reference
generation uses external texture fixture memory; only framebuffer traffic uses
the cycle MC in J1.

The next integration requires persistent sampling step/result-ready control and
a single shared MC dispatcher for texture and framebuffer clients. Existing
standalone adapters each own their MC advance and cannot be stepped independently
against one shared controller. J1 needs no new vendor/audited/ROP public API.
Actual branch execution, certified final/ROP arithmetic and render/display
ownership remain separate work. No emu-vs-RTL, PnR or board result is claimed here.
