# Explicit column scene firmware

`CPU_V3_BOOT_EXAMPLE=gpu_demo_column16` builds the existing serial `gpu_demo`
application for `texture16x8-column16`. Its box, RGB triangle, complete 948-face
rabbit, 24 meshlets, 1141 triangles, DS2 input, native precision and camera codes
are unchanged. The application records one CB, submits it, waits for GPU drain,
presents, and waits for old Display reads before reuse. It enables no capture,
RTT, CPU pixel conversion or additional submission scheduler. The default
`boot-applications.conf` and ordinary `gpu_demo` profile remain unchanged.

## Build and qualification boundary

Run from the repository root, passing the layout owner's actual contract:

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_demo/build.ps1 -Profile column16 -OutputDirectory target/column-scene/firmware -LayoutContract <cpu-pure-layout-contract-final.json>
```

The build script compiles from RCC into Cargo OUT_DIR, materializes those exact
assets, independently repacks CB06 and compares bytes, then runs the address and
asset oracle. `column-firmware-receipt.json` binds source, application, boot,
layout and static-asset SHA256 identities. It records the expected GPU export
from the supplied contract and leaves actual `hardware` null. A firmware receipt
does not certify a matching bitstream. The hardware owner must re-export the
intended firmware configuration, compare the actual RTL/boot/constraints and
refit/audit any changed image before programming.

## Memory layout and lifetime

`rcc/gpu_layout_column16.rs` is the explicit software allocation authority.
The native module resolver selects it only for this host-requested profile;
ordinary module resolution and formal renderer/capture compilation keep their
existing selection. Drivers use the selected plane capacity for alias checks.

| Allocation | Start | Exclusive end |
| --- | --- | --- |
| Color A | `0x400000` | `0x440000` |
| Color B | `0x440000` | `0x480000` |
| Depth | `0x480000` | `0x4c0000` |
| Slot 0 CB | `0x4c0000` | `0x4d0000` |
| Slot 0 objects | `0x4d0000` | `0x4e0000` |
| Immutable assets | `0x4e0000` | `0x7df800` |
| UI page 0 | `0x7df800` | `0x7dfc00` |
| UI page 1 | `0x7dfc00` | `0x7e0000` |
| Slot 1 CB | `0x7e0000` | `0x7f0000` |
| Slot 1 objects | `0x7f0000` | `0x800000` |

Each 400x240 plane uses row-major pixels inside 16x16 macros, with macros
ordered by column and macro height padded to 16:

`base + 512 * (((x >> 4) << 4) | (y >> 4)) + 32 * (y & 15) + 2 * (x & 15)`.

Visible pixels occupy 192000 bytes in a 204800-byte physical span, including
12800 bytes for y=240..255. CLEAR owns 1600 sectors per plane. Each allocation
reserves 262144 bytes; its final 57344 bytes are slack, not CLEAR-owned contents.
A diagnostic snapshot must include the full physical span. If it references the
reserved tail, that tail requires preimage/guard treatment rather than a
FULL_CLEAR assertion. This firmware does not activate a snapshot hook.

The existing full-cache clean, GPU completion and serial Display ownership
boundaries are retained. The CPU never rewrites immutable assets during draw.

## Offline assets

The atlas remains the existing 64-KiB glyph/alpha bank ABI at `0x4e0000`.
`room.bin` starts at `0x4f0000`; all 24 embedded directory pointers and the camera
root move with it. Vertex/index/native metadata and 256 camera records keep
their original numerical codes. `texture.bin` starts at `0x500000`.

Ordinary square POT mips use column-major 16x16 macros at each mip's actual macro
height. Small levels repeat periodically inside a complete 512-byte macro;
bases require 256-byte alignment. The 64-square full chain occupies 100 sectors
(12800 bytes), with level prefixes 0,4,8,12,16,20,36. Driver validation checks this
full footprint and alignment. UI grid order and glyph storage are unaffected.

The package oracle decodes every stored texel/padding word, mesh pointer,
vertex/index code and UI glyph. The bounded CPU caller test executes the real
serial two-frame entry and observes all 24 draws per frame, target/CB/object/DMA
addresses and completion-before-present with controlled GPU/Display peers.
These tests establish software/control and data-layout correctness, not GPU
rasterization, physical scanout, memory timing or board image correctness.
