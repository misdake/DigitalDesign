# Bloxorz presentation documents

The application uses the public SDK value constructors and producer-generated
Mesh/Texture/UI descriptions. `rcc/scene_resources.rs` owns the scene bodies for
the run; rendering borrows their original fields and passes Copy Ptrs to the GPU.
`rcc/game_assets.rs` contains the application's material style and registration
factories. Asset bytes, physical layout, rules and presentation scheduling remain
application-owned. Numeric HUD fields use the ordinary `ui_text` software layer.

- [Base textures](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/textures.md): original photo generation, per-item rings, base-only filtering, dimensions and LOD diagnostics.

- [Fragile batches](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/fragile-batches.md): per-level heap geometry, immediate fracture visibility, reversible native indices and retirement/cache ownership.

- [Transitions and storage](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/transitions.md): implemented rules/animation clocks, static color/depth caching, bridge command assembly and two-CB scheduling, source/depth lifetime, archive/camera bounds, colored veil arithmetic, compact undo storage and verification boundaries.

- [Ground shadow](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/shadow.md): CPU projection/clipping, receiver dependencies, immutable triangle Instances and cache lifetime.

- [Marker textures](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/markers.md): deterministic RGB atlas, padded UV regions, no-mip sampling and retained surface motion.

- [Pixel diagnostics](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/diagnostics.md): physical-input moving-frame system profiling, real CPU submission inputs, observation-only GPU copies, per-cover depth/UV/texel/write records and their limits.
