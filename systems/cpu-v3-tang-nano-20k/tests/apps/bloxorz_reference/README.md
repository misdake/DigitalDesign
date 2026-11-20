# Bloxorz reference tests

This test target replays the original game rules and checks small GPU scenes
with independent reference data. The shared rules, data and geometry are
owned by [applications/bloxorz](/systems/cpu-v3-tang-nano-20k/applications/bloxorz/README.md).

```powershell
cargo test --release -p cpu-v3-tang-nano-20k --test bloxorz_reference
```

All RCC fixtures use the CPU V3 compiler's default optimization settings.

The 400x240 static/batch callers use the current column layout: command sector
`0x9800`, object sector `0x9a00` and immutable asset sector `0x9c00`. The native
white texture has its complete aligned 512-byte allocation. CPU recording keeps
the independent geometry/image oracle and its submission, immutability and
batch-work assertions. Native RTL replay remains an explicit bounded test.
Ordinary recording tests replay the actual uploaded CB/object/asset bytes on
the bounded functional GPU and compare every RGB565 pixel, depth (tolerance 1)
and all immutable inputs/allocation guards with the independent oracle.

| Location | Contents |
| --- | --- |
| `applications/bloxorz/rcc/bloxorz_core.rs` | Integer rules, compact snapshots and bounded undo |
| `applications/bloxorz/assets/reference.txt` | 33 levels, two synthetic rule fixtures and original-engine transitions |
| `applications/bloxorz/assets/atlas-low.png` | Unmodified 512×512 atlas from the reference's 400×240 mode |
| `applications/bloxorz/assets/provenance.json` | Input identities and import metadata |
| `applications/bloxorz/tools/import-reference.mjs` | Explicit read-only reference importer |
| `applications/bloxorz/geometry.rs` | Host camera, rolling matrices and clipped shadow preparation |
| `rcc/bloxorz_scene.rs` | CPU fixed-point camera and whole-block pose preparation using GPU transform helpers |
| `rcc/bloxorz_static.rs` | Minimal RGB board/block scene through formal GPU Pass calls |
| `rcc/bloxorz_batch.rs` | Imported level recording with shared ground instances |
| `batch_scene.rs` | All-level bounds and per-cell/batched recording comparisons |
| `applications/bloxorz/tools/batch-scene.py` | Exact-grid static level grouping and native asset packing |
| `static_scene.rs` | CPU recording and separate native GPU replay |
| `applications/bloxorz/tools/static-scene.py` | Native static mesh producer and independent pixel reference |
| `rcc_geometry.rs` | Compiled CPU geometry/range checks against independent Three.js data |
| `applications/bloxorz/tools/import-geometry.mjs` | Independent Three.js geometry fixture importer |
| [Core contract](/systems/cpu-v3-tang-nano-20k/applications/bloxorz/docs/core.md) | Implemented data formats and execution boundary |

The checked-in inputs make replay independent of the original local directory.
Reimport explicitly with Node and the reference path:

```powershell
node systems/cpu-v3-tang-nano-20k/applications/bloxorz/tools/import-reference.mjs <reference-directory>
```

The importer uses the original JavaScript engine to produce expected states;
the RCC core does not generate its own expected results. Stage 23 remains marked
unverified in the imported data. The atlas is an input asset, not yet a native
RGB565/mip allocation. The static GPU fixture is described in
[Static native scene](/systems/cpu-v3-tang-nano-20k/applications/bloxorz/docs/static-scene.md).
The geometry fixture can be refreshed explicitly:

```powershell
node systems/cpu-v3-tang-nano-20k/applications/bloxorz/tools/import-geometry.mjs <reference-directory>
```
