# Fixed400x240 framebuffer texture view

`gpu-v2-vliw-export OUTPUT rtt400x240 [portable]` adds a base-only RGB565 view
over macro-column16 storage and the resident16x8 cache. Ordinary command,
command16x8 and layout-only command-column16 retain their own contracts. The
system selects CPU_V3_GPU_PROFILE=rtt400x240-column16 explicitly.

## Descriptor and storage

SET_RENDER's first little-endian u32 has these fields. Other fields retain the
[native formats](/ip/gpu-v2/docs/formats.md).

| Bits | Fixed view value |
| --- | --- |
| 15:0 | Base in128-byte sectors |
| 19:16 | 9, coordinate carrier exponent |
| 20 | 0, no mip chain |
| 22:21 | 0 nearest or1 bilinear |
| 23 | 1, framebuffer view |
| 31:24 | 0 |

Base is64KiB aligned and at most63488 sectors /byte0x7c0000, leaving262144 bytes
inside8MiB. System ownership adds reserved-range exclusions. Width400, height240,
RGB565, column macro pitch16, base-only and clamp are implicit. Physical content
spans204800 bytes:192000 logical bytes plus12800 macro padding. Allocation tails
are not texture content. D16, mip, repeat, general sizes and stride are absent.
The framebuffer_texture_word helper and CP simulation checks reject incompatible
fields. Bit23 zero preserves ordinary column POT semantics.

## Coordinates and addresses

Each signed18-bit F16 interpolated UV is clamped to0..65535 before its integer
and sign bits are discarded. For clamped q and W=400 or240:

```text
fine = floor(q*W/256) - (nearest ? 0 : 128)
fine = clamp(fine, 0, (W-1)*256)
index = fine >> 8
fraction = fine & 255
```

Nearest uses the first tap. Bilinear retains9-bit weights and per-tap channel
rounding. The far edge becomes the last integer texel with zero fraction;
out-of-image neighbors have no weight or cache group. Bilinear's half-texel
subtraction is not an extra nearest correction. The fullscreen demo uses actual
packed UV and pixel centers without a nearest half-texel bias.

The existing U/V calendar shares400/256=25/16 and240/256=15/16: compute5*q,
then4*(5*q) plus/minus5*q and shift four bits. Truncation is exact. Negative
bilinear coordinates are detected before unsigned subtraction wrap can escape;
the upper clamp compares integer texels. No coordinate stage, RAM or source
lifetime is added. LOD retains its calendar, clamped to level0 for this view.

For base B and logical(x,y):

```text
macro = (x >> 4)*16 + (y >> 4)
line = B + 512*macro + 256*((y >> 3)&1)
texel = line + 32*(y&7) + 2*(x&15)
refill = line + 128*sector, sector=0,1
```

The8KiB cache has32 lines,8 sets,4 ways and four1024x16 banks. Physical256-byte
tags remain compatible with column POT assets. Both128-byte children must succeed
before Ready. Sampling cannot depend on padded rows240..255. Framebuffer,
Display and memory-controller transports retain their contracts.

## Visibility and verification

Producer terminal must include FLUSH and real writeback completion. A consumer
executes TEX_INVALIDATE before reading, even on descriptor/base reuse;
SET_RENDER alone is insufficient. Source storage stays immutable/alive through
retirement and disjoint from target and protected Display storage. The
[resource API](/systems/cpu-v3-tang-nano-20k/docs/gpu-render-targets.md) owns these
pins; the GPU does not become an allocator or hazard scheduler.

The test_rtt400.py harness independently checks coordinates, packet addresses,
sampler arithmetic, signed UV, all logical texels and cache/macro/edge cases;
exhaustive-axis additionally covers every16-bit UV value. The coordinate
scoreboard checks the two-enabled-edge latency across CE pauses. The
test_rtt400_replay.py harness replays actual RCC submissions: X/Y source codes
identify every fullscreen source coordinate, while chain mode retains real GPU
writes and checks sampled RGB at raster UV against an independent integer
reference. Logical output, padding and allocation tails are checked, with an
explicit backend. The test_rtt400_command.py native micro-scene checks visibility,
uniform RGB, Q6 blend and malformed descriptors with bounded stalled memory.
These are offline checks, not physical MC timing or board qualification.
