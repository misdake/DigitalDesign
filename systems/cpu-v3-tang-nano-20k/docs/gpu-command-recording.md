---
id: gpu-command-recording
status: active
last-verified: 2026-11-23
---

# How a GPU command buffer works

For application startup, state sampling/lifetimes and reuse choices, read the
[public GPU guide](/systems/cpu-v3-tang-nano-20k/docs/gpu-api-guide.md) first.

## Start from the consumer

The GPU does not read a Rust `Transform`, `Pass`, object handle, CPU mesh directory,
or resource-fact table. It fetches a flat array of 32-bit instructions and follows
the physical addresses encoded in them. For a draw it needs:

1. Color/depth addresses in its live target registers.
2. A complete World block and a complete View block in local uniform RAM.
3. Material/scene state in command-processor (CP) registers.
4. Meshlet metadata and vertex/index payload in one of two input pages.

The command processor can also fetch an ordered table of
physical command slices. A slice can start and end inside a sector. Its last
instruction advances to the next slice with the same live state. Each slice's
DMA instructions retain their physical source addresses. The caller must
preserve the entry bindings and DMA dependencies when selecting a slice. The
RCC recorder can submit a flat CB or use `CommandList` to submit explicit spans.
See the
[segmented Device4 ABI](/ip/gpu-v2/docs/commands.md#segmented-submission).

Geometry composes World and View into derived uniform22, then runs the normal
vertex and triangle programs. The source blocks remain unchanged. This
preparation runs on **each DRAW**, even if consecutive draws use identical
source blocks: retaining bytes does not cache the matrix computation.

The driver therefore has three different things often called a "CB":

| Storage | What it contains | GPU fetches it? |
| --- | --- | --- |
| CPU recorder | Phase, cursors, desired camera/state, source revisions, resident-state proofs | No |
| External command allocation | Native 4-byte instructions, END, readable zero padding | Yes, command fetch |
| External push allocation | Immutable Target32, View128, World128 and optional 32-byte View-tail snapshots | Yes, data DMA at instruction-provided addresses |
| CPU resource facts | External extents and read/write access retained until reset | No; prepared merging and restricted target rebinding use them |

There is **no in-band command header**. CB06 is a boot image format, not this
stream. CPU words are 2 bytes; command words are 4 bytes; address fields often
use 32-byte or 128-byte units. Keep those units separate.

## Which transfers are necessary?

The minimum depends on changes and the hardware's fixed transfer units.

| Change / event | Necessary CPU preparation and publication | Necessary GPU input transfer |
| --- | --- | --- |
| First target | Encode Target32 once | 32 B to staging; SET_TARGET reads it |
| First View or changed Camera | Encode one projected View128 for this view state | 128 B into View23 |
| Only scene light / far / rays change | Encode the tail; preserve its billboard basis | 32 B into the tail of the already valid View23 |
| First World or changed Transform/material combination | Assemble World128 once for that source combination | 128 B into World20; material is already inside it |
| Same World and View on next draw | Append commands referencing the existing resident bindings | No uniform DMA; meshlet input DMA still occurs |
| New meshlet | Read its prepared compact directory entry to emit a command | 32 B metadata + 1..8 × 128 B payload |
| Append a prepared part | Copy its aligned command words; merge its resource facts | World payload is reused in place; the part's DMA commands load it when executed |
| Submit / replay | Publish CPU writes, check execution ownership/status/count/DSEG3, send COUNT/START | Fetch commands and execute the transfers they describe |

A material change currently requires a new **whole 128 B World** snapshot,
including unchanged matrices. This follows the selected World upload and
material-binding layout. It is not mathematically necessary; splitting it
would require a different transfer/binding policy. A 24-byte material change
therefore still causes a 128-byte World DMA.

A change to CP-consumed texture/specular/raster/lighting-kind or Scene state
requires WAIT(all). Uniform color is read from World by DRAW; a color-only change
does not require SET_RENDER or another render drain. A material response change
is different: geometry writes the products into live CP lighting registers,
so changed/unknown responses require a drain before that DRAW, without SET_RENDER.
Billboard skips response writes and cannot establish that proof for a subsequent
ordinary draw. Prepared-part replay likewise leaves response residency unknown.
WAIT(data) alone cannot replace that rendering boundary. Two DMA_LOCAL commands
can share their final WAIT(data), but there is only **one** data DMA descriptor:
back-to-back copies serialize, rather than operating as two DMA engines.

## Prepare at the change boundary

The source-to-consumer path has one native encoding and one external snapshot:

```text
Camera / State update ──> desired Pass snapshot ──> changed View / Scene tail ─┐
                                                                          │
Transform + Material ──> final World packet fields ──> changed World128 ──────────┼─> GPU local RAM
                                                                          │
Mesh registration / explicit refresh ──> CPU directory entries ──> commands ─┘
```

- Mesh directories are checked/captured at registration or explicit
  `Mesh::refresh_directory`, not rescanned in each draw. Each live Mesh owns
  two CPU u16 words per meshlet, plus allocator overhead. A child subset owns
  its own entries. Vertex/index payload remains external. Refresh affects
  future recordings; previously recorded commands retain their old addresses.
- Camera/Transform carry monotonic source revisions. A draw checks a small token,
  rather than assembling and comparing a full matrix packet. The recorder
  retains the current binding, not a global deduplication table: A/B/A can
  append a fresh A snapshot. Old A data must remain immutable.
- State setters mark scene changes. Raster/filter fields belong to Material24;
  light/far/rays/intensities belong to the View/Scene dependency. Changing
  raster state does not require recomputing view-light or matrix coefficients.
- Material encoding writes **directly into World byte104..127**. It is not
  encoded as a separate Render object and copied there a second time.
- Only a changed World copies the compact source position and normal matrices
  into the final native staging packet. Publication copies that packet once
  into its final push range. Keeping one aligned staging packet enables the
  existing bounded transfer/LCOPY machinery without several mappings or an
  additional persistent matrix mirror.
- Ordinary draws and prepared-part recording share one layout planner,
  payload publisher and command encoder. A prepared part owns its World,
  materials and resource facts; its enclosing Pass supplies the live View.
- Sealing establishes immutable structure. Reuse retains that proof until
  reset changes the CB generation; changed target/Display ownership still
  requires application ordering and the restricted rebind entry's checks.
  Binding generation checks remain in release builds. Ordinary submit does not
  validate every external alias or query Display Front.

Capacity and resource checks run before publishing an operation. A partial
transport failure poisons the CB; it must never be submitted. This is why a
small plan and a commit step remain necessary even when data is prepared once.

## A concrete example: draw the same mesh twice

The example uses one meshlet and no texture. Its metadata selects World20.
The source directory is at byte `0x500000`, with one record at `0x500080` and
one 128-byte payload sector after the record prefix. The CPU captures that
4-byte directory entry during Mesh registration; it is not in the CB.

```rust
{   // Resource creation and .ptr() arguments abbreviated here.
    let mut pass = cb.pass(back, depth, clear_black, clear_far);
    pass.set_camera(camera);
    pass.set_state(daylight);
    pass.draw_colored(transform, mesh, material);
    pass.draw_colored(transform, mesh, material);
}   // WAIT(all), FLUSH are recorded by Pass drop.
gpu.submit(cb); // Appends END/padding, publishes, then sends COUNT/START.
```

Both draws intentionally address the same mesh. The second draw still executes
and writes pixels; it is not eliminated as redundant rendering.

| Physical byte range | Meaning |
| --- | --- |
| `0x4C0000..0x4C0057` | 22 valid instructions = 88 B |
| `0x4C0058..0x4C007F` | 40 B readable zero padding, not extra valid instructions |
| `0x4D0000..0x4D001F` | Target32 |
| `0x4D0020..0x4D007F` | 96 B alignment gap, not payload uploaded to the GPU |
| `0x4D0080..0x4D00FF` | View128 |
| `0x4D0100..0x4D017F` | World128, including Material24 |

The push cursor reaches 384 B, but only **288 B** are payload. Command writes,
push payload, alignment gaps, command fetch, data DMA and framebuffer writes
are different quantities.

| CB byte offset | Native u32 | Operation |
| --- | --- | --- |
| `00` | `40000003` | WAIT(all) |
| `04` | `70000000` | FLUSH |
| `08` | `19A00228` | Target32 at `0x4D0000` → local `0x8A0` |
| `0C` | `40000001` | WAIT(data) |
| `10` | `A0000045` | SET_TARGET from local slot69 |
| `14` | `80000000` | CLEAR color black |
| `18` | `8001FFFF` | CLEAR depth far |
| `1C` | `40000003` | WAIT(all) before binding state |
| `20` | `19A012E4` | View128 at `0x4D0080` → local `0xB80` |
| `24` | `19A02284` | World128 at `0x4D0100` → local `0xA00` |
| `28` | `40000001` | Finish both uniform copies |
| `2C` | `500000DF` | Scene16 from View tail → CP scene registers |
| `30` | `600000D3` | Material24 from World tail → CP material registers |
| `34` | `2A001000` | Metadata + payload → meshlet input0 |
| `38` | `40000001` | WAIT(data) |
| `3C` | `30000002` | DRAW input0 with World/View preparation |
| `40` | `2A001000` | Load the same meshlet again |
| `44` | `40000001` | WAIT(data) |
| `48` | `30000002` | Second DRAW; no second View/World upload |
| `4C` | `40000003` | Wait for DMA and actual rendering/backend completion |
| `50` | `70000000` | Flush dirty framebuffer data |
| `54` | `F0000000` | END, then drain accepted transactions and clear busy |

Each u32 is stored low16 then high16. For example `19A012E4` occupies the CPU
words `12E4,19A0`, or bytes `E4,12,A0,19` in increasing address order.
COUNT is21 (`22-1`); START is `0x9800` (`0x4C0000/128`). Neither send transmits
matrices; the DMA instructions fetch them later.

## What is inside the two uniform blocks?

Offsets below are **bytes inside the block**, not CPU u16 indices.

| World128 offset | Fields | Producer |
| --- | --- | --- |
| 0..47 | Position 3×4, Q16.16 | Source Transform |
| 48..51 | Q16.16 one | Encoder |
| 52..59 | Ambient / directional material responses | Material |
| 60..63 | Zero padding | Encoder |
| 64..99 | Normal 3×3, Q16.16 | Source Transform |
| 100..103 | Zero padding | Encoder |
| 104..127 | Texture descriptor, specular, raster controls, padding, RGBA, lighting kind | Material + raster State |

| View128 offset | Fields | Producer |
| --- | --- | --- |
| 0..47 | Projected camera position 3×4 | Camera encoder |
| 48..83 | Reflected camera normal 3×3 | Camera encoder |
| 84..87 | Q16.16 one | Encoder |
| 88..95 | Vertical billboard horizontal-right X/Y | Camera encoder |
| 96..111 | Scene16: far, view-light and rays | Camera + State |
| 112..119 | Ambient / directional scene intensities | State |
| 120..127 | Vertical-right Z and camera-plane focal scale | Camera encoder |

The GPU's 32-byte tail update is byte96..127. It must include the unchanged
last8 bytes. Scene16 and intensities alone are24 bytes; silently zeroing the
remaining bytes would break billboards after changing only the light.

World20 lives at local `0xA00..0xA7F`, derived22 at `0xB00..0xB7F`, View23 at
`0xB80..0xBFF`. SET_RENDER uses slot83 (`0xA60`), skips the first8 bytes
(the final normal coefficient and padding), then reads Material24 at `0xA68`.
SET_GLOBAL uses slot95 (`0xBE0`) for Scene16. GPU "slot" here is **32-byte
addressing**, not a CPU resource handle or a meshlet input page.

Implemented field details remain in [native formats](/ip/gpu-v2/docs/formats.md),
the [command ABI](/ip/gpu-v2/docs/commands.md), and the
[driver storage contract](/systems/cpu-v3-tang-nano-20k/docs/gpu-api-storage.md).
