# From vertices to fragments

## Input and output

Geometry consumes the [packed meshlet](/ip/gpu-v2/docs/formats.md#meshlet-payload) selected by a
host start. It emits ordered 2 × 2 quads containing a coverage/numerical mask,
per-lane S12F10 normal, RGB888 or periodic UV16, D16 depth, and one U5F3 LOD.
The viewport is 400 × 240. Lanes are `(dx,dy)=(0,0),(1,0),(0,1),(1,1)`.

All vertices execute before any triangle reads their results. Triangle state
can be replaced after its own imports, scan and arithmetic returns drain;
previously copied quads remain owned by downstream processing. This permits
triangle overlap without changing global uniforms while old fragments are live.

## Conservative meshlet rejection

Before V admission, the meshlet controller tests the quantizer's loose cube:
`origin[k] <= p[k] <= origin[k] + (1023 << quant_shift)`. It needs no additional
metadata or payload scan. Supporting corners give maximum camera Z and maximum/
minimum camera X through the existing V/T MAC. Y participates in every dot
product; there is no independent vertical-plane test.

The optimization requires signed25 origins and `quant_shift <= 14`. Each tested
matrix row requires signed20 nontranslation coefficients and signed30 translation.
These bounds keep every corner transform within signed32 F16 and the MAC within
signed54. Ineligible input continues through normal V/T without rejection.

Let Z be `floor(max_raw_Z / 65536)`. Z<=0 rejects the whole meshlet. Otherwise
reject left when `max_raw_X + 65536*Z < -2^32`, or right when
`min_raw_X - 65536*Z > 2^32`. This uses a deliberately wider `|X|<=Z` cone than
the actual viewport, plus one camera-unit margin for the existing `scale_s`
truncation. Crossing or uncertain cases remain. The loose cube can retain a
meshlet whose actual vertices are all behind; this is conservative admission,
not polygon clipping or tight vertex-derived bounds.

Rejected meshlets emit no vertices or T records and do not toggle the attribute
epoch. DMA remains unchanged, so this saves compute rather than input traffic.
Retained meshlets execute the original vertex/triangle arithmetic and ROM words.

## Vertex transform

For component k, decode source position and normal as integer codes:

```text
p[k] = signed_32(origin[k] + (delta10[k] << quant_shift))
n[k] = signed_10(normal10[k]) * 128
```

Position uses the row-major 3 × 4 matrix M from the selected uniform block.
Normal uses its following 3 × 3 matrix A. The shared MAC retains the low 54 bits
of each signed accumulation. Its output-to-register step takes bits 47:16.
Equivalently, for the retained 32-bit result:

```text
P[j] = signed_32((sum(p[k]*M[4*j+k]) + 65536*M[4*j+3]) >> 16)
v[j] = signed_32(sum(n[k]*A[3*j+k]) >> 16)
```

Normal packing starts from v, after the MAC/register boundary:

```text
normal18: q = signed_18(v)
normal16: q = 4 * signed_16(v >> 2)
N12 = clamp(RNE(q/64), -2048, 2047)
```

Each normal result is an unnormalized S12F10 code. The normal16 path deliberately
discards v's low two bits before RNE. Performing that quantization earlier in
the MAC changes the output. Attributes pass through without acquiring alpha.

## Triangle coefficients and rejection

T reads each indexed P and arithmetic-shifts every component by metadata
`scale_s`. Every resulting component must lie in `[-131071,131071]`; signed 18-bit's
minimum code is excluded. A range violation is an error. All-three nonpositive
forward components reject wholly backward geometry. There is no near-clipping
fan construction in this path.

For shifted vertices a,b,c, construct:

```text
n0 = b cross c; n1 = c cross a; n2 = a cross b
delta = a dot n0
C_i = -399*n_i.x - 239*n_i.y + 512*n_i.z
```

Zero determinant rejects a degenerate triangle. A negative determinant rejects
when cull-negative is enabled; otherwise orientation is retained for canonical
coverage. T exports each `(n_i.x,n_i.y,C_i)` and `abs(delta)`, plus source normals
and attributes. The transport layout is described at the end of this chapter.

## Coverage and quad traversal

Raster works on integer edge fields, with canonical orientation and a top-left
inclusion rule. Geometry coefficients are symmetrically reduced: right shifts
of signed coefficients truncate toward zero, preserving sign reversal. For an edge, choose
`e=max(0,bitlen(abs(Nx))-17,bitlen(abs(Ny))-17,bitlen(abs(C))-27)`, with
`bitlen(0)=0`. Divide each signed coefficient by `2^e` toward zero. The retained
Nx/Ny magnitudes are below `2^17`; C magnitude is below `2^27`. An exponent
above 19 faults. Double the reduced Nx/Ny for pixel stepping in signed 30-bit storage.
At pixel `(x,y)`, its reduced field is `2*Nx*x + 2*Ny*y + C`.
An edge with `A=2*Nx`, `B=2*Ny` includes zero when
`A>0 || (A==0 && B>0)`; other edges require a strictly positive value.

After private bounds preparation, nonempty work starts R independently of
old I's tail. Empty bounds retire without R import or an I context switch.
R imports final dx/dy and packed N in parallel, then prepares scanner row
steps and sign-support bases. X/Y and upper support advance one level at a
time through the root's required depth. Lower support stops at level3, since
only nodes at levels2..4 consume it. The accumulators reuse dead Nx/Ny RF
rows; there is no later zero/dx/dy copy or diagonal-parameter preparation.
Origin restoration reads packed signed18 Nx/Ny through the two existing
18-bit step lanes and C through the work RF, then directly writes
`C+Nx*(2*x_root)+Ny*(2*y_root)` through a fused dual-product DSP. It does not
walk origin coordinate bits or save an intermediate X-restored vector. The
[origin calendar](/ip/gpu-v2/docs/execution.md#raster-origin-restoration)
overlaps the last DSP write with the next support read, without changing
the 30-bit field arithmetic or the pixel-admission interval.
Bounds preparation interleaves independent X/Y constraints from each adjacent
edge pair through the existing DSP and parameter ports. It retains the same
integer tile-interval tests, conservative seed and signed results. Its
[physical calendar](/ip/gpu-v2/docs/execution.md#interleaved-tile-bounds)
uses temporary PB/PC axis rows and retires one axis while launching the other;
it does not increase the multiplier or memory-port count.
For a single eligible axis, D and C products instead launch consecutively,
using the two PC rows as independent first-product feedback. Search omits high
bits already excluded by preceding constraints, preserving the same final interval.
Geometry reduction stores signed raw coefficients and uses three fixed signed
windows with a dedicated 36x18 DSP. A one-bit lost-tail marker and live DSP bias
preserve exact truncation and the existing per-edge exponents. It reads Nx/Ny/C
consecutively and writes canonical results directly to the final banks while
collecting the next edge; only PA's N weights form an unsigned17 magnitude. Delta alone uses the reversed 53-bit work register. The
[publication calendar](/ip/gpu-v2/docs/execution.md#geometry-reduction-and-publication)
defines the fixed latency and final-address sequence; no component tags or
iterative geometry-shift states remain.

The walker requests the target ROM word on the same enabled edge that retires
the deciding edge predicate. The epoch changes on that edge, invalidating any
younger speculative result. The next instruction's reads use the updated level
and coordinates; there is no additional pending-branch handoff state.

Full tiles keep a row-start vector and stream four pixels per quad through the
existing 16-cycle calendar. Above level4, a node whose upper support passes
descends immediately; a lower/full-coverage test cannot change that decision
and is omitted. Level4 and smaller nodes retain both tests. Full scan blocks
therefore stay within one external 16 x 16 framebuffer tile. Descendant tests
and mailbox traffic remain part of traversal rather than receiving an implicit
zero-cost full-region loop. Partial tiles arrive only at 4 x 4 pixels. Their
scanner uses two current/future vectors in one 16 x 30 local register file;
the address bank changes only when a rejected quad advances. Import writes
two signed18 Nx/Ny banks. Their fixed synchronous reads align with each
edge's stored h. Exact four-pixel feasibility is:

```text
f = floor(h/2)
inclusive = (tau == 0) || (h is odd)
edge_mask[i] = bitmask(pass_h(h), pass(f+Nx), pass(f+Ny), pass(f+Nx+Ny))
pass(sum) = sum > 0 || (sum == 0 && inclusive)
quad_possible = any(edge_mask[0] & edge_mask[1] & edge_mask[2])
```

The first `pass(h)` uses the ordinary signed30 top-left test. Each remaining
sum uses signed20 arithmetic. `abs(Nx),abs(Ny)<=131071`; when h fits signed20,
the half-h sums fit signed20, including the sequential diagonal sum. Outside
that window, positive h makes every corner pass and negative h makes every
corner fail. This follows from the maximum full-pixel offset524284. No h,
coefficient, exponent or rounding precision changes. Four mask bits identify
actual common corners, rather than accepting edges with different possible
corners. An empty quad therefore never enters I or allocates a result slot.

The scan-store write word feeds this classification. LOAD/PREFETCH classify
the first quad; the final pixel of a partial SCAN classifies its successor.
CHECK advances an already rejected quad and classifies the next position.
Each edge intersects a four-bit aggregate, which is registered before it
controls coordinates, banks or synchronous reads. The classifier uses local
RAM16 reads and adds no arbitration or pixel FIFO.

A registered rejected tail needs no successor calculation. At phi3, if SCAN
has classified the final quad as impossible, it finishes the tile directly.
When a CHECK on the first quad of the final 4 x 4 tile row classifies the last
quad as impossible, that same finish retires both rejected positions. No pixel
or result-pool slot is created for these tails. The next mailbox load can start
on that edge; without a next tile the scanner enters OFF. No scan-store/step
read is issued for the discarded tail, and no combinational ALU predicate
bypasses the existing flags.

For CHECK, the scanner ALU step is `2*dx`, or `2*dy-2*dx` at the end of the
two-quad row. The preparation ROM produces that row step and `3*dx-dy`, which
converts the last scanned pixel of a row into the following row's origin.

| Phase | Known rejected current quad | ALU/write and next read |
| --- | --- | --- |
| Previous phi3 | Request edge 0 | Read current vector |
| phi0 | Advance edge 0 | Write future edge 0, register its predicate; request edge 1 |
| phi1 | Advance edge 1 | Write future edge 1, register its predicate; request edge 2 |
| phi2 | Advance edge 2 | Write future edge 2 and register its predicate |
| phi3 | Select future bank and advance coordinates | Read future edge 0 for SCAN when its stored predicates pass, otherwise for another CHECK |

Partial SCAN's last-lane h writes register the next quad's predicates in the
same way. One phase marker defers the mode decision until phi3, after all three
edge contributions are stable; it does not add a quad-admission cycle. ALIGN similarly selects
SCAN or CHECK from the loaded tile's predicates. The first LOAD read is scheduled so its
last return precedes phi3. Neither a LOAD predicate nor a scan ALU result
bypasses the flag register into the next memory address.

Possible quads enter the original pixel calendar directly. Rejected quads need
no separate SKIP operation or row-start copy. A rejected tile-end quad can start
the next mailbox load on the same phi3. A final-quad PREFETCH owns the next
tile's predicate flags; the old tile's last quad does not overwrite them.
All state and RAM Q registers freeze under CE; output backpressure holds at
quad admission.
Threshold writes belong to preparation, while scanner reads belong to
execution, with no dynamic port arbitration. Full-tile row handling remains
independent of the partial-tile bank alternation.

The tile FIFO exposes metadata at its first h write. Its fixed fill schedule
guarantees each subsequent LOAD or PREFETCH read follows that word's write;
the producer holds one physical slot throughout all three writes. The earliest
LOAD reads h0 while h2 is written at a different address. This
[publication calendar](/ip/gpu-v2/docs/execution.md#tile-mailbox-publication)
needs no data bypass, extra buffer or complete-vector waiting state.

At admission of the final quad of a tile, an already available next mailbox
entry enables a fixed prefetch calendar. Lanes 0, 1 and 2 read its three h
values at phi2 and write them into the scan store at phi3, when the current
pixel update has finished. A following partial tile uses the inactive vector;
a following full tile uses the row-start vector, whose old value is dead after
the current final quad's first pixel. No additional h buffer is present.

The mailbox entry remains owned until lane 3 phi2. Handoff then installs its
metadata, advances the mailbox head and enters ALIGN; phi3 reads the next
tile's edge 0 directly. The current quad's final coverage still uses its old
metadata. If no next entry was available at final-quad admission, the scanner
uses the ordinary LOAD path; it does not start a partial prefetch or arbitrate
for the scan-store port. CE pauses freeze the calendar, and quad backpressure
is resolved before that admission.

The geometric mask and numerical mask are distinct. Numerical depth/denominator
checks may clear covered lanes, but the quad keeps its place in the ordered
stream. An empty numerical mask still publishes and retires in order. Coverage
is evaluated before framebuffer depth testing; there is no early-Z.

## Perspective interpolation

The three reduced edge values at a covered lane are nonnegative h30 codes.
Step them in their original edge scales before aligning to common exponent
`E=max(e0,e1,e2)`. Define `H_i=h_i >> (E-e_i)` and `D=sum(H_i)` in D32.
Discarded low bits are not added back. D=0 cannot produce a live fragment.

Normalize positive D with `e=floor(log2(D))` and the 18-bit mantissa
`m=floor(D*2^(17-e))`. With `j=(m>>9)&255` and `f=m&511`, the initialized table
produces `r=seed[j]-RNE(f*slope[j]/512)` in R18. The code uses a positive dummy
D=1 for zero-D arithmetic, while preserving the invalid marker.

For the first two H values, form `hm_i=floor(H_i*2^(16-e))`, then:

```text
w0 = min(65536, (r*hm0)>>17)
w1 = min(65536-w0, (r*hm1)>>17)
w2 = 65536-w0-w1
attribute = floor((a2*65536 + (a0-a2)*w0 + (a1-a2)*w1 + 32768)/65536)
```

Weights are nonnegative F16 and sum exactly 65536. Normal, RGB and UV parameter
pairs use the shared MAC's retained bit slices and half-LSB bias. Normal's
Vertex-boundary RNE is a separate earlier operation. Each source N36/attribute
word is accepted only on its packed-PC write, then the local ROM reads stored
vertices in order 2/0/1 to produce exact signed17 base/difference parameters.
The [parameter calendar](/ip/gpu-v2/docs/execution.md#triangle-parameter-storage-and-reads)
defines the independent stored-data lifetime after input acceptance.

## Depth and analytic LOD

Depth derives from the positive determinant and the same aligned denominator.
Let `ed=floor(log2(abs(delta)))`, `K=floor(abs(delta)*2^(30-ed))`, and
`right=e+E+38-ed-scale_s`. Form `Z_full=floor(r*K*2^-right)` under the admitted
integer widths; the output is `Z16=(Z_full>>8)&65535`. Live depth additionally
requires nonzero D, `Z_full < 2^32` (no arithmetic overflow),
`Z_full < 2^24` (upper depth-word bits zero), and
`Z16 <= cfg_far_z[23:8]`. A positive value quantizing to zero is allowed; this
depth check does not introduce a near plane.

UV quads select the first geometrically covered lane as the LOD anchor. Using
that lane's interpolated U/V, align Nx/Ny coefficients to E with truncation
toward zero. For each axis, prepare `S=sum(N_i)` and
`G_U=sum(U_i*N_i)`, `G_V=sum(V_i*N_i)`. The four analytic magnitudes are
`abs(G_channel-anchor_channel*S)` for Ux/Uy/Vx/Vy.

Log lookup uses exponent plus the floor six-bit mantissa grid from the
[initialized table](/ip/gpu-v2/docs/formats.md#interpolation-table). Each nonzero magnitude's
Q8 log adds 256 times its texture dimension exponent. The maximum is corrected
by `-log_q8(D)-3712+16*cfg_lod_bias`, clamped to `[0,256*cfg_max_mip]`, then shifted
right five to U5F3. A quad without a gradient yields LOD0. RGB mode bypasses
these derivative operations. Sampling consumes this final LOD without another
bias, derivative or logarithm stage.

## Retained V and T records

These are internal storage records, separate from96-bit source vertices and
three-triangle/64-bit index words. Meshlet capacity remains64 vertices/96 triangles.

| Record | Contents and read boundary |
| --- | --- |
| V items 0..2 | Three signed 32-bit F16 position results, retained for every indexed T reader |
| V item 3 | N36: three S12F10 components packed once when Nz is complete |
| V item 4 | attr32, interpreted by the meshlet's RGB/UV mode |
| T items 0..8 | Three triples `(Nx,Ny,C)` in 54-bit containers |
| T item 9 | Positive determinant magnitude in a 54-bit container |
| T items 10..15 | Source vertex0 N36/attr32, then vertex1, then vertex2; command profile uses item10 high18 and item11 high14 for mesh uniform RGBA |

V position and attribute reads have separate synchronous Q owners. T items 0..9
read low 36 and high 18 banks simultaneously at the same item address. A read
returns on the next enabled edge; data/index and both Q owners hold during CE
or downstream pause. Acceptance of item 15 releases the T record slot.

The default T queue has sixteen slots. Coefficients use ten field-major planes
of sixteen rows in the existing result banks; numeric formats are unchanged.
Descriptors reside in independent 16-deep distributed RAM, with one private
attribute-epoch bit in addition to the unchanged public 36-bit descriptor. Its
head stays immutable through item 15 acceptance, so attribute export selects
vertex references directly from that head instead of copying and rotating them.
The existing fetch index selects both vertex and N/attribute component. Header
acceptance requests coefficient 0 on the same enabled edge. Extended read/write
cursors supply full/empty tests without a separate occupancy counter.

The two existing 512x36 result memories retain independent read/write ports:

| Memory | Address | Lifetime |
| --- | --- | --- |
| Positions | `{0,component2,vertex6}`; 192 live rows | Last T computation of this meshlet |
| Export low | `{field4,slot4}`; rows 0..159 at depth16 | T item15 acceptance |
| Export low, N36 | `{1,0,epoch1,vertex6}`; rows 256..383 | Last indexed export of that epoch |
| Export low, attr32 | `{1,1,epoch1,vertex6}`; rows 384..511 | Last indexed export of that epoch |

The coefficient high18 memory is unchanged. No position copy or additional
BSRAM is needed. New V writes positions in place and attributes into the other
epoch while old T exports read the retained epoch. Shared V/T arithmetic still
executes all V before T; it is not duplicated.

Epochs alternate at a validated meshlet boundary. With at most two live epochs,
the alternate epoch is free exactly when the T FIFO is empty or its head belongs
to the current epoch. FIFO order and item15 retirement establish this condition;
there are no per-epoch reference counters. Input-page identity is independent
of attribute epoch. Header/bounds admission latches readiness on an enabled idle
edge without waiting for that epoch. A rejected meshlet can therefore complete
while old indexed exports are blocked. A retained meshlet waits at V admission,
then resets the V count and toggles the epoch before starting V. This preserves
the indexed attributes and epoch color of old queued triangles. Full frontend
idle still requires the complete T FIFO and R/I pipeline to drain.

`TRIANGLE_SLOT_BITS` selects power-of-two depths from 2 through 16 at elaboration.
Two attribute epochs reserve the upper half of the export-low memory; a 32-slot
T queue does not fit this layout. The public T record and global state ownership
are unchanged.

The implementation shares V/T ROM, register file and arithmetic; interpolation
preparation has its own local ROM schedule. Later returns write the result pool
directly. [Execution](/ip/gpu-v2/docs/execution.md) defines their reserved storage and calendar.

Implementation anchors: [V/T arithmetic](/ip/gpu-v2/src/frontend/vliw/frontend.v),
[MAC](/ip/gpu-v2/src/frontend/vliw/vertex/mac.v), [prepare](/ip/gpu-v2/src/frontend/vliw/prepare.v),
[coverage](/ip/gpu-v2/src/frontend/vliw/raster.v), [interpolation](/ip/gpu-v2/src/frontend/vliw/compute.v),
[LOD](/ip/gpu-v2/src/frontend/vliw/lod.v),
[independent integer equations](/ip/gpu-v2/examples/vliw_integration/reference/math/model.py).
