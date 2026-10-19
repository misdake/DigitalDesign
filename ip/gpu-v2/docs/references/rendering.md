# Numerical and controlled rendering APIs

## Triangle input and scope

`triangle::sim::oracle` consumes three owned `vertex::ports::Transformed`
records. Its Q16 entry accepts clip XYZW, S12F10 normal, UNORM12/4095 UV and
RGB565. Geometry ignores clip Z and derives depth from W. It performs XYW
clipping, exact represented-point snapping, integer coverage, perspective
fields, D16 and helper UV.

Near/far/guard/subpixel are reference configuration: defaults are
near 8199/65536, far 200, guard exponent 3, viewport 400 × 240 and Q4 snapping.
Geometry/interpolation use f64; source/coverage goldens use i128. This API's
clipping and helpers are independent of the selected
[geometry implementation](/ip/gpu-v2/docs/geometry.md).

### Coverage construction

Clip order is `W>=near,+X,-X,+Y,-Y`, with `abs(X/Y)<=2^g*W`. Only geometry clips;
original attributes remain in a self-contained source shared by all fans.
Canonical endpoint ordering gives reversed shared edges the same intersection.
On-plane endpoints are reused; quantization explicitly restores processed
halfspaces. Classification uses no epsilon adjustment.

A convex polygon has at most eight vertices/six fans. Project each unique
vertex once. Snap uses exact rational RNE of the represented dyadic point,
with Y down. Positive twice-area is front-facing; uncullled back faces reverse
coverage and retain a flip flag. For edge p->q:

```text
A = py-qy; B = qx-px; C = px*qy-qx*py
top_left = A>0 || (A==0 && B>0)
```

Only coverage subtracts one from non-top-left C. Unbiased source fields never
consume snapped/biased coverage equations. For subpixel fraction F, sample
pixel center `(x*2^F+2^(F-1),y*2^F+2^(F-1))`; edge steps per pixel/quad/tile
are `2^F*A`, `2^(F+1)*A`, `16*2^F*A`. Zero snapped area and empty bbox skip.
Snap-induced nonconvexity, duplicate coverage and singular surviving sources
are explicit diagnostics.

### Perspective fields

```text
Q_i = (X_i+W_i, W_i-Y_i, 2*W_i)
S = (Q_1-Q_0) cross (Q_2-Q_0)
C1 = Q_2 cross Q_0; C2 = Q_0 cross Q_1
delta = Q_0 dot S
F(C) = (height*C.x, width*C.y, width*height*C.z)
det = width*height*delta
D = F(S) dot (2*x+1,2*y+1,2)
N1/N2 = F(C1/C2) dot the same pixel vector
beta1=N1/D; beta2=N2/D; beta0=1-beta1-beta2
attribute=a0+(a1-a0)*beta1+(a2-a0)*beta2
W=det/(D*65536)
```

Viewport expansion follows cross products. Negative/extrapolated helper weights
remain legal. There is no per-channel area reciprocal or silent weight clamp.
Fields can recenter exactly before f64 conversion and quantize with a common
power-of-two scale. Basis/Planes are explicit numerical alternatives. Integer
stepping and point evaluation share these equations; independent linear solves
provide another comparison. These alternatives do not freeze hardware widths.

## Functional mesh-to-image renderer

`system::oracle::render(Scene,Config)` composes component oracles and returns
materialized RGB565. Input is an immutable Rust mesh. Each render owns its own
queues and cache state; it executes no timed/emulator/RTL engine.

| Step | Reference contract |
| --- | --- |
| Fetch | Ideal f64 vertices or PackedVertex v6: meshlet grid, S8F7 normal, UNORM12 UV, RGB565 tint |
| Vertex | Continuous matrix or quantized S16F16 position/Q14 normal matrix; optional normal transport comparison |
| Triangle | The clipping, snapping, coverage and shared fields above; four helper lanes |
| Shading | Component Lighting oracle, conserved UNORM9 Sampling, functional cache |
| Final | `base=nearest(tint*texture/255)`; saturated RNE of `(base*g+specular*h)/256` |
| Framebuffer | Materialized depth/blend oracle, atomic dirty eviction/writeback, final flush |

Continuous Vertex still enters a bounded Q28 backend with clip range[-32,32]
to bound homogeneous i128 products. Q16 remains a separate entry. Nonfinite or
range-invalid input fails.

Six configurable FIFOs cover fetch/Vertex/setup/raster/shader/Final/ROP
handoffs, each with capacity 1..64. Setup/raster retains one active report.
A downstream-first pump preserves triangle order and finite backpressure.
Overflow or max_steps exhaustion fails rather than returning a successful
partial frame. Host vectors/f64 fields are not hardware FIFO layouts.

Invalid covered W or UV fails. Only invalid-W or out-of-domain uncovered
helpers can use a benign substitute with a separate force-coarsest marker.
The marker survives bias; it cannot repair covered invalid data. Derivatives
precede repeat wrapping.

The functional texture cache has 16 sets/four ways with real RAW565 payloads.
The framebuffer oracle uses eight direct-mapped16 × 16 lines and atomic dirty
eviction/writeback. That replacement policy is distinct from the selected TDP
cache calendar and supplies no memory timing.

The native optional `parallel` feature uses ordered Rayon passes for referenced
vertices, setup, coverage and per-quad arithmetic. Final/cache/ROP commit remains
ordered through the same pump. There are no floating reductions or racing depth
winners. Precomputation bounds are 1,000,000 scanned quad candidates and 65,536
covered quad results/frame, in addition to scene/pump bounds. Exhaustion fails.
Default and WASM builds need no Rayon. Parallel runtime is not hardware cycles.

## Sampling preparation and Color components

Functional/counted/serial preparation captures all four unwrapped S18F16 UV
pairs once with RNE. Differences are exact signed 19-bit F16 codes. Covered overflow
fails; abnormal uncovered helpers force coarsest mip explicitly. Bias is Q8;
slope>2 selects coarsest, zero selects level0, otherwise bias precedes clamp.
LOG uses 64 × 8 entries with nearest mantissa-grid indexing and exponent carry at 64.
Coordinates are floor-Q8. Mip/tap/Group4 semantics retain conserved UNORM9
coefficients as described in [fragment arithmetic](/ip/gpu-v2/docs/fragment-math.md#texture-filtering).

Generic checked Color keeps full nonnegative products across groups and applies
one exact nearest division by 511 at the last group. Its valid accumulator is
0..130305. This is independent of the selected per-tap shift9 arithmetic.
Numerical stores and operation ledgers are not cycle or memory models.

PreparationEmu independently runs register kernels instead of replaying
precomputed Program answers. SamplerEmu adds demand cache, real refill events
and Color; completed output holds through backpressure. Runtime and row Runtime
use bounded Context/Raw/Ready/Work/Packet owners with old-state credits. These
helper/D/LOD stages belong to this path; selected hardware receives direct LOD.
Generic Color reserves finite output credits and holds the result until accepted.

## Lighting and controlled pixel alternatives

The ordinary Lighting power API has 886 16-bit base / 12-bit delta entries and a prepared
43-bit addressing context. Context preparation occurs once per drained config.
The selected direct-h9 table and arithmetic are described in
[fragment arithmetic](/ip/gpu-v2/docs/fragment-math.md#specular-power). Other compiled
calendars are explicit constructor choices.

Generic Lighting emu/RTL support independent ready/valid output backpressure.
Held output pauses arithmetic/token ages. Context loading requires drain and
suppresses pixel admission. Reset wins over CE and invalidates context/tokens.
Epoch returns with the result. Lit queues reject unlit mode; their parent owns
that bypass.

`system::pixel::Model` has 16 ordered global owners and separately written
Basic/Light/Sample stores. Every required lane branch writes exactly one result.
Only an old ready head joins; branch writes cannot bypass publication into a
same-edge read. Unlit supplies g=256/h=0 and untextured supplies white without
reading stale data. Its Final uses the combined-sum RNE expression above.

Source release, output-slot reuse, ROP completion and dirty ACK are distinct.
Finish stops admission, drains results, requests flush and waits for memory idle.
Fault drain is neither success nor rollback. Serial/overlapped framebuffer
models retain their own port calendars; selected TDP behavior is in
[framebuffer](/ip/gpu-v2/docs/framebuffer.md).

## Opt-in recovery arithmetic leaf

`triangle::recovery` binds six normal XYZ and RGB-or-UV lanes. Delta1/Delta2
issue on consecutive enabled edges through registered 9 × 9 multiplication and
phase-shared add/sub. The explicit default uses exact UV deltas, Q9 Color/normal,
split Q12 UV weights, signed 30-bit/F12 raw output and a 7-bit token. Restore four
zero bits for the F16 reference result.

Unsupported extrapolation emits an ordered fallback marker plus zero placeholder.
The caller must retain the source and run a wider path before completing that
pixel. The leaf implements no source fields, reciprocal, depth, raster, normal
saturation or fallback execution. It is an opt-in arithmetic API.

Implementation anchors: [triangle oracle](/ip/gpu-v2/src/triangle/sim/oracle.rs),
[owned Vertex ports](/ip/gpu-v2/src/vertex/ports.rs),
[functional renderer](/ip/gpu-v2/src/system/oracle/mod.rs),
[texture APIs](/ip/gpu-v2/src/texture/mod.rs),
[pixel Model](/ip/gpu-v2/src/system/pixel/model.rs),
[recovery](/ip/gpu-v2/src/triangle/recovery.rs).
