# Texture sampling models

## Selected render precision

The physical oracle/count/timed/emu/RTL preparation boundary uses unwrapped
S(18,16) UV (eight components per quad), with one RNE at raster/admission.
`capture_uv` preserves UV=1 as raw65536 until derivative calculation; only
afterwards does repeat wrap keep the low16 bits. Exact signed differences are
nineteen-bit integer codes at the same F16 scale, with eighteen-bit magnitudes.
The coordinate shift is `physical_size_log2 - 8`, preserving Q8 texel coordinates
and the existing UNORM9/511 filtering coefficients.

`force_coarsest` is captured per quad and survives the dispatcher, held offers
and actual serial RTL input. An uncovered helper outside the representable
domain uses a benign placeholder and forces the coarsest available mip, even
with negative bias or zero ordinary derivative. The D-to-LOD seam encodes that
flag as slope131073, strictly above the raw131072 derivative-limit threshold.
This is an explicit fallback, not a clamped or wrapped normal derivative.
Covered out-of-range UV is rejected. With no mip chain, the only available base
layer remains selected. Ideal and legacy oracle precision configurations remain
available; `Config::counted()` selects this physical contract.

The live kernels now use the short register boundaries documented under
[Runtime derivative and LOD arithmetic](#runtime-derivative-and-lod-arithmetic).
The selected serial RTL and live emulator controller data bank is332 bits,
including a26-bit LOD decision and compact coordinate metadata.
Register declarations, isolated-leaf PnR, whole-GPU timing and board validation
are separate evidence levels;
historical tables below do not establish the current implementation's timing.

## Current boundary

This component keeps three distinct execution paths: the functional oracle and
counted numerical ledger; the legacy timed/Session/Program replay controller;
and live register execution in `Runtime`, `PreparationEmu` and `SamplerEmu`.
The live path computes from accepted scalar inputs. It does not use the legacy
calendar's completion ages or its retained numerical answers as hardware state.
`texture::ports` owns slot, quad, Group4 and precision configuration records;
`texture::sim::oracle` owns preparation, filtering, a functional cache and an
independent continuous reference. `texture::sim::counted` implements the frozen
UNORM9 datapath with independently replayable numerical ledgers.
`texture::sim::timed` executes the bounded cache/color controller. It supports
the historical static preparation baseline and checked input from
`staged::bound`: universal periodic stage calendars with finite credits,
registered boundaries and optional early shared-context release.
`PreparationEmu`/`SamplerEmu` complement Runtime; the historical performance
receipts below retain their original provenance. Actual leaf RTL
and the composed preparation/cache/color whole-sampler RTL are qualified with
bounded differential tests. Complete serial-sampler fitting is documented below;
complete overlapping Runtime RTL and live DRAW/board integration remain open.
`texture::emu::color` executes the captured-texel color pipeline.
An optional concrete periodic FF-slice layout checks bit ownership at every
actual read edge; the dedicated allocation remains the default reference.

Refills reuse the existing GPU-owned `frontend::ports::MemoryPort` facade,
re-exported by texture. The existing generic test adapter connects it to the
vendor SDRAM `Service` from base commit
`fb472c98c5b4e9f32faac26929cea16534a21b74`, through a dev dependency only.
Each tile requests 128 aligned bytes and consumes sixteen actual little-endian
64-bit beats. Timed uses the GPU-owned `RefillPort` cycle projection of that
same service's submit/step/events. The composition adapter forwards directly;
the vendor service owns all latency/load behavior. There is no texture-specific
memory timing model or production vendor dependency. The bound-stage acceptance
fixture uses the actual `emu::service::Memory`, `Config::default()` serial MC
at source `358a4e9` (local dependency cherry-pick `f6b61ec`). Early grant and
512-byte groups stay disabled; each texture request is still one 128-byte tile.

`Config::default()` preserves the historical precision-study candidate.
`Config::counted()` selects the frozen step-1 contract below. Ongoing timed
targets remain in the local GPU v2 texture specification and development process.

## Row Runtime transport

The published pixel Live adapter selects `Runtime::new_rows`/`step_row`; its
four actual36-bit UV beats use the sole UV tail for reservation. Canonical Raw
writes on row0 and all existing admission owners publish on row3. The caller
holds its descriptor and immutable draw configuration throughout those rows.
Atomic `Runtime::new`/`step_raw` remains a separate compatibility/reference
transport selected at construction, rather than an additional hardware path.
`row_accepted` reports each row transfer; `accepted` reports fourth-row
publication. Coefficient/cache/Color still execute in the same register Runtime.
`rtl::runtime_rows` closes actual Context8, UV, Raw/Ready, D/LOD and compact C6.
Its default statically selects fixed EARLY ordered retirement and the canonical
C owner; generic construction profiles remain references. EARLY C consumption
releases its Context in order, so no additional reclaim pointer or reclaimable
array is stored. C6 retains owner24 plus result36: parent1[9], nearest[1],
level0[4], slot[4], key[6]. Consumption restores parent0=511-parent1,
level1=saturating(level0-1) and last_fine=(parent1==0). The actual LOD domain
qualifies those identities; checked standalone construction rejects malformed
owners. The trusted closed producer keeps the unchanged wide output interface.
The selected Runtime emulator uses the same physical codec and exposes actual
24-bit owner writes/consumes; generic wide references remain unchanged.

The output of `runtime_rows` alone is an external Coefficient boundary.
`rtl::runtime_backend` closes selected Coefficient ready2, short Member,
Work16, short Packet and the ordered Pool through Group72. Its old proposal
readiness does not depend on joined commit. Promised writes reserve Work credit
at Coefficient admission; actual Work acknowledgement releases it. Independent
proposals qualify a common commit with the frontend, without another C-head
payload bank. The composition export is `pixel_preparation_ingress`; cache/Color
and actual pixel-result completion remain external. These actual RTL scopes
are qualified separately from complete overlapping Runtime RTL. Current
source identities, bounded tests and matched fitting scopes are recorded at
`D:/github/DigitalDesign-code/target/pixel-composition-backend-20261010/review.json`.

`pixel_sampling_ingress` extends the actual row Runtime through native texture
cache and result16 Color/public RGB completion. The cache borrows the stable
Pool head instead of retaining another demand payload; one Group72 register is
shared by its paid data read and captured Color row. Refill and paid return
maintenance continue during CE0. Public16 retains valid1 plus last-covered
lane2 per ID; actual last-lane RGB consumption releases the ID. The independent
diagnostic model retains wider coverage tracking as a reference. Color uses
twelve separate MULT9X9 primitives and fabric addition, while D remains fabric.
The selected closed top relies on actual Pool stability and the single UV-tail
row0 reservation to omit repeated address/publication checks. Row-specific
proposals avoid propagating row0 public credit into row3 admission checks. Generic checked
factories remain available; this does not relax arbitrary transport validation.
In this closed top, Work16 reads immutable payload72 directly at reclaim_ptr
from one distributed-RAM read port. A separate control bank feeds emit4 to the
two logical heads, which retain cursor2 and the original fetch/ACK calendar.
Packet E0 captures the same old member; no wide head payload register or selector
is required. Old occupancy still prevents full-queue ACK from financing a put
on the same edge. Generic and compact Work references remain unchanged.
Independent Group/RGB joint qualification, full-scope resource reports and
current timing status are recorded at
`D:/github/DigitalDesign-code/target/pixel-composition-work-20261010/review.json`.
Result/done banks, ordered join/Final/ROP and live Draw/memory ownership remain
parent integration work; these results do not establish whole-GPU timing.

## Independent serial sampler

`sim::staged::bound::serial::PreparationEmu` accepts raw S(18,16) helper UVs,
Q8 bias and a bounded header. Its stepping path runs actual derivative, LOD,
coordinate, coefficient, membership and packet register kernels. It does not
admit a counted `Program`, execute an oracle, retain a numerical template or
schedule already computed answers. Closed counted goldens remain independent
test references. One quad is serialized deliberately; leaf II is not quad II.

| Step | Actual computation and retained boundary | Timing boundary |
| --- | --- | --- |
| Derivative | Shared axis extrema/differences, then exact max/force and header | 3 enabled edges, kernel II8 |
| LOD | Pre-lookup CLZ/normalization/RNE, registered LOG ROM, guarded tail, then levels/parents | 4 enabled edges, kernel II8 |
| Coordinate | Fine/coarse signed coordinates, then fractions and wrapped taps | 2 enabled edges, kernel II2 |
| Coefficient | Three shared one-edge products and exact complement subtracts; Serial has one72-bit ready row and one owner, Runtime retains ready2; single-copy metadata in BSRAM | Capture age5, old-state consumption age6; Runtime kernel II2 |
| Membership | Local 2x2 tap grouping at repeat/tile seams; first/last group identity | 7-edge default, 2-edge short configuration |
| Packet | Canonical key/local/weights/first/last/quad/lane into one held 72-bit word | 9-edge default, 2-edge short configuration |
| Demand cache | Literal slot/level address, tag/PLRU lookup, address/config FF stage, synchronous four-bank tap capture | Hit read captures on the enabled edge after address issue; miss waits for actual beats and terminal ACK |
| Color | RAW565 expansion, independent 9x9 products and registered fabric four-tap sums (independent emulator retains products), then feedback and exact nearest /511 | Queue age2, kernel II1; only last group emits a pixel |

The live serial emulator and selected RTL use332 data and11 control bits:
UV128, pending header54, LOD decision26, fractions16, weights72 and owned
metadata36. Metadata is captured once from
Coordinate and held through the last group, rather than echoed through
Coefficient. Its metadata BSRAM is removable in the serial composition; the
overlapping Runtime and standalone compact coefficient retain it. Member76
and Packet72 are borrowed directly from their existing leaf
output registers. The controller does not issue another job to either leaf
before its last read, and output stalls retain the current packet ownership.
Payload reset is unnecessary once control validity is cleared. `SamplerEmu`
connects the held packet to
`emu::cache::CacheEmu` and then `ColorEmu`, using pre-edge ready/valid throughout.
It retains one quad result bank: RGB96, quad4, mask4, done4 and two control bits.
One completed quad holds under CE/backpressure; its transfer does not fund
same-edge admission. Zero coverage runs no numerical/cache work.

The demand cache has 64 lines in 16 four-way sets, 3-bit tree-PLRU per set,
four synchronous 1024x16 banks, one 72-bit demand head, one captured result,
one read-address/config stage and one miss descriptor. No prefetch or hints are
implemented in this independent baseline. `cache::Allocation` is the independent
emulator's logical inventory, rather than the RTL's fitted FF count; slot
validity is a billed ROM bit, and read ownership pins
the line. A hit is visible only from old READY tags. ACK cannot fund same-edge
read issue; read capture cannot fund same-edge address issue. An old output
transfer can free the later result capture slot.

Each miss presents one aligned 128B request and consumes sixteen indexed 64-bit
beats plus an independent terminal ACK. Same-edge accept/first beat and
last-beat/ACK are supported. Delayed ACK without a beat is supported; last beat
alone never publishes. Memory maintenance runs on wall time under CE0 and
fault. Fault invalidates compute and FILLING publication, retains an already
presented request and drains to terminal. A malformed beat sharing a terminal
still consumes that terminal. Recovery requires recreation, not edge replay.

`emu::refill_bridge::RefillBridge` adapts the existing single-parent `RefillPort`
to this cache. Facade credit reservation is acceptance at this boundary;
physical Started/beat/terminal events remain distinct. It polls without clocking
the physical MC. The framebuffer owner advances the shared Combination once
after Sampling has polled. See [pixel-system.md](pixel-system.md).

`serial::Config` keeps both optimizations off by default. `nearest_bypass` skips
coefficient execution only for effective nearest filtering and supplies exact
511/zero parent rows through the same 171-bit boundary. `short_alignment` selects
the compact two-edge Membership/Packet paths, removing intermediate payload
copies and alignment tails. Their actual declarations are documented under
Runtime membership and packet arithmetic below. The bounded serial tests compare all four configurations against the same
packet goldens and RTL under CE and output stalls. Their fixture spans are not
memory-inclusive latency, sustained throughput or FPGA area. No multiplier
instance is removed by the runtime nearest bypass.

The historical pre-short-pipeline `texture_sampler_emu` matrix measured the
whole registered composition. Twenty repeated full-mask quads include negative/
repeat seams, a nonzero derivative, and (for trilinear) a half-level bias. The
fixture accepts one request/beat per wall edge, delays ACK by three, and uses
CE/output-ready continuously. All four configurations return identical oracle
pixels and leave asset/guards unchanged. The final hot window is actually steady:

| Filter | Default hot edges/quad | Nearest bypass | Short tails | Both |
| --- | ---: | ---: | ---: | ---: |
| Nearest | 240 | 184 | 208 | 152 |
| Bilinear | 296 | 296 | 232 | 232 |
| Trilinear | 504 | 504 | 336 | 336 |

These archived values predate the current short arithmetic pipeline. They are
one-quad serial-controller fixture intervals, not the periodic Runtime
or a shared-MC bandwidth result. Reproduce with `texture_sampler_emu`; exact
cold-return edges, requests and group counts are generated in
[historical exp-117](experiment-review.md#reproduction-and-historical-boundaries) at repository root.
Removing unused alignment ages helps every seam group; skipping coefficients
helps only effective nearest. Neither optimization widens a shared input mux or
changes arithmetic rounding. The next throughput frontier is overlapping real
quad/lane ownership across the existing kernels; arithmetic latency and scalar
controller waits must be separated before choosing more DSPs. The richer
periodic Runtime remains the comparison point rather than being deleted.

`rtl::sampler::{build_with_config,verilog_with_config}` composes the same
preparation, cache and Color leaves without duplicating numerical code. The
default factory remains configuration-identical. Its result bank is108 data
plus two control bits. Raw header slot/max_n/has_mip must correspond to the
immutable slot table: use `derivative::Input::capture` or the checked common
context adapter. The wrapper does not add another slot ROM or a second raw-header
lookup port. Every whole-RTL configuration is checked edge by edge against
`SamplerEmu`, including actual refill bytes, branch accepts and held outputs.

`texture_sampler_emu` checks 48 sequential quads across all filters, masks,
negative UV/repeat/mips, CE and request/result backpressure against independent
counted-contract oracle pixels. `texture_cache_actual{,_rtl}` checks separate
and same-edge ACKs, seams/PLRU/levels, port stalls, abort and numbered beats;
Icarus compares actual RTL and registers on every edge. Actual numerical leaf
RTL is separately covered by `texture_{calendar,coefficient,color,
serial_preparation}_rtl` integration tests and the private `runtime_rtl::tests`
Membership/Packet co-simulations. Arithmetic RTL lowers explicit expressions/registers; the coefficient leaf
now has actual shared multiplier input muxes. Register declarations alone do
not establish Gowin mapping, fmax, integrated GPU timing or board behavior. Earlier Runtime experiments below retain
their original, narrower qualification boundaries.
`texture_sampler_rtl` also exercises all filters/material levels0/1/3/6/10,
missing mips, masks, negative UV, CE/result/request stalls, both ACK seam timings,
more than64 cache lines and identity reuse. The whole wrapper has no external
abort port yet; cache fault/drain is independently qualified and Rust composition
exposes explicit abort/drain. Do not treat wrapper fault wiring as a tested
external cancellation command.

The Sampling schedule page `/web/sampling-schedule.html` reads the IP's certified
leaf calendars through `/schedule-ws` (`profile: "sampling"`). It also runs the
project's actual preparation emulator to export all four optimization state
traces, plus a two-quad cache/refill/color composition trace from `SamplerEmu`.
Its memory fixture is explicitly identified; it is not an averaged MC model.
State traces disable periodic-repeat display: their measured quad span does not
certify a universal initiation interval. Browser code contains no copied model
arithmetic; leaf packing inventories and actual implementation declarations are
identified separately.

## Frozen counted contract

The single signal-format source is [`texture-formats.csv`](../spec/texture-formats.csv).
It contains width, binary point, rounding/range and arithmetic route. The build
script generates typed values, typed stores and ROM literals; no runtime `Fixed`
constructor or division is used. `format.rs` includes that generated source.
UV is captured as unwrapped S(18,16), with explicit helper fallback. After the capture, most
signals carry integer codes (CSV fraction zero): coordinates are Q8 codes,
LOD/bias are Q8 codes and coefficients/colors are separately interpreted UNORM
codes. Explicit slicing and scaling preserve those units.

| Boundary | Frozen behavior |
| --- | --- |
| Helper UV | RNE S(18,16); all four unwrapped edges and both components |
| LOD | 64x8 ROM; `k=RNE(64*(mantissa-1))`; k=64 carries into exponent |
| LOD bias | RNE Q8 at capture; clamp to +/-32 before capture is output-equivalent for this UV/size contract |
| LOD guards | slope >2 forces coarsest available mip; zero slope selects zero; bias then clamp otherwise |
| Mips | Single-layer floor LOD; trilinear linear mip + bilinear; mag is one bilinear layer |
| Coordinates | Floor Q8; n=0/1 both address a physical 2x2 footprint |
| Weights | Nine-bit UNORM 0..511; each pixel sums exactly to 511 |
| Group4 | Actual ordered groups; 72 bits; 32-row numerical scratch FIFO |
| Color | RAW565 replication; 9x8 raw products; 17-bit sums <=130305; one final exact nearest /511 |

`prepare` reads inputs/context through typed stores and derives derivatives,
LOD/index/exponent, mip parents, coordinates, taps, coefficients, first/last,
complete packed Group4 records and 32-bit tile addresses in a single closed
frame. Slot/material validity and allocation bounds are checked at the external
boundary. Mask, filter and all data-dependent arithmetic paths are audited.
The preparation writes each real packet to a 32x72 scratch store; the complete
two-mip repeat-seam test fills all 32 rows. This establishes a numerical capacity
bound for one quad, not FIFO admission/credit behavior across quads.

`sample` transfers the **verbatim finished packet words**, not oracle goldens or
host-reconstructed weights, into per-lane color frames. Atomic cache reads supply
external RAW565 words and the short reserved line identity. Each color frame
audits virtual +1 bank/local addresses, expansion, all twelve products per group,
partial sums, running accumulators and final RGB. The functional adapter still
owns tag/PLRU/replacement/refill state; those controller operations are not in
the arithmetic ledger. Its real 128-byte MemoryPort transactions, beat counts
and warm/prefetch hits are separately tested against the existing SDRAM service.

Implemented reductions:

- LOD, mip parents/lambda, layer size, coordinate shift, tile stride and prefix
  are prepared once per quad. Layer prefix ROM reads are not repeated per group.
- Full-parent row split is `2*fv-(fv!=0)`, leaving two column products per pixel.
  General fractional trilinear requires two row and four column products.
- `lambda=2*f-(f>128)` exactly replaces the 511-by-fraction multiply/RNE.
- Coarse Q8 coordinates use signed `(fine_Q-128)>>1` when fine n>=2; n=1 to 0
  reuses Q. Integer/fraction extraction and constant right shifts use bit slices.
- Final normalization uses `h=N>>9`, an eight-bit carry from `h+N[7:0]`, and
  `increment=N[8] OR carry`; output is `h+increment`. No reciprocal/divider is
  needed. Exhaustive closed-frame audit covers all 130306 valid accumulator codes.

### Counted evidence and limits

`texture_counted` compares every published stage against independently executed
oracle goldens: 2112 mixed preparation cases, 1728 LOD-grid/tie stimuli, bias
boundaries, a full 32-group seam, 336 sampling cases and exhaustive normalization.
Sizes 0..10, full/missing mip chains, all filters/masks, helper-only gradients,
negative UV, UV endpoints, overflow and maximum slot/quad IDs are covered.
Tamper tests remove a DSP count or change an output and require audit rejection.
The 1024 nearest endpoint exposed an insufficient wrap temporary: both mux
inputs exist and 1024+1024 needs 13 signed bits. `WrapWork` now records that guard;
the narrower 12-bit integer coordinate remains valid.

The bounded probe audits 128 quads per profile. With optional existing photo
assets, all ten profiles total 1280 quads. The following are measured work counts,
not resource instances or cycles:

| Profile | Pixels | Groups/pixel | Coefficient products/pixel | Color products/pixel |
| --- | ---: | ---: | ---: | ---: |
| Bilinear / integer LOD | 512 | 1.265625 | 2 | 15.1875 |
| Fractional LOD | 512 | 2.529297 | 6 | 30.351563 |
| Affine mixed LOD | 512 | 2.203125 | 5 | 26.4375 |
| Perspective with mixed masks | 269 | 1.583643 | 3.011152 | 19.003717 |
| Two-mip repeat seam | 512 | 8 | 6 | 96 |

Every group writes/reads 72 payload bits, reads 64 RAW565 bits and issues twelve
9x8 color products, including zero-weight slots. The seam batch performs only
eight cold refills yet 4096 group reads: color work can dominate even with hot
cache data. The four 512-square natural-photo subsets use the same affine
coordinates and produce 1128 groups/512 pixels each. Max/mean RGB-code errors
against the continuous reference are Peppers 1.058388/0.225828, Mandrill
1.077418/0.252385, Sailboat 1.462432/0.241775 and Airplane 0.938736/0.210665.
These are bounded subset results with the new LOD algorithm, not replacements
for the larger historical photo study below.

The generic framework lowers each logical 9x8 product to one Native18/DSP18
operation; the ledger does **not** certify a small Logic multiplier or its area.
Equality tests expand into audited comparisons/adds, and nonoverlapping packet
packing/bank concatenation currently uses counted add operations. Physical
logic/wiring certificates must handle those graphs before sizing ALUs; primitive
operation counts must not be interpreted as dedicated physical adder counts.
The staged companion now consumes structural wiring/equality certificates;
the original static reservation deliberately retains its conservative mapping.
This numerical work ledger does not establish a schedule or FPGA resource result.
The separately checked timed baseline is described below.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label texture-counted -CargoArgs @('-p','gpu-v2','--lib','--test','texture_counted','--test','texture','--test','texture_sdram')
& scripts/run-cargo.ps1 -Subcommand run -Label texture-counted-probe -CargoArgs @('-p','gpu-v2','--example','texture_counted_probe','--','target/gpu-v2-texture-counted','target/gpu-v2-texture-photos/assets')
```

Omit the second probe path for six synthetic profiles without downloaded assets.
It exports `summary.csv`, `operations.csv` and first-case stage goldens. Relevant
tests and strict component clippy pass; only the pre-existing Windows linker
stdout diagnostic remains. No whole-repository checks were run for this step.

## Timed execution and reservations

`Program::compile` captures counted preparation and assigns its primitive graph
to fixed sites using two bounded scheduler candidates. Dependencies, port/lane
budgets, multiplier latencies, ordered packet writes and width-weighted retained
value occupancy are checked independently. Programs are immutable and tied to
their slot bindings; admission rejects stale bindings or incompatible hardware.
`Machine::step` executes the cache/color state, advancing the existing SDRAM
Service by one cycle. A report contains bounded offers, control, responses,
events, snapshots and ordered committed pixels.

There are two explicit input profiles:

- `Reserved` (default): one quad's complete counted primitive reservation at a
  time. Group4 packets release at their actual scheduled writes. A full FIFO
  freezes this preparation's local calendar; cache/refill/color keep advancing.
  These are numerical payloads evaluated by counted plus verified reservations,
  rather than an independent cycle executor of every preparation primitive.
- `PreparedGroups`: externally prepared, counted-verified packets are offered
  at up to one per enabled cycle. This measures cache/color capacity only. It
  does not charge the preparation graph to throughput or establish whole-sampler
  II=2. Immutable program/golden/trace arrays are harness data, not extra queues.

| Site / storage | Default timed declaration |
| --- | --- |
| Group4 FIFO | 32x72; 16 entries available for comparison; one write/read per enabled edge |
| Quad ingress | Four waiting descriptors, one active preparation; 16 live quad IDs |
| Hint FIFO | Eight keys; last three pending keys deduplicated; optional hints can drop |
| Miss directory | Four descriptors including active; one active 128-byte refill |
| Cache | 16 sets x four ways; coherent demand/prefetch tag views; one allocation/edge |
| Data banks | Four 1024x16 banks; one registered read and one refill write each |
| Result credits | 16, covering in-flight last groups and queued results; configurable 1..16 |
| Preparation arithmetic | Three 9x8 lanes; fixed one-cycle logic sites per operation/width; eight helper register reads; one ROM read/site |
| Preparation value budget | 16384 conservative bits; constants/wiring also counted; observed occupancy is reported by the probe |
| Color arithmetic | Twelve 9x8 lanes, II=1, latency 3; nine tree adders; three one-cycle feedback adders; per-channel carry/increment normalization |
| Run bounds | <=256 quads, <=2 million wall cycles, <=12000 preparation events/quad |

DSP packing is checked as one four-lane preparation macro (one lane spare) and
three four-lane color macros, within two DSP tiles. This is a placement
certificate for Multiply9, not a synthesis/frequency result. Color expansion is
bit replication. The color pipeline is statically read(1), multiply(3),
tree(2), feedback(1), normalize(2): nine enabled cycles from bank issue to result,
and a further handshake edge to commit. Continuous one-group samples therefore
need ten credits; four credits alone impose output stalls even with warm cache.
The declared logic stages still require implementation/timing validation in RTL.

```mermaid
flowchart LR
  Q[Bounded quad ingress] --> P[Static preparation reservation]
  P --> G[32 x 72 Group4 FIFO]
  P --> H[8 optional hints]
  G --> T[Demand tag view]
  H --> U[Prefetch tag view]
  T --> D[Single directory commit]
  U --> D
  D --> M[4 miss descriptors / 1 active refill]
  M --> S[Existing SDRAM Service]
  S --> B[4 data banks / refill writes]
  T --> B
  B --> C[Fixed color pipeline]
  C --> O[Reserved result credits / ordered commit]
```

Demand rechecks the logical key when consuming a group. READY ways cannot be
evicted until their short read reservation reaches word capture. Afterwards,
tokens carry the captured words; no way is retained across a whole sample/quad.
FILLING keys merge, demand promotes a pending descriptor, and active bursts
never preempt. A demand miss wins the single allocation; the prefetch view sees
the committed/forwarded allocation. Prefetch replacement protects the waiting
demand head, even under result backpressure. Fill, prefetch and demand touches
apply in that order to the shared PLRU state. A last-beat write cannot be read
from that same line on the same edge; other READY lines continue to serve hits.

Consumer CE gates admission, preparation, tag requests, color and output
commit. The non-backpressurable refill sink and already committed miss work
continue under CE=0, including READY publication and starting pending bursts.
This is an explicit interface boundary, not a freeze of the memory controller.
Last groups reserve output capacity at read issue. On `last`, normalization
uses that token's **post-add** accumulator. A quad ID cannot be reused until
preparation ends and every covered result commits. Slot rebind requires drain.
The FIFO and tokens transfer verbatim 72-bit counted packets; closed color
kernels slice weights and `first` from those packets. Host decoding is used for
controller keys/identity and independent inspection, never to rebuild arithmetic
operands between numerical stages.

Malformed IDs/beat order/last flags, incomplete completion, memory errors and
watchdog expiration cause a terminal sampler fault. FILLING is not published as
READY. An accepted Service transaction remains owned by the Service: drain it
externally before recreating the sampler; a sampler fault never cancels a burst.

### Closed preparation boundaries and historical control proposals

`sim::staged` finishes separate derivative, LOD, coordinate, row, column and
plane numerical frames. Each successor captures verbatim typed outputs of its
predecessors. Balanced derivative reduction, exact 20-bit normalization,
single-boundary tap wrapping and shared X/Y tile comparisons preserve the
existing counted/oracle packets. UNORM9 precision and rounding are unchanged.

`staged::binding` uses the public structural WiringAdd/Equality proofs without
changing the numerical ledger. It checks resource counts and dependency timing;
unlimited primitive lanes are a diagnostic assumption, not a physical schedule.
The historical LOD counterexample rejected a single-output cone that hid a
shared normalization/exponent intermediate. Current multi-output certificates
make escaping results and their retained alias sources explicit.

`staged::stream` executes bounded contexts, coordinate credits, coefficient
reservations and ordered plane/packet control. Fixed three-lane coefficient
issues have period two and are audited against all actual 9x8 products.
Optional shared-context release occurs after the last covered lane captures
its operands. Verbatim lane/plane records retain later operands, while a
separate 16-entry completion table protects live quad IDs until packet drain.
Sparse/empty masks still execute helper derivatives and LOD. CE freezes this
controller; the separate timed cache retains its independent refill contract.

These stage latencies remain **proposals**: the numerical primitives are not
independently executed each cycle, multi-output logic and register cuts are not
yet certified, and record-bit telemetry covers boundary payloads only. This
companion is not connected to cache/color and cannot establish whole-sampler
II, fitted area, production SDRAM-chain benefit or RTL correctness. The existing
`Reserved`/`PreparedGroups` evidence below retains its original meaning.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label texture-staged -CargoArgs @('-p','gpu-v2','--test','texture_staged')
& scripts/run-cargo.ps1 -Subcommand run -Label texture-staged-probe -CargoArgs @('-p','gpu-v2','--example','texture_staged_probe')
```

The probe exports bounded context/release/credit/mask/backpressure comparisons
and checked structural work. Every control CSV row has `latency_certified=false`.
Context release, slot reuse, captured operands, ordering and tampered snapshots
are audited; independent oracle/count comparisons include LOD ties and UV seams.

### Universal preparation binding and cycle-MC composition

This section records the historical primitive-calendar experiment. Its tables
and throughput receipts are not the current live Runtime register contract.

`staged::bound` replaces the proposed latencies with one calendar per kernel.
Memory shapes, literals, operands, control dependencies and value formats must
match across every numerical input; indexed ROM rows may vary. Input-specific
omission of covered lanes/zero-parent planes changes admission work, never the
arithmetic body or its phase. Equality cones and WiringAdd are structurally
proved. The LOD `h`/shift use charged public singleton cones, preserving both
outputs and the original escape counterexample without a fusion discount.

| Kernel | Fixed II | Primitive span | Rotating FF allocation |
| --- | ---: | ---: | ---: |
| Quad derivatives | 8 | 16 | 2440 bits |
| Quad LOD/context | 8 | 27 | 793 bits |
| Pixel coordinates | 2 | 9 | 890 bits |
| Pixel coefficients | 2 | 10 | 419 bits |
| Plane membership | 1 | 6 | 691 bits |
| Group4 packet | 1 | 7 | 674 bits |

These are declared registered primitive latencies, not a verified clock period.
There is a separate holding-register edge after primitive completion; consumers
read stable ready records on a later edge. D/LOD shared-context updates have
the same cuts. No same-edge producer-result bypass is assumed. CE freezes the
enabled phase clock and all preparation/color state; the MC and refill sink
continue on wall clocks. The coefficient body always pays for six 9x8 products
per covered pixel, including bilinear/nearest zeros, on three fixed sites.

FF input fields have continuous fanout. Each retained origin owns rotating
slots derived from its last read, with a checked overwrite distance; wiring
packet aggregates extend their physical field lifetimes instead of allocating
an extra 72-bit register at every concatenation. Shared ALUs and their operand
selection are explicit sites. ROM reads have one port/site and registered
outputs; replication needed for another simultaneous read is rejected.

Default preparation has eight contexts, six coordinate credits, sixteen plane
credits and sixteen packet credits. A last coordinate capture may release the
shared context, but a separate sixteen-entry completion table retains prep ID
ownership until every packet emits. The cache keeps the ID until every covered
pixel commits. The external 72-bit packet port is ready only when the bounded
Group FIFO has space; composition audit checks that readiness, CE, admission,
packet provenance, completion and physical DSP occupancy. It also retains the
independent cache/bank/color audit and compares pixels with the oracle.

```mermaid
flowchart LR
    D[Shared D / LOD] --> C[Coordinate capture]
    C --> R[Release shared slot after last lane]
    C --> W[Coordinates / coefficients]
    W --> P[94-bit plane work / packet expansion]
    P --> G[72-bit Group FIFO]
    G --> K[Cache / registered bank read / color]
    M[Cycle MC and ungated refill sink] --> K
    K --> O[Ordered pixel commit / outer ID release]
```

The cycle-MC probe uses RAW565 size 512, no prefetch, 32 Group entries and 16
result credits. The first 64 quads warm an initially invalid cache; the next 64
repeat that input. The hot window spans commit edges of quads 80..111, excluding
initial fill and final drain. All hot windows below submit zero GPU refills.
Whole-batch values include cold-cache misses and pipeline fill/drain, with the
MC's separately measured 10813 initialization clocks excluded. Units are wall
clocks per covered pixel:

| Profile | 5 early contexts: hot / batch | 8 late contexts: hot / batch | 8 early contexts: hot / batch |
| --- | ---: | ---: | ---: |
| Bilinear | 3.312500 / 3.394531 | 3.250000 / 3.394531 | 2.000000 / 2.503906 |
| Fractional trilinear | 3.109375 / 3.613281 | 3.500000 / 3.863281 | 2.703125 / 3.394531 |
| Two-mip repeat seams | 8.000000 / 8.609375 | 8.000000 / 8.609375 | 8.000000 / 8.609375 |

Eight early contexts are needed for this registered pipeline's full-quad rate;
the five-slot historical proposal does not carry over. Trilinear still pays
variable Group expansion and finite-record pressure. At a seam, eight groups
per pixel force eight clocks even when every tile hits. One/two covered lanes
per quad cost 8/4 clocks per pixel in the hot window because D/LOD still execute
once per quad. A 64-quad cold tile scan costs 7.476562 clocks/pixel, with 64
refills. One bilinear quad takes 142 clocks after MC initialization, or 10850
clocks including a fresh MC start. Four trilinear quads take 221 clocks.

The loaded comparison generates actual 32-byte reads from Display/Instruction/
Data every 128/256/512 clocks, with one outstanding request per client. Eight
early-context bilinear costs 2.519531 clocks/pixel for the batch and 2.000000 in
the hot window. This is bounded synthetic traffic; it does not establish a
production display/CDC margin or board behavior. Optional 512-byte groups are
not enabled or used to infer a gain for isolated 128-byte refills.

The original dedicated `inventory` bills allocations, not boundary-data peaks:
all kernel FF banks, contexts, pass-through/ready records, completion, phase/
queue control, slot/tag/PLRU/line state, miss directory, Group/result FIFOs and
color tokens/tree/feedback. The conservative mapping needs **16469 FF bits for
five contexts or 17690 for eight**, plus four data BSRAMs, eight DSP18 slots,
765 hard-DSP pipeline bits and **92 RAM16SDP4 cells**. The latter consists of
12 ROM, 24 plane-work, 20 four-way tag and 36 Group-FIFO cells. The public
framework's conservative 16x1 composition count is separately reported as 355;
it must not be confused with 355 four-bit primitives. The target-spec's six
Logic/cell accounting gives a 552-Logic RAM base fee before selection/control.

Rotating-bank read selection and stage operand selection also have declared
2:1 tree demand (2619 and 4526 bit nodes), not fitted Logic counts. Arithmetic,
write decode, distinct read phases and queue/control mux costs remain unpriced
in Logic. This deliberately simple FF mapping needs compaction before a viable
target implementation: even the five-slot allocation exceeds the board's total
15552 FF capacity. These allocations are not a minimum bound on another layout.
The 1500-Logic target and frequency are **unverified**; no texture RTL/PnR result
is claimed. Arithmetic goldens remain immutable closed frames evaluated ahead
of the finite cycle controller, rather than an independent arithmetic emulator.

Four new integration tests, the scoped 50-test regression and strict component
clippy pass. Coverage includes all size/filter shapes, missing mips, sparse/empty
masks, context/ID reuse, minimal/maximal credits, CE, result stalls, actual MC
beats during CE=0 and rejection of timing/storage/site/provenance mutations.
The probe exports `performance.csv`, `stages.csv` and `storage.csv` under the
chosen output directory; no full-repository or board validation is part of this
unit.

### Concrete periodic FF reuse

The following allocation table records the earlier calendar; current live
allocations are documented in the Runtime arithmetic sections below.

The optional `control::Storage::Packed` policy reuses bit-addressed FF slices
inside the six preparation kernels. It changes no arithmetic format, stage
II, context release, FIFO credit, cache port or MC behavior. Other allocations
retain the dedicated conservative bill. This bounded comparison uses exactly
two layouts; it does not search for a global minimum or select a fitted winner.

Structural wiring views resolve slices, signed extension, floor shifts, static
input aliases and certified disjoint field concatenation to their original
source bits. Product low bytes and sliced-away UV high bits stop occupying
storage after their last actual read. Constants remain literal wiring at each
consumer, independent of their sampled value. The unused D/LOD base publication
and LOD shift1 diagnostic publication are omitted from physical retention;
cache addresses still read the separately billed, drain-protected slot table.

Each field lists its producer edge and **every read offset**. A deterministic
first-fit allocation assigns contiguous physical FF ranges and a finite periodic
phase bitmap. Overlapping live masks, duplicate births, missing fields, forged
bit origins/read offsets, out-of-range addresses and altered bills are rejected.
Lifetimes include the final read edge: a new producer cannot overwrite an old
value on that edge. Every physical bit has at most one write per enabled edge;
reads use continuous FF fanout and explicitly billed phase-specific selectors.
No RAM port or cache maintenance port is borrowed.

| Kernel | Dedicated data FF | Packed data FF | Periodic live peak | Phase/valid FF | Read mux bit nodes | Write mux bit nodes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| D | 2440 | 1336 | 1159 | 49 | 1191 | 833 |
| LOD | 793 | 277 | 277 | 60 | 248 | 356 |
| coordinate | 890 | 718 | 691 | 18 | 806 | 211 |
| coefficient | 419 | 315 | 315 | 19 | 489 | 200 |
| membership | 691 | 722 | 649 | 15 | 1258 | 142 |
| packet | 674 | 673 | 673 | 16 | 1154 | 579 |

Data FF falls from 5907 to 4041, with **177 additional control FF** for six
CE-gated one-hot phase rings and fixed-delay issue-valid pipelines. Allocation
fragmentation makes membership larger despite its smaller live peak; the bill
uses actual allocated ranges rather than ideal simultaneous-live bits. The
new storage topology costs **5146 read and 2321 write 2:1 bit-mux nodes**, plus
**10871 conservative Boolean gates** for phase/valid enables and per-bit enable
OR trees. Shared enable factoring is not credited. Existing arithmetic operand
selection remains 4526 bit nodes. These are topology demands, not Logic/LUT counts;
arithmetic and non-stage queue/controller logic still require RTL synthesis.

The whole conservative storage bill is **14780 FF with five contexts, 16001
with eight**, versus the dedicated totals above. RAM stays 92 RAM16SDP4 cells
(355 separately reported framework 16x1 cells), four cache-data BSRAMs and eight
DSP18 slots with 765 hard pipeline bits. Macro register compatibility and the
declared soft captures remain unverified by RTL. Large retained allocations
include shared contexts, coordinate/coefficient boundary copies, packet holds
and whole color tokens. They are paid even when a hypothetical borrowed view or
field ring might remove a copy. Five contexts cost measured bilinear hot II
3.3125 instead of eight-context II2; their fractional hot II is 3.109375 instead
of 2.703125. Neither total establishes the 1500-Logic target or spare capacity
for the rest of the GPU. Eight contexts still exceed the device's entire FF count.

Actual issue traces replay every packed bit write/read with producer value ID,
source bit, unique issue clock, expiry and numerical value. CE freezes both
phase and valid calendars. Owner comparisons catch stale data even when colors
match. This checks storage transport from immutable closed-frame goldens;
the goldens are not an independent cycle arithmetic emulator.
Placement tables must remain in the canonical order used by binary search.
Replay visits every enabled edge from zero, including idle bubbles, and rejects
duplicate/skipped edges or issue clocks different from the current edge. Its
clock checker is host diagnostic state, not another datapath register bill.

The probe compares both layouts on the same 30 actual serial-MC cases. All
cycles, packets, refills, queue peaks and pixels match between layouts, including
one-quad/four-quad batches, warm steady windows, sparse masks, seams, cold scans,
loaded traffic and initialization inside the batch. The additional mixed-mode
test exercises ID/context reuse, long result backpressure and MC beats during
CE=0; certificate/address/lifetime/cost mutation tests fail as intended.
The scoped seven-suite regression passes 50 tests, including the integration
review's lookup-order and idle-edge negative cases; component clippy and fmt
checks pass. Observed stage live peaks reach every periodic peak in the table.
`layout.csv` exports concrete addresses/lifetimes, `storage_traffic.csv` records
observed per-stage live peaks and read/write bit traffic, and the three original
CSV files retain the full costs and performance comparison. Reproduce with the
existing `texture_bound_probe` command and an output directory such as
`target/gpu-v2-texture-packed`.

The native input remains a complete quad. The shared interconnect proposal's
four sequential U/V40 lane transfers therefore need a separately verified
reservation/assembly adapter. A simple standalone candidate needs 320 UV FF,
6 key FF and 16 assembly-control FF (342 total), before immutable draw/header
metadata, and accepts one lane every two clocks: at least eight clocks/quad
before the existing native admission and D phase alignment. Alternatively,
reservation could assemble directly in the already billed raw context region;
this needs a receiver reservation API and cannot be claimed as free today.
Likewise the proposed 64x24 public sample-result BSRAM and delayed done-after-store
adapter are not this native result FIFO. The public store is an additional
BSRAM outside the four sampler data banks; one pending key/valid needs seven FF.
Global quad ownership must continue through final output publication.

Further input borrowing needs an explicit parent-region reference, protected
last-read/release acknowledgement and a finite read-port certificate. For example,
a blocked lane record cannot read an early-released context after that context
is reused. Marking such input as a globally invariant constant would hide the
hazard. This unit keeps those copies, does not modify the public audited API,
and stops before arithmetic emulation, RTL or PnR.

Remaining storage decisions use the existing eight-context allocation and trace
credits. Occupied records are not field liveness: the current performance CSV
does not split ready occupancy from in-flight credits or export color/result
field lifetimes. Those missing measurements must not be replaced with ideal
live-bit estimates. No additional layout is implemented in this unit.

| Allocation | Paid FF | Existing trace evidence | Required ports and next decision |
| --- | ---: | --- | --- |
| shared contexts | 3096 | Peak 8 occupied slots; raw/D/LOD fields have distinct lifetimes | Admission, D capture and LOD capture can write different contexts in one edge; D, LOD and lane consumers can coincide. A single 1R1W bank is insufficient without field banks/prefetch/capture scheduling; keep short borrowed fields protected through last read. |
| ready and pass-through | 2021 | Coordinate aggregate credit peaks at 6; 185/594 pass bits cover fixed-latency overlap, 900/342 ready bits cover backpressure | Each ready FIFO has 1W+1R; synchronous RAM needs a reserved return/skid. Separate rounded 150/171-bit RAM16 rows cost 38/43 SDP4 cells at depth16 before control. Pass-through is fixed-delay data, better handled by field lifetimes or explicit taps than an unpriced multiport RF. |
| packet holding | 1152 | Packet credit peak 16 includes pipeline and stable output; it is not 16 occupied output rows | Output storage can use 1W+1R, 18 SDP4 cells for 16x72. Reserve the synchronous return and a stable 72-bit skid; retain output validity during stalls. Stage registers and packet-credit counting still remain. |
| color tokens | 2268 | Nine-token fixed pipeline capacity; per-field occupied peak not exported | Capture/partial/accumulate/normalize act on different ages in the same edge. One whole-token 1R1W RAM is insufficient. Reclaim fields at their actual consumers and keep feedback/markers in FF or separately ported banks; macro-product registers must remain counted. |
| slot table | 1024 | These 30 cases bind one slot; interface capacity remains 16 | 26 reserved ABI bits/entry can be constant wiring. Actual fields total 38 bits/entry. Admission validation and head/miss lookup must be separated or given two read views; a miss-only synchronous lookup is a candidate, not a proved hot-path replacement. Drain-protected writes must remain. |
| result FIFO | 480 | Capacity 16; occupied/field peak not exported here | Native FIFO is 1W+1R and could use eight SDP4 cells plus registered return/hold. The public 64x24 result BSRAM is an alternative protocol adapter, not a free replacement; done and global retirement must follow actual store/output publication. |

Two subsequent structural routes remain for budget discussion: (1) reserve and
bank contexts, borrow protected inputs, and put the independently stalled ready/
packet queues in 1R1W RAM with explicit return credits; or (2) first split color
tokens/pass-through by consumer lifetime and adopt the public result-store
adapter. Route 1 must resolve simultaneous context reads/writes and RAM-return
stalls; route 2 must resolve concurrent color ages, feedback, done timing and
global ID reuse. Both retain eight contexts, current precision and stage II.
The five-context comparison measures the throughput penalty only; it is not
an area-closure solution. Whole-GPU RAM/Logic budgeting precedes either route.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label texture-bound -CargoArgs @('-p','gpu-v2','--test','texture_bound')
& scripts/run-cargo.ps1 -Subcommand run -Label texture-bound-probe -CargoArgs @('-p','gpu-v2','--example','texture_bound_probe','--','target/gpu-v2-texture-bound')
```

### Timed evidence and remaining bottleneck

The independent audit replays controller decisions and also reconstructs
row-major tiles, PLRU victim order, reservations, word coverage, group/sample
ordering, RAW565 expansion, partial sums, feedback and exact /511 output. It
checks queue capacities and the static color/DSP issue calendar. Tampered
payloads, CE, totals, reservation calendars and value budgets are rejected.
Tests compare committed values with the independently implemented oracle,
rather than treating successful replay as a numerical golden.

Nine integration tests and one cache-control test cover sizes 0/1/3/5/9/10,
full/missing mips, filters/masks and quad-ID reuse; calibrated-average and both
chained/unchained services under solo/display/CPU/combined load; FIFO saturation,
hint drops, descriptor saturation, same-set pollution/refetch, pending-demand
promotion, non-preemption, short reservations, output backpressure, CE,
malformed beats, binding changes, configurable latencies/capacities and bounded
permanent stalls. The dedicated
prefetch test requires READY reads on the **same edges as actual refill beats**.
The bilinear probe additionally records 119 such simultaneous edges.

The bounded probe uses 64 quads/256 pixels per profile, starts cold and includes
pipeline fill/drain. These rows use the calibrated-average Service, prepared
Group4 input and 16 result credits; numbers are cycles per pixel:

| Profile | 16 entries, no hints | 16 + prefetch | 32 entries, no hints | 32 + prefetch |
| --- | ---: | ---: | ---: | ---: |
| Bilinear | 2.234375 | 1.710938 | 2.234375 | 1.488281 |
| Fractional trilinear | 3.640625 | 3.082031 | 3.640625 | 2.710938 |
| Sequential cold tiles | 7.046875 | 5.816406 | 7.046875 | 5.816406 |
| Two-mip repeat seams | 8.796875 | 8.742188 | 8.796875 | 8.742188 |

For 32 + prefetch, bilinear issues 320 groups/381 cycles (83.99% group-site
utilization) with ten refills; fractional issues 608/694 (87.61%) with thirteen.
Four result credits worsen bilinear to 2.625000 cycles/pixel, with 316 credit
stall cycles. Under the configured display+CPU unchained Service the four
prepared-input profiles take 3.851562, 5.062500, 8.515625 and 11.105469
cycles/pixel respectively. Queue depth cannot create refill bandwidth or reduce
the seam's eight groups/pixel. Optional existing Peppers/Mandrill/Sailboat/Airplane
assets each produce 256 committed pixels bit-identical to the oracle.

The default **Reserved end-to-end baseline** takes 76.285156, 128.539062,
76.253906 and 147.546875 cycles/pixel for those same profiles. Preparation
coefficient-lane utilization is only 0.87..1.56%; observed retained values peak
at 1977..2798 conservative bits. Serial quad preparation and literal primitive
graphs dominate, including equality expansion and field/address concatenation.
The ready/write calendar is input-path-specific and not a universal issue ROM.
No optimized II=2 preparation, small-logic lowering, cross-quad preparation
pipeline or complete-system throughput has been established. The next
optimization must certify legal comparison/wiring lowering and context lifetime
before replacing this conservative baseline; its cycle counts cannot be hidden
by presenting the prepared-input results as end-to-end sampler performance.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label texture-timed-control -CargoArgs @('-p','gpu-v2','--lib','texture::sim::timed::tests')
& scripts/run-cargo.ps1 -Subcommand test -Label texture-timed -CargoArgs @('-p','gpu-v2','--test','texture_timed','--test','texture_counted','--test','texture','--test','texture_sdram')
& scripts/run-cargo.ps1 -Subcommand run -Label texture-timed-probe -CargoArgs @('-p','gpu-v2','--example','texture_timed_probe','--','target/gpu-v2-texture-timed','target/gpu-v2-texture-photos/assets')
```

Omit the final path to run without existing photos. `summary.csv` records
profile, memory mode, preparation mode, capacities, workload, occupancy,
utilization and distinct stall counters; `overlap.csv` records simultaneous beat
and read edges. These counters can overlap and must not be summed as disjoint
causes. Validation remains confined to texture and the related SDRAM/modeling
paths; no whole-repository audit or board/RTL/PnR result is claimed.
Work stops here for discussion. Timed must execute the 32-entry queue, optional
prefetch, FILLING merge, demand recheck/reservation, READY hits during refill,
CE/backpressure and fault/drain behavior before concurrency can be claimed.

```mermaid
flowchart LR
    Q[Four unwrapped helper UVs and mask] --> L[Shared LOD]
    L --> C[Coordinates and conservative coefficients]
    C --> G[Ordered same-tile Group4 records]
    G --> K[Logical key lookup and atomic read reservation]
    K --> M[128 B refill through MemoryPort and SDRAM Service]
    M --> B[Four RGB565 banks]
    B --> A[Raw products and sample accumulator]
    A --> R[One final RNE and ordered lane results]
    Q --> F[Independent continuous floating sampler]
```

## Executable behavior

- Up to sixteen slots: aligned base, full-mip flag, size exponent 0..10 and
  validity. Invalid slots, overflowing 32-bit allocations, material/slot size
  disagreement and invalid tile keys are errors. Memory additionally checks
  fitted physical capacity. Mips are small to large, tiles and texels row major.
  Layer prefixes use a fixed lookup.
- For n<3, the asset must supply periodic 8x8 RAW565 payloads. Logical n=0 uses
  a virtual 2x2 footprint of equal values; missing padding is never invented.
- Quad UV is signed, unwrapped and row-major. All four horizontal/vertical edge
  differences participate in max-norm LOD, matching the triangle oracle. The
  mask selects output lanes only. Helper derivatives precede periodic wrapping.
- Filtered coordinates use `u*a-0.5`; nearest uses `floor(u*a)`. Negative UV,
  repeat seams and smallest-mip saturation are supported. Nearest texel and
  single-mip selection are separate policies. Missing mip chains use base only.
- Bilinear splits row then column with floor and conserved total; trilinear
  splits mip parents first and folds their weights into eight taps. Total weight
  is exactly the configured scale (`2^F` or `2^B-1`). There is no final two-mip lerp.
- Group4 retains four tap positions and zeroes other tiles' coefficients.
  Zero-only groups are omitted before rebuilding first/last. Groups are
  contiguous per sample, in first-tap order within a mip, finer mip first.
  The checked baseline 72-bit packing is for inspection, not an external ABI.
- Bank is `{y[0] XOR x[1],x[0]}`; local index is `{y[2:0],x[2]}`. Virtual +1
  coordinates precede bank selection, including small mips and tile crossings.
- RGB565 expands by bit replication to UNORM8. Raw products accumulate across
  every group; only the final sum is divided with ties-to-even. Baseline weight
  sum 256 bounds each accumulator to 65280. Higher-precision experiments use
  unrestricted host integers. Each covered lane produces one result.

## Precision controls and stage goldens

| Control | Default candidate | Experiment range |
| --- | --- | --- |
| Input UV quantization | RNE, 17 fractional bits | None, or 0..30 bits |
| Coordinate fraction | Floor, 8 bits | 1..16 bits |
| Coefficient unit | 256, 8 fractional bits | Binary 2^F or UNORM 2^B-1, 1..16 bits |
| LOD output | RNE, 8 fractional bits | 1..16 bits |
| Log2 method | 64x8 table, floor mantissa index | Exact CPU log2 or table |
| Single-mip selection | Nearest, half selects coarser | Floor or nearest |
| Derivative range | Difference magnitude <=2 | Positive finite limit |

Host inputs must be finite with absolute UV <=2^20. Finite derivatives beyond
the configured limit force coarsest available LOD regardless of bias; zero
derivatives select LOD zero. Otherwise bias precedes clamp. Table entries are
oracle-generated `RNE(log2(1+i/64)*256)`. The counted implementation owns
the selected immutable 64x8 table. Its nearest-grid indexing and frozen derivative
limit/single-mip policies are specified below; physical placement remains timed work.

`PreparedQuad` records UV, helper differences, rho, overflow, exponent/table
index, ideal/selected/raw LOD, mip parents, coordinate floors/fractions, taps,
coefficients and Group4 records. `Output` adds raw texels, expanded channels,
partial sums, post-add accumulator, RGB and cache events. These are stage goldens
for future comparison, never imported into audited arithmetic.

The independent `reference` uses continuous UV, exact log2 and standard floating
bilinear/trilinear weights. It reads linear payloads and independently derives
mip offsets, addresses and color expansion. It does not reuse bank addressing,
Group4, conservative splits or quantized LOD. The shared assumptions are the
RAW565 asset contract and max-norm LOD convention.

## Functional cache and memory boundary

Sixteen sets/four ways, four 1024x16 banks and tree-PLRU are modeled functionally.
Lookups use `{slot,n,tile_x,tile_y}`; prepared groups own no physical way.
Invalid ways precede PLRU replacement. Accepted demand hits, consumed prefetch
hits and complete refills touch PLRU. Rebind invalidates state at a synchronous
call boundary; stale tags and replacement bits need no reset.

Cache operations are serial host transactions: allocate FILLING, receive sixteen
beats, write banks, then publish READY. Group taps are captured before another
allocation, providing functional short read protection. The returned host Vec
is an unconstrained oracle sink, not a hardware buffer proposal. Service faults
are terminal: retain FILLING, publish no result and prohibit allocation/rebind.
Drain or recreate the service before starting a new cache; a watchdog must not
silently cancel an accepted refill or recycle its destination.

`prefetch` consumes an explicit optional hint atomically. Later demand rechecks
its logical key after eviction/invalidation. Hint FIFO/drop, dual tag reads,
same-cycle commit/touch ordering, FILLING merge, descriptor capacity/promotion,
CE/backpressure and hardware sink credits are future timed/emu work. Atomic
oracle calls do not certify those concurrent behaviors or predict cycle counts.

## Validation and reproduction

Bounded tests cover all 65536 bilinear fraction pairs; trilinear endpoints;
one/two/four/eight groups; seams; periodic small mips; all 64 bank/refill words;
constant colors; final-round ties and per-group-rounding failure; mip saturation;
missing mip chains; masked helper LOD; table error; input faults; PLRU/prefetch/
rebind; truncated payloads and service watchdogs. Independent float comparison
uses 256 deterministic quads. Both debug and release profiles are checked.

SDRAM integration compares one independent asset under calibrated average
service, Solo/Display/CPU/Both load and both unchained/chained policies. It checks
every returned word, sixteen beats before READY, completion counts, no new
traffic after warming, and unchanged backing data. This is Rust service evidence.

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label texture-oracle -CargoArgs @('-p','gpu-v2','--test','texture','--test','texture_sdram')
& scripts/run-cargo.ps1 -Subcommand run -Label texture-probe -CargoArgs @('-p','gpu-v2','--example','texture_oracle_probe','--','target/gpu-v2-texture')
```

The probe scans twenty configurations across 216 bounded pressure/random quads
(2592 output channels). An optional second argument selects size_log2 9 or 10
(512x512 or the default 1024x1024). Coordinates and helper gradients specified in
texels scale with that size; the deterministic random seed is unchanged. The
stress asset generates each mip independently, so size comparisons change the
source colors and are not comparisons of one downsampled photograph.

`precision.csv` reports max/mean RGB code error and LOD error against continuous
input; `stages.csv` exports named baseline goldens. `worst.csv` identifies each
configuration's maximum by case, lane and channel; `worst_taps.csv` exports its
ideal/actual tap colors and weights; `worst-detail.txt` contains the full stage
trace. `fixed_case.csv` compares configurations at the baseline's fixed maximum
input, avoiding attribution from aggregate maxima whose locations can change.
Reports stay in the requested worktree `target/` directory, not version control.
The mip-varying asset stresses filtering: these are sampled errors, not exhaustive
image-quality bounds. Increasing one precision can move conservative split
boundaries and need not monotonically reduce worst-case error.

Nine-bit coefficient experiments retain the default UV, coordinate and LOD
settings unless the configuration name says otherwise. UNORM9 has 512 values
including zero and unity, uses denominator 511, and conserves a total of 511.
`coefficient_fraction` selects its storage width B when the encoding is UNORM.
It fits the four existing nine-bit coefficient fields in `pack72`. The binary
alternative has 512 nonzero values plus zero and denominator 512. Its
`pack76_zero_mask` stores 1..511 directly, unity as code zero, and four extra zero
mask bits distinguish zero from unity. `zero_mask9` and `coefficient9` have the
same numerical result; the former names the tested storage alternative. The
functional cache consumes decoded weights. Neither encoding changes the default
configuration or freezes a counted/RTL ABI. UNORM final normalization uses RNE
division by 511; the binary alternative uses RNE division by 512.

Tests exhaust all 65536 bilinear coordinate fraction pairs for both new scales,
check trilinear conservation across the LOD fraction range, preserve black,
white and primary-color assets exactly, and distinguish all 513 binary weights
through the 76-bit packing. Exported ideal/actual tap contributions reconstruct
the independent reference and oracle accumulator, respectively.

```powershell
& scripts/run-cargo.ps1 -Subcommand run -Label texture-error-1024 -CargoArgs @('-p','gpu-v2','--example','texture_oracle_probe','--','target/gpu-v2-texture-error-study/1024','10')
& scripts/run-cargo.ps1 -Subcommand run -Label texture-error-512 -CargoArgs @('-p','gpu-v2','--example','texture_oracle_probe','--','target/gpu-v2-texture-error-study/512','9')
```

See [SDRAM integration](sdram-memory-controller.md) for service ownership and
assumptions. Discuss precision/LOD policy and implementation details before
starting counted work.

### Natural-photo study

The offline photo tools use four original 512x512 RGB photographs from the
[USC-SIPI miscellaneous collection](https://sipi.usc.edu/database/database.php?volume=misc):
Peppers (4.2.07), Mandrill (4.2.03), Sailboat (4.2.06), and Airplane (4.2.05).
Downloaded TIFFs, source URLs/hashes, generated assets and reports remain under
`target/gpu-v2-texture-photos`. No external images are checked in.

`texture_photo_assets.py` builds coherent mips by recursive RGB8 code-space BOX
downsampling, then quantizes each layer to nearest RAW565 values. Small layers
are padded periodically. Pillow/NumPy versions and asset hashes are recorded in
`assets/manifest.json`. Both the Rust float reference and quantized sampler read
the same asset; reported differences measure sampler precision rather than
RGB565 encoding loss relative to the source photograph.

The bounded Rust probe uses UV18, eight coordinate fractional bits and the
unchanged floor-indexed LOD table, comparing baseline /256, UNORM9 /511, and
binary9 with zero mask /512. Each photo has 183272 quads / 733081 active pixels:
seven full-image scales (640, 400, 256, 160, 80, 20 and 3 pixels per side) plus
16384 deterministic random affine quads spanning magnification and fractional
LOD minification. Repeat-border footprints are reported separately from image
interiors in `precision.csv`; `summary.csv` combines both scopes. Full-size PPM
renders, maximum sample traces and unamplified comparison figures are retained.

The Python report independently decodes all 1398784 RAW565 asset words, compares
2670180 rendered reference pixels with a planar-mip float sampler and replays
all 24 scope/configuration maxima from the original helper UVs. Maximum float
reference disagreement is zero at those samples. Twelve rendered channel codes
differ by one only at floating-point half-code rounding boundaries (distance
from the halfway value <1e-8). `independent-check.json` records these checks.

Observed maximum / mean RGB8 code errors, including repeat-border samples:

| Photo | Baseline /256 | UNORM9 /511 | Zero mask /512 |
| --- | --- | --- | --- |
| Peppers | 3.374 / 0.241 | 2.468 / 0.235 | 2.468 / 0.234 |
| Mandrill | 2.848 / 0.272 | 1.977 / 0.261 | 1.977 / 0.258 |
| Sailboat | 3.438 / 0.249 | 2.402 / 0.239 | 2.348 / 0.238 |
| Airplane | 3.375 / 0.200 | 2.518 / 0.195 | 2.518 / 0.194 |

For both nine-bit alternatives the largest measured error is Airplane case
179835/lane2, at UV (0.6726631784860123, 0.4248005344153869), with reference RGB
(89.51257753621461, 76.51823864168183, 118.26701445680673) and output (87,74,117).
Ideal LOD is 4.0432075410609905 and table LOD is 4.0234375. These are sampled
errors for this corpus, not an exhaustive image-quality bound. The study does
not measure concurrent cache behavior or advance counted/timed implementation.

```powershell
python ip/gpu-v2/examples/texture_photo_assets.py target/gpu-v2-texture-photos/assets
& scripts/run-cargo.ps1 -Subcommand run -Label texture-photos -CargoArgs @('-p','gpu-v2','--example','texture_photo_probe','--','target/gpu-v2-texture-photos')
python ip/gpu-v2/examples/texture_photo_report.py target/gpu-v2-texture-photos
```

### Arithmetic choice and sharing opportunities

The accepted target coefficient encoding is UNORM9. This is a Logic estimate,
not a synthesis result: it keeps the Group4 width at 72 bits (36 RAM16SDP4 cells
for 32 entries, versus 38 for a 76-bit zero-mask representation), represents
unity directly in nine bits, and needs no zero/unity mask decode. Accumulators
need 17 bits for a conserved bound of 255*511=130305. Division by 511 is not a
general divider: exact nearest rounding over that bound is

```text
h = N >> 9; l = N & 511
RGB8 = h + (h + l >= 256)
increment = l[8] OR carry8(h + l[7:0])
```

Since `N=511*h+(h+l)` and `h+l<=764`, `(h+l+255)/511` is either zero or one,
with threshold 256. The implementation needs a small carry calculation and an
eight-bit increment per channel. The final target format remains distinct from
the historical /256 default used by regression tests; UNORM9 studies explicitly
select the accepted encoding. Hardware formats/scheduling remain unimplemented.

`prepare` now computes level, two mip identities, lambda and parent weights once
per quad, outside the lane loop. UV quantization, all eight helper differences
and LOD were already shared. Pixel coordinates and coefficients remain separate.
For eight-bit LOD fraction f, `RNE(511*f/256)=2*f-(f>128)`; a full-parent split
uses `floor(511*f/256)=2*f-(f!=0)`. Both eliminate generic multiplication.
Coordinate size scaling and wrapping are bit operations in a fixed-point target.
If Q is a fine-mip filtered coordinate floored to eight fractional bits, the next
mip coordinate is exactly `floor((Q-128)/2)` for n>=2. For n=1 to n=0 both physical
addressing dimensions are two, so Q is reused unchanged. Negative values require
an arithmetic shift with adequate signed width before wrapping.

The diagnostic work module observes oracle goldens and reports structural
opportunities. **It is not the staged counted model, a schedule, or a performance
measurement.** It counts actual groups/nonzero coefficients and unique integer
multiply operands, tile keys and texels within each quad. Unique products are an
optimistic bound requiring additional comparison/storage; they are not free.
Color datapath slots count twelve per Group4 regardless of zero coefficients.

The photo probe writes `<photo>-work.csv`. Geometry determines these counts, so
all four photos give identical work profiles for the same sample inputs.
`texture_work_probe` additionally observes 32768 bounded synthetic perspective
helper quads, with w gradients of 2% or 30% and mixed masks. All four helper UVs
are evaluated from affine homogeneous numerator/w fields; the study does not
assume perspective UV is itself affine.

| Scenario | Generic coefficient multiplies/pixel after simple reductions | With perfect quad operand reuse | Reusable fraction | Groups/pixel |
| --- | ---: | ---: | ---: | ---: |
| Axis-aligned 256-pixel image, integer LOD | 2.0000 | 0.5000 | 75.00% | 1.2656 |
| Axis-aligned 400-pixel image, fractional LOD | 6.0000 | 4.9968 | 16.72% | 2.5072 |
| Random affine quad | 5.4235 | 5.4015 | 0.406% | 2.1893 |
| Mild perspective quad | 5.7971 | 5.7731 | 0.414% | 2.3141 |
| Strong perspective quad | 5.8677 | 5.8383 | 0.502% | 2.2254 |

General bilinear preparation needs two column multiplies; general trilinear
needs six split multiplies. Color work is up to 12/24 nonzero tap-channel
products per pixel; splitting into Group4 consumes more physical product slots.
The 400-pixel scene uses 30.0864 slots/pixel, of which 23.6532 are nonzero
(78.62%). Zero slots do not by themselves justify reducing the physical twelve
color multipliers or reducing the three coefficient multipliers needed for the
two-phase trilinear preparation target.

Prioritize unconditional quad scalar sharing, exact strength reductions,
zero-parent omission and existing bounded prefetch deduplication. In the
400-pixel scene, Group4 keys fall from 401152 references to 123072 distinct
quad keys, a potential 69.32% hint reduction. Perspective scenes show potential
59.75%/61.14% reductions. These unique-key bounds do not prove a three-entry
recent-key filter captures every duplicate. Demand must still recheck the
logical key at consumption; an old physical way cannot be held across eviction.

Do not add general coefficient memoization: perspective cases save only about
half a percent before paying for that state. Texel gathering can also reuse
38.5%/42.9% of nonzero references in the perspective cases, but needs buffering,
bank arbitration and read protection while per-lane color products remain. A
lower read count therefore does not establish a throughput improvement.

The n=0 layer is a separate optional opportunity guaranteed by the asset
contract: constant-only sampling can forward its color, and its weighted partial
in a two-layer sample can be computed once per quad. It occurs in 2843/3419
pixels of the two perspective sets (about 6.1%/7.4%), so any future bypass must
account for context lifetime, Group4 ordering and first/last across the FIFO.
Other small mips are not constant. No gather/memo/bypass architecture is added.

The work examples exhaust all 130306 valid accumulator values, all 256 LOD and
full-parent fractions, and 262145 signed coordinate stimuli to check the exact
strength reductions independently. Texture/SDRAM regression remains unchanged.

```powershell
& scripts/run-cargo.ps1 -Subcommand run -Label texture-work -CargoArgs @('-p','gpu-v2','--example','texture_work_probe','--','target/gpu-v2-texture-work')
```

### Bounded storage transport probe

`texture_storage_probe` is an independent study. It leaves the native bound
controller, numerical contract and previously recorded MC performance unchanged.
It exports real native field lifetimes and post-edge occupancy separately from
inclusive last-read occupancy, then checks three bounded transport components:

- Raw/wrapped input has one arithmetic UV window, a rescheduled derivative
  calendar, physical periodic FF owner/value replay and separately owned
  metadata/LOD ports. Its downstream lane-credit returns are a fixed-latency
  fixture, so its input interval is not a composed GPU throughput measurement.
- A 64-row packet pool uses one producer write and one synchronous head read,
  preserves the 16/32 logical credits, and counts pending/head within Group
  occupancy. Tests include row wrap, CE, long consumer stalls, terminal fault
  concurrent with capture and a partial actual serial-MC burst drained under
  fault. The single head deliberately has no return-to-consumer bypass.
- Color uses finite field registers and independent integer arithmetic checked
  against native partial/accumulator/RGB goldens. Its public result write precedes
  done, and global slot reuse waits for final's synchronous capture. A dense
  consumer-stall stimulus reaches all 16 global slots.

`allocation.csv` is one provisional replacement ledger, not a fitted resource
result: planned lane/plane/slot adapters are marked explicitly. RAM16SDP4,
FF, hard DSP registers and the separately budgeted public result BSRAM are
distinct. The raw and packet calendars expose throughput obstructions; their
individual successful replays do not establish a composed II or area closure.
No full staged arithmetic emulator, replacement cache implementation or RTL is
added.

```powershell
& scripts/run-cargo.ps1 -Subcommand run -Label texture-storage -CargoArgs @('-p','gpu-v2','--example','texture_storage_probe','--','target/gpu-v2-texture-storage')
```

### Per-lane storage reservation increment

`texture_storage_increment` first replaces the raw probe's four-ticket cohort
fixture with a finite 16-row lane controller. Each actual wrapped R reserves one
row and its return destination. Only the last required lane capture releases its
raw context; an unissued lane can pause while older stages and acknowledgements
continue. Coordinate/coefficient credits stay six, coefficient-ready credit two,
work credit sixteen and raw contexts eight.

Relative to coordinate capture C, geometry/operand W is C+10, coefficient R is
C+11 and its capture C+12. Weights W is C+23; member R is C+24 and full 171-bit
head capture C+25 returns the row. Coefficient R uses odd phases and member R
even phases of the same operand port. Head consumption starts C+26, with the
second plane on C+27. Source reservation uses accepted events, never predicted
future releases. CE freezes all local pending returns and fixed calendars.

Bilinear sustains quad II8 in this isolated fixture. Two-plane sampling reaches
work credit sixteen: edge85 cannot reserve two more work tokens, and the current
cohort pauses its next wrapped R until two actual acknowledgements arrive.
Arithmetic is still closed-frame golden data. Work acknowledgement is a bounded
external endpoint, not the native plane/packet/cache composition. Sparse/zero
coverage, signed UV, CE, ring reuse and equal-value stale-owner rejection are
checked. `lane_cost.csv` retains the previous control allowance and charges
additional references. Work admission reads sixteen paid per-row flags captured
from the actual LOD last-fine bit, rather than looking ahead at golden plane
counts or borrowing the geometry bank's member read port. Head consumption uses
its captured last-fine bit to end the lane. `lane_selectors.csv` separately
charges port and flag selection.
`lane_controls.csv` records finite counter/compare transitions awaiting lowering;
these declarations are not fitted Logic or an area closure result.

```powershell
& scripts/run-cargo.ps1 -Subcommand run -Label texture-storage-increment -CargoArgs @('-p','gpu-v2','--example','texture_storage_increment','--','target/gpu-v2-texture-storage-increment')
```

### Two-head packet storage increment

The same increment example separately checks two owned 72-bit head locations.
The original probe retains its single-head baseline. Payload banks still have
one R and one W; producer credit stays sixteen and Group credit thirty-two,
including queued indices, pending returns and captured heads. A read reserves
an empty target, full synchronous capture returns its row once, and actual
consumer issue returns Group credit. The consumer sees only a previous edge's
valid head. Full-Group producer readiness uses pre-edge occupancy.

The hot trace consumes packets on edges 12/13/14, sustaining one per enabled
clock. Edge 12 consumes head0, captures head1 and reserves the next head0 read.
All 1024 seam packets match their source goldens through sixteen ring wraps.
CE, long miss-style stalls and a sudden stop preserve both owned locations;
normal stalled cases reach P16/G32 and forty-six live rows. No third head, extra
credit, extra bank port or return-to-consumer bypass is used.

Fault at edge 12 suppresses consumption but still captures the reserved return;
reset waits through the last local W at edge 19. A separate accepted serial-MC
burst faults after beat 3 with both heads occupied, then drains all remaining
twelve beats, including beats under CE=0, before reset. Six injected violations
must fail at their intended checks: early index publication, early row release,
stale equal-value reply, pending reset, occupied-head reservation and return-edge
consumption. These tests retain the terminal drain/recreate boundary.

`packet_two_head_cost.csv` pays the extra payload, valid, target and pointers,
as well as payload/valid selection; `packet_two_head_controls.csv` lists capture
enable decode and finite credit logic. The former bank output reservation is
retained, without a third soft payload buffer. Mux nodes are not fitted LUTs.
`packet_two_head_summary.csv` distinguishes enabled clocks from wall clocks;
its sampled interval spans consumption indices 32 through 96 and can include a
stall. Ready is explicit stimulus here, so this is not a native cache/MC CPP
measurement. Native storage replacement and full GPU integration remain separate.

The two increments add 178 declared FF bits and 169 bit-mux nodes. Their combined
provisional inventory is 6873 FF bits, 156 RAM16 cells and eight sampling BSRAMs;
it still includes planned adapters and is not a fitted resource result. Lane and
packet throughput are measured independently and do not establish sampler II2.
Increment validation is limited to GPU v2 regressions, the finite probes, strict
clippy and formatting; no new CPU/system co-simulation, RTL or PnR run is claimed.
The lane stale-owner injection must execute and hit its designated owner check.

### Connected packet pool, cache and color

`bound::system::run_pooled` connects the existing bound preparation to a 64-row
72-bit pool and two synchronous heads, then to the actual cache/tag/data and
color pipeline. `run` retains the native storage path for comparison. Producer
credit sixteen includes all packet calculations and written rows waiting for
index transfer; the preparation controller cannot allocate a second credit
domain. Group credit thirty-two includes indices, pending returns and heads.
Pool rows are reserved on actual packet issue, written from its finished output,
released on full synchronous capture, and Group credit is released by cache Read.
There is one payload W and one R, with no return-edge consumer bypass.

Runtime key, coordinates, UNORM9 weights, first/last and quad/lane are decoded
from the captured payload. An admitted quad's four-bit slot survives preparation
release until cache/color retirement, and the live lane/immutable slot bindings
validate consumption. Golden Programs check payload/order but supply no Group
semantic fields to the external consumer. A test corrupts their semantic Group
fields while retaining the payload and proves the actual decode still governs.

The connected probe uses actual serial refills to warm lines. Its middle window
commits 64 pixels, has no refill submissions in the three hot cases, and reports
packet service separately from pixel/quad throughput:

| Hot profile | Native clocks/pixel | Pool clocks/pixel | Pool clocks/quad | Pool clocks/packet in window |
| --- | ---: | ---: | ---: | ---: |
| Bilinear | 2.125 | 2.25 | 9 | 2.0870 |
| Two-plane | 2.65625 | 2.65625 | 10.625 | 1.3934 |
| Seam expansion | 8 | 8 | 32 | 1 |

In the pool's bilinear/two-plane windows, respectively 75/48 clocks have no old
head to offer; none has a ready head waiting for cache, and producer credit is
never full. The existing preparation/input and transport availability therefore
limit those measured windows. Seam expansion sustains all 512 Reads in its
512-clock window. Bilinear's added handoff/rephasing cost remains visible;
this correct connection does not claim an unconditional quad II8.

A recovery profile holds result readiness low, freezes consumer CE periodically,
then changes to a previously cold tile. It stalls safely, accepts refill beats
under CE=0 and eventually commits every result. All eight native/pool runs match
independent oracle RGB. Additional tests cover slot changes, masks/zero coverage,
quad reuse, corrupt return ownership/data and forbidden capture-edge consumption.
The slot regression alternates distinct texture assets when reusing the same quad
ID and checks Reads after preparation release. Return negatives inject a wrong
pending owner and altered backing-row data at capture; a separate trace mutation
checks replay rejection.

The actual inventory removes `packet holding records` and `Group4 FIFO` in pool
mode. It retains all other bound preparation/cache/color storage, adds the two
payload BSRAMs, and conservatively pays short descriptors in FF, heads, pending,
pointers and slot lifetime state. Index FF selectors and head selection are
listed separately in `packet_selectors.csv`; write enables/compare/decode still
need lowering. These rows describe this connection, not the earlier complete
storage replacement plan or fitted Logic. No queue-depth scan or broader storage
replacement is required to merge this unit.

```powershell
& scripts/run-cargo.ps1 -Subcommand run -Label texture-packet-connected -CargoArgs @('-p','gpu-v2','--example','texture_packet_connected','--','target/gpu-v2-texture-connected')
```

The output directory contains actual packet/cache/commit edges, hot blockers,
native/pool allocation rows and the selector declaration. Miss absorption, more
storage conversion and arithmetic cones are subsequent independent work.

### Persistent sampling connection

`sim::staged::bound::session::Session` owns the existing preparation, packet
pool and cache machines plus the independent ColorEmu across consecutive
`step` calls. Its constructor accepts a finite controlled input sequence;
it is not a live CP/quad-dispatch port. Actual acceptance advances that input
sequence. Preparation still replays closed-frame audited arithmetic; it is not
an independent numerical preparation emulator. The cache program checks
packet provenance only; actual captured texels and decoded packet bits drive
ColorEmu. The closed cache/color commits remain an explicit diagnostic/control
shadow, not public results or permission to reuse a public quad ID.

The unit fixes P16/G32, the existing 64-row 1W/1R packet pool and two total
head/pending positions. Cache read latency is fixed at one enabled edge. The
existing CE-gated cache return stage retains its own reservation; this unit
does not claim a new wall-clock BSRAM-return implementation. One captured input
register (packet72 + texels64 + valid1 = 137 bits) holds until a real ColorEmu
acceptance. If occupied, it gates preparation/cache advancement while the single
RefillPort owner continues draining accepted MC/refill traffic on wall edges.
There is no added packet/texel FIFO and no queue-depth search.

Actual public result consumption clears one bit in a 16x4 lane-ownership mask
(64 bits); neither closed-color shadow commits nor preparation completion free
these identities. Result credit stays with ColorEmu's existing Result16 until
actual consumption. Added link state is 137 + 64 + terminal-fault1 = 202 logical
bits, excluding diagnostics and finite input/program fixtures. Existing models
and ColorEmu retain their own declared state; these are Rust bounds, not fitted
FPGA resource results. Session errors are terminal and require recreation.

`tests/texture_sampling_step.rs` covers cold misses, nearest/bilinear/mip/seam,
CE, prolonged result backpressure with refill drain, stable held results,
full ColorEmu credit, and reuse with different RGB data only after actual
consumption. `examples/texture_sampling_step.rs` writes bounded workload/trace
evidence with an explicit warm measurement window, separating packet, pixel
and quad intervals. Texture and framebuffer still require one shared MC owner
before system integration; their standalone adapters cannot advance a common
controller independently. No preparation RTL, full sampler RTL or board proof
is implied.

### Runtime quad ingress

The separate `system::pixel::quad_sampling_ingress` connects the actual Sample8
queue directly to `uv_rows`, preserving unwrapped helpers without an outer
three-row assembly. Its Context allocation and canonical Raw-bank endpoint are
parent-owned. The atomic Runtime model below is still a separate interface;
the row composition does not yet qualify a complete overlapping Runtime top.
The ingress and shared Lighting composition boundaries are documented in
[Pixel composition](pixel-system.md#composition-ingress-units).

`sim::staged::bound::runtime::Runtime` adds caller-owned live input to the
existing bounded path. `step_raw(memory, Option<&RawQuadInput>, Control)`
accepts signed18 Q16 helper UVs, coverage and result ID plus stable draw fields
(slot, material size, filter, signed16 Q8 bias and force-coarsest). Raw bias is
saturated in integer arithmetic at +/-8192, matching the legacy boundary.
`step(QuadInput)` remains a quantization-only adapter. Both report pre-edge
`input_ready` for that ID and actual `accepted`; acceptance requires valid,
ready and CE. Rejected input is not captured or validated beyond identity, so
the caller may replace it before acceptance. Ready excludes same-edge context/result credit release. A held color input
may coexist with raw admission when `ColorEmu::input_ready(last)` proves that
it can be consumed using old result credits on this edge; an empty link slot
is no longer required. The admission predicate and Color execution share this
check, so admission cannot promise an edge on which the link forces a hold. No complete-quad FIFO or
constructor input sequence is added. The finite `Session` API stays compatible.

Actual `FixedCoefficientEmu` consumes captured scalar coordinates and weights;
its local numeric clock can hold while actual membership and packet pipelines
drain on base CE. Work uses
mutable 94-bit SSRAM rows and two 92-bit registered heads. Asynchronous RAM
data and head-valid capture together on R; consumption is on a later edge,
as detailed below. Packet W and public color consumption remain
later, separate transfers.

Derivative, LOD and coordinate now advance actual scalar registers on the shared
Binding; the coordinate bank consumes the captured UV/LOD scalars and publishes
its own taps/weights. Live admission captures boundary fields only: it neither
compiles a Program nor computes future lane values, packets or packet totals.
Constructors extract input-independent structural calendars once. Legacy
Session/Program studies retain their independent provenance; Runtime never
constructs or retains those per-quad objects. `Stats.admissions` counts actual
acceptance; compatibility `compilations` and `peak_preparation_programs` stay
zero, and `peak_live_quads` reports bounded preparation IDs.

Coordinate issue visits accepted covered lanes in order. Actual LOD/coefficients
choose active planes; membership derives the emitted tile mask from registered
weights and coordinates. Packet E1 derives first from the fine plane/tap and
last from the final-plane flag and absence of a higher emitted tap. Only the
actual packet W with last clears that lane in preparation's remaining mask.
All bits clear releases preparation; mask0 releases after its real D/LOD path.
The cache independently checks admitted slot/lane, lowest remaining covered
lane, first/open continuity, last closure and an old producer reservation.
Premature/duplicate completion faults; no expected packet count is consulted.
Public result ownership still lasts until actual ColorEmu output consumption.

Color's feedback is unsigned17 and its four-product partial is unsigned19;
their20-bit sum cannot overflow. The sum-domain fault check therefore covers
every partial-domain failure at the same edge, allowing RTL to omit the
duplicate three-channel partial comparison. The emulator retains its precise
diagnostic message; terminal fault, atomic port suppression and all numerical
results are unchanged. Native fault/semantic replay and matched leaf fits are
recorded in [historical exp-216](experiment-review.md#reproduction-and-historical-boundaries).

Runtime selects the cache's capture-only pipeline: one pending packet72/line6
with valid1 survives from read issue to the next enabled capture. The four bank
words then enter the existing one-slot Color input register; the line pin is
released at capture. CE or a blocked Color input holds that pending read and
pin while accepted refill responses continue to drain. One capture and the next
read may share an edge, retaining a one-packet-per-edge handoff.

Only the separate timed/Session reference allocates numerical color tokens,
partial/sum values, feedback accumulator, result FIFO and result credits.
Those owners do not exist in the Runtime cache variant, and it emits no
Partial/Accumulate/Result/Commit events. Actual Color consumption explicitly
clears the cache's remaining lane bit; its quad releases after both preparation
completion and actual last-result consumption. No shadow age or delayed commit
queue substitutes for that event. Runtime's Step no longer exposes cache_commits;
the independent Session reference retains that diagnostic.

The retained preparation packets-left6/ID ceiling now holds remaining-mask4.
The cache's old validation-cursor6/ID allocation is replaced by remaining4,
open1 and live1; the Rust legacy fields are unused on this path. This alternative
allocation fits the existing receipt without extra FF/RAM/DSP claims. It is
not evidence of fitted hardware. Runtime adds no serial raster ingress or
complete-quad FIFO; the upstream foundation owns that transport.

The cache owns the sole public-result remaining mask, cleared only by actual
ColorEmu consumption; Runtime reads that mask for its diagnostic snapshot.
`Runtime::Step::result_quad_last` exports the transition to an empty mask on
that actual consumption edge. Pixel composition uses this sole completion
event to release the public-ID destination mapping after the result write;
preparation release and Context reclamation do not release that mapping.
Readiness is sampled before Color advances, so a last result cannot fund
same-edge ID reuse. Packet E0 exposes its actual operand key6 directly to the
reservation adapter. It does not reconstruct a physical lane from a diagnostic
covered-lane ordinal, and no accepted-coverage copy is retained in Runtime.
Runtime issue diagnostics use `KeyIssue` with that physical key; the independent
legacy `Issue` keeps its covered-lane ordinal. Canonical key is `quad*4+lane`;
Packet72's quad bits66..69 and lane bits70..71 must be decoded separately.
The existing preparation completion mask guards host issue invariants; this
does not claim a free multi-read hardware table or a fitted Logic reduction.
`LINK_STATE_BITS` is 138: capture137 + fault1. The
existing preparation/cache/color stores and P16/G32, Pool64 1W/1R, two total
head/pending positions, read1 and Result16 remain owned by their existing models.
Texture precision and cache capacities are unchanged. RefillPort drains accepted
beats even when CE or the color link stalls. Errors are terminal; accepted MC
work needs external drain before recreation. These are Rust bounds, not fitted area.

Runtime preparation now advances an explicit ordered Context queue controller
(`emu::runtime_frontend`), with live UV, D/LOD phase and Ready-metadata ports.
Allocate/reclaim and D/LOD/C heads/counts, held D/LOD owners and one 2-bit active
C-head covered-lane ordinal belong to this controller. Done or empty input clears
that ordinal; no per-context cursor array or diagnostic shadow survives. Payload and public IDs remain in their
existing owners. Admission uses old credits; same-edge reclaim cannot fund
reuse, and CE0 freezes control while a native UV return can arrive externally.
Its hand-written queue RTL is qualified against actual Runtime ports and the
frozen pre-change executor, projecting only expired/non-C-head ordinal diagnostics.
All functional ports, payloads and other owners remain exact. This is the queue subunit, with no complete closed
frontend or overlapping Runtime top yet. The RTL classifies each owned row once
as Raw, Derived or Ready; each legality port selects that one bit, preserving
all fault predicates and the same register state. Qualification and isolated probe have
one report home:
`D:/github/DigitalDesign-code/target/sampling-runtime-rounds-20261010/select-review.json`.

The fixed Runtime cache uses one demand descriptor. Only its strict global
Packet head can allocate, and that head remains Filling until physical terminal
completion; there are no prefetch hints. Thus pending plus presented/active
ownership cannot exceed one. The descriptor keeps its line/key/address through
presentation, Started, sixteen ordered beats and Complete, including consumer
CE0 and a delayed terminal event. The selected path has no descriptor queue or
promotion selector. Generic timed and Session keep their prefetch-capable queued
reference. This changes neither cache read scheduling nor the warm II2 path.
Runtime allocation counts one descriptor and transport state as a model ceiling;
the generic queued reference retains its separate four-descriptor allocation.
Actual Runtime/physical-MC per-edge comparisons, independent pixel goldens,
source proof and fault/drain boundaries are recorded in
[historical exp-350](experiment-review.md#reproduction-and-historical-boundaries); they do not establish
a fitted cache-controller saving or complete Runtime/GPU hardware qualification.

`new_runtime_capture` selects the trusted ordered PacketPool backend
(`emu::packet_store_ordered` and its independent RTL), with unchanged Producer16,
Group32,64 physical payload rows and two total owned head/pending positions.
Reservation is the actual Work-to-Packet E0 capture. Every reserved producer
advances on the same effective CE and writes exactly at E2, in capture order,
without holes, cancellation or overtaking. This permits ordered cursors and an
unwritten count to replace the expected-owner RAM and publication bitmap.
The two native512x36 payload banks, their hard72-bit DO and two72-bit soft heads
remain. CE0 holds the paid DO and its reservation; reset invalidates ownership
and macro output without clearing backing payload RAM.

W cannot fund same-edge Transfer, Transfer cannot fund same-edge payload R,
and full Producer/Group credits cannot borrow a simultaneous release. Old head
consumption may reserve a future read but cannot bypass a newly issued read.
The reachable invariant is `rows = Producer + Group - valid_heads`, with
`Producer <= 16`, `Group <= 32`, and at most two head/pending positions. Thus
Producer readiness implies row capacity. Consume uses an old valid head; an
unreserved write is rejected before shared advancement. The real
`rtl::runtime_packet_backend` combines compact Work16, short Packet and Pool64
on this shared edge, masks accepted malformed entries and parent abort, and
retains terminal state until local reset. Short Packet ready is `ce && !fault`,
so a healthy actual shared-edge issue always has Packet readiness. Reset of
this local backend does not cancel any external MC work.

Generic `emu::packet_store`/RTL retain the expected-owner memory and original
owner/key diagnostics. Generic timed Pool and Session retain arbitrary producer
timing and their tamper checks. They do not select this trusted producer profile.
The selected profile does not promise arbitrary internal control/RAM bit-flip
detection. Event serials and pending-row diagnostics are host witnesses, not
additional payload copies or readiness inputs.

Current compact Runtime preparation, cache, packet, Color and public events
match the retained generic Pool and independent integer pixels on84 calendars.
The selected Pool's portable/native RTL and the original Work/short Packet/Pool
backend replay those calendars; the generic-owner backend uses the identical
backend calendars. The final Work mapping is checked by generic/compact Work
tests, one actual default backend calendar in portable/native RTL, and bounded
source-versus-placed-netlist replay. Directed tests check full credits, wrap, CE0 paid DO, live-state reset,
entry-fault and parent-abort atomicity, then new payloads against an independent
Packet golden. Reproduce the relevant leaf tests with
`cargo test -p gpu-v2 --lib ordered_pool_directed_portable_native -- --ignored`.
Actual backend scripts and receipts are under
[historical exp-353](experiment-review.md#reproduction-and-historical-boundaries).
The actual-key/sole-mask composition is recorded in
[historical exp-352](experiment-review.md#reproduction-and-historical-boundaries). The final Work mapping
uses asynchronous distributed rows and two explicit payload registers.
Source-matched native fits and placed-netlist payload replay are recorded in
[historical exp-260](experiment-review.md#reproduction-and-historical-boundaries) and
[historical exp-262](experiment-review.md#reproduction-and-historical-boundaries). These qualify the
Work/Packet/Pool backend; the complete overlapping Runtime controller remains
outside that fitted and mapped unit.

A Runtime terminal error stops compute, not the independent transport owner.
That owner retains a presented request's descriptor until physical acceptance,
then keeps the accepted burst through all16 beats and terminal completion.
It may discard old returns only while draining the old owner; it must not route
them into a recreated Runtime. The bounded parent test faults both before RO
acceptance behind Display traffic and after a partial accepted128B burst,
externally drains to physical idle without resetting transport or reloading the
image, and only then reconstructs Runtime and checks new pixels. Reproduce with
`cargo test -p gpu-v2 --lib terminal_parent_fault_drains_presented`.

Matched native backend fits use the same observable wrapper,54MHz and placement
zero. Both generic-owner and selected ordered variants preserve asynchronous
Work rows. Their resource/timing report is
[historical exp-260](experiment-review.md#reproduction-and-historical-boundaries); the
explicit-head candidate and its source-matched baseline are in
[historical exp-262](experiment-review.md#reproduction-and-historical-boundaries).
The final placed-netlist replay covers477 wall edges including first write/fetch,
concurrent transfers, CE0 paid returns, two heads, stalls, wrap and complete
drain. It compares actual owned Member/Packet/Pool data and control using vendor
primitives and GSR; its receipt is in that task's
`actual-calendars/postmapped-selected-native-p0-diagnostic/receipt.json`.
Complete overlapping Runtime control RTL, whole Runtime/Serial/Quad/GPU
resources, miss/seam throughput and board proof remain separate; backend
differences are not added to Serial totals.

One immutable texture context lasts for the instance; drain and recreate before
changing slots. All four helper UV lanes affect derivatives for partial coverage;
only covered lanes yield key6/RGB24. Mask0 drains without results/refills.
Default-sample is upstream constant bypass (`None` offers no sampler work).
`tests/texture_runtime.rs` checks live replacement, same-instance different-RGB
reuse, partial/helper/default/mask0, actual P16/G32/Result16/16-ID limits, CE and
committed refill drain under sustained backpressure. `texture_runtime_probe`
writes drained warm-window wall/enabled calendars and actual event traces.
Its raw-port run preserves all sixteen first-stage event/edge traces byte for
byte across eight hot/paused cases, with zero live compilations. Additional
raw-input tests generate 64 quads after successive acceptances, overlap two
stable slots, exercise every coverage mask and extreme UV/bias/force fields,
then invoke the unchanged numerical oracle after drain. Direct cache tests
reject early/duplicate completion, wrong or uncovered lanes/slots, missing or
repeated first, repeated last, and duplicate admission using only live ownership.

### Runtime Work transport and allocation qualification

Logical Work capacity W remains 2..32, with physical depth
`max(16, next_power_of_two(W))` and modulo-W pointers. One R and one W can target
different rows. The producer fills the complete row before publishing it;
neither that write nor a same-edge ACK supplies new read/row credit. The two
heads form a fixed SPSC ring, not an allocator. R samples asynchronous SSRAM
into an old empty head and publishes its valid bit at that edge. Only an old
valid head may be consumed; there is no R-to-consume bypass or extra C edge.
Another published row may be prefetched while the current head expands.
The source row stays reserved through its last packet capture/ACK.
The RTL explicitly selects distributed RAM for the rows so Gowin preserves
the asynchronous read. Inferring synchronous BSRAM changes the first fetch
edge and violates this transport contract.
The two payload heads are explicit registers with one fixed output mux and
separate writes selected by the existing fill-head bit.
Coefficient admission reserves one or two Work
destinations from old free capacity. Its existing two ready rows are consumed
one active plane per base edge, releasing the whole row after its last plane.

The configuration-matched Runtime receipt replaces replay arithmetic banks
with the actual live owners, for either storage policy. Coefficient state uses
its `ALLOCATION` declaration; Membership/Packet use the short leaves below.
Work declares184 payload bits and10 head-control bits, plus three modulo-W row
pointers and materialized countQ: `194 + 3A + Q`, where `A=ceil(log2(W))` and
`Q=ceil(log2(W+1))`. The two-head selector covers94 payload/cursor bits.
Generic preparation/cache control remains conservative. The current Runtime
receipt replaces the old color tokens/tree/result/DSP rows with the one-slot
79-bit pending read; actual ColorEmu is the sole numerical/result owner.
This removes logical model allocations, not measured physical cells. The
event/port receipts are under [historical exp-252](experiment-review.md#reproduction-and-historical-boundaries);
the totals are not a fitted whole-sampler bill.
Legacy Session inventory and replay timing do not certify the locally held path.

`rtl::work` is an independent asynchronous-RAM/two-head RTL leaf, checked against
Work on every edge at logical depths 2/3/16/17/32. Random CE/consumer stalls,
logical/physical wrap, all fifteen nonempty emit masks, distinct
payloads, simultaneous read/write/consume and a separate FIFO semantic golden
are covered. Reset suppresses all transfers, clears ownership and pointers,
and preserves payload RAM; occupied reset followed by different replacement
rows is checked. The isolated saturated single-group test sustains one row per
enabled edge. This leaf is not yet a complete overlapped sampler RTL top, and
no Gowin fit or frequency result is claimed.

The archived two-head transport experiment supplied the same48 full-mask quads
after a same-instance cache warm-up. In the middle window (result indices
63 through 127), the old/new actual return intervals are:

| Case | Old result gaps | Two-head result gaps | New window edges/pixel |
| --- | --- | --- | ---: |
| Nearest | 64 x 3 | 64 x 2 | 2.000 |
| Bilinear | 64 x 3 | 64 x 2 | 2.000 |
| Mip | 64 x 6 | 56 x 2, 8 x 10 | 3.000 |
| Seam | 64 x 6 | 63 x 4, 1 x 108 | 5.625 |

All windows have zero new refills and preserve independently calculated pixels.
Nearest/bilinear shared-context lifetimes for accepted quads 16..31 fall from
95 to 63 edges with the same eight contexts. At that earlier checkpoint mip/seam still had long gaps;
neither their window average nor the filled/drained batch mean is a steady II.
In particular the old seam packet stream already had mixed 1/3-edge gaps.
Do not diagnose every seam packet as three edges or solve an unmeasured
bottleneck by adding contexts. Raw before/after calendars and lifecycle data
are frozen under [historical exp-236](experiment-review.md#reproduction-and-historical-boundaries), including the source
snapshot used for that first stage. The subsequent raw-ingress change and its
separate receipts are under [historical exp-240](experiment-review.md#reproduction-and-historical-boundaries).

The current short-pipeline/ingress experiment supersedes those earlier hot-flow
intervals. For warm, non-seam, true two-mip traffic, its four-nonzero-tap packets
sustain one packet per enabled edge; all 511 gaps between the 512 pixel results
are two enabled edges in each measured W configuration, with eight contexts
and the existing32-group capacity. The final trace, Work lifetimes and separate
queued versus earliest-return latencies are in
[historical exp-237](experiment-review.md#reproduction-and-historical-boundaries); this event evidence
is separate from isolated-leaf PnR and whole-GPU acceptance.

### Runtime derivative and LOD arithmetic

`DerivativeEmu` uses the square-edge partition `{0,3}` versus `{1,2}`. Per axis,
`max(max(A)-min(B), max(B)-min(A))` is exactly the largest absolute difference
on the four square edges. One shared axis datapath computes diagonal min/max,
two signed19 cross differences and their maximum on age0 for U and age1 for V.
Age2 selects their exact nonnegative18-bit maximum and force-coarsest guard.
The emitter checks both cuts' operations, formats, constants and bit connections
before sharing them on disjoint one-hot phases. The independent integer emulator
still executes the certified cuts and retains their separately allocated results.
A global UV range would also include
diagonals and is not equivalent. `LodEmu` has four boundaries: guarded CLZ/normalization/RNE
lookup preparation, registered LOG return, bias/clamp/guards, then levels/parents.
Normalization uses CLZ directly instead of two cancelling subtractions.
Zero/overflow priority, mip/filter selection and UNORM9 rounding are unchanged.
The shared-axis rewrite and matched complete serial sampler fits are recorded
in [historical exp-065](experiment-review.md#reproduction-and-historical-boundaries); declaration
budgets remain distinct from fitted registers and complete sampler resources.

| Actual kernel | II | Ready age | Numeric FF | Phase/valid FF |
| --- | ---: | ---: | ---: | ---: |
| Derivative | 8 | 3 | 219 | 8 + 4 |
| LOD | 8 | 4 | 83 | 8 + 5 |
| Coordinate | 2 | 2 | 188 | 2 + 3 |

These are the independent generic calendar declarations. The selected serial
`rtl::lod` replaces CLZ followed by a variable shifter with grouped16/8/4/2/1
zero tests and fixed shifts. It produces the same RNE six-bit ROM index and
exponent carry, reads the maintained `format::LOG_RAW` table once, then applies
signed bias/guards/clamp and derives levels/parents. Its declared numeric bank
is80 bits, with a three-bit phase and four validity bits. Checked admission
limits size to0..10 and bias to[-8192,8192]. Size cancels between the raw LOD
and the final fine-level subtraction: the selected path computes the signed
Q8 offset `(h-16+carry)*256+log+bias` directly. Its five-bit exponent ranges
from-16 to1; retained bias and offset use fifteen bits. A negative offset
derives the fine level and fraction directly; size only supplies the clamp
bound. At the lower boundary, distance equal to size retains a nonzero
fraction, while distance greater than size clamps to size and fraction zero.
This preserves precision and overflow/zero guards. Four advancing edges
including capture and II8 are unchanged; CE holds both payload and validity,
and reset drops ownership without resetting payload. The generic `LodEmu`
remains the reference. An independent integer/analytic golden checks every
18-bit slope code, power boundaries, all supported size/filter/mip combinations
and bias extremes, with input poisoning, output CE pauses and occupied reset.
The matched16-slot report is [historical exp-239](experiment-review.md#reproduction-and-historical-boundaries).

`CoordinateEmu` first computes signed fine/coarse Q8 coordinates, then fractions
and wrapped taps. Its input is wrapped16-bit Q16 UV, side2..1024, signed
`shift=log2(side)-8`, coarse side=max(2,fine/2), and halve iff fine side>2.
The signed floor, centered `-128`, negative repeat wrap and1024 endpoint rules
are unchanged. The generic diagnostic RTL scale uses a five-bit barrel distance with the complete
signed18 amount guard: large positive shifts return zero, and large negative
shifts sign-fill. Thus all amount codes retain their previous arithmetic
behavior, including the most-negative code; legal shifts remain -7..2. Its
calendar, allocation and independent integer emulator are unchanged. It uses
no multiplier, ROM or BSRAM. The full18-bit control replay and matched complete
leaf area/timing comparison are in the same arithmetic report.

The serial composition selects `rtl::coordinate` for this checked physical-mip
domain. Since legal shifts are-7..2, scaling is `(uv<<2)>>(2-shift)`, with one
shared unsigned18-bit right shifter and a four-bit distance0..9. Repeat uses
`floor(q/256) & (side-1)` and `(floor(q/256)+1) & (side-1)`; it preserves negative
centering and the side1024 endpoint without signed boundary comparisons.
Fine coordinates are computed once per axis and coarse coordinates derive
from their centered half. Admission computes U and writes its final56-bit
result slices; only V input16/configuration16 survive to the second edge,
when the same kernel writes V's final56 bits and publishes the joint result.
The selected `SharedCoordinateEmu` and RTL own144 numeric bits plus the
unchanged phase2/valid3; no temporary U result or precomputed V is retained.
The generic188-bit calendar/`CoordinateEmu` remains an independent reference.
II2, two advancing edges,
phase refusal and CE/reset validity are unchanged. Off-valid numeric output
bits are unspecified. Production LOD/Ready decoding establishes the legal
relationships; the generic full-domain lowering is still available for
diagnostics. The exhaustive RTL test visits all65536 UV codes on both axes for
all ten physical sizes and both nearest policies against integer multiply,
floor and modulo goldens, including pauses and reset with different operands.
The test poisons all external operands after U capture; both the Rust and RTL
executors must compute V from their owned registers. Shared-axis resource and
two-placement54MHz receipts are in [historical exp-217](experiment-review.md#reproduction-and-historical-boundaries).

LogicCone certificates retain every pure arithmetic event and explicitly export
escaping intermediate results. Exported aliases retain their producer bits;
undeclared escapes, missing operands, overlap and corrupted timing are rejected.
The executor reads external operands from old registers, evaluates internal wires
combinationally, then commits the single boundary write. Literal/format/wiring
metadata survives construction; sampled nonliteral values and FrameReports do
not. Poisoned-constructor and independent arithmetic tests enforce that boundary.

LOG reads issue at age1 and return at age2. The64x8 registered table is declared
as32 RAM16x1 /8 SDP4 accounting units. Runtime never consumes `prefix0/1`, so
prefix lookup is separated from the live LOD stage; it remains in the complete
counted model and cache address path. Diagnostic differences, LOD/lambda and
coordinate values remain audited without forcing additional registered stages.
Every numerical bank, phase and valid register holds under base CE0.

Fixed reset phases align consumers with captured old state: D phase0, LOD phase4
(eligible at global enabled phase4), and Coordinate phase0. D capture at age3
allows LOD issue on the next edge. After LOD capture at age4, the UV RAM request
uses the next odd enabled phase and Coordinate consumes its return on the next
even phase. There is no live rephasing. The directed paused test records
D/LOD/Coordinate issue at enabled8/12/18. The extra D edge consumes the previous
UV parity wait, so first Coordinate issue stays at enabled18. Context count8 and
steady quad II8/pixel II2 are unchanged. This Runtime evidence remains distinct
from the serial RTL fit; complete overlapping Runtime RTL is still absent.

Runtime now uses one bounded context ring. Admission/reclaim and the D/LOD/
Coordinate heads are modulo-capacity cursors; counts describe disjoint intervals
of the same eight slots, not three eight-entry queues. D and LOD each retain one
inflight slot owner and publish from the actual numerical valid return. No quad
search, free-slot search or absolute issue timestamp controls readiness.

Long-lived UV now occupies one native SDP36 block: four36-bit rows/context,
32 useful rows for eight contexts, with the full512x36 macro charged. Formal
admission captures one144-bit raw head. That same head supplies D and writes
one36-bit UV pair on each of four enabled edges, beginning with admission; it
is released only after both the final write and actual D capture. Input ready
uses the old empty head, so an unaccepted offer is never sampled and a same-edge
head release cannot fund admission. D reads the accepted head, never a second
per-context FF copy. Its wrapped output is exactly each raw operand's low16
bits; there is no D-return RAM write. Coordinate requests one covered-lane row
on an odd enabled phase and reads those low16 slices from the synchronous DO.
The reserved return is captured on the following wall edge including CE0.
It can directly supply the even-phase Coordinate issue; otherwise a32-bit
return head holds it until actual consumption. The owner remains reserved
through both RAM flight and that hold, preventing overwrite.

Runtime context metadata uses only Raw36 and canonical Ready26 fixed
asynchronous-read SRAM banks, each with16 physical rows for logical capacities
1..8. The selected `context_store::runtime::Store` and `runtime_source` RTL
retain9+7 RAM16SDP4 cells. Derived53 is a wire from D's actual terminal bank;
its former padded56-bit RAM is absent. Generic `Store`/`source` keep arbitrary
Ready83 and9+14+21 cells; `new_lod_ready`/`source_with_lod_ready` keep the
independent canonical three-bank reference. No generic caller changes format.

Ready26 retains fine level4, nearest1, coarse parent9 and quad/mask/slot12.
The coarse level is `max(fine-1,0)`, each physical side is
`1 << max(level,1)`, shift is `max(fine,1)-8`, the fine parent is
`511-coarse_parent`, halve is `fine>1`, and last-fine is `coarse_parent==0`.
Capture checks every expanded operand and rejects noncanonical or out-of-range
returns before any mutation. General callers are never silently truncated.
The logical83-bit edge/diagnostic interface remains unchanged; expanded fields
are combinational wires at use, without an additional payload head or pipeline
edge. Serial PreparationEmu/RTL retain this same26-bit decision instead of83
held bits. Capacity, old-state credit, issue/return phases and CE behavior are
unchanged. This removes redundant storage, rather than lowering precision.
Raw shares its read port between D issue at global phase0 and D return
identity at phase3. D returns publish one ordered Derived lease; LOD accepts it
at phase4, and reads its quad identity again at the following phase0. All reads
use old D bank bits before either leaf ticks, including simultaneous LOD return
and next D acceptance. Shared CE freezes both numerical phases and the lease.
D II8/span3 has one terminal bank and no arithmetic write after phase2, so
`derived_count<=1`; the owner is the old ordered `d_head` predecessor. No
Derived payload or owner shadow is added. Ready has one Coordinate read and
one LOD-return write; old Context/public-ID/UV credits and reclaim stay intact.
Diagnostic snapshots and `SharedRelease.program` use explicit non-hardware
peeks; these reads never drive readiness, identity checks or arithmetic.
Context storage/type/address/identity/control faults are preflighted before
preparation mutation, and bank writes commit only after a successful step.
This guarantee does not cover arbitrary errors elsewhere in Runtime.

Early release
retires after the final Coordinate consumption of the reserved UV row; late
release marks packet completion and reclaims only the ordered ring head.
Base addresses remain in the slot table. The new UV owner is176 soft payload
bits plus16 explicit control bits and one hard36-bit DO; the eight144-bit FF
UV rows are absent. The older387-bit context allowance and227-bit split-bank
checkpoint remain historical accounting, not measured hardware savings.

The independent three-bank reference qualification compares76 Runtime calendars with the committed
union-payload baseline on every wall edge, including admission, all preparation
events and exact public results. It retains warm pixel II2 and old-state credits.
Independent wire goldens and portable/native/newly-lowered-union RTL replay cover
capacities1..8, signed boundaries, padding, simultaneous bank writes, CE stalls,
fault suppression and same-instance contaminated-RAM reset/drain/reuse.
The matched60MHz placements0/1 fit the complete bank port controller with the
same observable wrapper. Retained ordered Runtime controls are external in both
variants; the union reference is newly lowered from the old Rust model, not
existing complete Runtime RTL. Physical numbers have one generated report home:
[historical exp-231](experiment-review.md#reproduction-and-historical-boundaries).
These generic-clock probes do not establish complete Runtime/system fit or board
results. Reproduce replay with `cargo test -p gpu-v2 --lib context_storage -- --ignored`.

Runtime Derived-loan qualification compares12 actual Runtime profiles to
frozen `538cbb9` execution on every wall edge, including zero/sparse masks,
CE0, C6/Context8 saturation, early/late release, miss and delayed terminal
completion. Live D/LOD RTL with portable and vendor RAM models checks both
LOD reads after D validity expires. The matched observable D/LOD/metadata leaf
fit is a limited subcomponent probe; its report is
`D:/github/DigitalDesign-code/target/sampling-runtime-rounds-20261010/derived-review.json`. It excludes
the Runtime frontend, UV, Coordinate, cache, Color, full Runtime and GPU.

Canonical Ready qualification exhausts levels/parents and identity combinations,
checks all seven independently inconsistent derived operands before commit, and
replays Raw→Derived→Ready reuse on the same instance at capacities1..8 through
portable, native and union-reference RTL. Independent three-bank76 Runtime port calendars and
serial/whole-sampler differential tests cover the integrated consumers. Receipts
for this newer format have a separate home:
[historical exp-245](experiment-review.md#reproduction-and-historical-boundaries). Earlier context fits above
retain their original83-bit format and do not measure this decision26 lowering.
Reproduce with `cargo test -p gpu-v2 --lib canonical_ready_ -- --include-ignored`
and `cargo test -p gpu-v2 --lib context_storage_runtime_native_replay -- --ignored`.

The earlier compact Coordinate implementation stores each reservation in two
asynchronous-read SRAM banks: owner38
(two9-bit parents, levels, slot/key, nearest and final) is written at issue;
fine fractions16/base coordinates20 are written at return. Native16x40 and16x36 banks use10+9
RAM16SDP4 cells, including two padded owner bits; total Coordinate credit stays6.
Coefficient reads the old completed row directly, with no full-row FF head or
shadow. Both bank writes and an older-row consume may share an edge. Neither a
return nor a consume grants same-edge ready/free credit. Three modulo-capacity
pointers and occupied/completed counts replace duplicate inflight counters and
per-row flags (15 control bits at capacity6); CE0 holds both banks and controls.
Reset invalidates ownership without clearing SRAM. The selected Runtime now
uses the canonical owner codec described under [Row Runtime transport](#row-runtime-transport);
the wide bank remains a generic reference. Historical Runtime calendars,
independent semantic goldens, native/FF replays and same-wrapper A/B storage fits
are in [historical exp-064](experiment-review.md#reproduction-and-historical-boundaries). This unit adds no
pipeline edge; the512-pixel warm two-mip flow retains all511 two-edge intervals.
UV actual-port calendars, all admission phases, CE-off returns, independent
goldens and behavioral/native controller replays are recorded in
[historical exp-067](experiment-review.md#reproduction-and-historical-boundaries). RAM peaks are one read and one write per
wall edge. Its matching storage-controller fit uses the same emitted native RTL
and a registered characterization wrapper; wrapper resources are reported
separately. That fit does not cover metadata/control outside the UV unit or the
whole sampler. The remaining generic preparation-control ceiling is unchanged.
Completion remains based on actual final packets and accepted masks. Both
Dedicated and Packed Runtime modes use these same actual packed scalar banks.
Input-write selectors and CE decode remain charged even with one cohort.

`texture_derivative_lod` checks462 overlapping inputs against independent integer
goldens, including mantissa ties/carry, signed extremes, force, missing mips,
filters and bias limits. `texture_calendar_rtl` covers full-leaf Icarus,
constructor poisoning, off-phase refusal and the actual LOD reset phase under CE
pauses. Multi-output tests independently check an exported truncated sum, a
same-edge doubled result and a later sum with an old external operand; malformed
certificates and missing alias-source storage must fail. Connected qualification
checks every live cut, packet/Pool writes and the dynamic no-counted-call guard.

The matched old/new complete-leaf fit uses identical registered wrappers and
hashes in [historical exp-237](experiment-review.md#reproduction-and-historical-boundaries). Its coordinator
review includes both placements at66MHz and the actual LOD phase5 source.
This isolated leaf result is separate from whole-GPU timing or board proof.

### Runtime membership and packet arithmetic

Runtime selects the adjacent-tap compact leaves. Membership E0 captures Common72: weights,
base tile/local coordinates and identity. Coordinate slicing is wiring at admission;
E1 tests nonzero weights, compares tiles and selects each tile's first nonzero
representative together, producing Member76. Old E1 writes Work on E2. Corner
weights retain their original positions rather than combining duplicate-tile
coefficients; intermediate Slice/Equal/Pairs payload copies are absent.

Packet E0 validates/captures the old Work head and sparse tap before cursor
advance or ACK; emit0 ends at that predicate, leaving77 retained bits. E1 selects
tile/header, corner masks and first/last, gates weights and concatenates Packet72
together. Old E0 valid requests synchronous owner R on E1; its hard DO is checked
when old E1 writes the reserved Pool64 row on E2. Membership/Packet keep II1,
old-state transfer and whole-pipeline CE hold. Work and packet-credit limits are
unchanged. The short emulator leaves never populate the removed alignment banks;
the RTL elaboration constant disables their writes and removes their observability.

Membership retains148 data bits and valid2/fault1; Packet retains149 data bits
and valid2/fault1. Runtime charges these actual owners and removes the obsolete
alignment/control allowances. The serial controller still supports the old7/9
edge leaves for comparison; its `short_alignment` flag selects2/2. Neither form
is a new RAM, FIFO or memory port.

Independent tuple goldens cover every cut, sparse taps, bubbles, input errors,
CE/result stalls, physical Pool writes and two-head Work R/C/ACK ownership.
All15,872 legal W/context/coordinate/storage/release allocation combinations
check the short owners; only executed configurations have cycle evidence.
`texture_serial_preparation_rtl` compares both leaf forms within complete serial
RTL under CE/backpressure. This does not establish an overlapping sampler RTL
top or fitted whole-GPU area.

Matched full-observability leaf fits, independent mathematical goldens, source
identities and validation receipts are indexed by
[historical exp-259](experiment-review.md#reproduction-and-historical-boundaries). Leaf fitting establishes
only the measured Membership/Packet deltas, not a whole Runtime fitted total.

### Independent coefficient cycle emulator and private composition

`texture::emu::coefficient::CoefficientEmu` accepts scalar parent weights,
fractions and owned metadata. It executes real fixed-width operations through
three actually shared9x8 multiply sites, each with one registered product edge.
Fine/coarse input muxes alternate vertical split, bottom horizontal and top
horizontal work; complement subtraction has no extra register. II remains2,
result capture is age5 and old-state consumption begins at age6. Three cohorts
and two ready rows are bounded; an old full ready queue holds the local numeric clock while
the consumer can transfer the existing head. `output()` reads that head without
adding retained state. Admission reserves one downstream work credit per
nonzero parent from the pre-edge available count; consuming a coefficient row
does not return those credits. The ready bank holds only72-bit weights in
two explicit FF rows, with no RAM16 cells. Age4 writes weights and age5 publishes
ownership. The99-bit metadata is written once at admission into three native
512x36 BSRAMs, using16 physical rows and at most five live owners. It remains
there until public consumption. A synchronous read on publication returns the
first head; consumption reads the following address. Both complete on the
existing edges, so age6 consumption and the II2 contract are preserved. There
is no metadata copy in ready storage and no soft FF payload head. Old-full
still freezes arithmetic on a consuming edge. CE0 holds both the unpublished
weight row and the hard metadata return; reset invalidates ownership and clears
the return without clearing backing RAM. Invalid admissions suppress both ports.
`ALLOCATION` declares122 numerical bits,8 phase/valid bits,14 queue-control
bits and one fault bit, plus144 ready-weight bits:289 soft bits plus51 hard
product bits and108 hard metadata return bits. Metadata BSRAM is charged separately.
These are declarations, not fitted cells. Portable RTL implements the same
weight asynchronous read and metadata synchronous read ports.
Both portable and native RTL select these same FF rows. The former SRAM/FF
characterization switch is removed: two separate registers prevent Gowin from
inferring a RAM16 array, and the complete16-slot serial fit favors Logic over
FF savings. The earlier SRAM/single-copy-metadata results remain historical in
[historical exp-219](experiment-review.md#reproduction-and-historical-boundaries). Current
source identity, independent lifetime and native Runtime replay, complete16-slot
two-placement fits and rejected candidates are recorded in
[historical exp-239](experiment-review.md#reproduction-and-historical-boundaries).

The former packed-bank RTL's operand-selection fits and rejected candidates
remain historical in [historical exp-221](experiment-review.md#reproduction-and-historical-boundaries).
The selected production `coefficient::compact_stage::CompactCoefficientEmu` and
`rtl::compact_coefficient` execute input37, horizontal-delay17, row-total18 and
fine-corner36 stages. Input37 holds parents18, fine fractions16, fine-base parity2
and arithmetic-halve1. On E2, the old input row reconstructs coarse V and captures
coarse U into the existing horizontal delay; E3 reads that delay. Nearest gates
the arithmetic fractions, while metadata captures borrow from the original raw
fractions. Metadata36 retains fine bases20, borrow2, level4, slot4 and public key6
in one synchronous BSRAM. Last-fine is reconstructed from the entire coarse
weight row being zero; no single corner can stand in for that predicate.
Coordinate retains a fine-only36 result alongside its V/config32 bank. Expanded
tap/fraction views are combinational wiring, with no second retained112-bit row.
Work stores Member76/cursor2 and two ordered76-bit heads; Packet captures77 bits
before emitting the unchanged canonical72-bit public packet. The selected domain
requires adjacent taps and the corresponding next mip/fractions; checked Rust
constructors reject non-adjacent inputs. Internal compact RTL trusts its closed
production producer. Generic Coordinate/Coefficient and Membership/Packet
references retain their arbitrary legal input semantics and original calendars.
Runtime and Serial use the compact executors. Runtime keeps Work16, Context8,
Group32 and ready2; Serial selects a single ready row and one outstanding
coefficient owner. Public IDs and every enabled stage age remain unchanged. Local parity, valid,
stages and DSPs freeze together on CE0 or old-full, including a full consuming
edge. Age4 write/age5 publication/age6 consumption and II2 are unchanged.
Closed Serial also borrows the eight wrapped16-bit UV outputs directly from
D's input bank. Those physical slices are written only on D acceptance and
remain unchanged through every covered lane's Coordinate acceptance. The next
nonempty quad can enter D only after the previous quad's final packet transfer.
This removes the Serial128-bit UV copy and its capture enable; the original
lane mux now reads the retained D outputs. CE0, fault and reset still gate
ownership, and no derivative arithmetic or stage age changes. PreparationEmu
reads the same physical bank through an audited output-field getter, without
a host UV shadow or oracle recomputation. Serial's controller data declaration
is204 bits; the D bank and its output widths are unchanged. This borrowing
contract applies to closed Serial; overlapping Runtime retains its own UV
lifetimes. Actual production source matching and the two qualified54MHz
layouts are recorded in
[historical exp-256](experiment-review.md#reproduction-and-historical-boundaries).
The complete compact source, matched 16-slot Serial fits, actual Runtime native
port replays and independent generic/tuple goldens have one evidence home:
[historical exp-063](experiment-review.md#reproduction-and-historical-boundaries). These Serial fits and
Runtime declarations do not qualify a full Runtime RTL top, whole GPU or board.
Matched complete16-slot fits, frozen source identity, independent execution,
both RTL backends and Runtime ready/metadata lifetime receipts are recorded in
[historical exp-222](experiment-review.md#reproduction-and-historical-boundaries).

The private `cfg(test)` coefficient hybrid uses actual output weights and
metadata to compile membership and one packet at a time into the existing
pooled cache and `ColorEmu`. It reserves Work16 before coefficient admission,
returns each plane credit at final packet operand capture, and retains public
lane ownership until actual color consumption. A fine/coarse cursor consumes
the existing ready row without a second payload queue. Its coefficient clock
can hold while older membership, packet and cache work drains on base CE;
actual MC returns continue on wall edges during caller CE holds.

This composition is a default-only private experiment, not a replacement for
public `Runtime`. Derivative, LOD, coordinate, membership and packet arithmetic
and their calendars remain counted replay; original completion totals and
independent cache provenance remain explicit dependencies. Its FF-backed Work
and additional control/fault/cursor state are conservatively charged, not fitted
area or a hardware lowering certificate. Formal Runtime configurations and
capacities remain unchanged.

`texture_coefficient` checks exhaustive splits, joint fraction boundaries,
physical register ownership, pauses and terminal faults. The private library
qualification adds hand packet/RGB literals, poisoned legacy preparation
results, full ready/work pressure, pre-edge credit checks, CE-held MC returns
and same-ID reuse with different colors. Run the focused integration test and
library tests matching `texture::emu::coefficient` in both profiles. These prove
the coefficient arithmetic frontier and bounded composition, not complete
preparation arithmetic, RTL, PnR or whole-GPU performance.

### Independent captured-texel color cycle emulator

`texture::emu::color::ColorEmu` takes an already captured `Input` containing the
actual 72-bit packet and four RGB565 texels in tap order. `tick(Tick)` decodes
UNORM9 weights/first/last/key and calculates runtime values; it never evaluates
Program/FrameReport results or calls counted, oracle or `Model::numerical`.
The existing bound/timed preparation, packet, cache and color path is unchanged.

| Enabled edges after input acceptance | Actual arithmetic/state |
| --- | --- |
| 0 | Decode weights/key/flags and expand the captured RGB565 words |
| 1 | Twelve registered9x8 products |
| 2 | Balanced four-tap sums, old accumulator feedback, exact /511 and last-result queue write |
| 3 or later | Asynchronous SRAM head is visible; consumed when CE and output_ready hold |

The pipe has two finite positions, can accept one group per enabled edge,
and freezes completely under CE=0. One input-stream owner checks contiguous
first/last groups; one accumulator holds feedback, including across input gaps.
Sixteen result credits include last-group pipe reservations and queued results.
Only actual output consumption returns credit; a full-credit last group cannot
borrow the same edge's return. A caller holds its offer while input_ready is low.
Malformed owner/width/domain input or watchdog exhaustion is terminal until
`ColorEmu::reset()`. Reset clears control and pipeline ownership, retains result
RAM contents, and supports reuse of the same instance. This local
leaf reset is not an in-place MC recovery mechanism.

The independent product-based emulator's `ALLOCATION` declares twelve Multiply9 lanes in three macros (six
DSP18 equivalents),204 hard product bits,215 other datapath bits and19 result/
control bits:234 soft bits plus eight RAM16SDP4 (32 RAM16x1), with no head register
or BSRAM. This reference inventory totals438 bits including product registers,
excluding RAM cells; it is not the selected RTL's physical register inventory.
These field/role declarations
exclude host traces/watchdogs and do not substitute for fitted Logic or timing.

The selected `texture::rtl::color` retains the public two-edge calendar and
captures the complete four-tap sum at age1. Each RGB channel uses four unsigned
`MULT9X9` cells with bypassed product output registers. Two18-bit fabric pair
sums feed one19-bit fabric full sum, captured in a CE-gated soft register.
Twelve small multipliers can pack into three DSP macros; no fused multiply-add
or separate ALU54D is used. The portable implementation uses ordinary integer
products with the same balanced tree, and the original product-based emulator
remains an independent cycle and numerical reference. The selected binding
trades fabric addition for lower DSP demand without changing public latency
or packet II1.

The selected declaration inventory is twelve logical multiplier lanes, no
fused macros, three19-bit registered partials (57 meaningful soft output bits),
six pair adders, three full-sum adders, three feedback adders and291 total
meaningful soft state bits. There are no individual registered
products. The full19-bit partial supports four unconserved raw511 weights:
`4*511*255=521220`; its20-bit addition with old17-bit feedback preserves the
original age2 range-fault check. Fault, CE, reset, old-credit and result-SRAM
edges are unchanged. Tests explicitly reject that maximum raw partial on the
original age2 edge in both portable and native primitive simulation.

The selected16-slot complete sampler's Color source, hierarchy, both
54MHz placements, DSP kinds and commit mapping have one evidence home:
`D:/github/DigitalDesign-code/target/sampling-structural-rounds-20261010/review.json`.
The preceding soft-partial binding remains in
`D:/github/DigitalDesign-code/target/sampling-color-fabric-sum-20261010/review.json`.
The earlier fused comparison remains in [historical exp-224](experiment-review.md#reproduction-and-historical-boundaries).
Its serial area excludes the
outer Quad queues and complete overlapping Runtime controller; emulator
product-role accounting is not a substitute for this physical fit.

Result16 uses eight native `RAM16SDP4` with one shared write address/enable and
one asynchronous read address: key6 and RGB24 occupy bits29:0; writes zero
bits31:30. The SRAM output directly supplies the head, with no soft payload copy
or extra queue capacity. Queue pointers/count, credits and fault remain soft.
An old-count1 pop+push exposes the replacement at the next pre-edge, without an
additional refill edge. Continuous last groups sustain every-edge admission and
output. CE=0 holds pointers and suppresses writes; the old head remains visible.
Reset or a detected fault suppresses writes and ownership transfers, including
an older age2 result that otherwise writes that edge. The asynchronous read has
no enable or reset; stale bits are harmless while output validity is false.
An empty-row write may share the unowned read address; an owned valid head never
aliases a write. Full16 old credits leave no result in flight to overwrite it.

The composed Serial wrapper always consumes Color outputs. It selects
the private `ColorEmu::new_serial_trusted` and `rtl::color::verilog_serial_trusted`:
one registered32-bit result word replaces Result16, with no RAM read/write pointers.
The old result is read before a same-edge replacement is written. One validity
bit and two credit bits cover the result and two arithmetic stages; at most
three result credits are live. CE0 freezes all owners and the row, reset drops
validity, and fault gating cancels writes on the detecting edge. Queue age2
and next-edge visibility remain unchanged. This profile requires an always-ready
sink on enabled edges; standalone and Runtime Color keep Result16 and its
backpressure contract. Continuous-last-group and mixed-group portable/native
replay compare the single row with the independent standalone emulator and
integer RGB oracle. Complete Serial qualification is recorded in
`D:/github/DigitalDesign-code/target/sampling-color-closed-20261009/review.json`;
its fit includes the cache, while overlapping Runtime and whole-GPU fits remain
separate work.

The private Serial numeric profile retains17-bit channel partials and sums.
Coefficient conserves a total weight of511, Membership/Packet emit each
nonzero tap exactly once. The private cache inverse-permutes the coefficients
to follow physical-bank texels, preserving every weighted contribution. Every
group and prefix is nonnegative and bounded by511, so a channel sum is at most
`255*511`, below17-bit capacity. This removes the redundant range comparison
without changing rounding, owner/credit checks, malformed-header detection or
refill fault handling. Public checked Color, including the single-result-row
profile, and Runtime retain the19-bit partial,20-bit sum and range detector.
Qualification checks actual emitted groups, integer RGB goldens, seams,
first/last boundaries, CE freezes and reset/fault drain. The subsequent actual
16-slot Serial combination with LOD cancellation and shared cache decoding is
recorded in `D:/github/DigitalDesign-code/target/sampling-serial-budget-20261009/review.json`.

The private native Serial binding captures the four weights at E0 in the
existing twelve MULT9X9 A input registers. RGB retains its soft decode bank;
B remains bypassed. E1 captures each17-bit product in the existing MULT9X9
output registers; the balanced fabric four-tap sum feeds E2 feedback and
normalization directly, removing the three17-bit soft partial registers.
Feedback and result publication remain at E2.
The same `ce && !fault` advances all DSP state. A detecting fault can advance
unowned DSP data, but the existing validity and write gates suppress its use;
reset discards ownership before another group is admitted. Portable RTL and the
independent product emulator retain their original arithmetic-stage reference;
portable RTL uses soft weight capture and the registered E1 full sum.
Allocation's logical pipeline totals describe that reference; the native binding
removes the shared36-bit soft weight bank and51-bit soft partial bank, using108
live hard input bits and204 live hard output bits across the three channels,
with no additional macro. Generic and overlapping Runtime
bindings retain their existing input registers and checked numeric profile.
Actual16-slot complete Serial qualification and physical resource comparisons
are recorded in
`D:/github/DigitalDesign-code/target/sampling-structural-rounds-20261010/review.json`.
The earlier fused weight-input qualification remains in
[historical exp-229](experiment-review.md#reproduction-and-historical-boundaries).

Color ownership compares old credits with `count+push+decoded_last` modulo32,
canceling incoming/pop arithmetic from the comparison. It separately rejects
`count_next+decoded_last+incoming_last>=32`, preserving the original six-bit
total's wrap behavior even in corrupt states. No extra register or pipeline
edge is introduced; the independent emulator retains its post-edge accounting.
Portable/native RTL compare all16384 five-bit count/credit/control combinations
with the original expression, in addition to lifecycle/fault-edge tests.

Matched complete-Color fits use the same33-output observable wrapper at60MHz,
placements0/1. Physical costs, hierarchy attribution and timing have one report
home: [historical exp-062](experiment-review.md#reproduction-and-historical-boundaries) (baseline and alternatives)
and [historical exp-062](experiment-review.md#reproduction-and-historical-boundaries) (production SRAM).
The original queue already infers its key array into SRAM, so logical payload
bits cannot be reported as fitted FF savings. Production SRAM preserves age3
first output and singleton replacement without a bubble, and avoids an additional
BSRAM. The verified BSRAM alternative saves Logic and leaf SRAM but introduces
synchronous first-fill/refill waits. The old SRAM characterization lacks atomic
fault gating and is not the delivered implementation. These are isolated Color
probes with a generic-clock placement warning, not composed sampler/system or
board proof. The default Runtime declaration is generated by
`runtime_inventory::describe` for the selected configuration. It is a model bill;
native binding overrides and physical fits are recorded separately.

The Color storage qualification independently computes integer RGB goldens,
tracks actual row ownership and read/write addresses, and replays directed and
live Runtime calendars through portable and native RTL. It covers128 consecutive
results at II1, full16 old credits, simultaneous pop/write, singleton replacement,
CE=0 head visibility, contaminated-RAM reset/reuse on the same instance, and
fault-edge suppression of an otherwise simultaneous pop/write. The live paused
Runtime calendar reaches16 stored results; ordinary and paused calendars also
check public pixels against the sampler oracle. Reproduce the dedicated replay
with `cargo test -p gpu-v2 --lib color_storage_portable_native_replay -- --ignored`.

Tests independently sum expanded texels and divide by 511, exhaust all 130306
legal normalization values, check stage timing, mixed multi-group owners, full
credits, CE stalls, invalid requests and bounded recovery. A directed regression
fills all 16 result credits with distinct RGB values across two keys, pauses CE
with queued and in-flight results, then blocks the seventeenth last packet until
an old credit is available. After the pipeline drains into the fully occupied
queue, it verifies ordered consumption and reuses both keys with different
multi-group colors on the same drained instance. The complete outputs match
independent integer goldens. Actual serial MC
Captured events, including coarse mip/seams/masks/ID reuse, are replayed into this
block and compared to sampler oracle RGB. This replay does not connect the live
cache reservation/backpressure boundary or public sample-result store. Its
original emulator evidence is separate from the Color result-RAM RTL replay and
isolated PnR above; neither proves the complete sampler's physical implementation.

### Complete serial-sampler fit checkpoints

`texture_sampler_export` exports the actual serial preparation/cache/refill/Color
composition with its native Gowin backend, a fixed slot fixture and all public
ports. For example, run `cargo run -p gpu-v2 --example texture_sampler_export --
target/texture-sampler-fit16 16`; slot counts1 and16 are supported. The selected
fixture uses short alignment and keeps nearest bypass disabled. Matched baseline
and decision26 candidates use identical slots, fully observable registered input
and output boundaries, clock constraints and placements0/1. Wrapper resources
are reported separately from the sampler synthesis hierarchy; PnR totals include
the wrapper. DSP counts name the reported primitive type, not an assumed tile
packing. Complete fit/source hashes, resource deltas, timing failures and the
resulting optimization priority live only in
[historical exp-245](experiment-review.md#reproduction-and-historical-boundaries) for the decision26 checkpoint.
The subsequent Logic-budget cache reconstruction, rejected alternatives and
matched complete native fits are recorded in
[historical exp-218](experiment-review.md#reproduction-and-historical-boundaries).
The checked-domain Coordinate/Color-credit checkpoint, exhaustive code-domain
qualification, rejected register-overlay candidate and matched complete fits
are recorded in [historical exp-235](experiment-review.md#reproduction-and-historical-boundaries).

This top serializes one quad. It is distinct from the overlapped Runtime with
Work16/context8 and its separately declared allocation. A complete serial fit
neither proves complete Runtime fit nor whole-GPU/system/board acceptance.
Future optimization checkpoints must refit this complete native composition;
leaf savings alone cannot establish the resource or timing outcome. The baseline
cache hierarchy motivated the following production reconstruction:

- A tag omits the four set-address bits and retains18 bits. Two36-bit native
  `SDPX9B` banks read all four ways at admission. Allocation writes only the
  selected18-bit half through two9-bit write masks; that edge inhibits reads.
  The held head refreshes its tag row before a later terminal can publish READY.
- Each set owns an8-bit state row and a3-bit PLRU row in distributed SRAM.
  Separate per-set reset masks hide stale contents. The held miss head and
  its unique descriptor always address the same set. Terminal publication and
  READY-hit issue are mutually exclusive, so each row needs one write port.
  Four fixed two-bit assignments select the updated way, replacing a variable
  part-select write. The input tile-domain check ORs the x/y bits and tests
  their shared power-of-two bound once; malformed levels still fault before
  allocation. Exhaustive checks cover every state byte/write-way combination
  and all raw level/x/y fields, including malformed levels.
- Refill beat bits select the bank-local address directly; bit1 swaps the
  low/high word pairs. A closed inverse bank map issues the four tap reads.
  The public cache uses two conditional swaps to derive canonical tap order
  from the held result payload and preserves Group4 unchanged. The private
  Serial composition keeps physical-bank texel order and inverse-permutes the
  four9-bit weights instead. This replaces64-bit texel selection with36-bit
  weight selection, preserving header, key, local coordinates and the exact
  weighted sum. Generic cache and overlapping Runtime retain the public order.
  No per-tap destination write decoder is retained.
- Valid tile x/y fields form a concatenated tile index. The mip prefix is added
  at bounded width before the final base-address addition. With no descriptor,
  a full set contains only READY lines and at most one pinned line, so the PLRU
  first choice or its sibling is sufficient; invalid-first ordering is preserved.

These changes preserve every public enabled-edge handshake and numerical result.
`texture_cache_actual_rtl` qualifies portable/native backends over all sets,
replacement, seams, levels, pauses, backpressure and actual numbered refill beats.
It asserts tag read/write exclusion, descriptor/set identity and sole PLRU writer.
`texture_cache_fault_rtl` verifies malformed traffic, fault drain and same-instance
reset with retained RAM contents and different refill data. Whole-sampler RTL
replays preserve all preparation configurations. The emulator remains an
independent logical reference; its tag/read/descriptor capacity must not be
subtracted from an FF model and presented as measured whole-GPU savings.

### Bilinear restart bubble: bounded negative experiment

After the connected baseline and review `82fbc3d`, one candidate forwarded an
old published producer descriptor into R on an empty Group index transfer edge.
Only `Pool::advance`'s transfer-time `written = Some(t)` overwrite was removed;
W remained older than R, capture returned the row, and only a later cache Read
returned Group credit. Pre-edge G32 and P16/64 rows/two heads/one W/R were kept.
It was rejected for unchanged hot throughput; production retains the baseline.

The same normally warmed bilinear input and commit indices 63..127 show:

| Event, wall clock | Native | Connected baseline | Forwarding candidate |
| --- | ---: | ---: | ---: |
| Reused quad 0 / program 16 admitted | 143 | 146 | 145 |
| Derivative / LOD issue | 145 / 169 | 153 / 177 | 153 / 177 |
| First packet issue / W | 231 / 240 | 239 / 248 | 239 / 248 |
| Index transfer / R / capture | n/a | 249 / 250 / 251 | 249 / 249 / 250 |
| First cache Read / pixel commit | 241 / 251 | 252 / 262 | 251 / 261 |
| Hot window start / end | 241 / 377 | 244 / 388 | 243 / 387 |
| Hot clocks / Reads / pixels | 136 / 68 / 64 | 144 / 69 / 64 | 144 / 69 / 64 |
| Clocks per Read / pixel / quad | 2 / 2.125 / 8.5 | 2.0870 / 2.25 / 9 | 2.0870 / 2.25 / 9 |

Admission follows stage issue on an edge. Even admission at 145 misses that
derivative phase; both pool paths wait for 153. Continuous bilinear Reads stay
two clocks apart. Advancing transport one clock advances both measured endpoints
one clock, leaving the extra eight clocks from preparation rephasing unchanged.
Two-plane/seam hot CPP remain 2.65625/8; all hot windows have zero new refills.
Recovery CPP changes 2.46875 to 2.453125, a separate miss/CE/backpressure window.

Candidate storage delta is zero FF/RAM/BSRAM/DSP. Its complete added source
selection is a six-bit 2:1 R-address mux plus `empty && transfer` and source-valid
OR; existing credit, head, R/W guard, ring updates and enables remain shared.
These are structural nodes, not fitted Logic. The rejected change is not shipped.
Actual MC/oracle runs cover all eight native/pool curves and safe CE/result/miss
recovery. `texture_packet_connected` now exports both paths' preparation and
cache edges, CSV-escaped stage names, and explicit hot endpoints. Reproduce with
the command above; repeat after the single experimental removal described here.

Historical native validation on this branch, predating these increments: GPU v2
release regression, texture debug tests,
strict workspace clippy, layering/source hygiene and required CPU/core/system
co-simulations passed. Full workspace testing stops at the unchanged legacy
GPU-display boot test, which expects an accepted command from the retired GPU.
The two Gowin reference documents missing from the initial worktree have been
restored from the main checkout and indexed in the vendor document README.
Subsequent validation is limited to the requested component and changed files;
the earlier quick validation aggregate cannot be reported as passing.

## Explicit continuous preparation and integer filter candidate

The continuous Context8 constructor shares one prepare head and one four-state
owner across UV input, D and LOD. D accepts at phase0 and returns at phase3; LOD
accepts at phase4 and returns at the next phase0. A simultaneous LOD return and
new D accept preserves old-state credit. The real D terminal bank remains leased
through its late phase0 identity read; CE or a denied global edge freezes the
lease and owner. Raw/Ready counts and Coordinate ownership remain separate.
The original controller stays an independent reference; continuous preparation
is an explicit construction choice rather than a replacement for all profiles.

The continuous constructor now selects the ordered-frontier physical controller
in both emulation and RTL. Occupied regions are determined by the prepare and C
heads, counts and one four-state prepare owner; per-slot validity and stage
registers are absent. Diagnostic classes are reconstructed from those regions.
Parallel covered-lane rank decode replaces the procedural priority chain.
Context8 capacity, old-state admission, denied-edge holds and immediate fault
fencing remain unchanged. The generic Controller is the independent reference;
arbitrary corruption of removed class registers is outside the physical state.
The Group48 exporter is byte-identical to the qualified successor in both
backends. Source binding and bounded default-constructor verification are in
[historical exp-024](experiment-review.md#reproduction-and-historical-boundaries) in the main workspace; matched
complete-postquad placement results remain in the linked retry report there.

The closed integer filter candidate conserves the existing total weight511 and
retains RAW565 cache payloads and the twelve9x9 product sites. Each RGB8 tap is
`(weight*channel+256)>>9`, with independent positive rounding before summing.
For channel<=255 each term is at most floor(weight/2), so every valid partitioned
prefix is at most255. Accumulator and feedback become eight bits; the previous
Q8.1 ties-to-even logic and final /511 projection are absent in this explicit
profile. Group/tile splitting, public latency, CE and FIFO ownership do not
change. Generic Color and the original cadence profile retain their contracts.

Pure floor rounding was rejected because its scalar picture study is consistently
darker. Half-up8 is the composed candidate. Its scalar RGB565 image comparison
and actual whole-chain numeric/transport qualification have separate evidence
in [the combined report](../../../.agent/projects/cpu-v3-tang-nano-20k/records/experiment-consolidation-20261015/catalog.md#exp-055).
These studies do not establish a full RTL scene render or physical image quality.

## Unique weights and compact Group candidate

The explicit complete-chain `combined-unique-group34` factory retains the
physical bank-order Color path. Four unmasked nine-bit weights are written
once to a128x36 native bank under Public16 ownership. Group34 carries the
original header without its four-bit texture slot plus two chosen-axis bits.
Tap membership is reconstructed from level, local texel coordinates and those
two bits; immutable draw configuration supplies the texture slot. Pool64 fits
one36-bit native column, with no separate stored mask column. The freed Pool
bank becomes the weight bank rather than reducing total BSRAM count.

A weight read is coissued with the real texel read. The paid synchronous return
remains owned through CE0 and backpressure, and a separate captured weight word
permits the next read while an earlier result is occupied. Public publication
and final-result release remain the existing authority; Work frees on final
PacketE0 acknowledgement. No refcount, queue capacity or cache data format is
added. Generic Group72 modules and the previous complete factory remain
independent reference choices.

The opt-in `texture::emu::unique_weights::Store` models physical weight RAM,
Public leases, paid DO and captured results; it is not a replacement full
Runtime emulator. Actual native/portable chain comparisons, independent Store
traces, real Packet RTL geometry checks, two-draw slot rebind and whole-chain
fit identities have one home in
[historical exp-044](experiment-review.md#reproduction-and-historical-boundaries).
The immutable texture slot contract lasts until `pipeline_idle`; changing it
while an old queued Group remains live is outside this specialized profile.


## Balanced Group48 complete-chain profile

The explicit `combined-group48-pool64` postquad factory raises the ordered
Group credit from32 to48 within the existing ring64. Producer16, row/counter
widths, two captured heads, one paid read and the single native36-bit Pool
column remain unchanged. The private guard rejects `rows>=63`; actual fixed-E2
producer/head reachability keeps the maximum at62. No same-edge freed credit
funds admission. The generic/default Group32 stores and complete factory remain
independent references. `packet_store_ordered::Store::group48`, the explicit
`Pool64Group48` timed profile and `Runtime::new_group48` select the matching
register model without changing default constructors.

Actual RTL full48, multi-wrap, CE0 and fault checks, independent whole-chain
numeric/DRAM comparisons, cold buffering, steady II2, and same-source complete
PnR evidence have one home in
[historical exp-022](experiment-review.md#reproduction-and-historical-boundaries).
This is a balanced worktree successor; it does not imply a larger Public/Context
window, new memory-controller bandwidth or a whole-GPU/board qualification.
