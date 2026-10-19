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

R prepares geometry step rows while I prepares a tile bounding box. R then
restores the aligned origin and walks supported regions with fixed 4/2/1
alignment and Y tile search. A terminal rejected region check ends traversal.
The 53-bit work register serves disjoint magnitude and delta lifetimes, and
bit-reversed wiring shares its normalization window.

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

These are internal storage records, separate from the external V96/T64 layout.

| Record | Contents and read boundary |
| --- | --- |
| V items 0..2 | Three signed 32-bit F16 position results, retained for every indexed T reader |
| V item 3 | N36: three S12F10 components packed once when Nz is complete |
| V item 4 | attr32, interpreted by the meshlet's RGB/UV mode |
| T items 0..8 | Three triples `(Nx,Ny,C)` in 54-bit containers |
| T item 9 | Positive determinant magnitude in a 54-bit container |
| T items 10..15 | Source vertex 0 N36/attr32, then vertex 1, then vertex 2; upper 18 zero |

V position and attribute reads have separate synchronous Q owners. T items 0..9
read low 36 and high 18 banks simultaneously at the same item address. A read
returns on the next enabled edge; data/index and both Q owners hold during CE
or downstream pause. Acceptance of item 15 releases the T record slot.

The implementation shares V/T ROM, register file and arithmetic; interpolation
preparation has its own local ROM schedule. Later returns write the result pool
directly. [Execution](/ip/gpu-v2/docs/execution.md) defines their reserved storage and calendar.

Implementation anchors: [V/T arithmetic](/ip/gpu-v2/src/frontend/vliw/frontend.v),
[MAC](/ip/gpu-v2/src/frontend/vliw/vertex/mac.v), [prepare](/ip/gpu-v2/src/frontend/vliw/prepare.v),
[coverage](/ip/gpu-v2/src/frontend/vliw/raster.v), [interpolation](/ip/gpu-v2/src/frontend/vliw/compute.v),
[LOD](/ip/gpu-v2/src/frontend/vliw/lod.v),
[independent integer equations](/ip/gpu-v2/examples/vliw_integration/reference/math/model.py).
