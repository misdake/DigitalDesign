# Tank Racer PVS storage

The PVS is a conservative candidate set for an XZ cell and one of 32 headings.
It rejects metadata chunks and groups before the existing precise visibility
tests. The guarded cone, pitch eligibility and route fallback are defined in the
[application guide](../applications/tankracer/README.md#static-visibility-and-transparent-order-priors).
The default shared-block format preserves every mask bit, including unused tail bits. It adds no
occlusion decision or dependency on a recorded camera path.

The current map reuses the route-location XZ grid: each cell covers 32×32
source-world units (4×4 render units at scale 0.125). Its 18×27 rectangle has
486 cells, of which 180 near the route have eligible PVS data. Each cell has
32 heading bins, 11.25 degrees each. Position within a cell does not change
the key; a cell or heading-bin transition can update the cached mask. Pitch
outside the guarded domain uses ordinary dynamic visibility instead.

## Shared-block format

`tools/view_pvs.py` produces one sector-aligned `MAP_VIEW_PVS` asset. Integers are
little-endian u16. The generated constants identify three section starts and
the root-address shift; addresses are relative to this asset.

| Section | Record | Lookup |
| --- | --- | --- |
| Cell directory | One u16 per cell | Index of a shared heading row |
| Heading rows | 32 u16 references, 64 bytes per row | Heading selects a root reference |
| Mask roots | One u16 block ID per eight mask words | Ordered native blocks reconstruct the mask |
| Blocks | Eight u16 words, 16 bytes | Global dictionary shared across all roots |

Cell directories that contain identical heading rows share the whole row.
Each unique mask keeps one root, while identical 16-byte blocks share storage
even when their masks belong to different cells or headings. Block 0 is zero;
block 1 is all ones. The final block is zero-padded beyond the logical mask.

Sections start on 128-byte boundaries. Root rows use the smallest power-of-two
address unit from 4 through 128 bytes that keeps each root reference below
`0xffff`; their stride is rounded to that unit. Runtime root addresses use a
shift and mask, avoiding a wide product. Block IDs also exclude `0xffff`, the
cache's invalid marker. A layout outside these bounds or the 2-MiB budget
disables the PVS and retains ordinary route/visibility selection.

For the current ENGLAND map:

| Section | Allocated bytes |
| --- | ---: |
| 486 cell entries and padding | 1,024 |
| 181 shared heading rows and padding | 11,648 |
| 5,742 roots and padding | 114,944 |
| 10,997 shared blocks and padding | 176,000 |
| Total | 303,616 |

## Runtime reuse

`rcc/pvs.rs` owns the persistent cache behind World's original PVS pointer:

1. An unchanged cell/heading key uses the existing CPU mask without reading
   the external table. Views outside the guarded domain retain the fallback
   without destroying the cache.
2. A changed cell reads its directory entry and the shared 32-heading row.
   A heading change in the same cell uses that cached row.
3. An unchanged root needs no block work. Otherwise the cache reads its block
   references and compares them with the previous references.
4. Changed zero/full blocks use a raw vector store. Other changed blocks use
   the existing bounded `gpu_window_asset_read` for exactly eight words.
   Unchanged blocks are neither fetched nor rewritten.

Random entry follows the same fixed lookup levels; there is no delta chain or
prerequisite previous pose. With ten blocks, at most 13 bounded window reads
are needed after an arbitrary cell change. All remaining visibility tests read
the CPU mask in its original representation.

There is one startup allocation and no per-frame allocations. It contains the
padded mask, 32 heading references, incoming root references, cached block IDs
and the current cell/root. ENGLAND uses 284 bytes of payload, 288 bytes including
the heap tag and cache-line sizing padding. Reference scratch shares this allocation; World keeps its original
size and stack layout.

## Validation

```powershell
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_pvs_storage.py
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_pvs_storage.py --reference BASELINE-SNAPSHOT --packed CANDIDATE-SNAPSHOT
python -B systems/cpu-v3-tang-nano-20k/applications/tankracer/tools/check_view_pvs.py CANDIDATE-SNAPSHOT --probes 1024 --output target/pvs-visibility.json
```

The storage check compares every key bit-for-bit with a frozen reference. It
also compiles the actual RCC cache body with bounded host memory operations,
then checks forward, reverse and random entries and repeated-key read counts.
Small masks, dirty tail bits, root-address widening and budget fallback have
independent fixtures. RCC application replay validates color, depth and draw
order; CPU measurements must use identical fixed camera sequences. Functional
GPU CPU measurements exclude GPU/Display SDRAM contention and board frame time.
