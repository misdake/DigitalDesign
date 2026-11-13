# First GPU application

A colored triangle and `HELLO GPU` in the shared native 8×16 font. It is a small
application template, independent of the capability demo and its menus.

From the repository root:

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- preview hello-gpu
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- run hello-gpu
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build hello-gpu --out target/hello-gpu
```

`preview` builds and verifies a snapshot, runs two functional presents and prints
the frame path. `run` opens the same application in a window. Both commands are
bounded; their host time does not measure board performance. `--frames N` optionally
halts the application after N completed frames, with zero meaning continuous.

Read the files in this order:

1. [`rcc/main.rs`](rcc/main.rs): declare storage, initialize resources, record Back,
   submit, recover the command buffer, and confirm the Display swap.
2. [`rcc/scene.rs`](rcc/scene.rs): own the mesh/model/camera/material and draw through
   their Copy Ptr values. Main keeps the scene alive until all submissions finish.
3. [`tools/assets.py`](tools/assets.py): supply positions, colors and indices; the
   producer handles native geometry, directories, font coverage and relocations.
4. [`application.rs`](application.rs): select the source, producer and startup option.

To make another application, copy this directory, change the recipe's name,
selection, source/producer paths and owner, then register the recipe once in
`application_catalog!` in `applications/build/mod.rs`. The recipe is the template;
there is no second generated registration or handwritten image to keep in sync.
File modules load from their normal directories, without listing subdirectories
in `ordinary_sources`.

The default source-world camera looks along -Z; the triangle model sits at z=-4.

The build places and loads assets/workspace storage. The GPU API keeps capacity,
ownership and submission explicit; this example adds no renderer framework.
Continue with the [application guide](/systems/cpu-v3-tang-nano-20k/README.md),
[static asset guide](/systems/cpu-v3-tang-nano-20k/docs/static-assets.md) and
[capability demo](/systems/cpu-v3-tang-nano-20k/examples/core_demo/README.md).
