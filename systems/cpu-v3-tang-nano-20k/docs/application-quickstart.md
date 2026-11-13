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
parameter is not supported. Application macros and `const fn` are also not
currently supported. Do not infer support from what rust-analyzer accepts.

## Build a known application

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- list
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build core-demo --help
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build core-demo --out target/core-demo/app
```

`--out` names a new or empty directory. An existing snapshot is immutable; choose
a new path for the next build. The recipe produces its static assets, compiles
the program once through Cargo OUT_DIR, packs CB06 and verifies the exported
snapshot. `application.build.json` binds code, resources, sources and debug info.
`cpu-v3-boot.bin` is the application package; it is not an FPGA configuration.

## See the first frame

```powershell
cargo run --release -p cpu-v3-tang-nano-20k --bin cpu-v3-display-sim -- --package target/core-demo/app/cpu-v3-boot.bin --cpu-mode instruction --stop-after-present 2 --frames 120 --max-cpu-steps 2000000 --out-dir target/core-demo/preview --ppm
```

This consumes existing bytes without building an application. It runs the CPU
instruction model and functional GPU/Display, with a finite CPU budget and host
sample limit, and writes the displayed frame and run receipt. These results
establish functional behavior; host elapsed time and instruction steps are not
physical GPU/CPU performance. The [profiling guide](/systems/cpu-v3-tang-nano-20k/docs/application-profiling.md)
explains the different backends when measurements are needed.

For live input, add Cargo feature `--features display-window` and runner options
`--window --xinput-slot 0`, retaining explicit limits sized for the session.
The simulator has no connected controller by default. See the
[core demo controls](/systems/cpu-v3-tang-nano-20k/examples/core_demo/README.md#controls).
To start a specific page, build another snapshot with `--page 5`, for example.

## Follow the source before creating a new application

Open `examples/core_demo/rcc/main.rs`, then `state.rs` and one page module.
The page returns to the main loop after updating its UI grid. Main records into
Back, submits, recovers the CB from GPU wait, requests that completed target and
confirms the Display swap before reusing resources. Static memory is explicit;
no frame allocator or hidden renderer is required.

The normal application addition has two pieces:

1. An application directory with its RCC entry/modules and an owned
   `application.rs` recipe. The recipe selects the source, checked GPU policy,
   optional asset producer and startup options. Follow the compact core-demo
   recipe; leave diagnostic capture disabled unless it is actually needed.
2. Import that recipe and add its `APPLICATIONS` slice in
   `applications/build/mod.rs`. The generic frontend then discovers the name
   through `cpu-v3-app list` and exposes the declared options through `--help`.

Plain app modules are loaded from their application directory. Shared resource
implementations must use the approved GPU API; copying its internals into an
application does not confer library privileges. The authoritative build records
all loaded source origins for the artifact.

## Add static content

An asset producer writes `assets.json`, resource payloads and named RCC constants
into the build-owned directory supplied by `--output`. The core-demo producer is
a small complete example. Reuse the canonical `tools/gpu_layout.py` pool/range
checks; do not duplicate the memory map. GPU addresses are bytes in the manifest,
while `GpuMemory` descriptors use 128-byte sectors.

The [UI atlas format](/ip/gpu-v2/docs/ui.md) uses 64 KiB at a 64-KiB-aligned base, with 256-byte glyph records and
glyph 255 reserved for skip. Each source pixel is R5:G5:B5:A1, alpha in bit 0;
odd rows exchange the two pixel pairs within each four-pixel group. The producer
implements this static layout; the application uses a generated address and an
explicit unsafe resource binding. Mesh and texture producers have their own
documented formats; UI pixels are not RGBA4444 texture pixels.

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
