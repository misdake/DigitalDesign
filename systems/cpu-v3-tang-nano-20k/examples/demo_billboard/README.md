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
export's checksum. The build packs the poses as immutable native meshlets. The
CPU chooses an update, computes each transparent layer's camera depth once,
and Shell-sorts the layer indices back to front. Layers at the same depth keep
their input order. The GPU expands the tree along the horizontal camera-right
vector and expands the effect in the camera plane.

Opaque geometry, tree and effects use static World with a shared View. Geometry
ROM prepares position and the selected Billboard basis before entering the
unchanged expansion program. Camera changes update only View; no per-Billboard
CPU MV cache or demo-specific scratchpad patch is needed. Rebuild with matching
World/View and Material24-capable gateware.

The scene is uniformly scaled to one eighth of the source units to fit the
Billboard offset representation. The RGB565 stage writes depth first. The
RGBA4444 layers use bilinear sampling and depth-tested SrcOver. The effect
textures encode emissive intensity as alpha to approximate the source additive
blend while retaining the source hue. The scene uses standard projected depth
throughout.
