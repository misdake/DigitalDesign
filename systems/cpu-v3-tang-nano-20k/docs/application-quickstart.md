# First CPU V3 application

Read this from the repository root. This tutorial uses the core demo's normal
build and functional simulator; no board, Gowin installation or HDL simulator
is required to see the first frame.

## Prepare the tools

Use the repository-pinned Rust nightly, Python 3 and PowerShell. On Windows,
use an MSVC x64 developer environment. Cargo and Python must be on PATH.
Asset generation uses Python; it is part of the ordinary application build.

The language uses Rust syntax with RCC restrictions. Ordinary value structs are
passed/returned by value or through explicit `Array<T>` views. References to
GPU resource objects are a separate checked facility; a normal Rust `&MyStruct`
parameter is not supported. Pure nongeneric `const fn`, value constructors,
the decimal `fix!` macro and `include_bytes!` use the restricted [static expression subset](/systems/cpu-v3-tang-nano-20k/docs/static-assets.md).
General application macros remain unsupported. Use the [SDK value helpers](/systems/cpu-v3-tang-nano-20k/docs/sdk-values.md)
for readable colors, transforms and resource descriptions.

Resources can be grouped in an ordinary named owning struct, as in
`GeometryPage`; its methods borrow the original page and clean its live fields
at the end of its lifetime. `Mesh` and the other resource bodies own their
registrations. Their copyable `*Ptr` values are passed to draw/update APIs, while
copyable `*Desc` values describe creation or static storage. Keep each body alive
until its last pointer use. Command buffers and passes retain their exclusive
ownership protocol. See the [resource lifetime contract](/systems/cpu-v3-tang-nano-20k/docs/gpu-api-storage.md).

## Preview a known application

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- list
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- preview core-demo --help
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- preview core-demo
```

This builds a fresh snapshot, verifies it and writes a two-present headless
preview under the repository's `target/apps/core-demo/`. The command prints the
snapshot and `frame-final.ppm` paths. The recipe produces its static assets, compiles
the program once through Cargo OUT_DIR, packs CB06 and verifies the exported
snapshot. `application.build.json` binds code, resources, sources and debug info.
`cpu-v3-boot.bin` is the application package; it is not an FPGA configuration.

## Run with input, or reuse a snapshot

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- run core-demo
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- preview core-demo --snapshot target/core-demo/app
```

`run` builds and opens a window with Windows XInput slot 0. `--snapshot` instead
consumes a previously exported snapshot without building an application; replace
the example path with the printed snapshot path. Both modes use the CPU
instruction model and functional GPU/Display, with finite CPU, sample and host
time limits, and write the displayed frame, diagnostic and run receipt. These results
establish functional behavior; host elapsed time and instruction steps are not
physical GPU/CPU performance. The [profiling guide](/systems/cpu-v3-tang-nano-20k/docs/application-profiling.md)
explains the different backends when measurements are needed.

Use `--headless` for a run without a window, or `--xinput-slot 1` to select another
controller. The [workflow guide](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md#preview-and-run)
lists defaults and overrides. See the
[core demo controls](/systems/cpu-v3-tang-nano-20k/examples/core_demo/README.md#controls).
To start a specific page, use `preview core-demo --page 5`, for example.
To export without running, use `build core-demo --out target/core-demo/app`;
`build --out` requires a new or empty directory and never overwrites a snapshot.
Build failures print the main diagnostic and full log directory; matching
runtime failures resolve source lines through the snapshot's debug metadata.

## Follow the source before creating a new application

Open `examples/core_demo/rcc/main.rs`, then `common/state.rs` and one module
under `pages/`.
The page returns to the main loop after updating its UI grid. Main records into
Back, submits, recovers the CB from GPU wait, requests that completed target and
confirms the Display swap before reusing resources. Static memory is explicit;
no frame allocator or hidden renderer is required.

The normal application addition has two pieces:

1. An application directory with its RCC entry/modules and an owned
   `application.rs` recipe. Follow `examples/core_demo/application.rs`: it selects
   the checked GPU source, producer and startup options. Diagnostic capture stays
   disabled for an ordinary application.
2. One entry in `application_catalog!` in `applications/build/mod.rs`, naming the
   recipe path. That entry loads the module and registers its `APPLICATIONS`
   slice; there is no second enumeration to update.

Declare a numeric startup constant once in the recipe:

```rust
options: &[
    BuildOption::integer_constant("page", "CORE_DEMO_PAGE", 0, 0, 5),
],
```

The arguments name the CLI flag, environment/RCC constant, default, minimum and
maximum. Shared preparation validates CLI and direct-build values, watches the
variable for incremental builds, and emits the `u16` constant. No custom parser,
`rerun-if-env-changed` or constant string is needed. Producer-specific choices
and input files use `BuildOption::environment` and retain application-owned
configuration.

Plain app modules are loaded from their application directory. Shared resource
implementations must use the approved GPU API; copying its internals into an
application does not confer library privileges. The authoritative build records
all loaded source origins for the artifact.

## Add static content

An asset producer writes `assets.json`, native payloads and typed RCC
constants into its build-owned `--output` directory. Follow
`examples/core_demo/tools/assets.py` and its small `geometry.py`: these are actual
consumers of the shared font, mesh and texture encoders. `native_asset_source.Sources`
emits `AssetBytes`, aligned `#[asset]` declarations and complete native descriptions;
the system places the blobs and applies sector relocations. Do not repeat GPU
addresses or manually rebuild those descriptions in the application.

```rust
mod boot_resources;
use boot_resources::*;

// Producer-generated names; storage placement is resolved by the build.
let mesh = Mesh::from_native(GEOMETRY_CUBE);
let texture = Texture::from_native(GEOMETRY_CHECKER);
let atlas = UiAtlas::from_desc(CORE_ATLAS);
```

The [static asset guide](/systems/cpu-v3-tang-nano-20k/docs/static-assets.md)
describes source placement, producer metadata and native relocations. Manual
`#[asset]` and unsafe bindings remain advanced interfaces. GPU descriptors use
128-byte sectors; source blobs are outside the CPU static arena.

Declare writable storage with the corresponding const descriptions:

```rust
#[static_buffer]
const COLOR: Rgb565Surface = Rgb565Surface::allocate();
#[static_buffer]
const DEPTH: DepthSurface = DepthSurface::allocate();
#[static_buffer]
const GRID: UiGridStorage = UiGridStorage::allocate();
#[static_buffer]
const COMMANDS: CommandMemory = CommandMemory::allocate(4096, 4096);

// In main, after opening the GPU:
let color = ColorBuffer::from_surface(COLOR);
let depth = DepthBuffer::from_surface(DEPTH);
let mut grid = UiGrid::from_storage(GRID, &atlas);
```

Import `surface` and `gpu_api` as shown by `rcc/common/working_set.rs`. Storage is placed
and zeroed by the build/loader; live resources are registered once at startup.
`rcc/geometry_page.rs` then shows const world/normal preparation and projection
alongside a dynamically updated model. The GPU branch in `rcc/main.rs` records
actual geometry and UI, waits for completion and presents Back. Its source-world
X basis is reflected at draw conversion; the example places the fixed model on
the screen's left and the animated model on the right.

Use the shared [project font](/assets/fonts/README.md) for on-device text.
`tools/ui_font.py` packs selected native 8×16 glyphs with `base_asset`; see the
core-demo producer for the required extent and coordinated constants. Include `ui_font.SOURCES` in the
producer's repository-relative `sources` manifest and enable the recipe's
`manifest_sources` so font edits trigger an incremental rebuild. No TTF or OS
font is needed for ordinary builds.

The recipe turns the resource manifest into normal CB06 COPY records. Use the
generated repack project with `cpu-v3-pack` to reconstruct existing inputs;
do not create a second hand-maintained Flash or instruction array. Read
[application workflow](/systems/cpu-v3-tang-nano-20k/docs/application-workflow.md)
and [Flash layout](/systems/cpu-v3-tang-nano-20k/docs/flash-layout.md) for packaging
and matching FPGA configuration. Device programming is an explicit next step.
