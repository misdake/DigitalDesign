# Billboard demo

This standalone Tang Nano 20K demo shows a grass platform with a road and stone
markers. The two panels of the tree stay upright as the camera orbits. Beside
the tree, a captured flash and ten smoke particles play through 199 recorded
updates, retire, and loop after a short pause.

| Control | Action |
| --- | --- |
| Left stick | Orbit the stage |
| Right stick up/down | Zoom |
| Cross | Restart playback |
| Square | Pause or resume |
| D-pad left/right | Step to the adjacent update and pause |
| Triangle | Reset the camera |

Build and run from the repository root.

```sh
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build demo-billboard --gpu-debug 0 --out target/tree-explosion-demo
cargo run --release -p cpu-v3-tang-nano-20k --bin cpu-v3-display-sim -- --package target/tree-explosion-demo/cpu-v3-boot.bin --frames 24 --max-cpu-steps 4000000 --ppm --out-dir target/tree-explosion-preview
```

The supplied inspection export provides the seven source textures, the assembled
tree geometry and the recorded sprite positions, Q12 rotations, UVs and
brightness. `assets/scene.json` retains one row per 16 ms update and the input
export's checksum. The build stores compact sprite parameters, one immutable
quad template and nine static stage/tree meshlets. A 2,688-byte external working
allocation holds the native directory and packed dynamic batches. Updates occur
only after the preceding submission retires and its CB is reset.

The CPU computes each transparent layer's depth once and Shell-sorts indices
back to front, preserving input order at equal depth. It then groups adjacent
items only when texture and uniform alpha agree and their centers fit a shared
exact quantization grid. The supplied capture has distinct per-particle alpha,
so it usually remains one quad per batch; the demo preserves that appearance
rather than quantizing alpha to manufacture larger batches. Compatible inputs
can share up to sixteen quads per meshlet. Camera-only frames reuse native
geometry when their sorted order remains unchanged.

`SpriteStream` retains only cross-frame layer data, previous order and the draw
plan. The thirteen-word source rows are temporary storage in its rebuild
function. The optional shared `billboard` helper generates native vertices,
indices and metadata; application code owns rotation, appearance, sorting and
batch boundaries. One Mesh is registered and its CPU directory refreshed after
rebuilding. `MeshPtr::slice` selects a published batch without a new resource.
The GPU expands the tree along horizontal camera right and effects in the
camera plane.

Opaque geometry, tree and effects use static World with a shared View. Geometry
ROM prepares position and the selected Billboard basis before entering the
unchanged expansion program. Camera changes update only View; no per-Billboard
CPU MV cache or demo-specific scratchpad patch is needed. Rebuild with matching
World/View and Material24-capable gateware.

The 199 captured updates contain 1,302 sprite samples, not 1,302 meshlets.
At most ten dynamic batches plus nine static meshlets are present. Animation
parameters occupy 33,920 bytes; static mesh data and the template occupy
6,912 bytes. The original thirteen-word capture format is unchanged.

Each captured sprite uses thirteen CPU words:

| Word | Value |
| --- | --- |
| 0 | Effect kind and brightness byte |
| 1..6 | Center XYZ, Q16.16 low/high halves in GPU source coordinates |
| 7..8 | Source half-width/height in 1/1024 units, before the scene's 1/8 scale |
| 9..10 | Signed Q12 sine/cosine |
| 11..12 | Packed u0/v0 and u1/v1 source texel coordinates |

The producer validates corner ranges but does not bake those corners into
meshlets. Runtime uses ties-to-even at the native offset boundary. Captured
centers lie on a 1/1024 grid; a batch uses its minimum XYZ as origin and shift6,
covering up to 1023/1024 world units without additional center error. Wider runs
split. The shared writer's layout and ownership contract are documented in
[SDK values](/systems/cpu-v3-tang-nano-20k/docs/sdk-values.md#dynamic-billboard-batches).
The asset producer emits complete MeshDesc/TextureDesc constants and GpuMemory
spans, so the application does not coordinate sector, size and format arguments.

The scene is uniformly scaled to one eighth of the source units to fit the
Billboard offset representation. The RGB565 stage writes depth first. The
RGBA4444 layers use bilinear sampling and depth-tested SrcOver. The effect
textures encode emissive intensity as alpha to approximate the source additive
blend while retaining the source hue. The scene uses standard projected depth
throughout.
