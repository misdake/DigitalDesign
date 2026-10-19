# Input and asset encodings

## Units and representation

All multibyte input objects and texels are little-endian. Host master addresses
are 22-bit halfword addresses (`byte_address/2`); local input RAM addresses are
64-bit qword indices (`byte_offset/8`). Texture and framebuffer layouts use byte
addresses before conversion at the memory wrapper. Never interchange these units.

| Local object | Qword address |
| --- | --- |
| Page p payload | `128*p`, capacity 128 qwords (1024 bytes) |
| Page p metadata | `256+4*p`, four qwords |
| Matrix/config area | Starts at 320, up to 64 qwords |
| Uniform base b | `16*b`; b is 20..23 |

There are two payload pages, for 2 KiB total. A descriptor always writes config
from local qword 320; `config_bytes` selects how much of that area to refresh.
Metadata selects the matrix block used by a meshlet.

## Meshlet payload

A meshlet contains at most 64 vertices and 96 triangles. All V96 records precede
a separately located T64 index area. Metadata remains a separate object.
Payload start is 128-byte aligned; its byte length need only be a multiple of 8.

Each V96 is three u32 words:

| Word | Bits | Meaning |
| --- | --- | --- |
| 0 | X9:0, Y19:10, Z29:20 | Three unsigned delta10 position components |
| 1 | Nx9:0, Ny19:10, Nz29:20 | Three signed 10-bit normal components |
| 2 | All 32 | UV: U low 16, V high 16; RGB: R low 8, G15:8, B23:16 |

Top two bits of words 0/1 and RGB's high byte are producer-written zero. RGB and
UV are mutually exclusive. Decode uses metadata origin/quant_shift and the
normal transform described in [geometry](/ip/gpu-v2/docs/geometry.md#vertex-transform).

One triangle is `i0 | (i1<<6) | (i2<<12)`. Three triangles occupy the low 54 bits
of one u64. High padding and unused tail indices are zero; each live index must
be below V count. Let `vwords=ceil(3*V/2)` and `twords=ceil(T/3)` in qwords.
The metadata T offset is at least vwords and must keep every fetched T word in
the page. Tight length is `8*(vwords+twords)` bytes. The maximum is
`768+256=1024` bytes. A V=3/T=1 example is [included in submission](/ip/gpu-v2/docs/submission.md#addressing-example).

## Meshlet metadata

| Qword | Bit fields |
| --- | --- |
| 0 | V count 6:0; T count 14:8; uniform base21:16; T qword offset 30:24; cull-negative 31 |
| 1 | scale_s3:0; RGB mode 4 (0=UV, 1=RGB); sequence start 11:5; generation 15:12 |
| 2 | origin_x31:0; origin_y63:32 |
| 3 | origin_z31:0; quant_shift36:32; normal18 bit 40 |

All other metadata bits are producer-written zero. Nonempty T requires nonzero
V. `scale_s` is an arithmetic right shift before T's signed 18-bit coordinate check;
`quant_shift` controls source position decode. They are separate operations.
`normal18` selects the Vertex normal quantization path.

## Matrix block

Each uniform block contains 32 u32 words in 16 qwords. Matrix coefficients and
origins use signed 32-bit F16 values.

| u32 word index | Contents |
| --- | --- |
| 0..11 | Position 3 × 4 matrix, row-major: three coefficients then translation per row |
| 12..20 | Normal 3 × 3 matrix, row-major |
| 21..31 | Unused padding, written zero |

Translation is a coefficient in the fourth column, not an extra source vertex
component. A block's position result has three components; there is no implied
fourth clip-Z output or once-rounded 4 × 4 transform.

## Interpolation table

Initialize every index j=0..255 with a 36-bit word:

```text
seed[j]  = RNE(2^25 / (256+j))                  # j=0..256 for generation
slope[j] = seed[j] - seed[j+1]
log[k]   = RNE(256 * log2(1+k/64))              # k=0..63
word[j]  = slope[j] | (seed[j]<<9) | (log[j>>2]<<27)
```

Bits 8:0 hold 9-bit slope, bits 26:9 18-bit seed, bits 34:27 8-bit log; bit 35 is zero. Generation
uses mathematical log2; runtime performs table lookup. The write handshake and
whole-GPU initialization boundary are in [submission](/ip/gpu-v2/docs/submission.md#prepare-the-gpu).

## Texture assets

Up to 16 factory-supplied slots contain validity, aligned byte base, full-mip
flag and size exponent 0..10. Assets are square power-of-two RAW RGB565. There is
no texture alpha. Each 8 × 8 tile occupies 128 bytes, with row-major little-endian
texels. Levels smaller than 8 × 8 still contain periodic 8 × 8 padding supplied
by the producer. Full mip chains are ordered smallest to largest.

Layer offsets in 128-byte tiles, for exponent 0..10, are:

| Exponent | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Offset | 0 | 1 | 2 | 3 | 4 | 8 | 24 | 88 | 344 | 1368 | 5464 |

For exponent n, `side=1<<max(n-3,0)` and
`byte_address=base+128*(prefix+tile_y*side+tile_x)`. A texture without mips uses
prefix 0 for its base layer. The allocation must fit both the 32-bit byte-address
range and actual system memory. Filtering coordinates and coefficients are in
[fragment arithmetic](/ip/gpu-v2/docs/fragment-math.md#texture-filtering); physical cache mapping
is in [execution](/ip/gpu-v2/docs/execution.md#texture-cache).

Framebuffer color/depth plane layout has its own owner in
[framebuffer](/ip/gpu-v2/docs/framebuffer.md#surface-layout), where pixel writes and tile transfers
can be read together.

Implementation anchors: [metadata decode](/ip/gpu-v2/src/frontend/vliw/meshlet.v),
[input decoder](/ip/gpu-v2/src/frontend/vliw/frontend.v),
[integer/table generator](/ip/gpu-v2/examples/vliw_integration/reference/scene/oracle.py),
[texture asset ports](/ip/gpu-v2/src/texture/ports.rs).
