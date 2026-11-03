# Command Sampling cycle owners

`vliw_cycle::sampling::direct::Direct` models the actual command source-Q
coordinate kernel and its six owner/result rows.
`sampling::coefficient::Coefficient` models the direct-weight command
coefficient leaf. `member`, `work`, `packet` and `group` model the selected
physical transport; `packet_backend::Backend` connects the last three owners.
`color::Color` models the selected six-DSP terminal and Public16 owner.
`cache::Cache` owns the selected texture and sole weight banks. Coefficient
Work reservations and the complete source-to-terminal connection belong to
`pipeline::Sampler`.

## Source-Q coordinates and six-row ownership

The installed mip count, U5F3 LOD and filter select the fine level and a
conserving pair of nine-bit parents. No derivative or logarithm runs here.
For a trilinear fraction `f`, the coarse parent is `64*f-(f>4)` and the fine
parent is its nine-bit complement. At the coarsest level, fraction is zero.

| Advancing edge | Coordinate and storage action |
| --- | --- |
| U issue | Use old free-row credit, write canonical owner24 and capture X/fraction U |
| Following V | Consume the still-held V/config source Q and capture Y/fraction V |
| Following U | Return the previous complete coordinate36 to its row; a new independent U may issue |
| After return | Completed row is visible at the asynchronous head until an accepted take |

The coordinate calendar alternates U/V on every allowed compute edge, even
with no issue. Commit refusal and CE0 freeze it. An ACK on U does not release
the V source loan until the following advancing V edge. The original V guard
checks UV V, physical size, nearest flag and framebuffer-view flag.

POT coordinates use repeat wrapping after the signed half-texel subtraction.
RTT400x240 uses the actual U25/V15 shift/add scale and clamps the negative edge
and last texel, including its fraction. Small mip levels retain the physical
two-texel side. Owner metadata is stored as canonical24 and expanded at the
head; no sampled coordinate or fraction is duplicated there.

Three independent modulo-six cursors own admission, return and consumption.
Old occupied count supplies admission credit; a same-edge take cannot lend it.
Control resets while payload rows and partial coordinate registers retain
their contents. The native RAM16SDP4 INIT defaults start payload rows at zero;
the portable register arrays start unknown. `Direct::native()` makes this
simulation distinction explicit.

## Three-product coefficient calendar

The command coefficient consumes the direct owner/result tuple. Its selected
numeric state consists of input37, horizontal17, row totals18, fine mask4 and
three registered 17-bit products. Fine and coarse arithmetic reuse these
same products and row registers on alternating phases. Old row totals are
consumed before the same-edge next-plane replacement.

| Enabled age from admission | Producer and publication boundary |
| --- | --- |
| E0 | Capture parent/fine fractions/parities, write metadata36 and capture key/parity |
| E1/E2 | Issue vertical and horizontal products from old stage values |
| E3 | Write four fine weights directly to the external immutable bank; retain only their nonzero mask |
| E4 | Write coarse weights at opposite mip parity unless level0 suppresses that row; write eight ready mask bits |
| E5 | Publish the ready row and pay the metadata read |
| E6 or later | Hold masks and metadata Q while the parent issues fine/coarse members, then consume the pair once its final member issues |

Each plane conserves its parent through bottom/top and left/right complement
subtractions. Metadata stays in its sixteen-row admission bank until public
consumption; at most three cohorts and two ready rows are owned. The ready
bank contains two eight-bit masks, not a second copy of the 72 numerical
weight bits. Four mask flags expand to nine-bit zero/one lanes at its port.
The two exact weight writes expose address7/data36 for the future cache owner.

Ready capacity, local parity, cohort capacity and old Work availability govern
admission. A queued row may pop while a full ready bank has stopped arithmetic;
that pop does not retrospectively advance this edge's products. Metadata reads
on publication or a pop with an existing successor update only the held Q.
Payload rows, ready masks, product Q and key payload retain through reset;
control and the selected stage registers follow their actual reset assignments.

The native multiplier model explicitly evaluates a known-zero operand before
multiplication. Its cold invalid `X*0` result can therefore be known zero while
portable Verilog multiplication remains unknown. `Coefficient::native()`
reproduces that vendor simulation behavior without assigning a reset to DSP Q.

## Member, Work16 and physical packets

Member uses two enabled register edges and the command 16x8 line geometry.
It partitions adjacent nonzero tap flags without moving their exact weights.
The first flag in each geometric line selects one representative tap. Fine and
coarse level/coordinate selection consumes the held coefficient metadata Q.
Metadata and flags form the actual common72/member76 transport registers.

Work has sixteen metadata36 and control6 rows. Two paid control heads retain
the emit mask and tap cursor; payload is the live row at the reclaim cursor.
Fetching a header uses old materialized/head counts, without borrowing a
same-edge ACK. Each accepted representative advances its tap cursor; only the
last representative ACK reclaims that Work row. Native command metadata drops
the unused texture-slot bits. Rows and head/cursor payload retain over reset;
valids, pointers and owned counts clear.

Packet captures one representative's metadata and mip/base prefix, then forms
Group35 on the following enabled edge. Each mip owns complete padded 16x16
macrotile storage, split into 256-byte 16x8 half-lines. Its fields are physical
half-line15, local XY7, first/last/key8, mip parity1 and coverage4. Coverage
selects canonical coefficient taps; exact weights stay in their sole bank.
RTT400x240 uses sixteen macro rows per column and checks physical bounds.

## Group48 in native ring64

One logical forty-eight-entry FIFO uses sixty-four physical rows and a
registered native lookahead Q. Reservation advances the owned prefix at E0;
the two-edge Packet writes its reserved payload later. At most two reserved
rows remain unwritten. A same-edge pop cannot supply old Group48 admission
credit. The high cursor bit and native row address survive ring wrap.

| Group action | Last-consumer boundary |
| --- | --- |
| `pr` | Pay a payload RAM read into the primitive's bypass register |
| Following `load` | Clock the primitive output through OCE; publish lookahead Q |
| `promote` | Copy old lookahead Q into the held public head; a later read may start |
| `take` | Consume the held head and reclaim its reserved row |

The next physical half-line key is visible from lookahead Q before head
consumption, allowing the cache's tag lookup to overlap promotion.
CE0 holds all queue access stages. Native SDP memory INIT and both B registers
start zero; B reset clears both bypass and output Q independently of CEB/OCE.
The copied head payload is unreset, and RAM cells retain their contents.

The packet backend propagates only explicit abort as sticky stop. Its original
passive monitors reject bad offers and ownership without adding rollback.
Each child clocks once from old wiring, even if a sibling reports an error.

## Physical cache, weights and paid DO

Thirty-two 256-byte texture lines form eight sets of four ways. H15 supplies
the set from its low three bits and an eighteen-bit zero-extended physical
tag from the remaining bits. Each set has four two-bit invalid/filling/ready
states and a three-bit tree PLRU row, with separate initialized masks. Reset
and texture invalidation clear these masks while retaining RAM contents.
The lowest unpinned invalid way wins; otherwise tree PLRU selects a way or its
sibling when the first choice is pinned. The highest matching ready way wins.

Each parent refill owns the borrowed Group head through two separately offered
128-byte child requests. Each child receives sixteen ordered 64-bit beats and
a terminal; the last beat and terminal may occur on different wall edges.
The parent remains filling after its first terminal and becomes ready only
after its successful second terminal. Tag reads are speculative wall-clock
accesses, independent of compute CE. A head take can prefetch the next Group's
tag set. The descriptor, presented request and terminal stages retain their
own state; an accepted request is never replaced by an atomic line transfer.

| Cache boundary | Stored state and final consumer |
| --- | --- |
| Read issue | Capture paid Group35, pin its physical line, and read the sole weight RAM |
| Wall return capture | Copy paid metadata and weight Q into the result slot, clock four texel-bank Qs, and release the pin |
| Color ACK | Release the result slot and its borrowed physical texel DO |

The paid and result stages can both be occupied. Return capture may proceed
at CE0, or overlap an old result pop, but cannot borrow an occupied result slot
without that pop. There is no extra texel copy in the result slot. Its four
sixteen-bit synchronous bank Qs hold the Color loan directly.

The only weight RAM has 128 rows of 36 bits, addressed by `{quad4,lane2,mip1}`.
Coefficient E3/E4 writes reach this RAM directly. A paid read supplies four
nine-bit canonical weights; Group coverage masks them before the inverse
physical-bank permutation. For local texel `(x,y)`, the bank is
`(x&1) | (((y&1)^((x>>1)&1))<<1)` and its row is `y*4+x/4`.
Four 1024-row texel banks use these rows within each pinned line. Refill beats
apply the matching alternating-row bank permutation. Raw texels stay RAW565.

Explicit abort latches fault, invalidates the active parent and suppresses
further bank writes. An unpresented child can cancel; a presented child remains
offered through acceptance and drains through its terminal. Abort does not
start an unpresented next sector. Existing bank writes remain present. Only
descriptor ownership determines physical `drained`; logical result stages need
not empty after stop. The abort signal remains asserted through the stopped
drain. Original monitors reject head changes during its borrowed descriptor,
bad return framing, bad terminal status and a released sticky-fault abort.

Portable tag/weight/texel cells start unknown. Native primitive INIT and tag Q
start zero. Both profiles reset weight and texel Q but retain payload cells;
tag Q has no reset. Unknown speculative tag comparisons, priority and PLRU
cones retain their four-state known bits until their paid use.

## Six-DSP Color and Public16

The source-reserved command specialization holds sixteen live public IDs and
their last covered lane. Publication requires old free credit; a simultaneous
last-lane result cannot supply that credit. Reset clears live/control bits and
retains lane, key, product and accumulator payload.

| Enabled edge | Resource action |
| --- | --- |
| Start | Three channels times two DSPs multiply the first tap pair; no source ACK |
| Following ACK | Load/add the rounded first pair and multiply the second pair from the same held cache DO |
| Following completion | Add the second pair and pipeline its key/last metadata |
| Visible last-group result | Write the parent's final bank directly; last covered lane releases the public ID |

Each tap expands RAW565 by bit replication, then computes `(weight*channel+256)
>> 9`. Eight-bit pair sums occupy RGB sections of a 54-bit packed accumulator.
There is no local result FIFO. A new group can start while the previous second
pair adds. Compute CE0 freezes the arithmetic and metadata; an advancing
completion must be accepted by the closed result bank. Cache payload and texel
DO remain borrowed through ACK, including any intervening stalled edges.

The native MULT9X9 model short-circuits a known zero operand, and its ALU54D
output starts zero. Portable arithmetic starts unknown. Neither accumulator
resets on a warm reset. Known bits are retained explicitly in both profiles.
The selected source-reserved wrapper ties its publication diagnostic low:
an offer without publication credit does not fire. Unowned accepted input and
unowned output proposals retain the original passive preflight monitor,
including when proposal commit is refused.

## Closed Sampling connection

`pipeline::Sampler` connects the coordinate, coefficient, Member, Work/Packet,
Group, cache and Color owners. Its constructor selects the portable or native
RAM/DSP initial semantics. Every child reads the same old-state wires and
clocks once, including when another child reports a contract error.

Sixteen Work reservations account for accepted fine/coarse coefficient cost
before Member materializes. Only the final representative ACK releases a Work
reservation; that ACK does not supply same-edge old admission credit. The
coefficient head issues fine then coarse Member, or its one active plane, and
consumes only after its last Member issue. Conserving floor products guarantee
corner0 is nonzero whenever its parent is nonzero, so that flag also identifies
an active plane. E3/E4 writes connect directly to the sole cache weight RAM.

Pool source-ready combines the coordinate U slot with public credit on the
first covered lane. First-lane acceptance publishes its coverage and low public
ID. The full five-bit source slot accompanies publication to the parent upper
map; later six-bit Color keys recover that fifth bit for final-bank writes.
Pool UV Q stays borrowed through the following V edge after U source acceptance.
There is no second UV payload queue or externally supplied Sampling answer.

`Renderer::with_sampling` closes both Lighting and Sampling. The installed
`draw::Gpu::with_draws` uses this constructor, installed texture configuration,
and real router texture feedback. The lower `with_triangles` and `with_lighting`
constructors preserve explicit remaining consumers for isolated composition.
Global stop freezes logical compute while cache request/return/terminal owners
continue on wall edges. Physical drain observes descriptors, independently of
stopped logical pool, Work, Group or ROP contents.

## Qualification boundary

`vliw_cycle_sampling_direct` checks independent integer POT/RTT coordinates,
six-row pressure, wrap, commit refusal, CE0 and warm reset. Actual portable and
native factories compare all exported and selected internal known/unknown bits.
Compiled old-credit/axis mutations and original mask/V-loan monitors exercise
the ownership boundaries.

`vliw_cycle_sampling_coefficient` checks independent conserving corner weights,
E3/E4 write addresses and values, output masks and metadata over stalls, full
ready rows, key/cursor wrap and warm reset. Portable and vendor-DSP RTL compare
the actual products, stage registers, payload masks and ports. Compiled parity
and early-mask mutations plus the original proposal-credit monitor are bounded
negative checks.

`vliw_cycle_sampling_packet` compares independent geometric line partition,
padding/mip offsets and exact first/last/key fields. POT, selected coarse and
RTT cases reach both Work16 and Group48 capacity, cross ring wrap and apply
commit refusal, CE0 and warm reset. Actual factory modules include the vendor
registered Group RAM; internal paid Q, header/cursor and pipeline observations
retain explicit known bits. Four compiled mutations cover last-tap ACK,
native OCE, mip prefix and old Group credit; two original monitors reject an
empty member and unaligned physical base. These leaves do not yet qualify an
integrated sampler or UV/lit DRAW.

`vliw_cycle_sampling_color` independently sums individually rounded tap products
for sparse masks, one/four groups, two planes, all sixteen live IDs, reuse,
source loans, CE0, commit refusal and warm reset. Actual portable and vendor
six-DSP/ALU54D factories compare raw product, accumulator, metadata and owner
known bits. Three compiled mutations cover first-pair load, second-pair source
selection and old public credit. Original monitors reject a refused closed-bank
completion and an unowned input proposal with commit refused. These are terminal
leaf checks; they do not establish integrated texture fills or UV/lit DRAW.

`vliw_cycle_sampling_cache` checks independent row-major backing texels and
canonical weight addressing, colliding tags, repeated hits, both sectors,
separated last data/terminal, held requests, two occupied return stages,
CE0 captures, invalidation and warm reset. Actual portable/native cache
factories compare controls, RAM Qs, state rows and retained known bits.
Five CE0 abort phases check unpresented/presented/accepted children in both
sectors and preserve the exact written prefix. Invalidation with a paid pin
forces a different victim before pending DO capture. Five compiled mutations
cover wall capture, weight identity, bank permutation, first-sector publication
and pin eviction; three original monitors reject framing, status and borrowed
head violations. Complete Sampling and UV/lit DRAW remain separate integration
qualifications.

`vliw_cycle_sampling` checks the complete selected sampler against independent
coordinate integers, conserving tap weights, padded mip addresses and per-tap
RAW565 rounding. Both RAM profiles cover nearest, bilinear, trilinear, all mip
fractions, small periodic mip padding and fixed RTT400x240. Sparse public masks,
low-ID reuse, fifth-bit source slots, U/V loans, held memory offers, CE/commit
stalls and full Work16 pressure execute without numerical peers.

`vliw_cycle_draw_uv` closes actual CP/DMA, both shader owners, Final/ROP and the
native pin backing, and compares complete CP-to-router production RTL. Its
whole-root and physical-memory boundaries are described in
[the pixel composition](/ip/gpu-v2/docs/references/cycle-draw-pixel.md).
