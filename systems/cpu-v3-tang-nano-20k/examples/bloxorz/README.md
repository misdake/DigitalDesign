# Bloxorz system example

This directory owns the Bloxorz application, beside the other Tang Nano 20K
system programs. Its current executable compiles the RCC game core and replays
the imported reference transitions on CPU V3. A separate ignored test records a static scene on the compiled CPU and renders
its actual command bytes on native GPU RTL. It is not a boot-image selection yet. The default system application is unchanged.

```powershell
& scripts/run-cargo.ps1 -Subcommand run -Label bloxorz-replay -CargoArgs @('-p','cpu-v3-tang-nano-20k','--example','bloxorz')
& scripts/run-cargo.ps1 -Subcommand test -Label bloxorz-core -CargoArgs @('-p','cpu-v3-tang-nano-20k','--example','bloxorz')
```

All RCC fixtures use the CPU V3 compiler's default optimization settings.

| Location | Contents |
| --- | --- |
| `rcc/bloxorz_core.rs` | Integer rules, compact snapshots and bounded undo |
| `assets/reference.txt` | 33 levels, two synthetic rule fixtures and original-engine transitions |
| `assets/atlas-low.png` | Unmodified 512×512 atlas from the reference's 400×240 mode |
| `assets/provenance.json` | Input identities and import metadata |
| `tools/import-reference.mjs` | Explicit read-only reference importer |
| `geometry.rs` | Host camera, rolling matrices and clipped shadow preparation |
| `rcc/bloxorz_scene.rs` | CPU fixed-point camera and whole-block pose preparation using GPU transform helpers |
| `rcc/bloxorz_static.rs` | Minimal RGB board/block scene through formal GPU Pass calls |
| `rcc/bloxorz_batch.rs` | Imported level recording with shared ground instances |
| `batch_scene.rs` | All-level bounds and per-cell/batched recording comparisons |
| `tools/batch-scene.py` | Exact-grid static level grouping and native asset packing |
| `static_scene.rs` | CPU recording and separate native GPU replay |
| `tools/static-scene.py` | Native static mesh producer and independent pixel reference |
| `rcc_geometry.rs` | Compiled CPU geometry/range checks against independent Three.js data |
| `tools/import-geometry.mjs` | Independent Three.js geometry fixture importer |
| [Core contract](/systems/cpu-v3-tang-nano-20k/examples/bloxorz/docs/core.md) | Implemented data formats and execution boundary |

The checked-in inputs make replay independent of the original local directory.
Reimport explicitly with Node and the reference path:

```powershell
node systems/cpu-v3-tang-nano-20k/examples/bloxorz/tools/import-reference.mjs <reference-directory>
```

The importer uses the original JavaScript engine to produce expected states;
the RCC core does not generate its own expected results. Stage 23 remains marked
unverified in the imported data. The atlas is an input asset, not yet a native
RGB565/mip allocation. The static GPU fixture is described in
[Static native scene](/systems/cpu-v3-tang-nano-20k/examples/bloxorz/docs/static-scene.md).
Display presentation, the animation session and input-device integration are not
part of the executable. Pending designs remain in the local
project design directory.

Run the host geometry summary by appending `'--','--geometry'` to the run
arguments. Its scope and numeric limits are documented in
[Host scene geometry](/systems/cpu-v3-tang-nano-20k/examples/bloxorz/docs/geometry.md).
The geometry fixture can be refreshed explicitly:

```powershell
node systems/cpu-v3-tang-nano-20k/examples/bloxorz/tools/import-geometry.mjs <reference-directory>
```
